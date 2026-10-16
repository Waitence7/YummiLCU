import asyncio
import time
import unittest
from unittest.mock import patch

from relay.connections import ConnectionManager


class _WebSocketStub:
    def __init__(self) -> None:
        self.payloads: list[dict[str, object]] = []
        self.close_codes: list[int] = []

    async def send_json(self, payload: dict[str, object]) -> None:
        self.payloads.append(payload)

    async def close(self, code: int = 1000) -> None:
        self.close_codes.append(code)


class _HangingWebSocketStub(_WebSocketStub):
    async def send_json(self, payload: dict[str, object]) -> None:
        await asyncio.sleep(60)


class AgentConnectionFreshnessTests(unittest.IsolatedAsyncioTestCase):
    async def _bound(self, websocket: _WebSocketStub) -> ConnectionManager:
        manager = ConnectionManager()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 1234))
        return manager

    async def test_online_requires_fresh_heartbeat(self) -> None:
        websocket = _WebSocketStub()
        manager = await self._bound(websocket)
        self.assertTrue(manager.is_online(1234))

        manager._ws_last_heartbeat[id(websocket)] = time.monotonic() - 76.0
        self.assertFalse(manager.is_online(1234))
        self.assertFalse(await manager.send_command(1234, {"type": "test"}))

        manager.mark_agent_heartbeat(websocket)
        self.assertTrue(manager.is_online(1234))
        self.assertTrue(await manager.send_command(1234, {"type": "test"}))

    async def test_send_timeout_unregisters_stuck_agent(self) -> None:
        websocket = _HangingWebSocketStub()
        manager = ConnectionManager()
        await manager.attach_session("session-1", websocket, "token")
        # Bind manually to avoid intentionally hanging during session_bound delivery.
        manager._session_ws.pop("session-1")
        manager._session_ws_token.pop("session-1")
        manager._ws_session.pop(id(websocket), None)
        manager._by_discord[1234] = websocket
        manager._ws_discord[id(websocket)] = 1234

        with patch("relay.connections.AGENT_WS_SEND_TIMEOUT_SEC", 0.01):
            self.assertFalse(await manager.send_command(1234, {"type": "test"}))

        self.assertFalse(manager.is_online(1234))
        self.assertEqual(websocket.close_codes, [1001])
