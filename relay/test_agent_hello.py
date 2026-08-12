import os
import unittest
from unittest.mock import patch

from relay import config
from relay.app import (
    MAX_WS_AUTH_MESSAGE_BYTES,
    _agent_hello_info,
    _read_ws_auth_payload,
    _safe_compare_digest,
)
from relay.connections import ConnectionManager


class AgentHelloTests(unittest.TestCase):
    def test_non_ascii_secret_comparison_fails_closed(self) -> None:
        self.assertFalse(_safe_compare_digest("１２３４５６", "１２３４５６"))

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
    def __init__(self) -> None:
        self.payload: dict[str, object] | None = None
        self.closed = False

    async def send_json(self, payload: dict[str, object]) -> None:
        self.payload = payload

    async def close(self, code: int = 1000) -> None:
        self.closed = code == 1000


class _AuthWebSocketStub:
    async def receive_text(self) -> str:
        return "x" * (MAX_WS_AUTH_MESSAGE_BYTES + 1)

    async def send_json(self, payload: dict[str, object]) -> None:
        self.payload = payload


class PendingAgentHelloTests(unittest.IsolatedAsyncioTestCase):
    async def test_oversized_auth_message_is_rejected(self) -> None:
        self.assertIsNone(await _read_ws_auth_payload(_AuthWebSocketStub()))

    async def test_hello_before_oauth_binding_is_preserved(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        info = {"version": "0.6.7", "capabilities": {"runes": True}}

        await manager.attach_session("session-1", websocket, "token")
        self.assertIsNone(await manager.set_agent_info_for_ws(websocket, info))
        self.assertTrue(await manager.bind_discord("session-1", 42))

        self.assertEqual(manager.agent_info(42), info)

    async def test_replacing_session_closes_previous_websocket(self) -> None:
        manager = ConnectionManager()
        previous = _WebSocketStub()
        current = _WebSocketStub()
        await manager.attach_session("session-1", previous, "token-1")
        self.assertTrue(await manager.bind_discord("session-1", 42))
        await manager.attach_session("session-1", current, "token-2")

        self.assertTrue(previous.closed)
        self.assertTrue(manager.has_active_session_ws("session-1"))

    async def test_live_game_updates_are_forwarded_only_to_subscribers(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.register_bot_ws(websocket)

        self.assertFalse(await manager.forward_live_game_update(42, {"participants": []}))
        manager.subscribe_live_game(42)
        self.assertTrue(
            await manager.forward_live_game_update(42, {"participants": [{"kills": 3}]})
        )
        self.assertEqual(websocket.payload["type"], "live_game_update")
        self.assertEqual(websocket.payload["data"]["participants"][0]["kills"], 3)


class RelayUrlSecurityTests(unittest.TestCase):
    def test_public_relay_requires_https_or_exact_loopback(self) -> None:
        with patch.dict("os.environ", {"RELAY_PUBLIC_BASE_URL": "https://relay.example"}):
            config.relay_public_base_url_must_be_https()
        with patch.dict("os.environ", {"RELAY_PUBLIC_BASE_URL": "http://localhost:8790"}):
            config.relay_public_base_url_must_be_https()
        with patch.dict(
            "os.environ", {"RELAY_PUBLIC_BASE_URL": "http://localhost.attacker.test"}
        ):
            with self.assertRaises(RuntimeError):
                config.relay_public_base_url_must_be_https()

    def test_compose_yummi_api_names_are_supported(self) -> None:
        with patch.dict(
            os.environ,
            {
                "YUMMI_API_BASE_URL": "http://api:4000",
                "YUMMI_BOT_INTERNAL_TOKEN": "secret",
            },
        ):
            self.assertEqual(config.tournament_api_base_url(), "http://api:4000")
            self.assertEqual(config.tournament_bot_internal_token(), "secret")
