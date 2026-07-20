import unittest

from relay.app import _agent_hello_info
from relay.connections import ConnectionManager


class AgentHelloTests(unittest.TestCase):
    def test_legacy_agent_hello_uses_compatible_defaults(self) -> None:
        info = _agent_hello_info(
            {"type": "agent_hello", "version": "0.5.9", "os": "windows", "lcu_ready": True}
        )

        self.assertEqual(info["protocol_version"], 0)
        self.assertEqual(info["capabilities"], {})
        self.assertTrue(info["lcu_ready"])

    def test_invalid_capabilities_and_fields_are_safely_normalized(self) -> None:
        info = _agent_hello_info(
            {
                "type": "agent_hello",
                "version": None,
                "lcu_ready": "true",
                "protocol_version": True,
                "capabilities": {"runes": True, "rewards": "yes", 42: False},
            }
        )

        self.assertEqual(info["version"], "")
        self.assertEqual(info["os"], "")
        self.assertFalse(info["lcu_ready"])
        self.assertEqual(info["protocol_version"], 0)
        self.assertEqual(info["capabilities"], {"runes": True})


class _WebSocketStub:
    async def send_json(self, payload: dict[str, object]) -> None:
        self.payload = payload


class PendingAgentHelloTests(unittest.IsolatedAsyncioTestCase):
    async def test_hello_before_oauth_binding_is_preserved(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        info = {"version": "0.6.7", "capabilities": {"runes": True}}

        await manager.attach_session("session-1", websocket, "token")
        self.assertIsNone(await manager.set_agent_info_for_ws(websocket, info))
        self.assertTrue(await manager.bind_discord("session-1", 42))

        self.assertEqual(manager.agent_info(42), info)
