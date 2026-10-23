import asyncio
import json
import os
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from relay import config
from relay.actions import action_policy
from relay.app import (
    MAX_WS_AUTH_MESSAGE_BYTES,
    _agent_hello_info,
    _agent_message_type_for_log,
    _agent_error_report,
    _agent_diagnostic_report,
    _agent_log_chunk,
    _agent_update_report,
    _agent_error_last_by_discord,
    _agent_error_recent,
    _handle_agent_message,
    _handle_bot_message,
    _read_ws_auth_payload,
    _safe_compare_digest,
    _server_hello,
)
from relay.connections import AGENT_SESSION_CONFLICT_CLOSE_CODE, ConnectionManager


class AgentHelloTests(unittest.TestCase):
    def test_invalid_message_types_do_not_disconnect_agent(self) -> None:
        for value in ([], {}, None, 42, True):
            with self.subTest(value=value):
                asyncio.run(_handle_agent_message(
                    SimpleNamespace(), ConnectionManager(), json.dumps({"type": value})
                ))

    def test_excessively_nested_json_is_ignored_at_all_ws_entrypoints(self) -> None:
        message = '{"type":' + '[' * 2000 + '0' + ']' * 2000 + '}'
        ws = SimpleNamespace(receive_text=AsyncMock(return_value=message))
        self.assertIsNone(asyncio.run(_read_ws_auth_payload(ws)))
        self.assertEqual(_agent_message_type_for_log(message), "unknown")
        asyncio.run(_handle_agent_message(ws, ConnectionManager(), message))
        asyncio.run(_handle_bot_message(ConnectionManager(), AsyncMock(), message))

    def test_json_recursion_errors_are_contained(self) -> None:
        ws = SimpleNamespace(receive_text=AsyncMock(return_value="{}"))
        with patch("relay.app.json.loads", side_effect=RecursionError):
            self.assertIsNone(asyncio.run(_read_ws_auth_payload(ws)))
            self.assertEqual(_agent_message_type_for_log("{}"), "unknown")
            asyncio.run(_handle_agent_message(ws, ConnectionManager(), "{}"))
            asyncio.run(_handle_bot_message(ConnectionManager(), AsyncMock(), "{}"))

    def test_oversized_json_integer_is_ignored(self) -> None:
        message = '{"type":' + '1' * 5000 + '}'
        self.assertEqual(_agent_message_type_for_log(message), "unknown")
        asyncio.run(_handle_agent_message(SimpleNamespace(), ConnectionManager(), message))

    def tearDown(self) -> None:
        _agent_error_last_by_discord.clear()
        _agent_error_recent.clear()

    def test_window_failures_and_ui_stalls_are_accepted(self) -> None:
        report = {
            "report_id": "123e4567-e89b-42d3-a456-426614174000",
            "occurred_at_ms": 1,
            "component": "window",
            "code": "command_failed",
            "summary": "drag_set_position: failed",
            "app_version": "0.7.16",
            "release_label": "0.7.16",
            "release_channel": "stable",
            "build_id": "build-1",
            "git_commit": "abc123",
        }
        self.assertIsNotNone(_agent_error_report(report))
        report["code"] = "ui_stall"
        self.assertIsNotNone(_agent_error_report(report))
        report["code"] = "unexpected_window_code"
        self.assertIsNone(_agent_error_report(report))

    def test_non_ascii_secret_comparison_fails_closed(self) -> None:
        self.assertFalse(_safe_compare_digest("１２３４５６", "１２３４５６"))

    def test_legacy_agent_hello_uses_compatible_defaults(self) -> None:
        info = _agent_hello_info(
            {"type": "agent_hello", "version": "0.5.9", "os": "windows", "lcu_ready": True}
        )

        self.assertEqual(info["protocol_version"], 0)
        self.assertEqual(info["capabilities"], {})
        self.assertTrue(info["lcu_ready"])

    def test_agent_hello_normalizes_lcu_identity_without_logging_secrets(self) -> None:
        info = _agent_hello_info(
            {
                "type": "agent_hello",
                "version": "0.7.32",
                "os": "windows",
                "lcu_ready": True,
                "protocol_version": 1,
                "capabilities": {},
                "bound_discord_id": 42,
                "puuid": "  puuid-123  ",
                "riot_id": " Name#KR1 ",
            }
        )

        self.assertEqual(info["bound_discord_id"], 42)
        self.assertEqual(info["puuid"], "puuid-123")
        self.assertEqual(info["riot_id"], "Name#KR1")

    def test_new_device_action_policy_is_narrow(self) -> None:
        for action in (
            "ping",
            "accept_match",
            "leave_lobby",
            "create_ranked_lobby",
            "create_normal_lobby",
            "invite_party_members",
            "check_party_members",
            "kick_party_member",
        ):
            with self.subTest(action=action):
                policy = action_policy(action)
                self.assertIsNotNone(policy)
                self.assertTrue(policy.allow_new_device)

        for action in ("queue_start", "champ_select_action", "dodge", "set_status"):
            with self.subTest(action=action):
                policy = action_policy(action)
                self.assertIsNotNone(policy)
                self.assertFalse(policy.allow_new_device)

    def test_server_hello_negotiates_only_mutual_capabilities(self) -> None:
        hello = _server_hello(
            {
                "protocol_version": 2,
                "capabilities": {
                    "event_ack": True,
                    "durable_event_replay": True,
                    "live_game_delta_v1": True,
                    "unexpected_error_reports": True,
                    "diagnostic_reports": True,
                    "diagnostic_log_chunks": True,
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
                "live_game_delta_v1": True,
                "unexpected_error_reports": True,
                "diagnostic_reports": True,
                "diagnostic_log_chunks": True,
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

    def test_agent_diagnostic_report_is_bounded_and_redacted(self) -> None:
        report = _agent_diagnostic_report({
            "type": "agent_diagnostic_report",
            "report_id": "123e4567-e89b-42d3-a456-426614174000",
            "occurred_at_ms": 1,
            "category": "lcu",
            "code": "authentication_failed",
            "detail": "password=secret endpoint=/lol-summoner/v1/current-summoner",
            "app_version": "0.7.14",
            "release_label": "0.7.14",
            "release_channel": "stable",
            "build_id": "build-1",
            "git_commit": "abc123",
        })
        self.assertIsNotNone(report)
        self.assertEqual(report["category"], "lcu")
        self.assertEqual(report["code"], "authentication_failed")
        self.assertNotIn("secret", report["detail"])
        self.assertIsNone(_agent_diagnostic_report({
            "report_id": "123e4567-e89b-42d3-a456-426614174000",
            "occurred_at_ms": 1,
            "category": "unknown",
            "code": "bad code!",
            "detail": "x",
        }))

    def test_agent_log_chunk_is_bounded_and_redacted(self) -> None:
        chunk = _agent_log_chunk({
            "type": "agent_log_chunk",
            "batch_id": "123e4567-e89b-42d3-a456-426614174000",
            "occurred_at_ms": 1,
            "source": "runtime_trace",
            "chunk_index": 0,
            "chunk_count": 1,
            "content": "phase=reader token=secret\nnext=line",
            "app_version": "0.7.29",
            "release_label": "0.7.29",
            "release_channel": "stable",
            "build_id": "build-1",
            "git_commit": "abc123",
        })
        self.assertIsNotNone(chunk)
        assert chunk is not None
        self.assertEqual(chunk["source"], "runtime_trace")
        self.assertIn("next=line", chunk["content"])
        self.assertNotIn("secret", chunk["content"])
        self.assertIsNone(_agent_log_chunk({
            "batch_id": "123e4567-e89b-42d3-a456-426614174000",
            "occurred_at_ms": 1,
            "source": "unknown",
            "chunk_index": 0,
            "chunk_count": 1,
            "content": "x",
        }))

    def test_agent_update_report_is_bounded_and_redacted(self) -> None:
        report = _agent_update_report({
            "type": "agent_update_report",
            "report_id": "123e4567-e89b-42d3-a456-426614174000",
            "occurred_at_ms": 1,
            "stage": "blocked",
            "detail": "gameflow_phase=WaitingForStats token=secret",
            "target_version": "0.7.6",
            "app_version": "0.7.4",
            "release_label": "0.7.4",
            "release_channel": "stable",
            "build_id": "build-1",
            "git_commit": "abc123",
        })
        self.assertIsNotNone(report)
        self.assertEqual(report["stage"], "blocked")
        self.assertEqual(report["target_version"], "0.7.6")
        self.assertNotIn("secret", report["detail"])
        self.assertIsNone(_agent_update_report({
            "report_id": "123e4567-e89b-42d3-a456-426614174000",
            "occurred_at_ms": 1,
            "stage": "unknown",
            "detail": "x",
        }))

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
        self.close_codes: list[int] = []
        self.app = SimpleNamespace(state=SimpleNamespace(http=object(), redis=object()))

    async def send_json(self, payload: dict[str, object]) -> None:
        self.payload = payload
        self.payloads.append(payload)

    async def close(self, code: int = 1000) -> None:
        self.closed = True
        self.close_codes.append(code)


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

    async def test_bound_agent_hello_refreshes_and_forwards_connected_status(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))

        with patch("relay.app.mark_lcu_linked", new=AsyncMock()), patch(
            "relay.app.mark_lcu_agent_version", new=AsyncMock()
        ) as mark_version, patch(
            "relay.app._forward_participant_status", new=AsyncMock()
        ) as forward_status:
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({
                    "type": "agent_hello",
                    "version": "0.7.10",
                    "os": "windows",
                    "lcu_ready": True,
                    "protocol_version": 1,
                    "capabilities": {},
                }),
            )

        status = manager.get_participant_status(42)
        self.assertIsNotNone(status)
        self.assertTrue(status["agent_online"])
        self.assertTrue(status["lcu_ready"])
        mark_version.assert_awaited_once_with(websocket.app.state.redis, 42, "0.7.10")
        forward_status.assert_awaited_once()
        self.assertEqual(forward_status.await_args.args[2], 42)
        self.assertTrue(forward_status.await_args.args[3]["lcu_ready"])

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

    async def test_bound_agent_diagnostic_is_stored_and_logged(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))

        with patch("relay.app._remember_agent_diagnostic", new=AsyncMock()) as remember, self.assertLogs(
            "yummi_lcu.relay", level="INFO"
        ) as logs:
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({
                    "type": "agent_diagnostic_report",
                    "report_id": "123e4567-e89b-42d3-a456-426614174000",
                    "occurred_at_ms": 1,
                    "category": "relay",
                    "code": "connection_closed",
                    "detail": "authenticated=true",
                    "app_version": "0.7.14",
                    "release_label": "0.7.14",
                    "release_channel": "stable",
                    "build_id": "build-1",
                    "git_commit": "abc123",
                }),
            )

        remember.assert_awaited_once()
        self.assertIn("category=relay", logs.output[0])
        self.assertIn("discord_id=42", logs.output[0])

    async def test_bound_agent_log_chunk_is_stored(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))

        with patch("relay.app._remember_agent_log_chunk", new=AsyncMock()) as remember:
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({
                    "type": "agent_log_chunk",
                    "batch_id": "123e4567-e89b-42d3-a456-426614174000",
                    "occurred_at_ms": 1,
                    "source": "flight_memory",
                    "chunk_index": 0,
                    "chunk_count": 1,
                    "content": "123 [relay] connected",
                    "app_version": "0.7.29",
                    "release_label": "0.7.29",
                    "release_channel": "stable",
                    "build_id": "build-1",
                    "git_commit": "abc123",
                }),
            )

        remember.assert_awaited_once()

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

    async def test_durable_rofl_is_acked_only_after_persist(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))
        event_id = "123e4567-e89b-42d3-a456-426614174020"
        payload = {"schemaVersion": 1, "kind": "movement", "gameId": "123", "part": 0}

        with patch("relay.app._forward_match_rofl", new=AsyncMock(return_value=True)):
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({"type": "match_rofl", "event_id": event_id, "data": payload}),
            )
        self.assertEqual(
            websocket.payloads[-1],
            {"type": "event_ack", "event_id": event_id},
        )

        before = len(websocket.payloads)
        with patch("relay.app._forward_match_rofl", new=AsyncMock(return_value=False)):
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({
                    "type": "match_rofl",
                    "event_id": "123e4567-e89b-42d3-a456-426614174021",
                    "data": payload,
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
        with patch("relay.app._forward_guild_match_eog", new=AsyncMock(return_value=False)), patch(
            "relay.app._forward_match_eog", new=AsyncMock(return_value=False)
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

        bot_ws = _WebSocketStub()
        await manager.register_bot_ws(bot_ws)
        manager.subscribe_gameflow(42)
        with patch("relay.app._forward_guild_match_eog", new=AsyncMock(return_value=False)), patch(
            "relay.app._forward_match_eog", new=AsyncMock(return_value=False)
        ), patch("relay.app.BOT_EOG_PERSIST_ACK_TIMEOUT_SEC", 0.01):
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

        with patch("relay.app._forward_guild_match_eog", new=AsyncMock(return_value=False)), patch(
            "relay.app._forward_match_eog", new=AsyncMock(return_value=False)
        ):
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


    async def test_guild_eog_unmatched_is_terminal_after_archive(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))
        event_id = "123e4567-e89b-42d3-a456-426614174099"

        with patch("relay.app._forward_guild_match_eog", new=AsyncMock(return_value=False)), patch(
            "relay.app._forward_match_eog", new=AsyncMock(return_value=True)
        ):
            await _handle_agent_message(
                websocket,
                manager,
                json.dumps({
                    "type": "guild_match_eog",
                    "event_id": event_id,
                    "data": {"participants": [{"summonerName": "test"}]},
                }),
            )

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

    async def test_discord_profile_is_stored_in_agent_info_on_bind(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        info = {"version": "0.7.22", "lcu_ready": False, "capabilities": {}}

        await manager.attach_session("session-1", websocket, "token")
        self.assertIsNone(await manager.set_agent_info_for_ws(websocket, info))
        self.assertTrue(
            await manager.bind_discord(
                "session-1",
                42,
                {"name": "Test User", "avatar": "https://example.invalid/avatar.png"},
            )
        )

        stored = manager.agent_info(42)
        self.assertEqual(stored["discord_name"], "Test User")
        self.assertEqual(stored["discord_avatar"], "https://example.invalid/avatar.png")

    async def test_agent_hello_refresh_preserves_discord_profile(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42, {"name": "Test User"}))

        await manager.set_agent_info_for_ws(
            websocket,
            {"version": "0.7.22", "lcu_ready": True, "capabilities": {}},
        )

        stored = manager.agent_info(42)
        self.assertEqual(stored["discord_name"], "Test User")
        self.assertEqual(stored["version"], "0.7.22")
        self.assertTrue(stored["lcu_ready"])

    async def test_reconnect_replaces_stale_offline_status_with_online_agent(self) -> None:
        manager = ConnectionManager()
        first = _WebSocketStub()
        await manager.attach_session("session-1", first, "token-1")
        self.assertTrue(await manager.bind_discord("session-1", 42))
        offline = await manager.unregister_ws(first)
        self.assertIsNotNone(offline)
        self.assertFalse(manager.get_participant_status(42)["agent_online"])

        second = _WebSocketStub()
        await manager.attach_session("session-2", second, "token-2")
        self.assertTrue(await manager.bind_discord("session-2", 42))

        status = manager.get_participant_status(42)
        self.assertIsNotNone(status)
        self.assertTrue(status["agent_online"])
        self.assertFalse(status["lcu_ready"])
        self.assertEqual(status["status"], "waiting")

    async def test_participant_status_keeps_agent_lcu_metadata_in_sync(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))
        await manager.set_agent_info_for_ws(
            websocket,
            {"version": "0.7.10", "lcu_ready": False, "capabilities": {}},
        )

        manager.set_participant_status(
            42,
            {"status": "lobby", "phase": "Lobby", "lcu_ready": True, "agent_online": True},
        )
        self.assertTrue(manager.agent_info(42)["lcu_ready"])

        manager.set_participant_status(
            42,
            {"status": "waiting", "phase": "None", "lcu_ready": False, "agent_online": True},
        )
        self.assertFalse(manager.agent_info(42)["lcu_ready"])

    async def test_gameflow_ready_signal_refreshes_stale_agent_metadata(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.attach_session("session-1", websocket, "token")
        self.assertTrue(await manager.bind_discord("session-1", 42))
        await manager.set_agent_info_for_ws(
            websocket,
            {"version": "0.7.10", "lcu_ready": False, "capabilities": {}},
        )

        self.assertFalse(await manager.forward_gameflow_update(
            42, {"phase": "ReadyCheck", "lcu_ready": True}
        ))
        self.assertTrue(manager.agent_info(42)["lcu_ready"])

    async def test_same_session_reconnect_replaces_stale_websocket(self) -> None:
        manager = ConnectionManager()
        previous = _WebSocketStub()
        current = _WebSocketStub()
        self.assertTrue(await manager.attach_session("session-1", previous, "token-1"))
        self.assertTrue(await manager.bind_discord("session-1", 42))

        # 이전 WS의 finally/unregister보다 재접속이 먼저 도착하는 레이스를 재현합니다.
        self.assertTrue(await manager.attach_session("session-1", current, "token-1"))
        self.assertTrue(await manager.bind_discord("session-1", 42))

        self.assertTrue(previous.closed)
        self.assertEqual(previous.close_codes, [AGENT_SESSION_CONFLICT_CLOSE_CODE])
        self.assertIsNone(manager.discord_id_for_ws(previous))
        self.assertEqual(manager.discord_id_for_ws(current), 42)
        self.assertTrue(manager.has_active_session_ws("session-1"))

        # 늦게 도착한 이전 WS 정리가 새 연결을 지우면 안 됩니다.
        self.assertIsNone(await manager.unregister_ws(previous))
        self.assertEqual(manager.discord_id_for_ws(current), 42)
        self.assertTrue(manager.is_online(42))

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

    async def test_live_game_subscription_replays_cached_snapshot(self) -> None:
        manager = ConnectionManager()
        websocket = _WebSocketStub()
        await manager.register_bot_ws(websocket)
        payload = {
            "game": {"time_seconds": 42},
            "participants": [{"kills": index} for index in range(10)],
        }
        manager.set_live_game(42, payload)
        manager.subscribe_live_game(42, "1771")

        self.assertTrue(await manager.replay_live_game_update(42))
        self.assertEqual(websocket.payload["type"], "live_game_update")
        self.assertEqual(websocket.payload["discord_id"], 42)
        self.assertEqual(websocket.payload["data"], payload)

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
