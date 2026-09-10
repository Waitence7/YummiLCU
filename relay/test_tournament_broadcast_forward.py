from __future__ import annotations

import asyncio
from unittest.mock import patch

from relay.app import _forward_tournament_broadcast_lcu


class _Response:
    def __init__(self, status: int, body: dict) -> None:
        self.status = status
        self._body = body

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    async def json(self, content_type=None):
        return self._body


class _Http:
    def __init__(self, response: _Response) -> None:
        self.response = response
        self.calls: list[tuple[str, dict, dict]] = []

    def post(self, url: str, *, headers: dict, json: dict):
        self.calls.append((url, headers, json))
        return self.response


def test_tournament_broadcast_forward_uses_internal_actor_identity() -> None:
    async def run() -> None:
        http = _Http(_Response(200, {"matched": True, "code": "ABCDE", "phase": "ChampSelect"}))
        with patch("relay.app.config.tournament_api_base_url", return_value="http://api:4000"), patch(
            "relay.app.config.tournament_bot_internal_token", return_value="secret"
        ):
            matched = await _forward_tournament_broadcast_lcu(
                http, 12345, "gameflow", {"phase": "ChampSelect", "lcu_ready": True}
            )
        assert matched is True
        assert http.calls == [
            (
                "http://api:4000/api/bot/tournaments/lcu-broadcast",
                {
                    "content-type": "application/json",
                    "x-internal-bot-token": "secret",
                    "x-actor-discord-user-id": "12345",
                },
                {"kind": "gameflow", "data": {"phase": "ChampSelect", "lcu_ready": True}},
            )
        ]

    asyncio.run(run())


def test_tournament_broadcast_forward_ignores_unbound_reporter() -> None:
    async def run() -> None:
        http = _Http(_Response(200, {"matched": False, "reason": "no_active_broadcast_binding"}))
        with patch("relay.app.config.tournament_api_base_url", return_value="http://api:4000"), patch(
            "relay.app.config.tournament_bot_internal_token", return_value="secret"
        ):
            matched = await _forward_tournament_broadcast_lcu(http, 12345, "champ_select", {"active": True})
        assert matched is False

    asyncio.run(run())


def test_tournament_broadcast_forward_accepts_live_game() -> None:
    async def run() -> None:
        payload = {"game": {"time_seconds": 600}, "participants": [{} for _ in range(10)]}
        http = _Http(_Response(200, {"matched": True, "code": "ABCDE"}))
        with patch("relay.app.config.tournament_api_base_url", return_value="http://api:4000"), patch(
            "relay.app.config.tournament_bot_internal_token", return_value="secret"
        ):
            matched = await _forward_tournament_broadcast_lcu(http, 12345, "live_game", payload)
        assert matched is True
        assert http.calls[0][2] == {"kind": "live_game", "data": payload}

    asyncio.run(run())
