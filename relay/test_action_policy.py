import re
import unittest
from pathlib import Path

from relay.actions import ACTION_POLICIES, validate_riot_identity_payload


class ActionPolicyContractTests(unittest.TestCase):
    def test_relay_policy_matches_agent_action_parser(self) -> None:
        source = Path("agent-tauri/src-tauri/src/relay/protocol.rs").read_text(encoding="utf-8")
        rust_actions = set(
            re.findall(r'"([a-z][a-z0-9_]*)"\s*=>\s*Self::[A-Za-z0-9_]+', source)
        )
        self.assertTrue(rust_actions, "Rust Action::parse entries were not found")
        self.assertEqual(set(ACTION_POLICIES), rust_actions)

    def test_policy_limits_are_bounded(self) -> None:
        for name, policy in ACTION_POLICIES.items():
            self.assertGreater(policy.timeout_sec, 0, name)
            self.assertLessEqual(policy.timeout_sec, 300, name)
            for key, limit in policy.list_limits:
                self.assertTrue(key, name)
                self.assertGreater(limit, 0, name)
                self.assertLessEqual(limit, 100, name)

    def test_identity_lookup_is_read_only_bounded_and_not_allowed_on_new_devices(self) -> None:
        policy = ACTION_POLICIES["resolve_riot_identity"]
        self.assertFalse(policy.allow_new_device)
        self.assertEqual(policy.list_limits, (("riot_ids", 5),))
        self.assertTrue(validate_riot_identity_payload({"riot_ids": ["Player#KR1"]}))
        for ids in ([], ["A#KR1"] * 6, ["A"], ["A#"], ["A#KR1", 42], ["A\n#KR1"], ["A#B#C"]):
            self.assertFalse(validate_riot_identity_payload({"riot_ids": ids}), ids)
