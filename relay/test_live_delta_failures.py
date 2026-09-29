import json
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from relay.app import _handle_agent_message, _load_live_delta_state
from relay.connections import ConnectionManager

STREAM = "123e4567-e89b-42d3-a456-426614174000"


class _Redis:
    def __init__(self) -> None:
        self.values: dict[str, str] = {}

    async def get(self, key: str):
        return self.values.get(key)

    async def set(self, key: str, value: str, ex=None, nx: bool = False):
        if nx and key in self.values:
            return False
        self.values[key] = value
        return True

    async def delete(self, key: str):
        return 1 if self.values.pop(key, None) is not None else 0


class _WebSocket:
    def __init__(self, redis_store: _Redis) -> None:
        self.payloads: list[dict[str, object]] = []
        self.app = SimpleNamespace(state=SimpleNamespace(http=object(), redis=redis_store))

    async def send_json(self, payload: dict[str, object]) -> None:
        self.payloads.append(payload)

    async def close(self, code: int = 1000) -> None:
        return None


def _full(event_id: str, *, time_seconds: int = 10) -> dict[str, object]:
    return {
        "type": "live_game_update",
        "event_id": event_id,
        "data": {
            "format": "live-delta-v1",
            "stream_id": STREAM,
            "seq": 1,
            "base_seq": 0,
            "kind": "full",
            "state": {
                "captured_at_ms": 1_000,
                "game": {"id": 7, "time_seconds": time_seconds},
                "participants": [{"name": f"P{i}", "kills": 0} for i in range(10)],
                "events": [],
            },
        },
    }


def _delta(event_id: str, seq: int, base_seq: int, kills: int) -> dict[str, object]:
    return {
        "type": "live_game_update",
        "event_id": event_id,
        "data": {
            "format": "live-delta-v1",
            "stream_id": STREAM,
            "seq": seq,
            "base_seq": base_seq,
            "kind": "delta",
            "ops": [
                ["s", ["captured_at_ms"], seq * 1_000],
                ["s", ["game", "time_seconds"], 9 + seq],
                ["s", ["participants", 0, "kills"], kills],
            ],
        },
    }


async def _bound(redis_store: _Redis) -> tuple[ConnectionManager, _WebSocket]:
    websocket = _WebSocket(redis_store)
    manager = ConnectionManager()
    await manager.attach_session("session-1", websocket, "token")
    assert await manager.bind_discord("session-1", 42)
    return manager, websocket


def _count_type(websocket: _WebSocket, message_type: str) -> int:
    return sum(1 for payload in websocket.payloads if payload.get("type") == message_type)


class LiveDeltaFailureSimulationTests(unittest.IsolatedAsyncioTestCase):
    async def test_api_outage_holds_ack_avoids_resend_loop_and_recovers_in_order(self) -> None:
        r = _Redis()
        manager, websocket = await _bound(r)
        full = _full("123e4567-e89b-42d3-a456-426614174101")
        deltas = [
            _delta(
                f"123e4567-e89b-42d3-a456-426614174{100 + seq:03d}",
                seq,
                seq - 1,
                seq - 1,
            )
            for seq in range(2, 11)
        ]

        with patch("relay.app._deliver_guild_match_live", new=AsyncMock(return_value=(False, False))):
            await _handle_agent_message(websocket, manager, json.dumps(full))
        self.assertEqual(_count_type(websocket, "event_ack"), 0)
        self.assertIsNone(await _load_live_delta_state(r, 42, STREAM))

        # While the expected base is API-blocked, later seq values must not cause
        # range replay amplification. They remain in the Agent durable queue.
        for delta in deltas:
            with patch("relay.app._deliver_guild_match_live", new=AsyncMock()) as deliver:
                await _handle_agent_message(websocket, manager, json.dumps(delta))
            deliver.assert_not_awaited()
        self.assertEqual(_count_type(websocket, "live_delta_resend"), 0)
        self.assertEqual(_count_type(websocket, "event_ack"), 0)

        # Durable replay tick fires after the API recovers: replay the full base and
        # every pending delta in original order.
        with (
            patch("relay.app._deliver_guild_match_live", new=AsyncMock(return_value=(True, True))),
            patch("relay.app._forward_tournament_broadcast_lcu", new=AsyncMock(return_value=True)),
        ):
            await _handle_agent_message(websocket, manager, json.dumps(full))
            for delta in deltas:
                await _handle_agent_message(websocket, manager, json.dumps(delta))

        committed = await _load_live_delta_state(r, 42, STREAM)
        self.assertIsNotNone(committed)
        self.assertEqual(committed[0], 10)
        self.assertEqual(committed[1]["participants"][0]["kills"], 9)
        self.assertEqual(_count_type(websocket, "event_ack"), 10)

    async def test_lost_ack_replay_is_deduplicated_without_second_api_write(self) -> None:
        r = _Redis()
        manager, websocket = await _bound(r)
        full = _full("123e4567-e89b-42d3-a456-426614174111")

        with (
            patch("relay.app._deliver_guild_match_live", new=AsyncMock(return_value=(True, True))) as first,
            patch("relay.app._forward_tournament_broadcast_lcu", new=AsyncMock(return_value=True)),
        ):
            await _handle_agent_message(websocket, manager, json.dumps(full))
        first.assert_awaited_once()
        self.assertEqual(_count_type(websocket, "event_ack"), 1)

        # Simulate the Agent never receiving that ACK and replaying the same event.
        with patch("relay.app._deliver_guild_match_live", new=AsyncMock()) as second:
            await _handle_agent_message(websocket, manager, json.dumps(full))
        second.assert_not_awaited()
        self.assertEqual(_count_type(websocket, "event_ack"), 2)

    async def test_api_commit_with_lost_response_retries_same_event_id_safely(self) -> None:
        r = _Redis()
        manager1, websocket1 = await _bound(r)
        full = _full("123e4567-e89b-42d3-a456-426614174141")
        delta = _delta("123e4567-e89b-42d3-a456-426614174142", 2, 1, 1)

        with (
            patch("relay.app._deliver_guild_match_live", new=AsyncMock(return_value=(True, True))),
            patch("relay.app._forward_tournament_broadcast_lcu", new=AsyncMock(return_value=True)),
        ):
            await _handle_agent_message(websocket1, manager1, json.dumps(full))

        # Model: API DB commit succeeded, but the HTTP response disappeared. Relay
        # sees it as unhandled, so Redis stays at seq=1 and no ACK is emitted.
        before_acks = _count_type(websocket1, "event_ack")
        with patch("relay.app._deliver_guild_match_live", new=AsyncMock(return_value=(False, False))):
            await _handle_agent_message(websocket1, manager1, json.dumps(delta))
        self.assertEqual(_count_type(websocket1, "event_ack"), before_acks)
        self.assertEqual((await _load_live_delta_state(r, 42, STREAM))[0], 1)

        # A replay uses the same event_id. The API route deduplicates that event_id
        # and returns success; Relay can then advance seq and ACK it.
        manager2, websocket2 = await _bound(r)
        with (
            patch("relay.app._deliver_guild_match_live", new=AsyncMock(return_value=(True, True))),
            patch("relay.app._forward_tournament_broadcast_lcu", new=AsyncMock(return_value=True)),
        ):
            await _handle_agent_message(websocket2, manager2, json.dumps(delta))
        self.assertEqual((await _load_live_delta_state(r, 42, STREAM))[0], 2)
        self.assertEqual(websocket2.payloads[-1]["type"], "event_ack")
        self.assertEqual(websocket2.payloads[-1]["event_id"], delta["event_id"])

    async def test_relay_restart_after_commit_before_ack_recovers_from_redis(self) -> None:
        r = _Redis()
        manager1, websocket1 = await _bound(r)
        full = _full("123e4567-e89b-42d3-a456-426614174121")
        delta = _delta("123e4567-e89b-42d3-a456-426614174122", 2, 1, 1)

        with (
            patch("relay.app._deliver_guild_match_live", new=AsyncMock(return_value=(True, True))),
            patch("relay.app._forward_tournament_broadcast_lcu", new=AsyncMock(return_value=True)),
        ):
            await _handle_agent_message(websocket1, manager1, json.dumps(full))
            await _handle_agent_message(websocket1, manager1, json.dumps(delta))
        self.assertEqual((await _load_live_delta_state(r, 42, STREAM))[0], 2)

        # New Relay process: new manager/websocket, same Redis. The Agent replays seq=2
        # because its ACK was lost during the crash.
        manager2, websocket2 = await _bound(r)
        with patch("relay.app._deliver_guild_match_live", new=AsyncMock()) as deliver:
            await _handle_agent_message(websocket2, manager2, json.dumps(delta))
        deliver.assert_not_awaited()
        self.assertEqual(websocket2.payloads[-1]["type"], "event_ack")
        self.assertEqual(websocket2.payloads[-1]["event_id"], delta["event_id"])

    async def test_redis_loss_with_only_a_pending_delta_requires_stream_recovery(self) -> None:
        r = _Redis()
        manager, websocket = await _bound(r)
        full = _full("123e4567-e89b-42d3-a456-426614174131")
        delta = _delta("123e4567-e89b-42d3-a456-426614174132", 2, 1, 1)

        with (
            patch("relay.app._deliver_guild_match_live", new=AsyncMock(return_value=(True, True))),
            patch("relay.app._forward_tournament_broadcast_lcu", new=AsyncMock(return_value=True)),
        ):
            await _handle_agent_message(websocket, manager, json.dumps(full))
        self.assertEqual((await _load_live_delta_state(r, 42, STREAM))[0], 1)

        # Redis flush/restart loses the reconstruction base after seq=1 was ACKed and
        # therefore removed from the Agent's pending queue.
        r.values.clear()
        with patch("relay.app._deliver_guild_match_live", new=AsyncMock()) as deliver:
            await _handle_agent_message(websocket, manager, json.dumps(delta))
        deliver.assert_not_awaited()
        self.assertEqual(websocket.payloads[-1]["type"], "live_delta_resend")
        self.assertEqual(websocket.payloads[-1]["from_seq"], 1)
        self.assertEqual(websocket.payloads[-1]["to_seq"], 2)


if __name__ == "__main__":
    unittest.main()
