import unittest
from unittest.mock import patch

from relay.app import _deliver_guild_match_live, _forward_guild_match_live


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
        self.posts = 0

    def post(self, *args, **kwargs) -> _Response:
        self.posts += 1
        return self.response




class _FailingHttp:
    def __init__(self, error: Exception) -> None:
        self.error = error
        self.posts = 0

    def post(self, *args, **kwargs):
        self.posts += 1
        raise self.error


class GuildMatchLiveIngestTests(unittest.IsolatedAsyncioTestCase):
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

    async def test_matched_false_is_terminal_handled_and_will_not_retry(self) -> None:
        http = _Http(_Response(200, {"matched": False, "reason": "no_active_match"}))
        with (
            patch("relay.app.config.tournament_api_base_url", return_value="http://api"),
            patch("relay.app.config.tournament_bot_internal_token", return_value="token"),
        ):
            handled, matched = await _deliver_guild_match_live(
                http, 42, {"participants": [{} for _ in range(10)]}, "event-unmatched"
            )
        self.assertTrue(handled)
        self.assertFalse(matched)

    async def test_consecutive_meaningful_updates_are_both_forwarded(self) -> None:
        http = _Http(_Response(200, {"matched": True, "matchId": "match-1"}))
        with (
            patch("relay.app.config.tournament_api_base_url", return_value="http://api"),
            patch("relay.app.config.tournament_bot_internal_token", return_value="token"),
        ):
            first = await _forward_guild_match_live(
                http,
                42,
                {"participants": [{} for _ in range(10)], "captured_at_ms": 1000},
                "event-1",
            )
            second = await _forward_guild_match_live(
                http,
                42,
                {"participants": [{} for _ in range(10)], "captured_at_ms": 2000},
                "event-2",
            )

        self.assertTrue(first)
        self.assertTrue(second)
        self.assertEqual(http.posts, 2)

    async def test_http_500_is_retryable_and_not_handled(self) -> None:
        http = _Http(_Response(503, {"error": "maintenance"}))
        with (
            patch("relay.app.config.tournament_api_base_url", return_value="http://api"),
            patch("relay.app.config.tournament_bot_internal_token", return_value="token"),
        ):
            handled, matched = await _deliver_guild_match_live(
                http, 42, {"participants": [{} for _ in range(10)]}, "event-503"
            )
        self.assertFalse(handled)
        self.assertFalse(matched)

    async def test_api_timeout_is_retryable_and_not_handled(self) -> None:
        http = _FailingHttp(TimeoutError("api timeout"))
        with (
            patch("relay.app.config.tournament_api_base_url", return_value="http://api"),
            patch("relay.app.config.tournament_bot_internal_token", return_value="token"),
        ):
            handled, matched = await _deliver_guild_match_live(
                http, 42, {"participants": [{} for _ in range(10)]}, "event-timeout"
            )
        self.assertFalse(handled)
        self.assertFalse(matched)
        self.assertEqual(http.posts, 1)

    async def test_matched_commit_exposes_web_confirmed_snapshot_to_relay(self) -> None:
        web_data = {
            "phase": "InProgress",
            "game": {"id": "8458112300"},
            "participants": [{"team": "ORDER", "kills": 19, "deaths": 7, "assists": 27}],
        }
        http = _Http(_Response(200, {
            "matched": True,
            "matchId": "match-1",
            "canonicalLive": {
                "gameId": "8458112300",
                "version": 1794655957000,
                "data": web_data,
            },
        }))
        out: dict = {}
        with (
            patch("relay.app.config.tournament_api_base_url", return_value="http://api"),
            patch("relay.app.config.tournament_bot_internal_token", return_value="token"),
        ):
            handled, matched = await _deliver_guild_match_live(
                http, 42, {"participants": [{} for _ in range(10)]},
                canonical_out=out,
            )
        self.assertTrue(handled)
        self.assertTrue(matched)
        self.assertEqual(http.posts, 1)
        self.assertEqual(out, {
            "matchId": "match-1",
            "gameId": "8458112300",
            "version": 1794655957000,
            "data": web_data,
        })

    async def test_unmatched_or_unconfirmed_web_response_never_invents_canonical(self) -> None:
        for outcome in (
            {"matched": False, "reason": "unbound"},
            {"matched": True, "matchId": "match-1"},
            {"matched": True, "matchId": "match-1", "canonicalLive": {
                "gameId": "8458112300", "version": "bad", "data": {}
            }},
        ):
            out: dict = {}
            http = _Http(_Response(200, outcome))
            with (
                patch("relay.app.config.tournament_api_base_url", return_value="http://api"),
                patch("relay.app.config.tournament_bot_internal_token", return_value="token"),
            ):
                await _deliver_guild_match_live(http, 42, {"participants": []}, canonical_out=out)
            self.assertEqual(out, {})

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
