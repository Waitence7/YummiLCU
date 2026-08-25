import unittest

from relay.logging_safety import redact_log_text


class LoggingSafetyTests(unittest.TestCase):
    def test_redacts_key_values_bearer_and_query_secrets(self) -> None:
        text = (
            "token=abc123456 authorization: Bearer abcdefghijklmnop "
            "https://example.test/cb?code=secret-code&ok=1"
        )
        redacted = redact_log_text(text)
        self.assertNotIn("abc123456", redacted)
        self.assertNotIn("abcdefghijklmnop", redacted)
        self.assertNotIn("secret-code", redacted)
        self.assertGreaterEqual(redacted.count("[REDACTED]"), 3)

    def test_keeps_non_secret_diagnostics(self) -> None:
        self.assertEqual(
            redact_log_text("discord_id=42 status=200 action=ping"),
            "discord_id=42 status=200 action=ping",
        )
