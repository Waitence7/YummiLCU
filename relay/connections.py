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
    """에이전트·봇 WebSocket 연결을 discord_id로 라우팅합니다."""

    def __init__(self) -> None:
        self._by_discord: dict[int, WebSocket] = {}
        self._session_ws: dict[str, WebSocket] = {}
        self._session_ws_token: dict[str, str] = {}  # session_id -> ws_token (에이전트만 보유)
        self._ws_discord: dict[int, int] = {}  # id(ws) -> discord_id
        self._ws_session: dict[int, str] = {}  # id(ws) -> session_id (인증 전)
        self._ws_session_id: dict[int, str] = {}  # id(ws) -> session_id (TTL 갱신용)
        self._pending_results: dict[tuple[int, str], asyncio.Future[dict[str, Any]]] = {}
        self._bot_ws: WebSocket | None = None
        self._party_subscribers: set[int] = set()  # creator discord_id
        self._gameflow_subscribers: set[int] = set()
        self._agent_info: dict[int, dict[str, Any]] = {}
        self._lock = asyncio.Lock()

    async def attach_session(self, session_id: str, ws: WebSocket, ws_token: str) -> None:
        """OAuth 완료 전 session_id + ws_token 으로 WS를 임시 보관합니다."""
        async with self._lock:
            old = self._session_ws.get(session_id)
            if old is not None and old is not ws:
                self._drop_ws_locked(old)
            self._session_ws[session_id] = ws
            self._session_ws_token[session_id] = ws_token
            wid = id(ws)
            self._ws_session[wid] = session_id
            self._ws_session_id[wid] = session_id

    async def bind_discord(self, session_id: str, discord_id: int) -> bool:
        """ws_token 이 등록된 session WS만 discord_id에 바인딩합니다."""
        async with self._lock:
            ws = self._session_ws.pop(session_id, None)
            ws_token = self._session_ws_token.pop(session_id, None)
            if ws is None or not ws_token:
                return False
            self._ws_session.pop(id(ws), None)
            prev = self._by_discord.get(discord_id)
            if prev is not None and prev is not ws:
                self._drop_ws_locked(prev)
            self._by_discord[discord_id] = ws
            self._ws_discord[id(ws)] = discord_id
            logger.info("에이전트 등록: discord_id=%s session=%s", discord_id, session_id[:8])
        try:
            await ws.send_json({"type": "session_bound", "discord_id": int(discord_id)})
        except Exception:
            logger.exception("session_bound 전송 실패 discord_id=%s", discord_id)
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
            self._agent_info.pop(did, None)
            self._cancel_pending_for_discord_locked(did)
            logger.info("에이전트 해제: discord_id=%s", did)
        sid = self._ws_session.pop(wid, None)
        if sid is not None and self._session_ws.get(sid) is ws:
            del self._session_ws[sid]
            self._session_ws_token.pop(sid, None)
        self._ws_session_id.pop(wid, None)

    def session_ws_token(self, session_id: str) -> str | None:
        return self._session_ws_token.get(session_id)

    def ws_session_id(self, ws: WebSocket) -> str | None:
        return self._ws_session_id.get(id(ws))

    def _cancel_pending_for_discord_locked(self, discord_id: int) -> None:
        for key in [k for k in self._pending_results if k[0] == discord_id]:
            fut = self._pending_results.pop(key, None)
            if fut is not None and not fut.done():
                fut.cancel()

    def is_online(self, discord_id: int) -> bool:
        return discord_id in self._by_discord

    def set_agent_info(self, discord_id: int, info: dict[str, Any]) -> None:
        self._agent_info[int(discord_id)] = dict(info)

    def agent_info(self, discord_id: int) -> dict[str, Any] | None:
        row = self._agent_info.get(int(discord_id))
        return dict(row) if row else None

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
        self._gameflow_subscribers.add(int(discord_id))

    def unsubscribe_gameflow(self, discord_id: int) -> None:
        self._gameflow_subscribers.discard(int(discord_id))

    async def forward_gameflow_update(self, discord_id: int, data: dict[str, Any]) -> bool:
        async with self._lock:
            if int(discord_id) not in self._gameflow_subscribers:
                return False
            ws = self._bot_ws
        if ws is None:
            return False
        try:
            await ws.send_json(
                {
                    "type": "gameflow_update",
                    "discord_id": int(discord_id),
                    "data": data,
                }
            )
            return True
        except Exception:
            logger.exception("봇 WS gameflow_update 전달 실패 discord_id=%s", discord_id)
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
            await ws.send_json(
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
