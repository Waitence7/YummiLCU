# region [Imports]
"""discord_id ↔ WebSocket 매핑 (Relay 프로세스 메모리)."""

from __future__ import annotations

import asyncio
import logging
from typing import Any

from fastapi import WebSocket

logger = logging.getLogger("yummi_lcu.relay.connections")
# endregion


class ConnectionManager:
    """에이전트 WebSocket 연결을 discord_id로 라우팅합니다."""

    def __init__(self) -> None:
        self._by_discord: dict[int, WebSocket] = {}
        self._session_ws: dict[str, WebSocket] = {}
        self._ws_discord: dict[int, int] = {}  # id(ws) -> discord_id
        self._ws_session: dict[int, str] = {}  # id(ws) -> session_id (인증 전)
        self._pending_results: dict[tuple[int, str], asyncio.Future[dict[str, Any]]] = {}
        self._lock = asyncio.Lock()

    async def attach_session(self, session_id: str, ws: WebSocket) -> None:
        """OAuth 완료 전 session_id로 WS를 임시 보관합니다."""
        async with self._lock:
            old = self._session_ws.get(session_id)
            if old is not None and old is not ws:
                self._drop_ws_locked(old)
            self._session_ws[session_id] = ws
            self._ws_session[id(ws)] = session_id

    async def bind_discord(self, session_id: str, discord_id: int) -> bool:
        """session WS를 discord_id에 등록합니다. 성공 시 True."""
        async with self._lock:
            ws = self._session_ws.pop(session_id, None)
            if ws is None:
                return False
            self._ws_session.pop(id(ws), None)
            prev = self._by_discord.get(discord_id)
            if prev is not None and prev is not ws:
                self._drop_ws_locked(prev)
            self._by_discord[discord_id] = ws
            self._ws_discord[id(ws)] = discord_id
            logger.info("에이전트 등록: discord_id=%s session=%s", discord_id, session_id[:8])
            return True

    async def unregister_ws(self, ws: WebSocket) -> None:
        async with self._lock:
            self._drop_ws_locked(ws)

    def _drop_ws_locked(self, ws: WebSocket) -> None:
        wid = id(ws)
        did = self._ws_discord.pop(wid, None)
        if did is not None:
            if self._by_discord.get(did) is ws:
                del self._by_discord[did]
            self._cancel_pending_for_discord_locked(did)
            logger.info("에이전트 해제: discord_id=%s", did)
        sid = self._ws_session.pop(wid, None)
        if sid is not None and self._session_ws.get(sid) is ws:
            del self._session_ws[sid]

    def _cancel_pending_for_discord_locked(self, discord_id: int) -> None:
        for key in [k for k in self._pending_results if k[0] == discord_id]:
            fut = self._pending_results.pop(key, None)
            if fut is not None and not fut.done():
                fut.cancel()

    def is_online(self, discord_id: int) -> bool:
        return discord_id in self._by_discord

    def discord_id_for_ws(self, ws: WebSocket) -> int | None:
        return self._ws_discord.get(id(ws))

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

    async def send_command(self, discord_id: int, payload: dict[str, Any]) -> bool:
        async with self._lock:
            ws = self._by_discord.get(discord_id)
        if ws is None:
            return False
        try:
            await ws.send_json(payload)
            return True
        except Exception:
            logger.exception("WS send 실패 discord_id=%s", discord_id)
            await self.unregister_ws(ws)
            return False
