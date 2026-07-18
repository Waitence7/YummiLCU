# region [Imports]
"""
LCU Relay FastAPI — OAuth, 에이전트 WebSocket, 봇 internal HTTP.

실행: uv run yummi-lcu-relay
"""

from __future__ import annotations

import asyncio
import html
import json
import logging
import secrets
import time
import uuid
from contextlib import asynccontextmanager
from pathlib import Path
from typing import Any

import aiohttp
import redis.asyncio as redis
from fastapi import FastAPI, Header, HTTPException, Query, Request, WebSocket, WebSocketDisconnect
from fastapi.responses import HTMLResponse, JSONResponse, RedirectResponse
from pydantic import BaseModel, Field

from relay import auth, config
from relay.actions import ALLOWED_ACTIONS, is_allowed_action
from relay.connections import ConnectionManager
from relay.lcu_linked import is_lcu_linked, lcu_linked_map, mark_lcu_linked

logger = logging.getLogger("yummi_lcu.relay")
# endregion

SESSION_KEY = "relay:session:{session_id}"
SESSION_STATUS_KEY = "relay:session_status:{session_id}"
WS_TOKEN_KEY = "relay:ws_token:{session_id}"
OAUTH_LINK_PENDING_KEY = "relay:oauth_link_pending:{session_id}"
DISCORD_PROFILE_KEY = "relay:discord_profile:{session_id}"
OAUTH_STATE_KEY = "relay:oauth_state:{oauth_state}"
OAUTH_STATE_TTL_SEC = 600
OAUTH_LINK_CODE_TTL_SEC = 600
WS_AUTH_TIMEOUT_SEC = 15.0

_INTERNAL_AUTH_MAX_FAILS = 20
_INTERNAL_AUTH_WINDOW_SEC = 60.0
_INTERNAL_AUTH_REDIS_KEY = "relay:internal_auth_fail:{ip}"

# OAuth 6자리 링크 코드 brute-force 방어: 세션별 실패 N회 시 코드 폐기/잠금
OAUTH_LINK_MAX_ATTEMPTS = 5
_OAUTH_LINK_ATTEMPT_REDIS_KEY = "relay:oauth_link_attempt:{session_id}"

_LONG_COMMAND_ACTIONS = frozenset({"launch_client", "play_ranked_solo", "play_normal_draft"})
_COMMAND_TIMEOUT_DEFAULT_SEC = 30.0
_COMMAND_TIMEOUT_LONG_SEC = 300.0
_MAX_PARTY_INVITE_RIOT_IDS = 20


def _session_redis_key(session_id: str) -> str:
    return SESSION_KEY.format(session_id=session_id)


def _status_redis_key(session_id: str) -> str:
    return SESSION_STATUS_KEY.format(session_id=session_id)


def _oauth_state_redis_key(oauth_state: str) -> str:
    return OAUTH_STATE_KEY.format(oauth_state=oauth_state)


def _ws_token_redis_key(session_id: str) -> str:
    return WS_TOKEN_KEY.format(session_id=session_id)


def _oauth_link_pending_redis_key(session_id: str) -> str:
    return OAUTH_LINK_PENDING_KEY.format(session_id=session_id)


def _discord_profile_redis_key(session_id: str) -> str:
    return DISCORD_PROFILE_KEY.format(session_id=session_id)


def _discord_profile(user: dict[str, Any], discord_id: int) -> dict[str, str]:
    """Tauri UI에 표시할 공개 Discord 프로필만 보관한다. 토큰은 저장하지 않는다."""
    name = str(user.get("global_name") or user.get("username") or "Discord 사용자").strip()[:64]
    profile = {"name": name or "Discord 사용자"}
    avatar = user.get("avatar")
    if isinstance(avatar, str) and avatar:
        ext = "gif" if avatar.startswith("a_") else "png"
        profile["avatar"] = f"https://cdn.discordapp.com/avatars/{discord_id}/{avatar}.{ext}?size=128"
    return profile


def _oauth_link_attempt_redis_key(session_id: str) -> str:
    return _OAUTH_LINK_ATTEMPT_REDIS_KEY.format(session_id=session_id)


def _internal_auth_redis_key(ip: str) -> str:
    return _INTERNAL_AUTH_REDIS_KEY.format(ip=ip)


def _generate_link_code() -> str:
    return "".join(secrets.choice("0123456789") for _ in range(6))


async def _claim_or_verify_ws_token(r: redis.Redis, session_id: str, ws_token: str) -> bool:
    """첫 WS 연결이 ws_token 을 선점. 이후 동일 session_id 는 같은 토큰만 허용."""
    key = _ws_token_redis_key(session_id)
    stored = await r.get(key)
    ttl = config.relay_session_ttl_sec()
    if stored is None:
        return bool(await r.set(key, ws_token, ex=ttl, nx=True))
    return secrets.compare_digest(stored, ws_token)


async def _refresh_session_ttl(r: redis.Redis, session_id: str) -> None:
    """연결 중인 에이전트 세션 Redis TTL 연장."""
    ttl = config.relay_session_ttl_sec()
    pipe = r.pipeline()
    pipe.expire(_session_redis_key(session_id), ttl)
    pipe.expire(_status_redis_key(session_id), ttl)
    pipe.expire(_ws_token_redis_key(session_id), ttl)
    await pipe.execute()


async def _complete_oauth_link(
    conn: ConnectionManager,
    r: redis.Redis,
    websocket: WebSocket,
    code: str,
) -> bool:
    """브라우저 OAuth 후 에이전트에 입력한 6자리 코드로 discord_id 바인딩."""
    session_id = conn.ws_session_id(websocket)
    if not session_id:
        return False
    # brute-force 방어: 세션별 시도 횟수 초과 시 코드 폐기 후 거부(재-OAuth 필요)
    if await _oauth_link_attempts_exceeded(r, session_id):
        await r.delete(_oauth_link_pending_redis_key(session_id))
        logger.warning("OAuth 링크 코드 시도 횟수 초과 — 세션 잠금 session=%s", session_id[:8])
        return False
    raw_pending = await r.get(_oauth_link_pending_redis_key(session_id))
    if not raw_pending:
        return False
    try:
        pending = json.loads(raw_pending)
    except json.JSONDecodeError:
        return False
    if not isinstance(pending, dict):
        return False
    expected = str(pending.get("code") or "").strip()
    submitted = code.strip().replace(" ", "")
    if not expected or not secrets.compare_digest(expected, submitted):
        count = await _record_oauth_link_failure(r, session_id)
        if count >= OAUTH_LINK_MAX_ATTEMPTS:
            await r.delete(_oauth_link_pending_redis_key(session_id))
            logger.warning("OAuth 링크 코드 %d회 실패 — 코드 폐기 session=%s", count, session_id[:8])
        return False
    # 성공: 시도 카운터 정리
    await r.delete(_oauth_link_attempt_redis_key(session_id))
    try:
        discord_id = int(pending["discord_id"])
    except (KeyError, TypeError, ValueError):
        return False
    if discord_id <= 0:
        return False

    ttl = config.relay_session_ttl_sec()
    await r.set(_session_redis_key(session_id), str(discord_id), ex=ttl)
    profile = pending.get("profile") if isinstance(pending.get("profile"), dict) else {}
    await r.set(_discord_profile_redis_key(session_id), json.dumps(profile), ex=ttl)
    await r.set(_status_redis_key(session_id), "ok", ex=ttl)
    await r.delete(_oauth_link_pending_redis_key(session_id))
    bound = await _try_bind_discord(conn, r, session_id, discord_id)
    logger.info("OAuth 링크 코드 확인 discord_id=%s session=%s bound_ws=%s", discord_id, session_id[:8], bound)
    return bound


async def _try_bind_discord(
    conn: ConnectionManager,
    r: redis.Redis,
    session_id: str,
    discord_id: int,
) -> bool:
    """OAuth 완료 후 ws_token 이 일치하는 에이전트 WS 만 discord_id 에 바인딩."""
    ws_token = conn.session_ws_token(session_id)
    if not ws_token:
        return False
    stored = await r.get(_ws_token_redis_key(session_id))
    if not stored or not secrets.compare_digest(stored, ws_token):
        return False
    profile: dict[str, str] = {}
    raw_profile = await r.get(_discord_profile_redis_key(session_id))
    if raw_profile:
        try:
            parsed = json.loads(raw_profile)
            if isinstance(parsed, dict):
                profile = {
                    key: str(value)[:512]
                    for key, value in parsed.items()
                    if key in {"name", "avatar"} and isinstance(value, str)
                }
        except json.JSONDecodeError:
            pass
    bound = await conn.bind_discord(session_id, discord_id, profile)
    if bound:
        await mark_lcu_linked(r, discord_id)
    return bound


async def _forward_participant_status(
    conn: ConnectionManager,
    r: redis.Redis,
    discord_id: int,
    data: dict[str, Any],
) -> None:
    payload = dict(data)
    if payload.get("agent_online"):
        await mark_lcu_linked(r, discord_id)
    payload["lcu_linked"] = await is_lcu_linked(r, discord_id)
    await conn.forward_participant_status_update(discord_id, payload)


def _client_ip(request: Request) -> str:
    # nginx 가 설정한 X-Real-IP 우선 (클라이언트 X-Forwarded-For 스푸핑 완화)
    real_ip = request.headers.get("x-real-ip")
    if real_ip:
        return real_ip.strip()
    forwarded = request.headers.get("x-forwarded-for")
    if forwarded:
        return forwarded.split(",")[0].strip()
    if request.client:
        return request.client.host
    return "unknown"


async def _record_internal_auth_failure(r: redis.Redis, ip: str) -> None:
    key = _internal_auth_redis_key(ip)
    count = await r.incr(key)
    if count == 1:
        await r.expire(key, int(_INTERNAL_AUTH_WINDOW_SEC))


async def _is_internal_auth_rate_limited(r: redis.Redis, ip: str) -> bool:
    raw = await r.get(_internal_auth_redis_key(ip))
    if raw is None:
        return False
    try:
        return int(raw) >= _INTERNAL_AUTH_MAX_FAILS
    except ValueError:
        return False


async def _record_oauth_link_failure(r: redis.Redis, session_id: str) -> int:
    key = _oauth_link_attempt_redis_key(session_id)
    count = await r.incr(key)
    if count == 1:
        await r.expire(key, OAUTH_LINK_CODE_TTL_SEC)
    return count


async def _oauth_link_attempts_exceeded(r: redis.Redis, session_id: str) -> bool:
    raw = await r.get(_oauth_link_attempt_redis_key(session_id))
    if raw is None:
        return False
    try:
        return int(raw) >= OAUTH_LINK_MAX_ATTEMPTS
    except ValueError:
        return False


async def _read_ws_auth_payload(websocket: WebSocket) -> dict[str, Any] | None:
    """첫 WebSocket 메시지에서 auth JSON 파싱 (쿼리 시크릿 미사용)."""
    try:
        msg = await asyncio.wait_for(websocket.receive_text(), timeout=WS_AUTH_TIMEOUT_SEC)
    except (asyncio.TimeoutError, WebSocketDisconnect):
        return None
    if msg == "ping":
        await websocket.send_json({"type": "pong"})
        try:
            msg = await asyncio.wait_for(websocket.receive_text(), timeout=WS_AUTH_TIMEOUT_SEC)
        except (asyncio.TimeoutError, WebSocketDisconnect):
            return None
    try:
        data = json.loads(msg)
    except json.JSONDecodeError:
        return None
    return data if isinstance(data, dict) else None


@asynccontextmanager
async def _lifespan(app: FastAPI):
    try:
        config.relay_public_base_url_must_be_https()
    except RuntimeError as e:
        logger.error("%s", e)
        raise
    if not config.oauth_configured():
        logger.warning("Discord OAuth 미설정 — /login 동작하지 않습니다 (.env 확인)")
    if not config.relay_internal_secret():
        logger.warning("RELAY_INTERNAL_SECRET 미설정 — /internal/command 비활성에 가깝습니다")
    timeout = aiohttp.ClientTimeout(total=30)
    app.state.http = aiohttp.ClientSession(timeout=timeout)
    app.state.redis = redis.from_url(config.redis_url(), decode_responses=True)
    app.state.connections = ConnectionManager()
    try:
        await app.state.redis.ping()
        logger.info("Redis 연결 OK")
    except Exception as e:
        logger.error("Redis 연결 실패: %s", e)
    yield
    await app.state.http.close()
    await app.state.redis.aclose()


app = FastAPI(title="YummiLcu Relay", lifespan=_lifespan)


# * ========================================================
# * # OAuth · 세션 파트 #
# * ========================================================


@app.get("/health")
async def health() -> dict[str, str]:
    """공개 헬스체크 — 설정 노출 최소화."""
    return {"status": "ok"}


_AGENT_VERSION_FILE = Path(__file__).resolve().parents[1] / "deploy" / "agent-version.json"


@app.get("/agent/version.json")
async def agent_version_manifest() -> JSONResponse:
    """에이전트 자동 업데이트용 manifest (deploy/agent-version.json)."""
    if not _AGENT_VERSION_FILE.is_file():
        return JSONResponse({"version": "0.0.0", "url": "", "notes": "manifest not configured"})
    data = json.loads(_AGENT_VERSION_FILE.read_text(encoding="utf-8"))
    return JSONResponse(data)


@app.get("/login")
async def login(
    request: Request,
    session_id: str = Query(..., min_length=8, max_length=64),
) -> RedirectResponse:
    """에이전트가 연 브라우저용 Discord OAuth 시작 (에이전트 WS 선연결 필수)."""
    if not config.oauth_configured():
        raise HTTPException(503, "OAuth not configured")
    try:
        uuid.UUID(session_id)
    except ValueError as e:
        raise HTTPException(400, "invalid session_id") from e
    conn: ConnectionManager = request.app.state.connections
    if not conn.has_active_session_ws(session_id):
        raise HTTPException(400, "에이전트를 먼저 연결해 주세요.")
    oauth_state = secrets.token_urlsafe(32)
    r: redis.Redis = request.app.state.redis
    await r.set(_oauth_state_redis_key(oauth_state), session_id, ex=OAUTH_STATE_TTL_SEC)
    return RedirectResponse(auth.build_login_url(oauth_state), status_code=302)


@app.get("/auth/callback")
@app.get("/auth/discord/callback")
async def auth_callback(
    request: Request,
    code: str | None = None,
    state: str | None = None,
    error: str | None = None,
) -> HTMLResponse:
    """Discord redirect — session_id(state)에 discord_id 저장. `/auth/discord/callback` 별칭 포함."""
    if error:
        safe = html.escape(error, quote=True)
        return HTMLResponse(f"<h1>로그인 취소됨</h1><p>{safe}</p>", status_code=400)
    if not code or not state:
        raise HTTPException(400, "missing code or state")
    r: redis.Redis = request.app.state.redis
    session_id = await r.getdel(_oauth_state_redis_key(state))
    if not session_id:
        return HTMLResponse("<h1>로그인 세션 만료</h1><p>에이전트에서 다시 연결해 주세요.</p>", status_code=400)
    conn: ConnectionManager = request.app.state.connections
    http: aiohttp.ClientSession = request.app.state.http

    token_data = await auth.exchange_code(http, code)
    if not token_data:
        return HTMLResponse("<h1>토큰 교환 실패</h1>", status_code=502)
    access = token_data.get("access_token")
    if not access:
        return HTMLResponse("<h1>access_token 없음</h1>", status_code=502)

    user = await auth.fetch_discord_user(http, access)
    discord_id = auth.parse_discord_id(user or {})
    if discord_id is None:
        return HTMLResponse("<h1>Discord 사용자 ID 조회 실패</h1>", status_code=502)

    ttl = config.relay_session_ttl_sec()
    link_code = _generate_link_code()
    pending = json.dumps({
        "discord_id": discord_id,
        "code": link_code,
        "profile": _discord_profile(user or {}, discord_id),
    })
    await r.set(_oauth_link_pending_redis_key(session_id), pending, ex=OAUTH_LINK_CODE_TTL_SEC)
    await r.set(_status_redis_key(session_id), "link_pending", ex=ttl)
    logger.info("OAuth 대기(링크 코드) discord_id=%s session=%s", discord_id, session_id[:8])
    safe_code = html.escape(link_code, quote=True)
    return HTMLResponse(
        "<h1>Discord 로그인 완료</h1>"
        f"<p>아래 <strong>6자리 코드</strong>를 복사해 에이전트에서 '붙여넣기'를 누르세요.</p>"
        f"<p style='font-size:2rem;letter-spacing:0.3em;font-family:monospace'>{safe_code}</p>"
        # link_code는 숫자(0-9)만 생성되므로 JS 문자열에 그대로 넣어도 안전하다.
        f"<button type='button' style='font-size:1rem;padding:0.4em 1em;cursor:pointer' "
        f"onclick=\"navigator.clipboard.writeText('{safe_code}').then(function(){{this.textContent='복사됨!';}}.bind(this)).catch(function(){{}});\">"
        "코드 복사</button>"
        "<p>코드는 10분간 유효합니다. 이 창은 닫아도 됩니다.</p>",
        status_code=200,
    )


@app.get("/auth/status")
async def auth_status(request: Request, session_id: str = Query(..., min_length=8, max_length=64)) -> JSONResponse:
    """에이전트 폴링 — pending | link_pending | ok | expired."""
    try:
        uuid.UUID(session_id)
    except ValueError as e:
        raise HTTPException(400, "invalid session_id") from e
    r: redis.Redis = request.app.state.redis
    conn: ConnectionManager = request.app.state.connections
    st = await r.get(_status_redis_key(session_id))
    if st == "expired":
        return JSONResponse({"status": "expired"})
    if st == "link_pending":
        return JSONResponse({"status": "link_pending"})
    raw = await r.get(_session_redis_key(session_id))
    if raw is None:
        return JSONResponse({"status": "pending"})
    try:
        discord_id = int(raw)
    except ValueError:
        return JSONResponse({"status": "pending"})
    if not conn.has_active_session_ws(session_id):
        return JSONResponse({"status": "pending"})
    # WS가 아직 없을 수 있음 — ws_token 검증 후 bind 시도
    await _try_bind_discord(conn, r, session_id, discord_id)
    await _refresh_session_ttl(r, session_id)
    return JSONResponse({"status": "ok"})


# * ========================================================
# * # WebSocket (에이전트) 파트 #
# * ========================================================


async def _forward_guild_match_eog(
    http: aiohttp.ClientSession,
    discord_id: int,
    payload: dict[str, Any],
) -> None:
    api_base = config.tournament_api_base_url()
    token = config.tournament_bot_internal_token()
    if not token:
        logger.warning("TOURNAMENT_BOT_INTERNAL_TOKEN 미설정 — 내전 LCU 전송 생략")
        return

    url = f"{api_base}/api/bot/guild-match/lcu-ingest"
    headers = {
        "content-type": "application/json",
        "x-internal-bot-token": token,
        "x-actor-discord-user-id": str(discord_id),
    }
    body = {"rawData": payload}
    try:
        async with http.post(url, headers=headers, json=body) as res:
            text = await res.text()
            if res.status >= 400:
                logger.warning(
                    "내전 LCU ingest 실패 discord_id=%s status=%s body=%s",
                    discord_id,
                    res.status,
                    text[:500],
                )
                return
            logger.info("내전 LCU ingest OK discord_id=%s body=%s", discord_id, text[:300])
    except Exception:
        logger.exception("내전 LCU ingest 요청 실패 discord_id=%s", discord_id)


async def _forward_match_eog(
    http: aiohttp.ClientSession,
    discord_id: int,
    payload: dict[str, Any],
) -> None:
    api_base = config.tournament_api_base_url()
    token = config.tournament_bot_internal_token()
    if not token:
        logger.warning("TOURNAMENT_BOT_INTERNAL_TOKEN 미설정 — LCU 종료 매치 저장 생략")
        return

    url = f"{api_base}/api/bot/lcu/matches/eog-ingest"
    headers = {
        "content-type": "application/json",
        "x-internal-bot-token": token,
        "x-actor-discord-user-id": str(discord_id),
    }
    body = {"rawData": payload}
    try:
        async with http.post(url, headers=headers, json=body) as res:
            text = await res.text()
            if res.status >= 400:
                logger.warning(
                    "LCU 종료 매치 저장 실패 discord_id=%s status=%s body=%s",
                    discord_id,
                    res.status,
                    text[:500],
                )
                return
            logger.info("LCU 종료 매치 저장 OK discord_id=%s body=%s", discord_id, text[:300])
    except Exception:
        logger.exception("LCU 종료 매치 저장 요청 실패 discord_id=%s", discord_id)


async def _handle_agent_message(
    websocket: WebSocket,
    conn: ConnectionManager,
    msg: str,
) -> None:
    try:
        data = json.loads(msg)
    except json.JSONDecodeError:
        return
    if not isinstance(data, dict):
        return

    msg_type = data.get("type")
    if msg_type == "complete_oauth_link":
        code = data.get("code")
        if not isinstance(code, str) or not code.strip():
            return
        r: redis.Redis = websocket.app.state.redis
        ok = await _complete_oauth_link(conn, r, websocket, code)
        if ok:
            await websocket.send_json({"type": "oauth_linked"})
        else:
            await websocket.send_json({"type": "oauth_link_failed", "message": "invalid or expired code"})
        return

    if msg_type == "guild_match_eog":
        discord_id = conn.discord_id_for_ws(websocket)
        payload = data.get("payload")
        if discord_id is None or not isinstance(payload, dict):
            logger.warning("guild_match_eog 무시: discord_id=%s payload=%s", discord_id, type(payload))
            return
        http: aiohttp.ClientSession = websocket.app.state.http
        await _forward_guild_match_eog(http, discord_id, payload)
        return

    if msg_type == "match_eog":
        discord_id = conn.discord_id_for_ws(websocket)
        payload = data.get("payload")
        if discord_id is None or not isinstance(payload, dict):
            logger.warning("match_eog 무시: discord_id=%s payload=%s", discord_id, type(payload))
            return
        http: aiohttp.ClientSession = websocket.app.state.http
        await _forward_match_eog(http, discord_id, payload)
        return

    if msg_type == "agent_hello":
        discord_id = conn.discord_id_for_ws(websocket)
        if discord_id is None:
            return
        info = {
            "version": str(data.get("version") or ""),
            "lcu_ready": bool(data.get("lcu_ready")),
            "os": str(data.get("os") or ""),
        }
        conn.set_agent_info(discord_id, info)
        r: redis.Redis = websocket.app.state.redis
        await mark_lcu_linked(r, discord_id)
        logger.info(
            "agent_hello discord_id=%s version=%s lcu_ready=%s",
            discord_id,
            info["version"],
            info["lcu_ready"],
        )
        return

    if msg_type == "party_lobby_update":
        discord_id = conn.discord_id_for_ws(websocket)
        payload = data.get("data")
        if discord_id is None or not isinstance(payload, dict):
            return
        await conn.forward_party_lobby_update(discord_id, payload)
        return

    if msg_type == "ready_check_update":
        discord_id = conn.discord_id_for_ws(websocket)
        payload = data.get("data")
        if discord_id is None or not isinstance(payload, dict):
            return
        await conn.forward_ready_check_update(discord_id, payload)
        return

    if msg_type == "champ_select_update":
        discord_id = conn.discord_id_for_ws(websocket)
        payload = data.get("data")
        if discord_id is None or not isinstance(payload, dict):
            return
        await conn.forward_champ_select_update(discord_id, payload)
        return

    if msg_type == "gameflow_update":
        discord_id = conn.discord_id_for_ws(websocket)
        payload = data.get("data")
        if discord_id is None or not isinstance(payload, dict):
            return
        await conn.forward_gameflow_update(discord_id, payload)
        if isinstance(payload, dict) and payload.get("phase") == "ReadyCheck":
            await conn.forward_ready_check_update(
                discord_id, {"active": True, "source": "gameflow"}
            )
        return

    if msg_type == "participant_status_update":
        discord_id = conn.discord_id_for_ws(websocket)
        payload = data.get("data")
        if discord_id is None or not isinstance(payload, dict):
            return
        stored = conn.set_participant_status(discord_id, payload)
        r: redis.Redis = websocket.app.state.redis
        await _forward_participant_status(conn, r, discord_id, stored)
        return

    if msg_type == "command_result":
        discord_id = conn.discord_id_for_ws(websocket)
        request_id = data.get("request_id")
        if discord_id is None or not isinstance(request_id, str) or not request_id:
            return
        result: dict[str, Any] = {
            "ok": bool(data.get("ok")),
            "message": str(data.get("message") or ""),
        }
        extra = data.get("data")
        if isinstance(extra, dict):
            result["data"] = extra
        conn.complete_pending_result(discord_id, request_id, result)


@app.websocket("/ws/agent")
async def ws_agent(
    websocket: WebSocket,
    session_id: str = Query(..., min_length=8, max_length=64),
) -> None:
    """C# 에이전트 연결. session_id(URL) + 첫 메시지 ws_token 으로 OAuth 바인딩 보호."""
    try:
        uuid.UUID(session_id)
    except ValueError:
        await websocket.close(code=1008)
        return

    await websocket.accept()
    auth_payload = await _read_ws_auth_payload(websocket)
    if auth_payload is None or auth_payload.get("type") != "auth":
        await websocket.close(code=1008)
        return
    ws_token = auth_payload.get("ws_token")
    if not isinstance(ws_token, str) or len(ws_token) < 16 or len(ws_token) > 128:
        await websocket.close(code=1008)
        return

    r: redis.Redis = websocket.app.state.redis
    if not await _claim_or_verify_ws_token(r, session_id, ws_token):
        await websocket.close(code=1008)
        return

    conn: ConnectionManager = websocket.app.state.connections
    await conn.attach_session(session_id, websocket, ws_token)

    raw = await r.get(_session_redis_key(session_id))
    if raw is not None:
        try:
            await _try_bind_discord(conn, r, session_id, int(raw))
        except ValueError:
            pass

    try:
        while True:
            msg = await websocket.receive_text()
            if msg == "ping":
                sid = conn.ws_session_id(websocket)
                if sid:
                    await _refresh_session_ttl(r, sid)
                await websocket.send_json({"type": "pong"})
                continue
            await _handle_agent_message(websocket, conn, msg)
    except WebSocketDisconnect:
        pass
    finally:
        offline = await conn.unregister_ws(websocket)
        if offline is not None:
            did, payload = offline
            await _forward_participant_status(conn, r, did, payload)


# * ========================================================
# * # WebSocket (YummiBot) 파트 #
# * ========================================================


async def _handle_bot_message(conn: ConnectionManager, r: redis.Redis, msg: str) -> None:
    if msg == "ping":
        return
    try:
        data = json.loads(msg)
    except json.JSONDecodeError:
        return
    if not isinstance(data, dict):
        return

    msg_type = data.get("type")
    if msg_type == "subscribe_party":
        raw_id = data.get("discord_id")
        if isinstance(raw_id, int) and raw_id > 0:
            conn.subscribe_party_lobby(raw_id)
            if conn.is_online(raw_id):
                await conn.send_command(raw_id, {"type": "request_party_snapshot"})
        return
    if msg_type == "unsubscribe_party":
        raw_id = data.get("discord_id")
        if isinstance(raw_id, int) and raw_id > 0:
            conn.unsubscribe_party_lobby(raw_id)
        return
    if msg_type == "subscribe_gameflow":
        raw_id = data.get("discord_id")
        if isinstance(raw_id, int) and raw_id > 0:
            conn.subscribe_gameflow(raw_id)
        return
    if msg_type == "unsubscribe_gameflow":
        raw_id = data.get("discord_id")
        if isinstance(raw_id, int) and raw_id > 0:
            conn.unsubscribe_gameflow(raw_id)
        return
    if msg_type == "subscribe_match_dm":
        raw_id = data.get("discord_id")
        if isinstance(raw_id, int) and raw_id > 0:
            conn.subscribe_match_dm(raw_id)
        return
    if msg_type == "unsubscribe_match_dm":
        raw_id = data.get("discord_id")
        if isinstance(raw_id, int) and raw_id > 0:
            conn.unsubscribe_match_dm(raw_id)
        return
    if msg_type == "subscribe_participant_status":
        raw_id = data.get("discord_id")
        if isinstance(raw_id, int) and raw_id > 0:
            conn.subscribe_participant_status(raw_id)
            if conn.is_online(raw_id):
                await conn.send_command(
                    raw_id, {"type": "request_participant_status"}
                )
            cached = conn.get_participant_status(raw_id)
            if cached is None:
                cached = conn.get_participant_statuses([raw_id]).get(raw_id)
            if cached is not None:
                await _forward_participant_status(conn, r, raw_id, cached)
        return
    if msg_type == "unsubscribe_participant_status":
        raw_id = data.get("discord_id")
        if isinstance(raw_id, int) and raw_id > 0:
            conn.unsubscribe_participant_status(raw_id)


@app.websocket("/ws/bot")
async def ws_bot(websocket: WebSocket) -> None:
    """YummiBot 실시간 이벤트 수신. 첫 메시지로 RELAY_INTERNAL_SECRET 인증."""
    await websocket.accept()
    auth_payload = await _read_ws_auth_payload(websocket)
    expected = config.relay_internal_secret()
    if not expected or auth_payload is None or auth_payload.get("type") != "auth":
        await websocket.close(code=1008)
        return
    secret = auth_payload.get("secret")
    if not isinstance(secret, str) or not secrets.compare_digest(secret, expected):
        await websocket.close(code=1008)
        return

    conn: ConnectionManager = websocket.app.state.connections
    r: redis.Redis = websocket.app.state.redis
    await conn.register_bot_ws(websocket)

    try:
        while True:
            msg = await websocket.receive_text()
            if msg == "ping":
                await websocket.send_json({"type": "pong"})
                continue
            await _handle_bot_message(conn, r, msg)
    except WebSocketDisconnect:
        pass
    finally:
        await conn.unregister_bot_ws(websocket)


# * ========================================================
# * # Internal API (Yummibot) 파트 #
# * ========================================================


class InternalCommandBody(BaseModel):
    """봇 → Relay 명령."""

    discord_id: int = Field(..., gt=0)
    action: str = Field(..., min_length=1, max_length=64)
    payload: dict[str, Any] = Field(default_factory=dict)


async def _verify_internal_secret(
    request: Request,
    x_relay_internal_secret: str | None = Header(None),
) -> None:
    ip = _client_ip(request)
    r: redis.Redis = request.app.state.redis
    if await _is_internal_auth_rate_limited(r, ip):
        raise HTTPException(429, "too many requests")
    expected = config.relay_internal_secret()
    if not expected:
        raise HTTPException(503, "internal API not configured")
    if not x_relay_internal_secret or not secrets.compare_digest(x_relay_internal_secret, expected):
        await _record_internal_auth_failure(r, ip)
        raise HTTPException(401, "unauthorized")


@app.post("/internal/command")
async def internal_command(
    request: Request,
    body: InternalCommandBody,
    x_relay_internal_secret: str | None = Header(None),
) -> JSONResponse:
    """누른 유저 discord_id의 에이전트로만 action 전달."""
    await _verify_internal_secret(request, x_relay_internal_secret)
    if not is_allowed_action(body.action):
        raise HTTPException(400, f"action not allowed: {body.action}")
    if body.action in ("invite_party_members", "check_party_members"):
        for key in ("riot_ids", "check_riot_ids"):
            ids = body.payload.get(key)
            if isinstance(ids, list) and len(ids) > _MAX_PARTY_INVITE_RIOT_IDS:
                raise HTTPException(400, f"{key} max {_MAX_PARTY_INVITE_RIOT_IDS}")
    conn: ConnectionManager = request.app.state.connections
    if not conn.is_online(body.discord_id):
        raise HTTPException(404, "agent not connected")
    req_id = secrets.token_hex(8)
    pending = conn.register_pending_result(body.discord_id, req_id)
    ok = await conn.send_command(
        body.discord_id,
        {
            "type": "command",
            "action": body.action,
            "request_id": req_id,
            "payload": body.payload,
        },
    )
    if not ok:
        conn.cancel_pending_result(body.discord_id, req_id)
        raise HTTPException(502, "failed to send to agent")

    timeout_sec = (
        _COMMAND_TIMEOUT_LONG_SEC
        if body.action in _LONG_COMMAND_ACTIONS
        else _COMMAND_TIMEOUT_DEFAULT_SEC
    )
    try:
        result = await asyncio.wait_for(pending, timeout=timeout_sec)
    except asyncio.TimeoutError:
        conn.cancel_pending_result(body.discord_id, req_id)
        raise HTTPException(504, "agent response timeout") from None
    except asyncio.CancelledError:
        raise HTTPException(502, "agent disconnected") from None

    return JSONResponse({"ok": True, "request_id": req_id, "result": result})


@app.get("/internal/participant-status")
async def internal_participant_status(
    request: Request,
    ids: str = Query(..., min_length=1, max_length=4096),
    x_relay_internal_secret: str | None = Header(None),
) -> JSONResponse:
    """참가자별 최신 LCU 상태 (에이전트 push 캐시)."""
    await _verify_internal_secret(request, x_relay_internal_secret)
    conn: ConnectionManager = request.app.state.connections
    discord_ids: list[int] = []
    for part in ids.split(","):
        part = part.strip()
        if part.isdigit():
            discord_ids.append(int(part))
    if not discord_ids:
        raise HTTPException(400, "ids required")
    if len(discord_ids) > 50:
        raise HTTPException(400, "ids max 50")
    statuses = conn.get_participant_statuses(discord_ids)
    r: redis.Redis = request.app.state.redis
    linked = await lcu_linked_map(r, discord_ids)
    for did in discord_ids:
        row = statuses.get(did)
        if row is None:
            row = {
                "status": "offline",
                "phase": "None",
                "game_started_at_ms": None,
                "lcu_ready": False,
                "agent_online": False,
                "updated_at": 0.0,
            }
            statuses[did] = row
        row["lcu_linked"] = linked.get(did, False)
    return JSONResponse({"statuses": {str(k): v for k, v in statuses.items()}})


@app.get("/internal/online/{discord_id}")
async def internal_online(
    request: Request,
    discord_id: int,
    x_relay_internal_secret: str | None = Header(None),
) -> JSONResponse:
    """에이전트 연결 여부 (봇 UI용)."""
    await _verify_internal_secret(request, x_relay_internal_secret)
    conn: ConnectionManager = request.app.state.connections
    online = conn.is_online(discord_id)
    body: dict[str, Any] = {
        "online": online,
        "allowed_actions": sorted(ALLOWED_ACTIONS),
    }
    if online:
        agent = conn.agent_info(discord_id)
        if agent:
            body["agent"] = agent
    return JSONResponse(body)
