import unittest

from relay.actions import action_policy, validate_web_input_payload
from relay.connections import ConnectionManager


class LivePreviewContractTests(unittest.TestCase):
    def test_web_input_action_is_allowlisted_but_not_new_device(self) -> None:
        policy = action_policy("web_input")
        self.assertIsNotNone(policy)
        assert policy is not None
        self.assertFalse(policy.allow_new_device)
        self.assertTrue(validate_web_input_payload({"kind": "button", "name": "confirm", "value": None}))
        self.assertTrue(validate_web_input_payload({"kind": "value", "name": "volume", "value": 42}))
        self.assertFalse(validate_web_input_payload({"kind": "keyboard", "name": "KeyA"}))
        self.assertFalse(validate_web_input_payload({"kind": "button", "name": "../bad"}))
        self.assertFalse(validate_web_input_payload({"kind": "value", "name": "x", "value": {"nested": True}}))

    def test_live_preview_keeps_only_latest_frame(self) -> None:
        manager = ConnectionManager()
        manager.set_live_preview(
            123,
            sequence=1,
            captured_at_ms=1000,
            width=960,
            height=540,
            jpeg=b"\xff\xd8one\xff\xd9",
        )
        manager.set_live_preview(
            123,
            sequence=2,
            captured_at_ms=1333,
            width=960,
            height=540,
            jpeg=b"\xff\xd8two\xff\xd9",
        )
        row = manager.get_live_preview(123)
        self.assertIsNotNone(row)
        assert row is not None
        self.assertEqual(row["sequence"], 2)
        self.assertEqual(row["jpeg"], b"\xff\xd8two\xff\xd9")
        self.assertEqual(row["width"], 960)
        self.assertEqual(row["height"], 540)


if __name__ == "__main__":
    unittest.main()
