import asyncio
import json
import time
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from relay.app import ws_agent
from relay.connections import (
    AGENT_HEARTBEAT_STALE_SEC,
    AGENT_SESSION_CONFLICT_CLOSE_CODE,
    ConnectionManager,
)


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

        manager._ws_last_heartbeat[id(websocket)] = (
            time.monotonic() - AGENT_HEARTBEAT_STALE_SEC - 1.0
        )
        self.assertFalse(manager.is_online(1234))
        self.assertFalse(await manager.send_command(1234, {"type": "test"}))

        manager.mark_agent_heartbeat(websocket)
        self.assertTrue(manager.is_online(1234))
        self.assertTrue(await manager.send_command(1234, {"type": "test"}))


    async def test_any_agent_activity_refreshes_connection_freshness(self) -> None:
        websocket = _WebSocketStub()
        manager = await self._bound(websocket)
        manager._ws_last_heartbeat[id(websocket)] = (
            time.monotonic() - AGENT_HEARTBEAT_STALE_SEC - 1.0
        )
        self.assertFalse(manager.is_online(1234))

        manager.mark_agent_activity(websocket)

        self.assertTrue(manager.is_online(1234))

    async def test_competing_saved_session_retries_cannot_displace_healthy_agent(self) -> None:
        active = _WebSocketStub()
        manager = await self._bound(active)
        await manager.set_agent_info_for_ws(active, {"version": "0.7.23", "lcu_ready": True})
        before = manager.get_participant_status(1234)

        # Legacy Agents reconnect after every close. Repeated attempts must
        # leave routing, LCU state, and the active socket untouched.
        for _ in range(10):
            duplicate = _WebSocketStub()
            await manager.attach_session("session-2", duplicate, "token-2")
            self.assertFalse(await manager.bind_discord("session-2", 1234))
            self.assertEqual(duplicate.close_codes, [AGENT_SESSION_CONFLICT_CLOSE_CODE])
            self.assertIsNone(await manager.unregister_ws(duplicate))
            self.assertFalse(manager.has_active_session_ws("session-2"))
            self.assertIsNone(manager.ws_session_id(duplicate))
            self.assertIsNone(manager.session_ws_token("session-2"))

        self.assertEqual(active.close_codes, [])
        self.assertEqual(manager.discord_id_for_ws(active), 1234)
        self.assertEqual(manager.get_participant_status(1234), before)
        self.assertTrue(manager.agent_info(1234)["lcu_ready"])
        self.assertTrue(await manager.send_command(1234, {"type": "test"}))

    async def test_explicit_login_takes_over_but_old_saved_session_cannot_take_back(self) -> None:
        previous = _WebSocketStub()
        manager = await self._bound(previous)
        current = _WebSocketStub()
        await manager.attach_session("session-2", current, "token-2")
        self.assertTrue(await manager.bind_discord("session-2", 1234, allow_takeover=True))
        self.assertEqual(previous.close_codes, [AGENT_SESSION_CONFLICT_CLOSE_CODE])
        self.assertIsNone(await manager.unregister_ws(previous))

        retry = _WebSocketStub()
        await manager.attach_session("session-1", retry, "token")
        self.assertFalse(await manager.bind_discord("session-1", 1234))
        self.assertEqual(current.close_codes, [])
        self.assertEqual(manager.discord_id_for_ws(current), 1234)
        self.assertTrue(manager.is_online(1234))

    async def test_different_session_can_replace_unresponsive_agent(self) -> None:
        previous = _WebSocketStub()
        manager = await self._bound(previous)
        manager._ws_last_heartbeat[id(previous)] = (
            time.monotonic() - AGENT_HEARTBEAT_STALE_SEC - 1
        )
        current = _WebSocketStub()
        await manager.attach_session("session-2", current, "token-2")
        self.assertTrue(await manager.bind_discord("session-2", 1234))
        self.assertEqual(previous.close_codes, [AGENT_SESSION_CONFLICT_CLOSE_CODE])
        self.assertIsNone(await manager.unregister_ws(previous))
        self.assertTrue(manager.is_online(1234))

    async def test_rejected_session_can_connect_after_active_agent_disconnects(self) -> None:
        active = _WebSocketStub()
        manager = await self._bound(active)
        duplicate = _WebSocketStub()
        await manager.attach_session("session-2", duplicate, "token-2")
        self.assertFalse(await manager.bind_discord("session-2", 1234))
        await manager.unregister_ws(active)

        retry = _WebSocketStub()
        await manager.attach_session("session-2", retry, "token-2")
        self.assertTrue(await manager.bind_discord("session-2", 1234))
        self.assertTrue(manager.is_online(1234))

    async def test_websocket_endpoint_exits_after_rejecting_competing_session(self) -> None:
        active = _WebSocketStub()
        manager = await self._bound(active)
        session_id = "12345678-1234-1234-1234-123456789012"
        token = "0123456789abcdef"

        async def get(key: str) -> str | None:
            return {
                f"relay:session:{session_id}": "1234",
                f"relay:ws_token:{session_id}": token,
            }.get(key)

        duplicate = _WebSocketStub()
        duplicate.app = SimpleNamespace(state=SimpleNamespace(
            redis=SimpleNamespace(get=AsyncMock(side_effect=get)), connections=manager,
        ))
        duplicate.accept = AsyncMock()
        duplicate.receive_text = AsyncMock(side_effect=[
            json.dumps({"type": "auth", "ws_token": token}),
            AssertionError("must not receive on the closed socket"),
        ])

        await ws_agent(duplicate, session_id=session_id)

        duplicate.receive_text.assert_awaited_once()
        self.assertEqual(duplicate.close_codes, [AGENT_SESSION_CONFLICT_CLOSE_CODE])
        self.assertIsNone(manager.discord_id_for_ws(duplicate))
        self.assertEqual(manager.discord_id_for_ws(active), 1234)
        self.assertTrue(manager.is_online(1234))

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

class AgentConnectionRecoveryTests(unittest.IsolatedAsyncioTestCase):
    async def test_replaced_old_socket_cannot_publish_offline(self) -> None:
        manager = ConnectionManager()
        old = _WebSocketStub()
        await manager.attach_session("session-1", old, "token", connection_id="old")
        self.assertTrue(await manager.bind_discord("session-1", 1234))

        new = _WebSocketStub()
        await manager.attach_session("session-1", new, "token", connection_id="new")
        self.assertEqual(old.close_codes, [AGENT_SESSION_CONFLICT_CLOSE_CODE])
        self.assertIsNone(await manager.unregister_ws(old))
        self.assertTrue(await manager.bind_discord("session-1", 1234))
        self.assertTrue(manager.is_online(1234))
        self.assertEqual(manager.connection_quality(1234)["connection_id"], "new")

    async def test_connection_quality_transitions(self) -> None:
        manager = ConnectionManager()
        ws = _WebSocketStub()
        await manager.attach_session("session-1", ws, "token", connection_id="c1")
        self.assertTrue(await manager.bind_discord("session-1", 1234))
        self.assertEqual(manager.connection_quality(1234)["quality"], "healthy")
        manager._ws_last_heartbeat[id(ws)] = time.monotonic() - 50
        self.assertEqual(manager.connection_quality(1234)["quality"], "degraded")
        manager._ws_last_heartbeat[id(ws)] = time.monotonic() - 100
        self.assertEqual(manager.connection_quality(1234)["quality"], "stale")
        await manager.unregister_ws(ws)
        self.assertEqual(manager.connection_quality(1234)["quality"], "reconnecting")

    async def test_force_reconnect_is_single_flight_with_cooldown(self) -> None:
        manager = ConnectionManager()
        ws = _WebSocketStub()
        await manager.attach_session("session-1", ws, "token", connection_id="c1")
        self.assertTrue(await manager.bind_discord("session-1", 1234))
        ok, detail = await manager.force_reconnect(1234, "test")
        self.assertTrue(ok)
        self.assertEqual(detail, "c1")
        self.assertEqual(ws.payloads[-1], {"type": "force_reconnect", "reason": "test"})
        ok, detail = await manager.force_reconnect(1234, "again")
        self.assertFalse(ok)
        self.assertEqual(detail, "cooldown")
