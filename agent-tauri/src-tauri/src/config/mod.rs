use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AgentError, AgentResult};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Config {
    pub relay_public_base_url: String,
    // Retained for agent.json compatibility; the WebSocket OAuth flow no longer polls HTTP.
    pub auth_poll_interval_ms: u64,
    pub lockfile_path: Option<String>,
    pub prevent_queue_after_dodge: bool,
    // Retained for compatibility until LCU event handling applies this preference.
    pub apply_default_status_on_connect: bool,
    // Retained for compatibility until ready-check event handling applies this preference.
    pub auto_accept_match: bool,
    // Retained for compatibility until process-follow behavior is implemented.
    pub follow_league_client: bool,
    pub update_manifest_url: Option<String>,
    pub check_updates_on_startup: bool,
    pub auto_update_enabled: bool,
    pub saved_session_max_age_days: u64,
    pub run_at_windows_startup: bool,
    pub ui_test_mode: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            relay_public_base_url: "https://yummi.duckdns.org".into(),
            auth_poll_interval_ms: 1500,
            lockfile_path: None,
            prevent_queue_after_dodge: true,
            apply_default_status_on_connect: true,
            auto_accept_match: false,
            follow_league_client: true,
            update_manifest_url: Some("https://yummi.duckdns.org/agent/version.json".into()),
            check_updates_on_startup: true,
            auto_update_enabled: true,
            saved_session_max_age_days: 14,
            run_at_windows_startup: true,
            ui_test_mode: false,
        }
    }
}

impl Config {
    fn path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(ToOwned::to_owned))
            .unwrap_or_default()
            .join("agent.json")
    }

    fn secure_url(raw: &str) -> String {
        let value = raw.trim().trim_end_matches('/');
        if value.starts_with("http://localhost") || value.starts_with("http://127.0.0.1") {
            return value.into();
        }
        value.replacen("http://", "https://", 1)
    }

    pub(crate) fn normalize(&mut self) {
        self.relay_public_base_url = Self::secure_url(&self.relay_public_base_url);
        self.update_manifest_url = self
            .update_manifest_url
            .as_ref()
            .map(|value| Self::secure_url(value));
    }

    pub(crate) fn load() -> Self {
        let mut config = fs::read_to_string(Self::path())
            .ok()
            .and_then(|raw| serde_json::from_str::<Self>(&raw).ok())
            .unwrap_or_default();
        config.normalize();
        config
    }

    pub(crate) fn save(&self) -> AgentResult<()> {
        fs::write(
            Self::path(),
            serde_json::to_string_pretty(self)
                .map_err(|error| AgentError::Config(error.to_string()))?,
        )?;
        Ok(())
    }

    pub(crate) fn ws_url(&self, session_id: &str) -> AgentResult<String> {
        let url = url::Url::parse(&self.relay_public_base_url)
            .map_err(|error| AgentError::Relay(error.to_string()))?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        Ok(format!(
            "{}://{}/ws/agent?session_id={}",
            scheme,
            url.host_str().unwrap_or_default(),
            urlencoding(session_id)
        ))
    }

    pub(crate) fn login_url(&self, session_id: &str) -> String {
        format!(
            "{}/login?session_id={}",
            self.relay_public_base_url,
            urlencoding(session_id)
        )
    }
}

fn urlencoding(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('?', "%3F")
        .replace('&', "%26")
        .replace('=', "%3D")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_agent_json_uses_defaults_for_missing_fields() {
        let config: Config = serde_json::from_str(
            r#"{
                "RelayPublicBaseUrl": "https://relay.example",
                "PreventQueueAfterDodge": false
            }"#,
        )
        .unwrap();

        assert_eq!(config.relay_public_base_url, "https://relay.example");
        assert!(!config.prevent_queue_after_dodge);
        assert_eq!(config.auth_poll_interval_ms, 1500);
        assert!(config.follow_league_client);
        assert_eq!(config.saved_session_max_age_days, 14);
    }

    #[test]
    fn https_upgrade_preserves_local_relay() {
        assert_eq!(
            Config::secure_url("http://localhost:8790"),
            "http://localhost:8790"
        );
        assert_eq!(
            Config::secure_url("http://example.com/a"),
            "https://example.com/a"
        );
    }

    #[test]
    fn auto_update_choice_is_not_overwritten_by_normalization() {
        let mut config = Config {
            auto_update_enabled: false,
            ..Config::default()
        };

        config.normalize();

        assert!(!config.auto_update_enabled);
    }
}
