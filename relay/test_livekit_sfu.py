import os
import unittest
from unittest.mock import patch

from relay import livekit_sfu


class LiveKitSfuTests(unittest.TestCase):
    def test_stream_profiles_include_higher_resolutions_and_are_bounded(self) -> None:
        balanced = livekit_sfu.stream_profile("balanced")
        high = livekit_sfu.stream_profile("high")
        ultra = livekit_sfu.stream_profile("ultra")
        self.assertEqual((balanced.width, balanced.height, balanced.fps), (1280, 720, 30))
        self.assertEqual((high.width, high.height), (1920, 1080))
        self.assertEqual((ultra.width, ultra.height), (2560, 1440))
        self.assertLessEqual(ultra.bitrate, 20_000_000)
        balanced_60 = livekit_sfu.stream_profile("balanced", 60)
        ultra_60 = livekit_sfu.stream_profile("ultra", 60)
        self.assertEqual((balanced_60.fps, balanced_60.bitrate), (60, 8_000_000))
        self.assertEqual((ultra_60.fps, ultra_60.bitrate), (60, 20_000_000))
        with self.assertRaises(ValueError):
            livekit_sfu.stream_profile("balanced", 45)
        with self.assertRaises(ValueError):
            livekit_sfu.stream_profile("4k")

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

    def test_shared_viewer_token_cannot_publish_data(self) -> None:
        with patch.dict(os.environ, {
            "LIVEKIT_PUBLIC_URL": "wss://live.example.test",
            "LIVEKIT_API_KEY": "test-key",
            "LIVEKIT_API_SECRET": "0123456789abcdef0123456789abcdef",
        }, clear=False):
            session = livekit_sfu.viewer_session(42, can_publish_data=False, name="Yummi Shared Viewer")
            claims = livekit_sfu.decode_unverified(session.token)
        self.assertTrue(claims["video"]["canSubscribe"])
        self.assertFalse(claims["video"]["canPublish"])
        self.assertFalse(claims["video"]["canPublishData"])
        self.assertEqual(claims["name"], "Yummi Shared Viewer")


if __name__ == "__main__":
    unittest.main()
