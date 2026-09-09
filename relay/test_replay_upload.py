from __future__ import annotations

import asyncio
import unittest
from types import SimpleNamespace

from fastapi import HTTPException

from relay.app import (
    _authenticate_agent_http,
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
