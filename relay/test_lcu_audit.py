from __future__ import annotations

import unittest
from unittest.mock import MagicMock

from relay.lcu_audit import agent_event_stats_map, observe_agent_event


class _Pipeline:
    def __init__(self, execute_result=None) -> None:
        self.calls: list[tuple[str, tuple, dict]] = []
        self.execute_result = execute_result if execute_result is not None else []

    def _record(self, name: str, *args, **kwargs):
        self.calls.append((name, args, kwargs))
        return self

    def sadd(self, *args, **kwargs):
        return self._record("sadd", *args, **kwargs)

    def hincrby(self, *args, **kwargs):
        return self._record("hincrby", *args, **kwargs)

    def hsetnx(self, *args, **kwargs):
        return self._record("hsetnx", *args, **kwargs)

    def hset(self, *args, **kwargs):
        return self._record("hset", *args, **kwargs)

    def hgetall(self, *args, **kwargs):
        return self._record("hgetall", *args, **kwargs)

    async def execute(self):
        return self.execute_result


class LcuAuditTests(unittest.IsolatedAsyncioTestCase):
    async def test_observe_agent_event_records_compact_stats_and_last_event(self) -> None:
        pipe = _Pipeline()
        redis = MagicMock()
        redis.pipeline.return_value = pipe

        await observe_agent_event(
            redis,
            42,
            "live_game_update",
            occurred_at_ms=123456,
            message_bytes=789,
            session_id="session-1",
            connection_id="connection-1",
        )

        self.assertIn(
            ("hincrby", ("lcu:agent_event_stats:v1:42", "live_game_update.count", 1), {}),
            pipe.calls,
        )
        self.assertIn(
            ("hsetnx", ("lcu:agent_event_stats:v1:42", "live_game_update.first_at_ms", "123456"), {}),
            pipe.calls,
        )
        self.assertIn(
            ("hset", ("lcu:agent_event_stats:v1:42", "live_game_update.last_at_ms", "123456"), {}),
            pipe.calls,
        )
        self.assertIn(
            ("hincrby", ("lcu:agent_event_stats:v1:42", "live_game_update.bytes", 789), {}),
            pipe.calls,
        )
        self.assertIn(
            ("hset", ("lcu:agent_state:v1:42", "last_event_type", "live_game_update"), {}),
            pipe.calls,
        )

    async def test_agent_event_stats_map_decodes_nested_metrics(self) -> None:
        pipe = _Pipeline(
            execute_result=[
                {
                    b"agent_hello.count": b"2",
                    b"agent_hello.first_at_ms": b"100",
                    b"agent_hello.last_at_ms": b"200",
                    b"agent_hello.bytes": b"500",
                    b"garbage": b"ignored",
                }
            ]
        )
        redis = MagicMock()
        redis.pipeline.return_value = pipe

        result = await agent_event_stats_map(redis, [42])

        self.assertEqual(
            result,
            {
                42: {
                    "agent_hello": {
                        "count": 2,
                        "first_at_ms": 100,
                        "last_at_ms": 200,
                        "bytes": 500,
                    }
                }
            },
        )


if __name__ == "__main__":
    unittest.main()
