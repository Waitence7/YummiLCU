from pathlib import Path
import unittest


class LiveNewDevicePolicyTests(unittest.TestCase):
    def test_livekit_endpoints_do_not_hard_block_new_device(self):
        source = Path(__file__).with_name("app.py").read_text(encoding="utf-8")
        session_block = source[source.index('@app.get("/internal/livekit-session/{discord_id}")'):source.index('@app.post("/internal/livekit-lease/{discord_id}")')]
        lease_block = source[source.index('@app.post("/internal/livekit-lease/{discord_id}")'):source.index('@app.get("/internal/live-preview/{discord_id}")')]
        self.assertNotIn("live stream unavailable for new device", session_block)
        self.assertNotIn("live stream unavailable for new device", lease_block)

    def test_other_new_device_command_policy_is_preserved(self):
        source = Path(__file__).with_name("app.py").read_text(encoding="utf-8")
        self.assertIn('if trust_level == "new_device" and not policy.allow_new_device:', source)
        self.assertIn('raise HTTPException(403, "action not allowed for new device")', source)


if __name__ == "__main__":
    unittest.main()
