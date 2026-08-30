from __future__ import annotations

import asyncio

from relay.connections import ConnectionManager


class _FakeWs:
    def __init__(self) -> None:
        self.sent: list[dict] = []

    async def send_json(self, payload: dict) -> None:
        self.sent.append(payload)


def test_gameflow_is_cached_before_bot_subscribes_and_replayed() -> None:
    async def run() -> None:
        conn = ConnectionManager()
        bot_ws = _FakeWs()
        await conn.register_bot_ws(bot_ws)  # type: ignore[arg-type]

        forwarded = await conn.forward_gameflow_update(
            42, {"phase": "ChampSelect", "lcu_ready": True}
        )
        assert forwarded is False
        assert bot_ws.sent == []

        conn.subscribe_gameflow(42)
        replayed = await conn.replay_gameflow_update(42)
        assert replayed is True
        assert bot_ws.sent == [
            {
                "type": "gameflow_update",
                "discord_id": 42,
                "data": {"phase": "ChampSelect", "lcu_ready": True},
            }
        ]

    asyncio.run(run())


def test_gameflow_replay_requires_same_active_subscription() -> None:
    async def run() -> None:
        conn = ConnectionManager()
        bot_ws = _FakeWs()
        await conn.register_bot_ws(bot_ws)  # type: ignore[arg-type]
        await conn.forward_gameflow_update(42, {"phase": "EndOfGame"})

        assert await conn.replay_gameflow_update(42) is False
        conn.subscribe_gameflow(42)
        conn.unsubscribe_gameflow(42)
        assert await conn.replay_gameflow_update(42) is False
        assert bot_ws.sent == []

    asyncio.run(run())
