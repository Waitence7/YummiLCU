import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from relay.app import (
    _oauth_link_attempt_redis_key,
    _oauth_link_pending_redis_key,
    auth_callback,
)


class OAuthAutoBindTests(unittest.IsolatedAsyncioTestCase):
    async def test_callback_binds_active_agent_without_manual_code(self) -> None:
        session_id = "12345678-1234-1234-1234-123456789012"
        redis = SimpleNamespace(
            getdel=AsyncMock(return_value=session_id),
            set=AsyncMock(),
            delete=AsyncMock(),
        )
        connections = object()
        request = SimpleNamespace(
            app=SimpleNamespace(
                state=SimpleNamespace(
                    redis=redis,
                    connections=connections,
                    http=object(),
                )
            )
        )

        with (
            patch(
                "relay.app.auth.exchange_code",
                new=AsyncMock(return_value={"access_token": "access-token"}),
            ),
            patch(
                "relay.app.auth.fetch_discord_user",
                new=AsyncMock(return_value={"id": "123456789", "username": "Tester"}),
            ),
            patch("relay.app.config.relay_session_ttl_sec", return_value=3600),
            patch("relay.app._try_bind_discord", new=AsyncMock(return_value=True)) as bind,
        ):
            response = await auth_callback(
                request,
                code="discord-authorization-code",
                state="oauth-state",
            )

        self.assertEqual(response.status_code, 200)
        body = response.body.decode("utf-8")
        self.assertIn("자동으로 연결", body)
        self.assertNotIn("6자리", body)
        self.assertNotIn("코드", body)
        bind.assert_awaited_once_with(connections, redis, session_id, 123456789)
        redis.delete.assert_awaited_once_with(
            _oauth_link_pending_redis_key(session_id),
            _oauth_link_attempt_redis_key(session_id),
        )


if __name__ == "__main__":
    unittest.main()
