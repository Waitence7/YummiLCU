import unittest

from relay.app import _agent_release_asset


class AgentReleaseProxyMappingTests(unittest.TestCase):
    def test_stable_manifest_uses_stable_channel_pointer(self):
        asset = _agent_release_asset("/agent/version.json")
        self.assertIsNotNone(asset)
        self.assertEqual(
            asset[0],
            "https://github.com/Waitence7/YummiLCU/releases/latest/download/agent-version.json",
        )
        self.assertEqual(asset[1], "application/json")

    def test_stable_versioned_archive_uses_immutable_release(self):
        asset = _agent_release_asset("/agent/releases/tauri/tauri-0.7.6.zip")
        self.assertIsNotNone(asset)
        self.assertEqual(
            asset[0],
            "https://github.com/Waitence7/YummiLCU/releases/download/v0.7.6/tauri-0.7.6.zip",
        )

    def test_beta_and_dev_use_separate_channel_pointers(self):
        beta = _agent_release_asset("/agent/releases/tauri/beta/version.json")
        dev = _agent_release_asset("/agent/releases/tauri/dev/latest-setup.exe")
        self.assertIsNotNone(beta)
        self.assertIsNotNone(dev)
        self.assertEqual(beta[0], "channel://beta/agent-version.json")
        self.assertEqual(dev[0], "channel://dev/Yummi-LCU-Agent-latest-setup.exe")

    def test_legacy_paths_remain_supported(self):
        self.assertIsNotNone(_agent_release_asset("/agent/setup.exe"))
        self.assertIsNotNone(_agent_release_asset("/agent/latest"))
        self.assertIsNotNone(_agent_release_asset("/agent/latest.json"))
        legacy_zip = _agent_release_asset("/agent/YummiLcuTauri.zip")
        self.assertIsNotNone(legacy_zip)
        self.assertEqual(legacy_zip[0], "latest-archive://stable")

    def test_unknown_or_traversal_like_paths_are_rejected(self):
        self.assertIsNone(_agent_release_asset("/agent/nope"))
        self.assertIsNone(_agent_release_asset("/agent/releases/tauri/../../secret"))


if __name__ == "__main__":
    unittest.main()
