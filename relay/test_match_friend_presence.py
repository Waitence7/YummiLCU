import asyncio
import json
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock, patch
from relay.app import MatchFriendPresenceBody, internal_match_friend_presence
from relay.actions import validate_match_friend_presence_payload
from fastapi import HTTPException

class FakeRedis:
    def __init__(self):
        self.data = {}
        self.ttl = {}
    async def set(self, key, value, ex=None, nx=False):
        if nx and key in self.data:
            return False
        self.data[key] = value
        self.ttl[key] = ex
        return True
    async def get(self, key):
        return self.data.get(key)
    async def mget(self, keys):
        return [self.data.get(key) for key in keys]
    async def delete(self, key):
        self.data.pop(key, None)

class FriendPresenceTests(unittest.TestCase):
    def test_payload(self):
        self.assertTrue(validate_match_friend_presence_payload({"riot_ids":["A#KR1"]}))
        for ids in ([],["A#B#C"],["A#"],["A#KR1"]*51,["A\n#KR1"],[2]):
            self.assertFalse(validate_match_friend_presence_payload({"riot_ids":ids}))
    def test_match_scope_consent_revocation_and_connection(self):
        async def run():
            r = FakeRedis()
            conn = Mock()
            online = {1:True,2:False,3:False}
            connection = {1:"c1",2:None,3:None}
            conn.is_online.side_effect = lambda did: online.get(did, False)
            conn.connection_quality.side_effect = lambda did: {"connection_id":connection.get(did)}
            conn.trust_level_for_discord.return_value = "trusted"
            conn.agent_info.return_value = {"lcu_ready":True,"capabilities":{"match_friend_presence_v1":True}}
            request = SimpleNamespace(app=SimpleNamespace(state=SimpleNamespace(connections=conn,redis=r)))
            async def call(actor,mode="read",match="m1",ids=None,**kwargs):
                body = MatchFriendPresenceBody(match_id=match,actor=str(actor),ids=ids or ["1","2","3"],mode=mode,**kwargs)
                return json.loads((await internal_match_friend_presence(request,body,"test")).body)
            with patch("relay.app._verify_internal_secret",new=AsyncMock()):
                await call(1,"consent",enabled=True)
                await call(2,"consent",enabled=True)
                lease = (await call(1,"lease"))["lease"]
                self.assertTrue(lease)
                self.assertIsNone((await call(1,"lease"))["lease"])
                await call(1,"store",lease=lease,online={"2":"Alt#KR1","3":"NonConsent#KR1","4":"Outside#KR1"},offline={"2":"Alt#KR1"},addable={"2":"Alt#KR1","3":"NonConsent#KR1"})
                source_view = await call(1)
                self.assertEqual(source_view["privateViewer"],"1")
                self.assertEqual(source_view["observations"],{"2":["Alt#KR1"]})
                self.assertEqual(source_view["offline"],{"2":["Alt#KR1"]})
                self.assertEqual(source_view["addable"],{"2":["Alt#KR1"]})
                self.assertEqual((await call(2))["offline"],{})
                self.assertEqual((await call(3))["addable"],{})
                # Neither a consenting target nor a non-sharing participant
                # can read the source Agent's private Flist observation.
                self.assertEqual((await call(2))["observations"],{})
                self.assertEqual((await call(3))["observations"],{})
                self.assertIsNone((await call(3,"lease"))["lease"])
                with self.assertRaises(HTTPException) as denied:
                    await call(4)
                self.assertEqual(denied.exception.status_code,400)
                self.assertEqual((await call(2,match="m2"))["observations"],{})
                self.assertEqual((await call(1,ids=["1","3"]))["observations"],{})
                self.assertTrue(any(v==40 for v in r.ttl.values()))
                # A different Agent in the same match gets its own independent view.
                online[3]=True
                connection[3]="c3"
                await call(3,"consent",enabled=True)
                lease3 = (await call(3,"lease"))["lease"]
                self.assertTrue(lease3)
                await call(3,"store",lease=lease3,online={"1":"Main#KR1"})
                self.assertEqual((await call(3))["observations"],{"1":["Main#KR1"]})
                self.assertEqual((await call(1))["observations"],{"2":["Alt#KR1"]})
                self.assertEqual((await call(2))["observations"],{})
                connection[1]="c1-new"
                self.assertEqual((await call(1))["observations"],{})
                connection[1]="c1"
                # Legacy shared-cache keys must not be read by private mode.
                key = "lcu:match_friend_presence:v1:m1:private-observed:1:c1"
                saved = r.data.pop(key)
                r.data["lcu:match_friend_presence:v1:m1:observed:1:c1"] = saved
                self.assertEqual((await call(1))["observations"],{})
                r.data[key] = saved
                await call(2,"consent",enabled=False)
                self.assertEqual((await call(1))["observations"],{})
                await call(2,"consent",enabled=True)
                self.assertEqual((await call(1))["observations"],{})
        asyncio.run(run())
