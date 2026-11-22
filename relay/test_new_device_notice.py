"""Regression tests for repeated LCU automatic-login DMs."""

import asyncio
import json
import unittest
import uuid
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from relay.app import (
    _handle_bot_message,
    _new_device_notice_redis_key,
    _send_new_device_login_notice,
    _session_device_fingerprint_redis_key,
)


class RedisMemory:
    def __init__(self):
        self.values = {}

    async def get(self, key):
        return self.values.get(key)

    async def set(self, key, value, *, ex=None, nx=False):
        if nx and key in self.values:
            return False
        self.values[key] = str(value)
        return True

    async def exists(self, key):
        return int(key in self.values)

    async def delete(self, key):
        return int(self.values.pop(key, None) is not None)


class BotRelay:
    def __init__(self, fingerprint=None):
        self.events = []
        self.fingerprint = fingerprint
        self.connected = True

    async def send_bot_event(self, payload):
        await asyncio.sleep(0)  # force concurrent callers to interleave
        if not self.connected:
            return False
        self.events.append(payload)
        return True

    def device_fingerprint_for_ws(self, _ws):
        return self.fingerprint

    def device_fingerprint_for_discord(self, _discord_id):
        return self.fingerprint

    def device_id_for_ws(self, _ws):
        return None


class NewDeviceLoginNoticeTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.redis = RedisMemory()
        self.fingerprint = "a" * 64
        self.session_id = str(uuid.uuid4())
        self.discord_id = 1234567890
        self.relay = BotRelay(self.fingerprint)
        self.redis.values[_session_device_fingerprint_redis_key(self.session_id)] = self.fingerprint
        self.patches = (
            patch("relay.app._lookup_ip_geo", new=AsyncMock(return_value={"region": None, "city": None, "isp": None, "asn": None})),
            patch("relay.app.append_archive_record", new=AsyncMock()),
        )
        for obj in self.patches:
            obj.start()
            self.addCleanup(obj.stop)

    async def notify(self, session_id=None, websocket=None):
        return await _send_new_device_login_notice(
            self.relay, self.redis, http=None, websocket=websocket,
            discord_id=self.discord_id, session_id=session_id or self.session_id,
            riot_id="Player#KR1", source="auto",
        )

    async def test_concurrent_hello_requests_only_send_one_dm(self):
        await asyncio.gather(self.notify(), self.notify(), self.notify())
        self.assertEqual(len(self.relay.events), 1)
        self.assertIn(_new_device_notice_redis_key(self.discord_id, self.session_id), self.redis.values)
        self.assertIn(_new_device_notice_redis_key(self.discord_id, self.fingerprint), self.redis.values)

    async def test_same_session_still_deduplicated_when_fingerprint_expires(self):
        await self.notify()
        self.redis.values.pop(_session_device_fingerprint_redis_key(self.session_id))
        self.relay.fingerprint = None  # no verified info accessible on bot reconnect
        await self.notify()
        self.assertEqual(len(self.relay.events), 1)

    async def test_bot_reconnect_recovers_verified_fingerprint_if_redis_expired(self):
        await self.notify()
        self.redis.values.pop(_session_device_fingerprint_redis_key(self.session_id))
        await self.notify()
        self.assertEqual(len(self.relay.events), 1)
        self.assertEqual(
            self.redis.values[_session_device_fingerprint_redis_key(self.session_id)],
            self.fingerprint,
        )

    async def test_different_sessions_same_fingerprint_send_one_dm(self):
        second_session = str(uuid.uuid4())
        self.redis.values[_session_device_fingerprint_redis_key(second_session)] = self.fingerprint
        await self.notify()
        await _handle_bot_message(self.relay, self.redis, json.dumps({
            "type": "new_device_login_notice_sent", "discord_id": self.discord_id,
            "device_session_id": self.session_id,
        }))
        await self.notify(second_session)
        self.assertEqual(len(self.relay.events), 1)
        self.assertIn(_new_device_notice_redis_key(self.discord_id, second_session), self.redis.values)

    async def test_bot_disconnected_does_not_consume_notification(self):
        self.relay.connected = False
        await self.notify()
        self.assertNotIn(_new_device_notice_redis_key(self.discord_id, self.session_id), self.redis.values)
        self.assertNotIn(_new_device_notice_redis_key(self.discord_id, self.fingerprint), self.redis.values)
        self.relay.connected = True
        await self.notify()
        self.assertEqual(len(self.relay.events), 1)

    async def test_existing_fingerprint_notice_is_respected(self):
        self.redis.values[_new_device_notice_redis_key(self.discord_id, self.fingerprint)] = "1"
        await self.notify()
        self.assertEqual(len(self.relay.events), 0)
        self.assertIn(_new_device_notice_redis_key(self.discord_id, self.session_id), self.redis.values)

    async def test_sent_ack_writes_session_alias_and_fingerprint(self):
        await _handle_bot_message(self.relay, self.redis, json.dumps({
            "type": "new_device_login_notice_sent", "discord_id": self.discord_id,
            "device_session_id": self.session_id,
        }))
        self.assertEqual(self.redis.values[_new_device_notice_redis_key(self.discord_id, self.fingerprint)], "1")
        self.assertEqual(self.redis.values[_new_device_notice_redis_key(self.discord_id, self.session_id)], "1")
        self.redis.values.pop(_session_device_fingerprint_redis_key(self.session_id))
        self.relay.fingerprint = None
        await self.notify()
        self.assertEqual(len(self.relay.events), 0)


if __name__ == "__main__":
    unittest.main()
