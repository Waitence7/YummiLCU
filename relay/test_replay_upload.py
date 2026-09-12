from __future__ import annotations

import asyncio
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from fastapi import HTTPException

from relay.app import (
    _authenticate_agent_http,
    _replay_upload_target,
    agent_replay_upload_target,
    _session_redis_key,
    _ws_token_redis_key,
    app,
)


class _Pipe:
    def __init__(self) -> None:
        self.expired: list[tuple[str, int]] = []

    def expire(self, key: str, ttl: int):
        self.expired.append((key, ttl))
        return self

    async def execute(self):
        return [True for _ in self.expired]


class _Redis:
    def __init__(self, values: dict[str, str]) -> None:
        self.values = values
        self.pipe = _Pipe()

    async def get(self, key: str):
        return self.values.get(key)

    def pipeline(self):
        return self.pipe


class _Connections:
    def __init__(self, active: bool) -> None:
        self.active = active

    def has_active_session_ws(self, _session_id: str) -> bool:
        return self.active


class ReplayUploadAuthTests(unittest.TestCase):
    def test_replay_upload_routes_do_not_replace_auth_status(self) -> None:
        routes = {(route.path, frozenset(route.methods or [])) for route in app.routes if hasattr(route, "methods")}
        self.assertIn(("/auth/status", frozenset({"GET"})), routes)
        self.assertIn(("/agent/replay-upload-target", frozenset({"GET"})), routes)
        self.assertIn(("/agent/replay-upload", frozenset({"POST"})), routes)

    def test_legacy_target_request_without_game_id_keeps_tournament_upload(self) -> None:
        async def run() -> None:
            request = SimpleNamespace(app=SimpleNamespace(state=SimpleNamespace(http=SimpleNamespace())))
            with patch("relay.app._authenticate_agent_http", new=AsyncMock(return_value=42)), patch(
                "relay.app._broadcast_replay_target",
                new=AsyncMock(return_value={"upload": True, "code": "TOURNEY"}),
            ):
                response = await agent_replay_upload_target(request, "session-id", None)
            body = response.body.decode("utf-8")
            self.assertIn('"upload":true', body)
            self.assertIn('"targetKind":"tournament_broadcast"', body)
            self.assertIn('"code":"TOURNEY"', body)

        asyncio.run(run())

    def test_replay_target_prefers_active_tournament_broadcast(self) -> None:
        async def run() -> None:
            guild_lookup = AsyncMock(return_value={"upload": True, "matchId": "match-1"})
            with patch(
                "relay.app._broadcast_replay_target",
                new=AsyncMock(return_value={"upload": True, "code": "TOURNEY"}),
            ), patch("relay.app._guild_match_replay_target", new=guild_lookup):
                target = await _replay_upload_target(SimpleNamespace(), 42, "KR-1")
            self.assertTrue(target["upload"])
            self.assertEqual(target["targetKind"], "tournament_broadcast")
            self.assertEqual(target["code"], "TOURNEY")
            guild_lookup.assert_not_awaited()

        asyncio.run(run())

    def test_replay_target_falls_back_to_matching_guild_match(self) -> None:
        async def run() -> None:
            with patch(
                "relay.app._broadcast_replay_target",
                new=AsyncMock(return_value={"upload": False}),
            ), patch(
                "relay.app._guild_match_replay_target",
                new=AsyncMock(return_value={"upload": True, "matchId": "match-1", "inviteCode": "ABC123"}),
            ):
                target = await _replay_upload_target(SimpleNamespace(), 42, "KR-2")
            self.assertTrue(target["upload"])
            self.assertEqual(target["targetKind"], "guild_match")
            self.assertEqual(target["matchId"], "match-1")
            self.assertEqual(target["inviteCode"], "ABC123")

        asyncio.run(run())

    def test_replay_target_rejects_unrelated_games(self) -> None:
        async def run() -> None:
            with patch(
                "relay.app._broadcast_replay_target",
                new=AsyncMock(return_value={"upload": False}),
            ), patch(
                "relay.app._guild_match_replay_target",
                new=AsyncMock(return_value={"upload": False}),
            ):
                target = await _replay_upload_target(SimpleNamespace(), 42, "KR-3")
            self.assertEqual(target, {"upload": False})

        asyncio.run(run())

    def test_agent_http_auth_requires_active_matching_session_token(self) -> None:
        async def run() -> None:
            session_id = "123e4567-e89b-42d3-a456-426614174000"
            token = "t" * 64
            redis = _Redis({
                _ws_token_redis_key(session_id): token,
                _session_redis_key(session_id): "42",
            })
            request = SimpleNamespace(
                headers={"x-yummi-ws-token": token},
                app=SimpleNamespace(state=SimpleNamespace(redis=redis, connections=_Connections(True))),
            )
            self.assertEqual(await _authenticate_agent_http(request, session_id), 42)
            self.assertGreaterEqual(len(redis.pipe.expired), 3)

            request.headers["x-yummi-ws-token"] = "wrong"
            with self.assertRaises(HTTPException) as caught:
                await _authenticate_agent_http(request, session_id)
            self.assertEqual(caught.exception.status_code, 401)

        asyncio.run(run())


if __name__ == "__main__":
    unittest.main()
