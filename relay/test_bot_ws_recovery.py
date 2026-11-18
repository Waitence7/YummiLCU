"""WebSocket send failure invalidates transport so the Bot reconnects."""
import asyncio
import unittest
from unittest.mock import AsyncMock

from relay.connections import ConnectionManager


class WsStub:
    def __init__(self, *, fail=False):
        self.sent = []
        self.closed = False
        self.codes = []
        self.fail = fail
        self.in_flight = 0
        self.max_in_flight = 0

    async def send_json(self, payload):
        self.in_flight += 1
        self.max_in_flight = max(self.max_in_flight, self.in_flight)
        try:
            await asyncio.sleep(0.002)
            if self.fail:
                raise asyncio.TimeoutError('simulated backpressure')
            self.sent.append(payload)
        finally:
            self.in_flight -= 1

    async def close(self, code=1000):
        self.closed = True
        self.codes.append(code)


class BotWsRecoveryTests(unittest.IsolatedAsyncioTestCase):
    async def test_failed_send_actually_closes_stale_transport_and_next_ws_works(self):
        manager = ConnectionManager()
        old = WsStub(fail=True)
        await manager.register_bot_ws(old)
        manager.subscribe_live_game(42)
        live = {'game': {'id': '8470000001'}, 'participants': [{} for _ in range(10)]}
        self.assertFalse(await manager.forward_live_game_update(42, live))
        self.assertFalse(manager.bot_ws_connected())
        self.assertTrue(old.closed)
        self.assertIn(1011, old.codes)
        fresh = WsStub()
        await manager.register_bot_ws(fresh)
        self.assertTrue(await manager.forward_live_game_update(42, live))
        self.assertEqual(fresh.sent[-1]['type'], 'live_game_update')

    async def test_new_ws_replaces_old_and_stale_cleanup_cannot_evict_replacement(self):
        manager = ConnectionManager()
        old, fresh = WsStub(), WsStub()
        await manager.register_bot_ws(old)
        pending = manager.register_pending_bot_eog("pending-eog")
        await manager.register_bot_ws(fresh)
        self.assertFalse(await pending)
        self.assertTrue(old.closed)
        self.assertIn(1001, old.codes)
        await manager.unregister_bot_ws(old)
        self.assertTrue(manager.bot_ws_connected())
        self.assertTrue(await manager.send_bot_event({'type': 'test'}))
        self.assertEqual(fresh.sent[-1]['type'], 'test')

    async def test_stale_send_failure_does_not_evict_new_connection(self):
        manager = ConnectionManager()
        old = WsStub()
        replacement = WsStub()
        await manager.register_bot_ws(old)

        async def fail_after_replacement(payload):
            await manager.register_bot_ws(replacement)
            raise ConnectionError("stale old socket failed after replacement")

        old.send_json = fail_after_replacement
        self.assertFalse(await manager.send_bot_event({"type": "old"}))
        self.assertTrue(manager.bot_ws_connected())
        self.assertTrue(old.closed)
        self.assertTrue(await manager.send_bot_event({"type": "new"}))
        self.assertEqual(replacement.sent[-1]["type"], "new")

    async def test_concurrent_send_calls_are_serialized(self):
        manager = ConnectionManager()
        ws = WsStub()
        await manager.register_bot_ws(ws)
        result = await asyncio.gather(*[
            manager.send_bot_event({'type': 'test', 'seq': i}) for i in range(15)
        ])
        self.assertTrue(all(result))
        self.assertEqual(ws.max_in_flight, 1)
        self.assertEqual(len(ws.sent), 15)
