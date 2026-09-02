import asyncio
import json
import os
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from relay import config
from relay.app import (
    MAX_WS_AUTH_MESSAGE_BYTES,
    _agent_hello_info,
    _agent_error_report,
    _agent_error_last_by_discord,
    _agent_error_recent,
    _handle_agent_message,
    _handle_bot_message,
    _read_ws_auth_payload,
    _safe_compare_digest,
    _server_hello,
)
from relay.connections import ConnectionManager


class AgentHelloTests(unittest.TestCase):
    def tearDown(self) -> None:
        _agent_error_last_by_discord.clear()
        _agent_error_recent.clear()

    def test_non_ascii_secret_comparison_fails_closed(self) -> None:
        self.assertFalse(_safe_compare_digest("１２３４５６", "１２３４５６"))

    def test_legacy_agent_hello_uses_compatible_defaults(self) -> None:
        info = _agent_hello_info(
            {"type": "agent_hello", "version": "0.5.9", "os": "windows", "lcu_ready": True}
        )

        self.assertEqual(info["protocol_version"], 0)
        self.assertEqual(info["capabilities"], {})
        self.assertTrue(info["lcu_ready"])

    def test_server_hello_negotiates_only_mutual_capabilities(self) -> None:
        hello = _server_hello(
            {
                "protocol_version": 2,
                "capabilities": {
                    "event_ack": True,
                    "durable_event_replay": True,
                    "unexpected_error_reports": True,
                    "unknown_future_feature": True,
                    "heartbeat": False,
                },
            }
        )
        self.assertEqual(hello["protocol_version"], 1)
        self.assertEqual(
            hello["capabilities"],
            {
                "durable_event_replay": True,
                "event_ack": True,
                "unexpected_error_reports": True,
            },
        )

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

    def test_agent_error_report_is_strictly_bounded_and_redacted(self) -> None:
        report = _agent_error_report({
            "type": "agent_error_report",
            "report_id": "123e4567-e89b-42d3-a456-426614174000",
            "occurred_at_ms": 1,
            "component": "updater",
            "code": "apply_failed",
            "summary": "failed\ntoken=secret keep=yes",
            "app_version": "0.6.14",
            "release_label": "0.6.14-beta",
            "release_channel": "beta",
            "build_id": "build-1",
            "git_commit": "abc123",
        })

        self.assertIsNotNone(report)
        self.assertNotIn("secret", report["summary"])
        self.assertNotIn("\n", report["summary"])
        self.assertIsNone(_agent_error_report({
            "report_id": "not-a-uuid",
            "occurred_at_ms": 1,
            "component": "updater",
            "code": "apply_failed",
            "summary": "x",
        }))


class _WebSocketStub:
    def __init__(self) -> None:
        self.payload: dict[str, object] | None = None
        self.payloads: list[dict[str, object]] = []
        self.closed = False
        self.app = SimpleNamespace(state=SimpleNamespace(http=object(), redis=object()))

    async def send_json(self, payload: dict[str, object]) -> None:
        self.payload = payload
        self.payloads.append(payload)

    async def close(self, code: int = 1000) -> None:
        self.closed = code == 1000


class _AuthWebSocketStub:
    async def receive_text(self) -> str:
        return "x" * (MAX_WS_AUTH_MESSAGE_BYTES + 1)

    async def send_json(self, payload: dict[str, object]) -> None:
        self.payload = payload


class PendingAgentHelloTests(unittest.IsolatedAsyncioTestCase):
    def tearDown(self) -> None:
        _agent_error_last_by_discord.clear()
        _agent_error_recent.clear()

    async def test_oversized_auth_message_is_rejected(self) -> None:
        self.assertIsNone(await _read_ws_auth_payload(_AuthWebSocketStub()))

    async def test_agent_hello_handler_returns_negotiated_server_hello(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")

        await _handle_agent_message(
            websocket,
            manager,
            json.dumps({
                "type": "agent_hello",
                "version": "0.6.14",
                "os": "windows",
                "lcu_ready": True,
                "protocol_version": 1,
                "capabilities": {
                    "event_ack": True,
                    "durable_event_replay": True,
                    "unexpected_error_reports": True,
                },
            }),
        )

        self.assertEqual(websocket.payloads[-1]["type"], "server_hello")
        self.assertTrue(websocket.payloads[-1]["capabilities"]["event_ack"])
        self.assertTrue(
            websocket.payloads[-1]["capabilities"]["unexpected_error_reports"]
        )

    async def test_bound_agent_can_report_only_minimal_unexpected_error(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))

        with self.assertLogs("yummi_lcu.relay", level="ERROR") as logs:
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({
                    "type": "agent_error_report",
                    "report_id": "123e4567-e89b-42d3-a456-426614174000",
                    "occurred_at_ms": 1,
                    "component": "ui",
                    "code": "uncaught_error",
                    "summary": "render failed",
                    "app_version": "0.6.14",
                    "release_label": "0.6.14-beta",
                    "release_channel": "beta",
                    "build_id": "build-1",
                    "git_commit": "abc123",
                }),
            )

        self.assertIn("component=ui", logs.output[0])
        self.assertNotIn("discord_id", logs.output[0])

    async def test_durable_eog_is_acked_only_after_forward_succeeds(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))
        event_id = "123e4567-e89b-42d3-a456-426614174000"

        with patch("relay.app._forward_match_eog", new=AsyncMock(return_value=True)):
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({
                    "type": "match_eog",
                    "event_id": event_id,
                    "data": {"participants": []},
                }),
            )
        self.assertEqual(
            websocket.payloads[-1],
            {"type": "event_ack", "event_id": event_id},
        )

        before = len(websocket.payloads)
        with patch("relay.app._forward_match_eog", new=AsyncMock(return_value=False)):
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({
                    "type": "match_eog",
                    "event_id": "123e4567-e89b-42d3-a456-426614174001",
                    "data": {"participants": []},
                }),
            )
        self.assertEqual(len(websocket.payloads), before)

    async def test_guild_eog_ack_requires_durable_web_or_bot_persist(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))
        event_id = "123e4567-e89b-42d3-a456-426614174010"

        before = len(websocket.payloads)
        with patch("relay.app._forward_guild_match_eog", new=AsyncMock(return_value=False)):
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({
                    "type": "guild_match_eog",
                    "event_id": event_id,
                    "data": {"participants": []},
                }),
            )
        self.assertEqual(len(websocket.payloads), before)

        bot_ws = _WebSocketStub()
        await manager.register_bot_ws(bot_ws)
        manager.subscribe_gameflow(42)
        with patch("relay.app._forward_guild_match_eog", new=AsyncMock(return_value=False)), patch(
            "relay.app.BOT_EOG_PERSIST_ACK_TIMEOUT_SEC", 0.01
        ):
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({
                    "type": "guild_match_eog",
                    "event_id": event_id,
                    "data": {"participants": []},
                }),
            )
        self.assertEqual(len(websocket.payloads), before)
        self.assertEqual(bot_ws.payloads[-1]["type"], "guild_match_eog")
        self.assertEqual(bot_ws.payloads[-1]["event_id"], event_id)

        with patch("relay.app._forward_guild_match_eog", new=AsyncMock(return_value=False)):
            task = asyncio.create_task(
                _handle_agent_message(
                    websocket,
                    manager,
                    json.dumps({
                        "type": "guild_match_eog",
                        "event_id": event_id,
                        "data": {"participants": []},
                    }),
                )
            )
            for _ in range(20):
                if manager._pending_bot_eog.get(event_id) is not None:
                    break
                await asyncio.sleep(0)
            self.assertTrue(manager.complete_pending_bot_eog(event_id, True))
            await task
        self.assertEqual(
            websocket.payloads[-1],
            {"type": "event_ack", "event_id": event_id},
        )

    async def test_bot_persistence_message_completes_event_waiter(self) -> None:
        manager = ConnectionManager()
        event_id = "123e4567-e89b-42d3-a456-426614174011"
        pending = manager.register_pending_bot_eog(event_id)
        await _handle_bot_message(
            manager,
            None,  # this message type does not touch Redis
            json.dumps({
                "type": "guild_match_eog_persisted",
                "event_id": event_id,
                "ok": True,
            }),
        )
        self.assertTrue(await pending)


    async def test_hello_before_oauth_binding_is_preserved(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        info = {"version": "0.6.7", "capabilities": {"runes": True}}

        await manager.attach_session("session-1", websocket, "token")
        self.assertIsNone(await manager.set_agent_info_for_ws(websocket, info))
        self.assertTrue(await manager.bind_discord("session-1", 42))

        self.assertEqual(manager.agent_info(42), info)

    async def test_duplicate_session_keeps_first_websocket_active(self) -> None:
        manager = ConnectionManager()
        previous = _WebSocketStub()
        current = _WebSocketStub()
        self.assertTrue(await manager.attach_session("session-1", previous, "token-1"))
        self.assertTrue(await manager.bind_discord("session-1", 42))
        self.assertFalse(await manager.attach_session("session-1", current, "token-2"))

        self.assertFalse(previous.closed)
        self.assertEqual(manager.discord_id_for_ws(previous), 42)
        self.assertIsNone(manager.discord_id_for_ws(current))
        self.assertTrue(manager.has_active_session_ws("session-1"))

    async def test_live_game_updates_are_forwarded_only_to_subscribers(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.register_bot_ws(websocket)

        self.assertFalse(await manager.forward_live_game_update(42, {"participants": []}))
        manager.subscribe_live_game(42)
        self.assertTrue(
            await manager.forward_live_game_update(
                42, {"participants": [{"kills": 3} for _ in range(10)]}
            )
        )
        self.assertEqual(websocket.payload["type"], "live_game_update")
        self.assertEqual(websocket.payload["data"]["participants"][0]["kills"], 3)

    async def test_partial_live_game_updates_are_not_cached_or_forwarded(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.register_bot_ws(websocket)
        manager.subscribe_live_game(42)

        self.assertFalse(
            await manager.forward_live_game_update(42, {"participants": [{"kills": 3}]})
        )
        self.assertIsNone(manager.get_live_game(42))
        self.assertEqual(websocket.payloads, [])

    async def test_live_game_update_is_cached_without_bot_subscription(self) -> None:
        manager = ConnectionManager()
        payload = {
            "game": {"time_seconds": 42},
            "participants": [{"summoner_name": f"P{index}"} for index in range(10)],
        }

        self.assertFalse(await manager.forward_live_game_update(42, payload))
        cached = manager.get_live_game(42)
        self.assertIsNotNone(cached)
        self.assertEqual(cached["data"], payload)

    async def test_agent_polling_follows_live_game_subscribers(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()

        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))
        self.assertEqual(websocket.payloads[-1], {"type": "live_game_polling", "enabled": False})

        manager.subscribe_live_game(42)
        self.assertTrue(await manager.sync_live_game_polling(42))
        self.assertEqual(websocket.payloads[-1], {"type": "live_game_polling", "enabled": True})

        manager.unsubscribe_live_game(42)
        self.assertTrue(await manager.sync_live_game_polling(42))
        self.assertEqual(websocket.payloads[-1], {"type": "live_game_polling", "enabled": False})


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
