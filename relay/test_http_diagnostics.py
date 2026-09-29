from __future__ import annotations

import asyncio
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock, patch

from relay.app import _forward_match_rofl
from relay.http_diagnostics import (
    MAX_UPSTREAM_ERROR_CHARS,
    summarize_upstream_error_bytes,
    upstream_error_detail,
)


class _Response:
    def __init__(self, status: int, body: bytes) -> None:
        self.status = status
        self.content = SimpleNamespace(read=AsyncMock(return_value=body))

    async def __aenter__(self):
        return self

    async def __aexit__(self, *_args):
        return None


class HttpDiagnosticsTests(unittest.TestCase):
    def test_error_reason_is_bounded_and_credentials_are_redacted(self) -> None:
        detail = summarize_upstream_error_bytes(
            b'{"error":"summary requires exactly 10 participants; token=private-value"}'
        )
        self.assertIn("summary requires exactly 10 participants", detail)
        self.assertNotIn("private-value", detail)
        self.assertLessEqual(len(detail), MAX_UPSTREAM_ERROR_CHARS)

    def test_reads_only_a_bounded_response_prefix(self) -> None:
        response = _Response(400, b"x" * 4_096)
        detail = asyncio.run(upstream_error_detail(response))
        response.content.read.assert_awaited_once_with(2_049)
        self.assertTrue(detail.endswith("[truncated]"))

    def test_rofl_rejection_logs_server_reason_and_payload_shape(self) -> None:
        response = _Response(400, b'{"error":"summary requires exactly 10 participants"}')
        http = SimpleNamespace(post=Mock(return_value=response))
        with patch("relay.app.config.tournament_bot_internal_token", return_value="test-token"), patch(
            "relay.app.config.tournament_api_base_url", return_value="http://api.test"
        ), self.assertLogs("yummi_lcu.relay", level="WARNING") as logs:
            saved = asyncio.run(_forward_match_rofl(
                http, 42, {"gameId": "8400013069", "kind": "summary", "participants": []}, "event-1"
            ))
        self.assertFalse(saved)
        self.assertIn("summary requires exactly 10 participants", logs.output[0])
        self.assertIn("participants=0", logs.output[0])


if __name__ == "__main__":
    unittest.main()
