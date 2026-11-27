import asyncio
import json
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock, patch

from relay.app import _handle_agent_message, internal_league_presence, league_presence_key, league_presence_riot_id

class LeaguePresenceTests(unittest.TestCase):
    def test_strict_presence_payload(self):
        self.assertEqual(league_presence_riot_id({"online": True, "riot_id": " A#KR1 "}), "A#KR1")
        for payload in ({"online": False, "riot_id": "A#KR1"}, {"online": 1, "riot_id": "A#KR1"}, {"online": True, "riot_id": "A#"}, {"online": True, "riot_id": "A#B#C"}, {"online": True, "riot_id": "A\nB#KR1"}):
            self.assertIsNone(league_presence_riot_id(payload))

    def test_only_fresh_current_connection_is_returned(self):
        async def run():
            connection = Mock()
            connection.is_online.side_effect = lambda did: did != 2
            connection.connection_quality.side_effect = lambda did: {"connection_id": f"c{did}"}
            redis = SimpleNamespace(mget=AsyncMock(return_value=["A#KR1", None]))
            request = SimpleNamespace(app=SimpleNamespace(state=SimpleNamespace(connections=connection, redis=redis)))
            with patch("relay.app._verify_internal_secret", new=AsyncMock()):
                result = await internal_league_presence(request, ids="1,2,3", x_relay_internal_secret="test")
            self.assertEqual(json.loads(result.body), {"online": {"1": "A#KR1"}})
            redis.mget.assert_awaited_once_with([league_presence_key(1,"c1"),league_presence_key(3,"c3")])
        asyncio.run(run())

    def test_replaced_connection_during_read_is_not_returned(self):
        async def run():
            connection = Mock()
            connection.is_online.return_value = True
            connection.connection_quality.side_effect = [{"connection_id":"old"}, {"connection_id":"new"}]
            redis = SimpleNamespace(mget=AsyncMock(return_value=["A#KR1"]))
            request = SimpleNamespace(app=SimpleNamespace(state=SimpleNamespace(connections=connection,redis=redis)))
            with patch("relay.app._verify_internal_secret", new=AsyncMock()):
                result = await internal_league_presence(request, ids="1", x_relay_internal_secret="test")
            self.assertEqual(json.loads(result.body), {"online": {}})
        asyncio.run(run())
