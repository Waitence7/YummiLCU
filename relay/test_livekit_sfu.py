import os
import unittest
from unittest.mock import patch

from relay import livekit_sfu


class LiveKitSfuTests(unittest.TestCase):
    def test_publisher_token_is_room_scoped_and_publish_only(self) -> None:
        with patch.dict(os.environ, {
            "LIVEKIT_PUBLIC_URL": "wss://live.example.test",
            "LIVEKIT_API_KEY": "test-key",
            "LIVEKIT_API_SECRET": "0123456789abcdef0123456789abcdef",
        }, clear=False):
            session = livekit_sfu.publisher_session(42)
            claims = livekit_sfu.decode_unverified(session.token)
        self.assertEqual(session.room, "yummi-live-42")
        self.assertEqual(claims["sub"], "agent-42")
        self.assertTrue(claims["video"]["roomJoin"])
        self.assertTrue(claims["video"]["canPublish"])
        self.assertFalse(claims["video"]["canSubscribe"])
        self.assertFalse(claims["video"]["canPublishData"])
        self.assertEqual(claims["video"]["canPublishSources"], ["screen_share"])

    def test_viewer_token_is_subscribe_only_and_unique(self) -> None:
        with patch.dict(os.environ, {
            "LIVEKIT_PUBLIC_URL": "wss://live.example.test",
            "LIVEKIT_API_KEY": "test-key",
            "LIVEKIT_API_SECRET": "0123456789abcdef0123456789abcdef",
        }, clear=False):
            first = livekit_sfu.viewer_session(42)
            second = livekit_sfu.viewer_session(42)
            claims = livekit_sfu.decode_unverified(first.token)
        self.assertNotEqual(first.identity, second.identity)
        self.assertEqual(claims["video"]["room"], "yummi-live-42")
        self.assertFalse(claims["video"]["canPublish"])
        self.assertTrue(claims["video"]["canSubscribe"])
        self.assertTrue(claims["video"]["canPublishData"])
        self.assertNotIn("canPublishSources", claims["video"])


if __name__ == "__main__":
    unittest.main()
