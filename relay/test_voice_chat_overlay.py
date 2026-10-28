import json
import unittest

from relay.app import _handle_bot_message


class _Conn:
    def __init__(self) -> None:
        self.sent: list[tuple[int, dict]] = []

    async def send_command(self, discord_id: int, payload: dict) -> bool:
        self.sent.append((int(discord_id), dict(payload)))
        return True


class VoiceChatOverlayRelayTests(unittest.IsolatedAsyncioTestCase):
    async def test_live_voice_message_is_fanned_out_to_targets(self) -> None:
        conn = _Conn()
        await _handle_bot_message(
            conn,  # type: ignore[arg-type]
            None,  # redis is unused for this message type
            json.dumps(
                {
                    "type": "voice_chat_message",
                    "discord_ids": [30, 20, 20],
                    "data": {
                        "guild_id": 1,
                        "channel_id": 2,
                        "message_id": 3,
                        "author_id": 4,
                        "author_name": "Yummi",
                        "avatar_url": "https://cdn.discordapp.com/avatars/4/a.png",
                        "content": "hello",
                        "timestamp_ms": 123456,
                    },
                }
            ),
        )
        self.assertEqual([uid for uid, _ in conn.sent], [20, 30])
        self.assertTrue(all(row["type"] == "discord_voice_chat_message" for _, row in conn.sent))
        self.assertTrue(all(row["content"] == "hello" for _, row in conn.sent))

    async def test_clear_is_forwarded_to_one_agent(self) -> None:
        conn = _Conn()
        await _handle_bot_message(
            conn,  # type: ignore[arg-type]
            None,
            json.dumps({"type": "voice_chat_clear", "discord_id": 42}),
        )
        self.assertEqual(conn.sent, [(42, {"type": "discord_voice_chat_clear"})])

    async def test_invalid_or_empty_content_is_dropped(self) -> None:
        conn = _Conn()
        await _handle_bot_message(
            conn,  # type: ignore[arg-type]
            None,
            json.dumps(
                {
                    "type": "voice_chat_message",
                    "discord_ids": [42],
                    "data": {
                        "guild_id": 1,
                        "channel_id": 2,
                        "message_id": 3,
                        "author_id": 4,
                        "author_name": "Yummi",
                        "avatar_url": None,
                        "content": "   ",
                        "timestamp_ms": 123456,
                    },
                }
            ),
        )
        self.assertEqual(conn.sent, [])


if __name__ == "__main__":
    unittest.main()
