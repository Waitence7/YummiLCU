import unittest
from unittest.mock import patch

from relay.app import _forward_guild_match_live, _live_game_web_ingest_at


class _Response:
    def __init__(self, status: int, outcome: object) -> None:
        self.status = status
        self._outcome = outcome

    async def __aenter__(self) -> "_Response":
        return self

    async def __aexit__(self, exc_type, exc, tb) -> None:
        return None

    async def json(self, content_type=None):
        return self._outcome


class _Http:
    def __init__(self, response: _Response) -> None:
        self.response = response

    def post(self, *args, **kwargs) -> _Response:
        return self.response


class GuildMatchLiveIngestTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        _live_game_web_ingest_at.clear()

    async def test_http_200_matched_false_is_not_reported_as_success(self) -> None:
        http = _Http(_Response(200, {"matched": False, "reason": "no_active_match"}))
        with (
            patch("relay.app.config.tournament_api_base_url", return_value="http://api"),
            patch("relay.app.config.tournament_bot_internal_token", return_value="token"),
            self.assertLogs("yummi_lcu.relay", level="WARNING") as logs,
        ):
            matched = await _forward_guild_match_live(
                http,
                42,
                {"participants": [{} for _ in range(10)]},
            )

        self.assertFalse(matched)
        self.assertIn("미매칭", logs.output[-1])
        self.assertIn("no_active_match", logs.output[-1])

    async def test_http_200_matched_true_is_success(self) -> None:
        http = _Http(
            _Response(
                200,
                {
                    "matched": True,
                    "matchId": "match-1",
                    "overlap": 10,
                    "promotedFromLive": True,
                },
            )
        )
        with (
            patch("relay.app.config.tournament_api_base_url", return_value="http://api"),
            patch("relay.app.config.tournament_bot_internal_token", return_value="token"),
        ):
            matched = await _forward_guild_match_live(
                http,
                42,
                {"participants": [{} for _ in range(10)]},
            )

        self.assertTrue(matched)


if __name__ == "__main__":
    unittest.main()
