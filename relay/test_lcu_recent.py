import json
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from relay.app import _handle_agent_message
from relay.connections import ConnectionManager
from relay.lcu_recent import (
    LCU_RECENT_DATA_TTL_SEC,
    mark_recent_lcu_data,
    recent_lcu_data_map,
)


class _WebSocketStub:
    def __init__(self, redis: object | None = None) -> None:
        self.app = SimpleNamespace(state=SimpleNamespace(http=object(), redis=redis or object()))
        self.payloads: list[dict[str, object]] = []

    async def send_json(self, payload: dict[str, object]) -> None:
        self.payloads.append(payload)

    async def close(self, code: int = 1000) -> None:
        return None


class RecentLcuRedisTests(unittest.IsolatedAsyncioTestCase):
    async def test_mark_recent_lcu_data_sets_timestamp_with_ttl(self) -> None:
        redis = SimpleNamespace(set=AsyncMock())

        await mark_recent_lcu_data(redis, 42, occurred_at_ms=123456)

        redis.set.assert_awaited_once_with(
            "lcu:recent_data:42",
            "123456",
            ex=LCU_RECENT_DATA_TTL_SEC,
        )
        self.assertEqual(LCU_RECENT_DATA_TTL_SEC, 30 * 60)

    async def test_recent_lcu_data_map_reports_present_and_expired_keys(self) -> None:
        redis = SimpleNamespace(mget=AsyncMock(return_value=["123456", None]))

        result = await recent_lcu_data_map(redis, [2, 1])

        redis.mget.assert_awaited_once_with(["lcu:recent_data:1", "lcu:recent_data:2"])
        self.assertEqual(
            result,
            {
                1: {"recent_lcu_data": True, "last_lcu_data_at_ms": 123456},
                2: {"recent_lcu_data": False, "last_lcu_data_at_ms": None},
            },
        )


class RecentLcuMessageTests(unittest.IsolatedAsyncioTestCase):
    async def _bound(self) -> tuple[ConnectionManager, _WebSocketStub]:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))
        return manager, websocket

    async def test_lcu_ready_participant_status_marks_recent_activity(self) -> None:
        manager, websocket = await self._bound()
        data = {
            "type": "participant_status_update",
            "data": {
                "status": "waiting",
                "phase": "Lobby",
                "lcu_ready": True,
                "agent_online": True,
            },
        }

        with patch("relay.app.mark_recent_lcu_data", new=AsyncMock()) as mark, patch(
            "relay.app._forward_participant_status", new=AsyncMock()
        ):
            await _handle_agent_message(websocket, manager, json.dumps(data))

        mark.assert_awaited_once_with(websocket.app.state.redis, 42)

    async def test_lcu_unavailable_status_does_not_refresh_recent_activity(self) -> None:
        manager, websocket = await self._bound()
        data = {
            "type": "participant_status_update",
            "data": {
                "status": "waiting",
                "phase": "None",
                "lcu_ready": False,
                "agent_online": True,
            },
        }

        with patch("relay.app.mark_recent_lcu_data", new=AsyncMock()) as mark, patch(
            "relay.app._forward_participant_status", new=AsyncMock()
        ):
            await _handle_agent_message(websocket, manager, json.dumps(data))

        mark.assert_not_awaited()
