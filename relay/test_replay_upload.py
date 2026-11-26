from __future__ import annotations

import asyncio
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from fastapi import HTTPException

from relay.app import (
    _authenticate_agent_http,
    _replay_upload_target,
    _one_off_scrim_replay_target,
    _one_off_rofl_backfill_tick,
    ONE_OFF_SCRIM_MISSING_KEY,
    ONE_OFF_SCRIM_REPLAY_KEY,
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
        self.assertIn(("/lcu/replay-upload-target", frozenset({"GET"})), routes)
        self.assertIn(("/lcu/replay-upload", frozenset({"POST"})), routes)

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

    def test_one_off_finished_game_can_download_after_web_linkage_delay(self) -> None:
        import json
        from time import time
        host = 229144663720853504
        now = int(time())
        record = json.dumps({
            "guild_id": 1485856315661226109,
            "created_at": now - 1800, "expires_at": now + 3600,
            "source_user_ids": [host, 123456789012345678],
            "games": [{"game_id": "8474323735", "first_seen_at": now - 1200,
                       "ended": True, "winner": "red"},
                      {"game_id": "8474397994", "first_seen_at": now - 200,
                       "rofl_sources": [host]}],
        })
        r = _Redis({ONE_OFF_SCRIM_REPLAY_KEY: record})

        async def run() -> None:
            with patch("relay.app._broadcast_replay_target", new=AsyncMock(return_value={"upload": False})), patch(
                "relay.app._guild_match_replay_target", new=AsyncMock(return_value={"upload": False})
            ):
                for game_id in ("8474323735", "8474397994", "KR-8474397994"):
                    target = await _replay_upload_target(SimpleNamespace(), host, game_id, r)
                    self.assertTrue(target["upload"])
                    self.assertEqual(target["targetKind"], "guild_match")
                self.assertEqual(await _replay_upload_target(SimpleNamespace(), host, "8474397995", r), {"upload": False})
                self.assertEqual(await _replay_upload_target(SimpleNamespace(), 123456789012345678, "8474323735", r), {"upload": False})
                self.assertEqual(await _replay_upload_target(SimpleNamespace(), host, "foo", r), {"upload": False})
        asyncio.run(run())

    def test_one_off_expired_or_untrusted_session_never_authorizes(self) -> None:
        import json
        from time import time
        host = 229144663720853504
        now = int(time())
        session = {"guild_id": 1485856315661226109, "created_at": now - 400,
                   "expires_at": now + 3600, "source_user_ids": [host],
                   "games": [{"game_id": "8474323735", "first_seen_at": now - 300}]}

        async def run() -> None:
            for key, value in (("expires_at", now - 1), ("guild_id", 1485856315661226110),
                               ("source_user_ids", []), ("games", [])):
                changed = {**session, key: value}
                r = _Redis({ONE_OFF_SCRIM_REPLAY_KEY: json.dumps(changed)})
                self.assertEqual(await _one_off_scrim_replay_target(r, host, "8474323735"), {"upload": False})
            self.assertEqual(await _one_off_scrim_replay_target(_Redis({}), host, "8474323735"), {"upload": False})
        asyncio.run(run())

    def test_missing_rofl_backfill_requests_only_valid_finished_game(self) -> None:
        import json
        from time import time
        host = 229144663720853504
        now = int(time())
        raw = json.dumps({"guild_id": 1485856315661226109,
                          "created_at": now - 100, "expires_at": now + 600,
                          "source_user_ids": [host],
                          "games": [{"game_id": "8474323735", "first_seen_at": now - 95,
                                     "ended": True}]})
        class ReplayRedis(_Redis):
            def __init__(self):
                super().__init__({ONE_OFF_SCRIM_REPLAY_KEY: raw,
                                  ONE_OFF_SCRIM_MISSING_KEY: json.dumps(["8474323735", "8474397994", "bad"])})
                self.claims = set()
            async def set(self, key, value, *, ex, nx):
                if key in self.claims: return False
                self.claims.add(key)
                return True
            async def delete(self, key):
                self.claims.discard(key)
        r = ReplayRedis()
        conn = SimpleNamespace(is_online=lambda did: did == host,
                               trust_level_for_discord=lambda _: "trusted",
                               send_command=AsyncMock(return_value=True))
        async def run():
            self.assertEqual(await _one_off_rofl_backfill_tick(r, conn), 1)
            conn.send_command.assert_awaited_once_with(host, {
                "type": "rofl_backfill_request", "game_id": "8474323735"})
            self.assertEqual(await _one_off_rofl_backfill_tick(r, conn), 0)
            conn.send_command.assert_awaited_once()
        asyncio.run(run())

    def test_missing_rofl_backfill_offline_and_new_device_are_skipped(self) -> None:
        r = _Redis({ONE_OFF_SCRIM_MISSING_KEY: '["8474323735"]'})
        conn = SimpleNamespace(is_online=lambda _: False,
                               trust_level_for_discord=lambda _: "trusted",
                               send_command=AsyncMock())
        async def run():
            self.assertEqual(await _one_off_rofl_backfill_tick(r, conn), 0)
            conn.is_online = lambda _: True
            conn.trust_level_for_discord = lambda _: "new_device"
            self.assertEqual(await _one_off_rofl_backfill_tick(r, conn), 0)
            conn.send_command.assert_not_awaited()
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
