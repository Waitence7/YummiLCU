import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock

from relay.lcu_linked import (
    LCU_LAST_AGENT_VERSION_KEY,
    lcu_agent_version_map,
    mark_lcu_agent_version,
)


class LcuAgentVersionRedisTests(unittest.IsolatedAsyncioTestCase):
    async def test_mark_agent_version_persists_last_version(self) -> None:
        redis = SimpleNamespace(hset=AsyncMock())

        await mark_lcu_agent_version(redis, 42, " 0.7.29 ")

        redis.hset.assert_awaited_once_with(LCU_LAST_AGENT_VERSION_KEY, "42", "0.7.29")

    async def test_agent_version_map_decodes_and_keeps_missing_users(self) -> None:
        redis = SimpleNamespace(hmget=AsyncMock(return_value=[b"0.7.28", None]))

        result = await lcu_agent_version_map(redis, [2, 1])

        redis.hmget.assert_awaited_once_with(LCU_LAST_AGENT_VERSION_KEY, ["1", "2"])
        self.assertEqual(result, {1: "0.7.28", 2: None})

    async def test_mark_agent_version_ignores_blank_version(self) -> None:
        redis = SimpleNamespace(hset=AsyncMock())

        await mark_lcu_agent_version(redis, 42, "  ")

        redis.hset.assert_not_awaited()

class LcuAgentVersionBindTests(unittest.IsolatedAsyncioTestCase):
    async def test_bind_persists_pending_hello_version(self) -> None:
        from unittest.mock import patch

        from relay.app import _try_bind_discord

        conn = SimpleNamespace(
            session_ws_token=lambda _session_id: "ws-token",
            bind_discord=AsyncMock(return_value=True),
            agent_info=lambda _discord_id: {"version": "0.7.29"},
        )
        redis = SimpleNamespace(
            get=AsyncMock(side_effect=["ws-token", None, None]),
            set=AsyncMock(),
        )

        with patch("relay.app.mark_lcu_linked", new=AsyncMock()) as mark_linked, patch(
            "relay.app.mark_lcu_agent_version", new=AsyncMock()
        ) as mark_version:
            bound = await _try_bind_discord(conn, redis, "session-1", 42)

        self.assertTrue(bound)
        mark_linked.assert_awaited_once_with(redis, 42)
        mark_version.assert_awaited_once_with(redis, 42, "0.7.29")
