import hashlib
import json
import unittest
import uuid

from relay.app import (
    _issue_refresh_token,
    _refresh_token_hash,
    _refresh_token_redis_key,
    _restore_session_from_refresh,
    _revoke_refresh_token,
    _session_auth_source_redis_key,
    _session_redis_key,
    _session_refresh_token_hash_redis_key,
)


class FakeRedis:
    def __init__(self) -> None:
        self.values: dict[str, str] = {}

    async def get(self, key: str):
        return self.values.get(key)

    async def set(self, key: str, value, **_kwargs):
        self.values[key] = value if isinstance(value, str) else str(value)
        return True

    async def delete(self, *keys: str):
        removed = 0
        for key in keys:
            removed += int(self.values.pop(key, None) is not None)
        return removed


class FakeConnections:
    def __init__(self, token_hash: str | None) -> None:
        self.token_hash = token_hash

    def session_refresh_token_hash(self, _session_id: str) -> str | None:
        return self.token_hash


class RefreshSessionTests(unittest.IsolatedAsyncioTestCase):
    async def test_issue_stores_only_hash_and_no_expiry_marker(self) -> None:
        redis = FakeRedis()
        session_id = str(uuid.uuid4())
        fingerprint = "a" * 64

        token = await _issue_refresh_token(redis, session_id, 42, fingerprint)
        token_hash = _refresh_token_hash(token)

        self.assertEqual(token_hash, hashlib.sha256(token.encode()).hexdigest())
        self.assertEqual(
            redis.values[_session_refresh_token_hash_redis_key(session_id)],
            token_hash,
        )
        record = json.loads(redis.values[_refresh_token_redis_key(token_hash)])
        self.assertEqual(record["discord_id"], 42)
        self.assertEqual(record["device_fingerprint"], fingerprint)
        self.assertNotIn(token, redis.values.values())

    async def test_refresh_restores_expired_short_session_on_same_device(self) -> None:
        redis = FakeRedis()
        session_id = str(uuid.uuid4())
        fingerprint = "b" * 64
        token = await _issue_refresh_token(redis, session_id, 823814708330627073, fingerprint)
        conn = FakeConnections(_refresh_token_hash(token))

        restored = await _restore_session_from_refresh(
            conn, redis, session_id, fingerprint
        )

        self.assertEqual(restored, 823814708330627073)
        self.assertEqual(
            redis.values[_session_redis_key(session_id)],
            "823814708330627073",
        )
        self.assertEqual(
            redis.values[_session_auth_source_redis_key(session_id)],
            "refresh",
        )


    async def test_explicit_logout_revokes_refresh_and_short_session(self) -> None:
        redis = FakeRedis()
        session_id = str(uuid.uuid4())
        fingerprint = "e" * 64
        token = await _issue_refresh_token(redis, session_id, 42, fingerprint)
        redis.values[_session_redis_key(session_id)] = "42"

        revoked = await _revoke_refresh_token(redis, session_id, token)

        self.assertTrue(revoked)
        self.assertNotIn(
            _refresh_token_redis_key(_refresh_token_hash(token)), redis.values
        )
        self.assertNotIn(_session_refresh_token_hash_redis_key(session_id), redis.values)
        self.assertNotIn(_session_redis_key(session_id), redis.values)

    async def test_refresh_rejects_different_device_fingerprint(self) -> None:
        redis = FakeRedis()
        session_id = str(uuid.uuid4())
        token = await _issue_refresh_token(redis, session_id, 42, "c" * 64)
        conn = FakeConnections(_refresh_token_hash(token))

        restored = await _restore_session_from_refresh(
            conn, redis, session_id, "d" * 64
        )

        self.assertIsNone(restored)
        self.assertNotIn(_session_redis_key(session_id), redis.values)


if __name__ == "__main__":
    unittest.main()
