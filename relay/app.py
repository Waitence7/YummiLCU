# region [Imports]
"""
LCU Relay FastAPI — OAuth, 에이전트 WebSocket, 봇 internal HTTP.

실행: uv run yummi-lcu-relay
"""

from __future__ import annotations

import json
import logging
import secrets
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

logger = logging.getLogger("yummi_lcu.relay")
# endregion

SESSION_KEY = "relay:session:{session_id}"
SESSION_STATUS_KEY = "relay:session_status:{session_id}"


def _session_redis_key(session_id: str) -> str:
    return SESSION_KEY.format(session_id=session_id)


def _status_redis_key(session_id: str) -> str:
    return SESSION_STATUS_KEY.format(session_id=session_id)


@asynccontextmanager
async def _lifespan(app: FastAPI):
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
async def health(request: Request) -> dict[str, Any]:
    return {
        "status": "ok",
        "oauth_configured": config.oauth_configured(),
        "internal_secret_configured": bool(config.relay_internal_secret()),
    }


_AGENT_VERSION_FILE = Path(__file__).resolve().parents[1] / "deploy" / "agent-version.json"


@app.get("/agent/version.json")
async def agent_version_manifest() -> JSONResponse:
    """에이전트 자동 업데이트용 manifest (deploy/agent-version.json)."""
    if not _AGENT_VERSION_FILE.is_file():
        return JSONResponse({"version": "0.0.0", "url": "", "notes": "manifest not configured"})
    data = json.loads(_AGENT_VERSION_FILE.read_text(encoding="utf-8"))
    return JSONResponse(data)


@app.get("/login")
async def login(session_id: str = Query(..., min_length=8, max_length=64)) -> RedirectResponse:
    """에이전트가 연 브라우저용 Discord OAuth 시작."""
    if not config.oauth_configured():
        raise HTTPException(503, "OAuth not configured")
    try:
        uuid.UUID(session_id)
    except ValueError as e:
        raise HTTPException(400, "invalid session_id") from e
    return RedirectResponse(auth.build_login_url(session_id), status_code=302)


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
        return HTMLResponse(f"<h1>로그인 취소됨</h1><p>{error}</p>", status_code=400)
    if not code or not state:
        raise HTTPException(400, "missing code or state")
    session_id = state
    r: redis.Redis = request.app.state.redis
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
    await r.set(_session_redis_key(session_id), str(discord_id), ex=ttl)
    await r.set(_status_redis_key(session_id), "ok", ex=ttl)
    bound = await conn.bind_discord(session_id, discord_id)
    logger.info("OAuth 완료 discord_id=%s session=%s bound_ws=%s", discord_id, session_id[:8], bound)
    return HTMLResponse(
        "<h1>로그인 완료</h1><p>이 창을 닫고 에이전트로 돌아가세요.</p>",
        status_code=200,
    )


@app.get("/auth/status")
async def auth_status(request: Request, session_id: str = Query(..., min_length=8, max_length=64)) -> JSONResponse:
    """에이전트 폴링 — pending | ok | expired."""
    r: redis.Redis = request.app.state.redis
    conn: ConnectionManager = request.app.state.connections
    raw = await r.get(_session_redis_key(session_id))
    if raw is None:
        st = await r.get(_status_redis_key(session_id))
        if st == "expired":
            return JSONResponse({"status": "expired"})
        return JSONResponse({"status": "pending"})
    try:
        discord_id = int(raw)
    except ValueError:
        return JSONResponse({"status": "pending"})
    # WS가 아직 없을 수 있음 — bind 시도
    await conn.bind_discord(session_id, discord_id)
    return JSONResponse({"status": "ok", "discord_id": str(discord_id)})


# * ========================================================
# * # WebSocket (에이전트) 파트 #
# * ========================================================


@app.websocket("/ws/agent")
async def ws_agent(websocket: WebSocket, session_id: str = Query(..., min_length=8, max_length=64)) -> None:
    """C# 에이전트 연결. OAuth 후 discord_id에 바인딩."""
    await websocket.accept()
    conn: ConnectionManager = websocket.app.state.connections
    r: redis.Redis = websocket.app.state.redis
    await conn.attach_session(session_id, websocket)

    raw = await r.get(_session_redis_key(session_id))
    if raw is not None:
        try:
            await conn.bind_discord(session_id, int(raw))
        except ValueError:
            pass

    try:
        while True:
            msg = await websocket.receive_text()
            if msg == "ping":
                await websocket.send_json({"type": "pong"})
    except WebSocketDisconnect:
        pass
    finally:
        await conn.unregister_ws(websocket)


# * ========================================================
# * # Internal API (Yummibot) 파트 #
# * ========================================================


class InternalCommandBody(BaseModel):
    """봇 → Relay 명령."""

    discord_id: int = Field(..., gt=0)
    action: str = Field(..., min_length=1, max_length=64)
    payload: dict[str, Any] = Field(default_factory=dict)


def _verify_internal_secret(x_relay_internal_secret: str | None = Header(None)) -> None:
    expected = config.relay_internal_secret()
    if not expected:
        raise HTTPException(503, "internal API not configured")
    if not x_relay_internal_secret or not secrets.compare_digest(x_relay_internal_secret, expected):
        raise HTTPException(401, "unauthorized")


@app.post("/internal/command")
async def internal_command(
    request: Request,
    body: InternalCommandBody,
    x_relay_internal_secret: str | None = Header(None),
) -> JSONResponse:
    """누른 유저 discord_id의 에이전트로만 action 전달."""
    _verify_internal_secret(x_relay_internal_secret)
    if not is_allowed_action(body.action):
        raise HTTPException(400, f"action not allowed: {body.action}")
    conn: ConnectionManager = request.app.state.connections
    if not conn.is_online(body.discord_id):
        raise HTTPException(404, "agent not connected")
    req_id = secrets.token_hex(8)
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
        raise HTTPException(502, "failed to send to agent")
    return JSONResponse({"ok": True, "request_id": req_id})


@app.get("/internal/online/{discord_id}")
async def internal_online(
    request: Request,
    discord_id: int,
    x_relay_internal_secret: str | None = Header(None),
) -> JSONResponse:
    """에이전트 연결 여부 (봇 UI용)."""
    _verify_internal_secret(x_relay_internal_secret)
    conn: ConnectionManager = request.app.state.connections
    return JSONResponse({"online": conn.is_online(discord_id), "allowed_actions": sorted(ALLOWED_ACTIONS)})
