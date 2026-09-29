import json
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from relay.app import (
    _apply_live_delta_ops,
    _commit_live_delta_state,
    _handle_agent_message,
    _prepare_live_game_frame,
)
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


class LiveDeltaTests(unittest.IsolatedAsyncioTestCase):
    def test_delta_ops_reconstruct_exact_state(self) -> None:
        previous = {
            "captured_at_ms": 1000,
            "game": {"id": 7, "time_seconds": 10},
            "participants": [
                {"name": "A", "kills": 0, "items": []},
                {"name": "B", "kills": 0, "items": []},
            ],
            "events": [{"id": 1}],
        }
        ops = [
            ["s", ["captured_at_ms"], 2000],
            ["s", ["game", "time_seconds"], 11],
            ["s", ["participants", 0, "kills"], 1],
            ["a", ["participants", 1, "items"], [{"id": 2003, "count": 1}]],
            ["a", ["events"], [{"id": 2}]],
        ]
        expected = {
            "captured_at_ms": 2000,
            "game": {"id": 7, "time_seconds": 11},
            "participants": [
                {"name": "A", "kills": 1, "items": []},
                {"name": "B", "kills": 0, "items": [{"id": 2003, "count": 1}]},
            ],
            "events": [{"id": 1}, {"id": 2}],
        }
        self.assertEqual(_apply_live_delta_ops(previous, ops), expected)

    def test_item_slot_ops_use_zero_based_slots_and_reconstruct_exact_state(self) -> None:
        empty = [None] * 7
        previous = {
            "participants": [
                {"items": list(empty)},
                {"items": [None, None, {"id": 2003, "name": "Health Potion", "count": 2}, None, None, None, None]},
                {"items": [{"id": 1056, "name": "Doran's Ring", "count": 1}, None, None, {"id": 1001, "name": "Boots", "count": 1}, None, None, None]},
                {"items": [None, None, None, None, None, {"id": 2055, "name": "Control Ward", "count": 1}, None]},
            ]
        }
        ops = [
            ["ia", 0, 1, 2003, "Health Potion", 2],
            ["ic", 1, 2, -1],
            ["is", 2, 0, 3],
            ["ir", 3, 5],
        ]
        state = _apply_live_delta_ops(previous, ops)

        self.assertEqual(state["participants"][0]["items"][1], {"id": 2003, "name": "Health Potion", "count": 2})
        self.assertEqual(state["participants"][1]["items"][2]["count"], 1)
        self.assertEqual(state["participants"][2]["items"][0]["id"], 1001)
        self.assertEqual(state["participants"][2]["items"][3]["id"], 1056)
        self.assertIsNone(state["participants"][3]["items"][5])
        self.assertEqual(len(state["participants"][0]["items"]), 7)

    def test_item_slot_ops_reject_out_of_range_slot(self) -> None:
        previous = {"participants": [{"items": [None] * 7}]}
        with self.assertRaisesRegex(ValueError, "invalid live item slot"):
            _apply_live_delta_ops(previous, [["ia", 0, 7, 2003, "Health Potion", 1]])

    async def test_full_then_delta_advances_sequence(self) -> None:
        r = _Redis()
        full_state = {
            "game": {"id": 7, "time_seconds": 10},
            "participants": [{"name": f"P{i}", "kills": 0} for i in range(10)],
            "events": [],
        }
        full = {
            "format": "live-delta-v1",
            "stream_id": STREAM,
            "seq": 1,
            "base_seq": 0,
            "kind": "full",
            "state": full_state,
        }
        prepared = await _prepare_live_game_frame(r, 42, full)
        self.assertEqual(prepared["status"], "ready")
        await _commit_live_delta_state(r, 42, STREAM, 1, prepared["state"])

        delta = {
            "format": "live-delta-v1",
            "stream_id": STREAM,
            "seq": 2,
            "base_seq": 1,
            "kind": "delta",
            "ops": [["s", ["participants", 3, "kills"], 1]],
        }
        prepared = await _prepare_live_game_frame(r, 42, delta)
        self.assertEqual(prepared["status"], "ready")
        self.assertEqual(prepared["seq"], 2)
        self.assertEqual(prepared["state"]["participants"][3]["kills"], 1)

    async def test_gap_requests_entire_missing_range_including_current_frame(self) -> None:
        r = _Redis()
        state = {
            "game": {"id": 7},
            "participants": [{"name": f"P{i}"} for i in range(10)],
        }
        await _commit_live_delta_state(r, 42, STREAM, 19, state)
        frame = {
            "format": "live-delta-v1",
            "stream_id": STREAM,
            "seq": 21,
            "base_seq": 20,
            "kind": "delta",
            "ops": [["s", ["game", "time_seconds"], 20]],
        }
        prepared = await _prepare_live_game_frame(r, 42, frame)
        self.assertEqual(prepared["status"], "gap")
        self.assertEqual(prepared["from_seq"], 20)
        self.assertEqual(prepared["to_seq"], 21)

    async def test_agent_handler_acks_committed_seq_and_requests_missing_range(self) -> None:
        r = _Redis()
        websocket = _WebSocket(r)
        manager = ConnectionManager()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))

        full_state = {
            "game": {"id": 7, "time_seconds": 10},
            "participants": [{"name": f"P{i}", "kills": 0} for i in range(10)],
            "events": [],
        }
        full_message = {
            "type": "live_game_update",
            "event_id": "123e4567-e89b-42d3-a456-426614174010",
            "data": {
                "format": "live-delta-v1",
                "stream_id": STREAM,
                "seq": 1,
                "base_seq": 0,
                "kind": "full",
                "state": full_state,
            },
        }

        with (
            patch("relay.app._deliver_guild_match_live", new=AsyncMock(return_value=(True, True))),
            patch("relay.app._forward_tournament_broadcast_lcu", new=AsyncMock(return_value=True)),
        ):
            await _handle_agent_message(websocket, manager, json.dumps(full_message))

        self.assertEqual(
            websocket.payloads[-1],
            {
                "type": "event_ack",
                "event_id": "123e4567-e89b-42d3-a456-426614174010",
            },
        )

        gap_message = {
            "type": "live_game_update",
            "event_id": "123e4567-e89b-42d3-a456-426614174012",
            "data": {
                "format": "live-delta-v1",
                "stream_id": STREAM,
                "seq": 3,
                "base_seq": 2,
                "kind": "delta",
                "ops": [["s", ["participants", 0, "kills"], 2]],
            },
        }
        with patch("relay.app._deliver_guild_match_live", new=AsyncMock()) as deliver:
            await _handle_agent_message(websocket, manager, json.dumps(gap_message))

        deliver.assert_not_awaited()
        self.assertEqual(
            websocket.payloads[-1],
            {
                "type": "live_delta_resend",
                "stream_id": STREAM,
                "from_seq": 2,
                "to_seq": 3,
            },
        )

    async def test_missing_base_replays_from_stream_start_before_resync(self) -> None:
        r = _Redis()
        frame = {
            "format": "live-delta-v1",
            "stream_id": STREAM,
            "seq": 9,
            "base_seq": 8,
            "kind": "delta",
            "ops": [],
        }
        prepared = await _prepare_live_game_frame(r, 42, frame)
        self.assertEqual(prepared["status"], "gap")
        self.assertEqual(prepared["from_seq"], 1)
        self.assertEqual(prepared["to_seq"], 9)

    async def test_missing_very_old_base_requests_full_resync(self) -> None:
        r = _Redis()
        frame = {
            "format": "live-delta-v1",
            "stream_id": STREAM,
            "seq": 2000,
            "base_seq": 1999,
            "kind": "delta",
            "ops": [],
        }
        prepared = await _prepare_live_game_frame(r, 42, frame)
        self.assertEqual(prepared["status"], "resync")


if __name__ == "__main__":
    unittest.main()
