# region [Imports]
"""discord_id ↔ WebSocket 매핑 (Relay 프로세스 메모리)."""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any

from fastapi import WebSocket

logger = logging.getLogger("yummi_lcu.relay.connections")
# endregion

LIVE_GAME_CACHE_TTL_SEC = 15.0
LIVE_GAME_PARTICIPANT_COUNT = 10
BOT_WS_SEND_TIMEOUT_SEC = 2.0
AGENT_WS_SEND_TIMEOUT_SEC = 5.0
AGENT_HEARTBEAT_STALE_SEC = 180.0
AGENT_QUALITY_HEALTHY_SEC = 40.0
AGENT_QUALITY_DEGRADED_SEC = 90.0
AGENT_RECONNECTING_WINDOW_SEC = 120.0
FORCE_RECONNECT_COOLDOWN_SEC = 10.0
AGENT_SESSION_CONFLICT_CLOSE_CODE = 4009
GAMEFLOW_EOG_GRACE_SEC = 180.0


async def _send_bot_json(ws: WebSocket, payload: dict[str, Any]) -> None:
    await asyncio.wait_for(ws.send_json(payload), timeout=BOT_WS_SEND_TIMEOUT_SEC)


class ConnectionManager:
    """에이전트·봇 WebSocket 연결을 discord_id로 라우팅합니다."""

    def __init__(self) -> None:
        self._by_discord: dict[int, WebSocket] = {}
        self._active_session_ws: dict[str, WebSocket] = {}
        self._session_ws: dict[str, WebSocket] = {}
        self._session_ws_token: dict[str, str] = {}  # session_id -> ws_token (에이전트만 보유)
        self._ws_discord: dict[int, int] = {}  # id(ws) -> discord_id
        self._ws_trust: dict[int, str] = {}  # id(ws) -> trusted | new_device
        self._ws_session: dict[int, str] = {}  # id(ws) -> session_id (인증 전)
        self._ws_session_id: dict[int, str] = {}  # id(ws) -> session_id (TTL 갱신용)
        self._ws_connection_id: dict[int, str] = {}
        self._ws_last_heartbeat: dict[int, float] = {}
        self._last_disconnect_at: dict[int, float] = {}
        self._reconnect_count: dict[int, int] = {}
        self._force_reconnect_last_at: dict[int, float] = {}
        self._pending_results: dict[tuple[int, str], asyncio.Future[dict[str, Any]]] = {}
        self._pending_bot_eog: dict[str, asyncio.Future[bool]] = {}
        self._bot_ws: WebSocket | None = None
        self._party_subscribers: set[int] = set()  # creator discord_id
        self._gameflow_subscribers: set[int] = set()
        self._gameflow_eog_grace_until: dict[int, float] = {}
        self._gameflow: dict[int, dict[str, Any]] = {}
        self._live_game_subscribers: set[int] = set()
        self._live_game_recruitments: dict[int, str | None] = {}
        self._match_dm_subscribers: set[int] = set()
        self._participant_status_subscribers: set[int] = set()
        self._participant_status: dict[int, dict[str, Any]] = {}
        self._live_game: dict[int, tuple[float, dict[str, Any]]] = {}
        self._agent_info: dict[int, dict[str, Any]] = {}
        self._pending_agent_info_by_ws: dict[int, dict[str, Any]] = {}
        self._lock = asyncio.Lock()

    async def attach_session(
        self,
        session_id: str,
        ws: WebSocket,
        ws_token: str,
        connection_id: str | None = None,
    ) -> bool:
        """검증된 동일 세션 재접속은 새 WS가 기존 WS를 교체하도록 준비합니다.

        실제 Discord 바인딩/기존 WS 종료는 ``bind_discord``에서 원자적으로 처리합니다.
        이렇게 해야 이전 연결의 ``finally`` 정리보다 재접속이 먼저 도착해도 새 WS가
        영구적인 미바인딩 대기 상태로 남지 않습니다.
        """
        replaced: WebSocket | None = None
        async with self._lock:
            old = self._active_session_ws.get(session_id)
            if old is not None and old is not ws:
                logger.info(
                    "에이전트 세션 재접속 즉시 교체: session=%s old_connection_id=%s new_connection_id=%s old_fresh=%s",
                    session_id[:8],
                    self._ws_connection_id.get(id(old), "unknown"),
                    connection_id or "unknown",
                    self._ws_is_fresh(old),
                )
                old_did = self._ws_discord.get(id(old))
                if old_did is not None:
                    self._last_disconnect_at[int(old_did)] = time.monotonic()
                    self._reconnect_count[int(old_did)] = self._reconnect_count.get(int(old_did), 0) + 1
                self._drop_ws_locked(old)
                replaced = old
            self._active_session_ws[session_id] = ws
            self._session_ws[session_id] = ws
            self._session_ws_token[session_id] = ws_token
            wid = id(ws)
            self._ws_session[wid] = session_id
            self._ws_session_id[wid] = session_id
            self._ws_connection_id[wid] = connection_id or f"server-{wid:x}"
            self._ws_last_heartbeat[wid] = time.monotonic()
        if replaced is not None:
            try:
                await replaced.close(code=AGENT_SESSION_CONFLICT_CLOSE_CODE)
            except Exception:
                logger.debug("교체된 세션 WebSocket 즉시 종료 실패", exc_info=True)
        return True

    async def bind_discord(
        self,
        session_id: str,
        discord_id: int,
        profile: dict[str, str] | None = None,
        *,
        allow_takeover: bool = False,
        trust_level: str = "trusted",
    ) -> bool:
        """세션을 바인딩하되 다른 정상 연결의 인계는 명시적 OAuth만 허용합니다."""
        if trust_level not in {"trusted", "new_device"}:
            raise ValueError("invalid trust_level")
        replaced: WebSocket | None = None
        rejected = False
        async with self._lock:
            ws = self._session_ws.pop(session_id, None)
            ws_token = self._session_ws_token.pop(session_id, None)
            if ws is None or not ws_token:
                return False
            self._ws_session.pop(id(ws), None)
            prev = self._by_discord.get(discord_id)
            if (
                prev is not None
                and prev is not ws
                and self._ws_session_id.get(id(prev)) != session_id
                and self._ws_is_fresh(prev)
                and not allow_takeover
            ):
                # Saved sessions must not steal a healthy connection back from
                # another Agent. Older Agents reconnect after every close, so
                # client-side close handling alone cannot prevent the loop.
                self._drop_ws_locked(ws)
                rejected = True
                logger.info(
                    "다른 Agent 연결 유지: discord_id=%s session=%s rejected_connection_id=%s current_connection_id=%s",
                    discord_id,
                    session_id[:8],
                    self._ws_connection_id.get(id(ws), "unknown"),
                    self._ws_connection_id.get(id(prev), "unknown") if prev is not None else "-",
                )
            if not rejected:
                if prev is not None and prev is not ws:
                    self._drop_ws_locked(prev)
                    replaced = prev
                self._by_discord[discord_id] = ws
                self._ws_discord[id(ws)] = discord_id
                self._ws_trust[id(ws)] = trust_level
                pending_agent_info = self._pending_agent_info_by_ws.pop(id(ws), None)
                info = dict(pending_agent_info or {})
                if profile:
                    if profile.get("name"):
                        info["discord_name"] = profile["name"]
                    if profile.get("avatar"):
                        info["discord_avatar"] = profile["avatar"]
                if info:
                    self._agent_info[discord_id] = info
                self._connected_participant_status_locked(
                    discord_id,
                    lcu_ready=bool(info.get("lcu_ready")),
                )
                logger.info(
                    "에이전트 등록: discord_id=%s session=%s connection_id=%s replaced_connection_id=%s",
                    discord_id,
                    session_id[:8],
                    self._ws_connection_id.get(id(ws), "unknown"),
                    self._ws_connection_id.get(id(replaced), "-") if replaced is not None else "-",
                )
        if rejected:
            await ws.close(code=AGENT_SESSION_CONFLICT_CLOSE_CODE)
            return False
        if replaced is not None:
            try:
                await replaced.close(code=AGENT_SESSION_CONFLICT_CLOSE_CODE)
            except Exception:
                logger.warning("교체된 Discord Agent WebSocket 종료 실패")
        try:
            payload: dict[str, Any] = {
                "type": "session_bound",
                "discord_id": int(discord_id),
                "trust_level": trust_level,
            }
            if profile:
                if profile.get("name"):
                    payload["discord_name"] = profile["name"]
                if profile.get("avatar"):
                    payload["discord_avatar"] = profile["avatar"]
            await ws.send_json(payload)
            await self.sync_live_game_polling(discord_id)
        except Exception:
            logger.exception("session_bound 전송 실패 discord_id=%s", discord_id)
        return True

    async def unregister_ws(self, ws: WebSocket) -> tuple[int, dict[str, Any]] | None:
        offline_id: int | None = None
        offline_payload: dict[str, Any] | None = None
        async with self._lock:
            wid = id(ws)
            did = self._ws_discord.get(wid)
            if did is not None and self._by_discord.get(int(did)) is ws:
                offline_id = int(did)
                self._last_disconnect_at[offline_id] = time.monotonic()
                offline_payload = self._offline_participant_status_locked(offline_id)
            self._drop_ws_locked(ws)
        if offline_id is not None and offline_payload is not None:
            return offline_id, offline_payload
        return None

    def _drop_ws_locked(self, ws: WebSocket) -> None:
        wid = id(ws)
        did = self._ws_discord.pop(wid, None)
        self._ws_trust.pop(wid, None)
        self._pending_agent_info_by_ws.pop(wid, None)
        if did is not None:
            if self._by_discord.get(did) is ws:
                del self._by_discord[did]
            self._agent_info.pop(did, None)
            self._gameflow.pop(did, None)
            self._live_game.pop(did, None)
            self._cancel_pending_for_discord_locked(did)
            logger.info("에이전트 해제: discord_id=%s", did)
        sid = self._ws_session.pop(wid, None)
        if sid is not None and self._session_ws.get(sid) is ws:
            del self._session_ws[sid]
            self._session_ws_token.pop(sid, None)
        for session_id, active_ws in list(self._active_session_ws.items()):
            if active_ws is ws:
                del self._active_session_ws[session_id]
        self._ws_session_id.pop(wid, None)
        self._ws_connection_id.pop(wid, None)
        self._ws_last_heartbeat.pop(wid, None)

    def session_ws_token(self, session_id: str) -> str | None:
        return self._session_ws_token.get(session_id)

    def has_active_session_ws(self, session_id: str) -> bool:
        """OAuth 시작 전 에이전트 WS 가 붙어 있는지."""
        return session_id in self._active_session_ws

    def ws_session_id(self, ws: WebSocket) -> str | None:
        return self._ws_session_id.get(id(ws))

    def ws_connection_id(self, ws: WebSocket) -> str | None:
        return self._ws_connection_id.get(id(ws))

    def _cancel_pending_for_discord_locked(self, discord_id: int) -> None:
        for key in [k for k in self._pending_results if k[0] == discord_id]:
            fut = self._pending_results.pop(key, None)
            if fut is not None and not fut.done():
                fut.cancel()

    def mark_agent_activity(self, ws: WebSocket) -> None:
        """Refresh liveness for any successfully received Agent message.

        Heartbeat pings are the normal source of activity, but Agent work such as
        LCU phase transitions may briefly delay the dedicated ping branch.  A
        diagnostic/event received on the same WebSocket is equally strong proof
        that the connection is alive and must prevent a false stale disconnect.
        """
        wid = id(ws)
        if wid in self._ws_session_id or wid in self._ws_discord:
            self._ws_last_heartbeat[wid] = time.monotonic()

    def mark_agent_heartbeat(self, ws: WebSocket) -> None:
        # Compatibility alias for callers/tests that still describe activity as
        # a heartbeat. New receive paths should use mark_agent_activity().
        self.mark_agent_activity(ws)

    def _ws_is_fresh(self, ws: WebSocket, *, now: float | None = None) -> bool:
        last = self._ws_last_heartbeat.get(id(ws))
        if last is None:
            return False
        current = time.monotonic() if now is None else now
        return current - last <= AGENT_HEARTBEAT_STALE_SEC

    def is_online(self, discord_id: int) -> bool:
        ws = self._by_discord.get(int(discord_id))
        return ws is not None and self._ws_is_fresh(ws)

    async def set_agent_info_for_ws(self, ws: WebSocket, info: dict[str, Any]) -> int | None:
        """OAuth 바인딩 전 hello는 WS 단위로 보관했다가 bind_discord에서 이전한다."""
        async with self._lock:
            discord_id = self._ws_discord.get(id(ws))
            if discord_id is None:
                self._pending_agent_info_by_ws[id(ws)] = dict(info)
                return None
            merged = dict(info)
            previous = self._agent_info.get(discord_id) or {}
            for key in ("discord_name", "discord_avatar"):
                if key in previous and key not in merged:
                    merged[key] = previous[key]
            self._agent_info[discord_id] = merged
            self._connected_participant_status_locked(
                discord_id,
                lcu_ready=bool(merged.get("lcu_ready")),
            )
            return discord_id

    def agent_info(self, discord_id: int) -> dict[str, Any] | None:
        row = self._agent_info.get(int(discord_id))
        return dict(row) if row else None

    def discord_id_for_ws(self, ws: WebSocket) -> int | None:
        return self._ws_discord.get(id(ws))

    def trust_level_for_discord(self, discord_id: int) -> str | None:
        ws = self._by_discord.get(int(discord_id))
        if ws is None or not self._ws_is_fresh(ws):
            return None
        return self._ws_trust.get(id(ws), "trusted")

    def active_new_device_logins(self) -> list[tuple[int, str, dict[str, Any]]]:
        rows: list[tuple[int, str, dict[str, Any]]] = []
        for discord_id, ws in self._by_discord.items():
            wid = id(ws)
            if not self._ws_is_fresh(ws) or self._ws_trust.get(wid) != "new_device":
                continue
            session_id = self._ws_session_id.get(wid)
            if not session_id:
                continue
            rows.append((discord_id, session_id, dict(self._agent_info.get(discord_id) or {})))
        return rows

    def is_current_ws(self, ws: WebSocket) -> bool:
        """Whether this socket is still the authoritative socket for its Discord id/session."""
        wid = id(ws)
        did = self._ws_discord.get(wid)
        if did is not None and self._by_discord.get(did) is not ws:
            return False
        sid = self._ws_session_id.get(wid)
        if sid is not None and self._active_session_ws.get(sid) is not ws:
            return False
        return did is not None or sid is not None

    def connection_quality(self, discord_id: int) -> dict[str, Any]:
        did = int(discord_id)
        now = time.monotonic()
        ws = self._by_discord.get(did)
        if ws is not None:
            last = self._ws_last_heartbeat.get(id(ws))
            age = None if last is None else max(0.0, now - last)
            if age is None or age > AGENT_QUALITY_DEGRADED_SEC:
                quality = "stale"
            elif age > AGENT_QUALITY_HEALTHY_SEC:
                quality = "degraded"
            else:
                quality = "healthy"
            return {
                "quality": quality,
                "connection_id": self._ws_connection_id.get(id(ws)),
                "last_activity_age_ms": int(age * 1000) if age is not None else None,
                "reconnect_count": self._reconnect_count.get(did, 0),
            }
        disconnected = self._last_disconnect_at.get(did)
        disconnect_age = None if disconnected is None else max(0.0, now - disconnected)
        quality = (
            "reconnecting"
            if disconnect_age is not None and disconnect_age <= AGENT_RECONNECTING_WINDOW_SEC
            else "offline"
        )
        return {
            "quality": quality,
            "connection_id": None,
            "last_activity_age_ms": None,
            "last_disconnect_age_ms": int(disconnect_age * 1000) if disconnect_age is not None else None,
            "reconnect_count": self._reconnect_count.get(did, 0),
        }

    async def force_reconnect(self, discord_id: int, reason: str = "operator_requested") -> tuple[bool, str]:
        did = int(discord_id)
        now = time.monotonic()
        async with self._lock:
            ws = self._by_discord.get(did)
            if ws is None:
                return False, "offline"
            last = self._force_reconnect_last_at.get(did, 0.0)
            if now - last < FORCE_RECONNECT_COOLDOWN_SEC:
                return False, "cooldown"
            self._force_reconnect_last_at[did] = now
            connection_id = self._ws_connection_id.get(id(ws), "unknown")
        payload = {
            "type": "force_reconnect",
            "reason": str(reason or "operator_requested")[:160],
        }
        try:
            await asyncio.wait_for(ws.send_json(payload), timeout=AGENT_WS_SEND_TIMEOUT_SEC)
            logger.warning(
                "Agent force reconnect 전송 discord_id=%s connection_id=%s reason=%s",
                did, connection_id, payload["reason"],
            )
            return True, connection_id
        except Exception:
            logger.exception("Agent force reconnect 전송 실패 discord_id=%s", did)
            return False, "send_failed"

    def register_pending_result(self, discord_id: int, request_id: str) -> asyncio.Future[dict[str, Any]]:
        fut: asyncio.Future[dict[str, Any]] = asyncio.get_running_loop().create_future()
        self._pending_results[(discord_id, request_id)] = fut
        return fut

    def complete_pending_result(self, discord_id: int, request_id: str, result: dict[str, Any]) -> bool:
        fut = self._pending_results.pop((discord_id, request_id), None)
        if fut is None or fut.done():
            return False
        fut.set_result(result)
        return True

    def cancel_pending_result(self, discord_id: int, request_id: str) -> None:
        fut = self._pending_results.pop((discord_id, request_id), None)
        if fut is not None and not fut.done():
            fut.cancel()

    def register_pending_bot_eog(self, event_id: str) -> asyncio.Future[bool]:
        existing = self._pending_bot_eog.pop(event_id, None)
        if existing is not None and not existing.done():
            existing.set_result(False)
        fut: asyncio.Future[bool] = asyncio.get_running_loop().create_future()
        self._pending_bot_eog[event_id] = fut
        return fut

    def complete_pending_bot_eog(self, event_id: str, ok: bool) -> bool:
        fut = self._pending_bot_eog.pop(event_id, None)
        if fut is None or fut.done():
            return False
        fut.set_result(bool(ok))
        return True

    def cancel_pending_bot_eog(self, event_id: str) -> None:
        fut = self._pending_bot_eog.pop(event_id, None)
        if fut is not None and not fut.done():
            fut.set_result(False)

    async def send_command(self, discord_id: int, payload: dict[str, Any]) -> bool:
        async with self._lock:
            ws = self._by_discord.get(int(discord_id))
            fresh = ws is not None and self._ws_is_fresh(ws)
        if ws is None or not fresh:
            return False
        try:
            await asyncio.wait_for(ws.send_json(payload), timeout=AGENT_WS_SEND_TIMEOUT_SEC)
            return True
        except Exception:
            logger.exception("WS send 실패 discord_id=%s", discord_id)
            await self.unregister_ws(ws)
            try:
                await ws.close(code=1001)
            except Exception:
                pass
            return False

    async def send_bot_event(self, payload: dict[str, Any]) -> bool:
        async with self._lock:
            ws = self._bot_ws
        if ws is None:
            return False
        try:
            await _send_bot_json(ws, payload)
            return True
        except Exception:
            logger.exception("봇 WS event 전달 실패 type=%s", payload.get("type"))
            await self.unregister_bot_ws(ws)
            return False

    async def register_bot_ws(self, ws: WebSocket) -> None:
        async with self._lock:
            if self._bot_ws is not None and self._bot_ws is not ws:
                logger.info("봇 WS 교체 — 이전 연결 해제")
            self._bot_ws = ws
            logger.info("봇 WS 등록 (party 구독 %s건)", len(self._party_subscribers))

    async def unregister_bot_ws(self, ws: WebSocket) -> None:
        async with self._lock:
            if self._bot_ws is ws:
                self._bot_ws = None
                for event_id, fut in list(self._pending_bot_eog.items()):
                    if not fut.done():
                        fut.set_result(False)
                    self._pending_bot_eog.pop(event_id, None)
                logger.info("봇 WS 해제")

    def subscribe_party_lobby(self, discord_id: int) -> None:
        self._party_subscribers.add(int(discord_id))
        logger.debug("party 구독 + discord_id=%s (총 %s)", discord_id, len(self._party_subscribers))

    def unsubscribe_party_lobby(self, discord_id: int) -> None:
        self._party_subscribers.discard(int(discord_id))
        logger.debug("party 구독 - discord_id=%s (총 %s)", discord_id, len(self._party_subscribers))

    def is_party_subscribed(self, discord_id: int) -> bool:
        return int(discord_id) in self._party_subscribers

    def party_subscribers_snapshot(self) -> set[int]:
        return set(self._party_subscribers)

    def bot_ws_connected(self) -> bool:
        return self._bot_ws is not None

    def subscribe_gameflow(self, discord_id: int) -> None:
        did = int(discord_id)
        self._gameflow_subscribers.add(did)
        self._gameflow_eog_grace_until.pop(did, None)

    def unsubscribe_gameflow(self, discord_id: int) -> None:
        did = int(discord_id)
        self._gameflow_subscribers.discard(did)
        # Stop normal gameflow/live polling immediately, but allow an already-running
        # game to deliver its slightly-late EOG for a bounded window.
        self._gameflow_eog_grace_until[did] = time.monotonic() + GAMEFLOW_EOG_GRACE_SEC
        self._clear_live_game_cache_if_unsubscribed(did)

    def live_game_polling_required(self, discord_id: int) -> bool:
        did = int(discord_id)
        return did in self._live_game_subscribers or did in self._gameflow_subscribers

    async def sync_live_game_polling(self, discord_id: int) -> bool:
        did = int(discord_id)
        enabled = self.live_game_polling_required(did)
        sent = await self.send_command(
            did,
            {"type": "live_game_polling", "enabled": enabled},
        )
        logger.info(
            "live_game polling 상태 동기화: discord_id=%s enabled=%s 전송=%s",
            did,
            enabled,
            sent,
        )
        return sent

    def _clear_live_game_cache_if_unsubscribed(self, discord_id: int) -> None:
        did = int(discord_id)
        if not self.live_game_polling_required(did):
            self._live_game.pop(did, None)

    def subscribe_live_game(self, discord_id: int, recruitment_id: str | None = None) -> None:
        did = int(discord_id)
        self._live_game_subscribers.add(did)
        self._live_game_recruitments[did] = recruitment_id
        logger.info(
            "live_game 구독 등록: discord_id=%s 대상=사용자 단위 모집_id=%s 총=%s",
            did,
            recruitment_id or "없음",
            len(self._live_game_subscribers),
        )

    def unsubscribe_live_game(self, discord_id: int) -> None:
        did = int(discord_id)
        self._live_game_subscribers.discard(did)
        recruitment_id = self._live_game_recruitments.pop(did, None)
        logger.info(
            "live_game 구독 해제: discord_id=%s 대상=사용자 단위 모집_id=%s 총=%s",
            did,
            recruitment_id or "없음",
            len(self._live_game_subscribers),
        )
        self._clear_live_game_cache_if_unsubscribed(did)

    def subscribe_match_dm(self, discord_id: int) -> None:
        self._match_dm_subscribers.add(int(discord_id))

    def unsubscribe_match_dm(self, discord_id: int) -> None:
        self._match_dm_subscribers.discard(int(discord_id))

    def subscribe_participant_status(self, discord_id: int) -> None:
        self._participant_status_subscribers.add(int(discord_id))
        logger.debug(
            "participant_status 구독 + discord_id=%s (총 %s)",
            discord_id,
            len(self._participant_status_subscribers),
        )

    def unsubscribe_participant_status(self, discord_id: int) -> None:
        self._participant_status_subscribers.discard(int(discord_id))
        logger.debug(
            "participant_status 구독 - discord_id=%s (총 %s)",
            discord_id,
            len(self._participant_status_subscribers),
        )

    def participant_status_subscribers_snapshot(self) -> set[int]:
        return set(self._participant_status_subscribers)

    def _connected_participant_status_locked(
        self, discord_id: int, *, lcu_ready: bool
    ) -> dict[str, Any]:
        payload = {
            "status": "waiting",
            "phase": "None",
            "game_started_at_ms": None,
            "lcu_ready": bool(lcu_ready),
            "agent_online": True,
            "updated_at": time.time(),
        }
        self._participant_status[int(discord_id)] = dict(payload)
        return payload

    def _offline_participant_status_locked(self, discord_id: int) -> dict[str, Any]:
        payload = {
            "status": "offline",
            "phase": "None",
            "game_started_at_ms": None,
            "lcu_ready": False,
            "agent_online": False,
            "updated_at": time.time(),
        }
        self._participant_status[int(discord_id)] = dict(payload)
        return payload

    def set_participant_status(self, discord_id: int, data: dict[str, Any]) -> dict[str, Any]:
        started_raw = data.get("game_started_at_ms")
        if started_raw is None:
            started_raw = data.get("game_started_at")
        payload = {
            "status": str(data.get("status") or "waiting"),
            "phase": data.get("phase"),
            "game_started_at_ms": None,
            "lcu_ready": bool(data.get("lcu_ready")),
            "agent_online": bool(data.get("agent_online", True)),
            "updated_at": time.time(),
        }
        if started_raw is not None:
            try:
                payload["game_started_at_ms"] = int(started_raw)
            except (TypeError, ValueError):
                payload["game_started_at_ms"] = None
        did = int(discord_id)
        self._participant_status[did] = payload
        agent = self._agent_info.get(did)
        if agent is not None:
            agent["lcu_ready"] = payload["lcu_ready"]
        return payload

    def get_participant_status(self, discord_id: int) -> dict[str, Any] | None:
        row = self._participant_status.get(int(discord_id))
        return dict(row) if row else None

    def get_participant_statuses(self, discord_ids: list[int]) -> dict[int, dict[str, Any]]:
        out: dict[int, dict[str, Any]] = {}
        for raw_id in discord_ids:
            did = int(raw_id)
            cached = self._participant_status.get(did)
            if cached is not None:
                out[did] = dict(cached)
            elif not self.is_online(did):
                out[did] = {
                    "status": "offline",
                    "phase": "None",
                    "game_started_at_ms": None,
                    "lcu_ready": False,
                    "agent_online": False,
                    "updated_at": 0.0,
                }
        return out

    def set_live_game(self, discord_id: int, data: dict[str, Any]) -> None:
        self._live_game[int(discord_id)] = (time.monotonic(), dict(data))

    def get_live_game(self, discord_id: int) -> dict[str, Any] | None:
        cached = self._live_game.get(int(discord_id))
        if cached is None:
            return None
        received_at, data = cached
        age = time.monotonic() - received_at
        if age > LIVE_GAME_CACHE_TTL_SEC:
            self._live_game.pop(int(discord_id), None)
            return None
        return {
            "data": dict(data),
            "age_ms": max(0, int(age * 1000)),
        }

    async def forward_participant_status_update(
        self, discord_id: int, data: dict[str, Any]
    ) -> bool:
        async with self._lock:
            if int(discord_id) not in self._participant_status_subscribers:
                return False
            ws = self._bot_ws
        if ws is None:
            return False
        try:
            await _send_bot_json(
                ws,
                {
                    "type": "participant_status_update",
                    "discord_id": int(discord_id),
                    "data": data,
                }
            )
            return True
        except Exception:
            logger.exception(
                "봇 WS participant_status_update 전달 실패 discord_id=%s", discord_id
            )
            await self.unregister_bot_ws(ws)
            return False

    async def forward_champ_select_update(self, discord_id: int, data: dict[str, Any]) -> bool:
        async with self._lock:
            if int(discord_id) not in (self._match_dm_subscribers | self._gameflow_subscribers):
                return False
            ws = self._bot_ws
        if ws is None:
            return False
        try:
            await _send_bot_json(
                ws,
                {
                    "type": "champ_select_update",
                    "discord_id": int(discord_id),
                    "data": data,
                }
            )
            return True
        except Exception:
            logger.exception(
                "봇 WS champ_select_update 전달 실패 discord_id=%s", discord_id
            )
            await self.unregister_bot_ws(ws)
            return False

    async def forward_ready_check_update(self, discord_id: int, data: dict[str, Any]) -> bool:
        async with self._lock:
            if int(discord_id) not in self._match_dm_subscribers:
                return False
            ws = self._bot_ws
        if ws is None:
            return False
        try:
            await _send_bot_json(
                ws,
                {
                    "type": "ready_check_update",
                    "discord_id": int(discord_id),
                    "data": data,
                }
            )
            return True
        except Exception:
            logger.exception("봇 WS ready_check_update 전달 실패 discord_id=%s", discord_id)
            await self.unregister_bot_ws(ws)
            return False

    async def replay_live_game_update(self, discord_id: int) -> bool:
        """늦은 Bot 구독에 마지막 live-game 스냅샷을 즉시 재전송합니다."""
        did = int(discord_id)
        async with self._lock:
            if did not in self._live_game_subscribers:
                return False
            cached = self.get_live_game(did)
            ws = self._bot_ws
        if cached is None or ws is None:
            return False
        data = cached.get("data")
        if not isinstance(data, dict):
            return False
        try:
            await _send_bot_json(
                ws,
                {
                    "type": "live_game_update",
                    "discord_id": did,
                    "data": dict(data),
                }
            )
            logger.info(
                "봇 WS live_game_update 캐시 재전송 discord_id=%s age_ms=%s",
                did,
                cached.get("age_ms"),
            )
            return True
        except Exception:
            logger.exception("봇 WS live_game_update 캐시 재전송 실패 discord_id=%s", did)
            await self.unregister_bot_ws(ws)
            return False

    async def replay_gameflow_update(self, discord_id: int) -> bool:
        """늦은 Bot 구독에 마지막 gameflow 스냅샷을 즉시 재전송합니다."""
        did = int(discord_id)
        async with self._lock:
            if did not in self._gameflow_subscribers:
                return False
            cached = self._gameflow.get(did)
            ws = self._bot_ws
        if cached is None or ws is None:
            return False
        try:
            await _send_bot_json(
                ws,
                {
                    "type": "gameflow_update",
                    "discord_id": did,
                    "data": dict(cached),
                }
            )
            logger.debug("봇 WS gameflow_update 캐시 재전송 discord_id=%s", did)
            return True
        except Exception:
            logger.exception("봇 WS gameflow_update 캐시 재전송 실패 discord_id=%s", did)
            await self.unregister_bot_ws(ws)
            return False

    async def forward_gameflow_update(self, discord_id: int, data: dict[str, Any]) -> bool:
        did = int(discord_id)
        async with self._lock:
            # Bot 구독보다 Agent phase 전환이 먼저 도착할 수 있으므로 항상 캐시합니다.
            self._gameflow[did] = dict(data)
            if data.get("lcu_ready") is True:
                agent = self._agent_info.get(did)
                if agent is not None:
                    agent["lcu_ready"] = True
            if did not in self._gameflow_subscribers:
                return False
            ws = self._bot_ws
        if ws is None:
            return False
        try:
            await _send_bot_json(
                ws,
                {
                    "type": "gameflow_update",
                    "discord_id": did,
                    "data": data,
                }
            )
            return True
        except Exception:
            logger.exception("봇 WS gameflow_update 전달 실패 discord_id=%s", did)
            await self.unregister_bot_ws(ws)
            return False

    async def forward_guild_match_eog(
        self, discord_id: int, data: dict[str, Any], event_id: str | None = None
    ) -> bool:
        """내전 EOG 결과와 durable event id를 gameflow 구독 중인 Bot에 전달합니다."""
        did = int(discord_id)
        async with self._lock:
            if did not in self._gameflow_subscribers:
                grace_until = self._gameflow_eog_grace_until.get(did, 0.0)
                if grace_until <= time.monotonic():
                    self._gameflow_eog_grace_until.pop(did, None)
                    return False
            ws = self._bot_ws
        if ws is None:
            return False
        try:
            payload: dict[str, Any] = {
                "type": "guild_match_eog",
                "discord_id": int(discord_id),
                "data": data,
            }
            if event_id is not None:
                payload["event_id"] = event_id
            await _send_bot_json(ws, payload)
            return True
        except Exception:
            logger.exception("봇 WS guild_match_eog 전달 실패 discord_id=%s", discord_id)
            await self.unregister_bot_ws(ws)
            return False

    async def forward_live_game_update(self, discord_id: int, data: dict[str, Any]) -> bool:
        did = int(discord_id)
        game_id = (data.get("game") or {}).get("id") if isinstance(data.get("game"), dict) else None
        participant_count = len(data.get("participants", [])) if isinstance(data.get("participants"), list) else 0
        event_count = len(data.get("events", [])) if isinstance(data.get("events"), list) else 0
        if participant_count != LIVE_GAME_PARTICIPANT_COUNT:
            logger.info(
                "live_game_update 폐기: discord_id=%s game_id=%s participants=%s 사유=10명 미만 또는 초과",
                did,
                game_id,
                participant_count,
            )
            return False
        async with self._lock:
            self.set_live_game(did, data)
            recruitment_id = self._live_game_recruitments.get(did)
            # 기존 gameflow 구독 봇도 별도 구독 명령 없이 실시간 경기 정보를 받을 수 있게
            # 하되, 어느 이벤트에도 구독하지 않은 세션으로는 전달하지 않습니다.
            if (
                did not in self._live_game_subscribers
                and did not in self._gameflow_subscribers
            ):
                logger.info(
                    "live_game_update 저장만 함: discord_id=%s game_id=%s participants=%s events=%s "
                    "전송=아니오 사유=구독자없음 모집_id=%s",
                    did,
                    game_id,
                    participant_count,
                    event_count,
                    recruitment_id or "없음",
                )
                return False
            ws = self._bot_ws
        if ws is None:
            logger.warning(
                "live_game_update 전송 불가: discord_id=%s game_id=%s participants=%s events=%s "
                "전송=아니오 사유=봇WS없음 모집_id=%s",
                did,
                game_id,
                participant_count,
                event_count,
                recruitment_id or "없음",
            )
            return False
        try:
            await _send_bot_json(
                ws,
                {
                    "type": "live_game_update",
                    "discord_id": did,
                    "data": data,
                }
            )
            logger.info(
                "live_game_update 전송 완료: discord_id=%s game_id=%s participants=%s events=%s "
                "전송=봇WS 모집_id=%s 대상=사용자 구독",
                did,
                game_id,
                participant_count,
                event_count,
                recruitment_id or "없음",
            )
            return True
        except Exception:
            logger.exception(
                "봇 WS live_game_update 전달 실패: discord_id=%s game_id=%s 모집_id=%s",
                did,
                game_id,
                recruitment_id or "없음",
            )
            await self.unregister_bot_ws(ws)
            return False

    async def forward_party_lobby_update(self, discord_id: int, data: dict[str, Any]) -> bool:
        async with self._lock:
            if int(discord_id) not in self._party_subscribers:
                return False
            ws = self._bot_ws
        if ws is None:
            return False
        try:
            await _send_bot_json(
                ws,
                {
                    "type": "party_lobby_update",
                    "discord_id": int(discord_id),
                    "data": data,
                }
            )
            return True
        except Exception:
            logger.exception("봇 WS party_lobby_update 전달 실패 discord_id=%s", discord_id)
            await self.unregister_bot_ws(ws)
            return False
