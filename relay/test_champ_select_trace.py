import unittest
from unittest.mock import AsyncMock, patch

from relay.connections import ConnectionManager, log_champ_select_receipt, _champ_select_receipt_state


class ChampSelectTraceTests(unittest.IsolatedAsyncioTestCase):
    async def test_forward_decision_changes_are_logged_without_repeating_unchanged_events(self):
        manager = ConnectionManager()
        payload = {"active": True, "game_id": "game-1", "phase": "BAN_PICK"}
        with self.assertLogs("yummi_lcu.relay.connections", level="INFO") as logs:
            self.assertFalse(await manager.forward_champ_select_update(42, payload))
            self.assertFalse(await manager.forward_champ_select_update(42, payload))
            manager.subscribe_gameflow(42)
            self.assertFalse(await manager.forward_champ_select_update(42, payload))
            manager._bot_ws = AsyncMock()
            self.assertTrue(await manager.forward_champ_select_update(42, payload))
        output = "\n".join(logs.output)
        self.assertEqual(output.count("decision=unsubscribed"), 1)
        self.assertIn("decision=bot_disconnected", output)
        self.assertIn("decision=forward", output)
        self.assertIn("champ select relay sent", output)
        self.assertEqual(manager._bot_ws.send_json.await_count, 1)

    async def test_receipt_is_sampled_and_game_change_is_logged(self):
        _champ_select_receipt_state.clear()
        with patch("relay.connections.time.monotonic", side_effect=[1.0, 2.0, 3.0, 62.0]):
            with self.assertLogs("yummi_lcu.relay.connections", level="INFO") as logs:
                for game in ("one", "one", "two", "two"):
                    log_champ_select_receipt(99, {"active": True, "game_id": game})
        self.assertEqual(len(logs.output), 2)
        self.assertIn("game=one", logs.output[0])
        self.assertIn("game=two", logs.output[1])
