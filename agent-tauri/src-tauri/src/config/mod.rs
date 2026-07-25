use std::{
    fs,
    io::{Read, Write},
    net::IpAddr,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use url::{Host, Url};
use uuid::Uuid;

use crate::error::{AgentError, AgentResult};

const PUBLIC_UPDATE_MANIFEST_URL: &str = "https://yummi.duckdns.org/agent/version.json";

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
    pub update_channel: String,
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
            update_manifest_url: Some(PUBLIC_UPDATE_MANIFEST_URL.into()),
            check_updates_on_startup: true,
            auto_update_enabled: true,
            update_channel: "stable".into(),
            saved_session_max_age_days: 14,
            run_at_windows_startup: true,
            ui_test_mode: false,
        }
    }
}

impl Config {
    const MAX_CONFIG_BYTES: u64 = 64 * 1024;

    fn path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(ToOwned::to_owned))
            .unwrap_or_default()
            .join("agent.json")
    }

    fn secure_url(raw: &str) -> String {
        let value = raw.trim().trim_end_matches('/');
        let Ok(mut url) = Url::parse(value) else {
            return value.into();
        };
        if url.scheme() == "http" && !is_loopback_url(&url) {
            let _ = url.set_scheme("https");
        }
        url.to_string().trim_end_matches('/').to_owned()
    }

    pub(crate) fn normalize(&mut self) {
        self.relay_public_base_url = Self::secure_url(&self.relay_public_base_url);
        self.update_manifest_url = self
            .update_manifest_url
            .as_ref()
            .map(|value| Self::secure_url(value));
    }

    pub(crate) fn load() -> Self {
        let path = Self::path();
        let mut config = fs::symlink_metadata(&path)
            .ok()
            .filter(|metadata| {
                metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() <= Self::MAX_CONFIG_BYTES
            })
            .and_then(|_| {
                let mut bytes = Vec::new();
                fs::File::open(path)
                    .ok()?
                    .take(Self::MAX_CONFIG_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .ok()?;
                (bytes.len() as u64 <= Self::MAX_CONFIG_BYTES)
                    .then(|| String::from_utf8(bytes).ok())
                    .flatten()
            })
            .and_then(|raw| serde_json::from_str::<Self>(&raw).ok())
            .unwrap_or_default();
        config.normalize();
        let defaults = Self::default();
        if validate_relay_base_url(&config.relay_public_base_url, cfg!(debug_assertions)).is_err() {
            config.relay_public_base_url = defaults.relay_public_base_url;
        }
        if validate_update_url(
            config.update_manifest_url.as_deref(),
            cfg!(debug_assertions),
        )
        .is_err()
        {
            config.update_manifest_url = defaults.update_manifest_url;
        }
        config
    }

    pub(crate) fn save(&self) -> AgentResult<()> {
        self.validate()?;
        let path = Self::path();
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(AgentError::Config(
                "설정 저장 경로가 올바르지 않습니다.".into(),
            ));
        }
        let serialized = serde_json::to_vec_pretty(self)
            .map_err(|_| AgentError::Config("설정 직렬화 실패".into()))?;
        let parent = path
            .parent()
            .ok_or_else(|| AgentError::Config("설정 저장 경로 오류".into()))?;
        let temporary = parent.join(format!(".agent-{}.tmp", Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::File::options()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&serialized)?;
            file.sync_all()?;
            fs::rename(&temporary, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|_| AgentError::Config("설정 저장 실패".into()))?;
        Ok(())
    }

    pub(crate) fn validate(&self) -> AgentResult<()> {
        validate_relay_base_url(&self.relay_public_base_url, cfg!(debug_assertions))?;
        validate_update_url(self.update_manifest_url.as_deref(), cfg!(debug_assertions))?;
        validate_update_channel(&self.update_channel)?;
        if self
            .lockfile_path
            .as_deref()
            .is_some_and(|value| value.len() > 1_024 || value.contains('\0'))
        {
            return Err(AgentError::Config(
                "lockfile 경로가 올바르지 않습니다.".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn ws_url(&self, session_id: &str) -> AgentResult<String> {
        let mut url = validate_relay_base_url(&self.relay_public_base_url, cfg!(debug_assertions))?;
        let websocket_scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(websocket_scheme)
            .map_err(|_| AgentError::Relay("Relay WebSocket URL 오류".into()))?;
        url.set_path("/ws/agent");
        url.set_query(None);
        url.query_pairs_mut().append_pair("session_id", session_id);
        Ok(url.into())
    }

    pub(crate) fn login_url(&self, session_id: &str) -> AgentResult<String> {
        let mut url = validate_relay_base_url(&self.relay_public_base_url, cfg!(debug_assertions))?;
        url.set_path("/login");
        url.set_query(None);
        url.query_pairs_mut().append_pair("session_id", session_id);
        Ok(url.into())
    }
}

pub(crate) fn validate_update_channel(raw: &str) -> AgentResult<()> {
    match raw.trim() {
        "stable" | "beta" | "dev" => Ok(()),
        _ => Err(AgentError::Config(
            "업데이트 채널은 stable, beta, dev 중 하나여야 합니다.".into(),
        )),
    }
}

fn is_loopback_url(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
        Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
        None => false,
    }
}

fn validate_relay_base_url(raw: &str, allow_insecure_loopback: bool) -> AgentResult<Url> {
    let url = Url::parse(raw.trim())
        .map_err(|_| AgentError::Config("Relay URL이 올바르지 않습니다.".into()))?;
    let secure = url.scheme() == "https";
    let local_debug = allow_insecure_loopback && url.scheme() == "http" && is_loopback_url(&url);
    if !secure && !local_debug {
        return Err(AgentError::Config(
            "Relay URL은 HTTPS를 사용해야 합니다.".into(),
        ));
    }
    if url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(AgentError::Config(
            "Relay URL에는 호스트와 포트만 입력하세요.".into(),
        ));
    }
    Ok(url)
}

fn validate_update_url(raw: Option<&str>, allow_custom: bool) -> AgentResult<()> {
    let Some(raw) = raw else {
        return Ok(());
    };
    let url = Url::parse(raw.trim())
        .map_err(|_| AgentError::Config("업데이트 URL이 올바르지 않습니다.".into()))?;
    if url.scheme() != "https"
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AgentError::Config(
            "업데이트 URL은 인증 정보가 없는 HTTPS URL이어야 합니다.".into(),
        ));
    }
    if !allow_custom && url.as_str() != PUBLIC_UPDATE_MANIFEST_URL {
        return Err(AgentError::Config(
            "배포 빌드에서는 공식 업데이트 URL만 사용할 수 있습니다.".into(),
        ));
    }
    Ok(())
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
        assert_eq!(config.update_channel, "stable");
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
    fn production_rejects_plaintext_relay_even_on_loopback() {
        assert!(validate_relay_base_url("http://localhost:8790", false).is_err());
        assert!(validate_relay_base_url("https://relay.example", false).is_ok());
    }

    #[test]
    fn relay_url_preserves_port_and_rejects_credentials() {
        let config = Config {
            relay_public_base_url: "https://relay.example:9443".into(),
            ..Config::default()
        };
        assert_eq!(
            config.ws_url("session").unwrap(),
            "wss://relay.example:9443/ws/agent?session_id=session"
        );
        assert!(validate_relay_base_url("https://user:secret@relay.example", true).is_err());
    }

    #[test]
    fn production_update_url_is_fixed_to_the_public_manifest() {
        assert!(validate_update_url(Some(PUBLIC_UPDATE_MANIFEST_URL), false).is_ok());
        assert!(validate_update_url(Some("https://attacker.example/version.json"), false).is_err());
        assert!(validate_update_url(Some("https://attacker.example/version.json"), true).is_ok());
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

    #[test]
    fn update_channel_is_limited_to_known_release_tracks() {
        assert!(validate_update_channel("stable").is_ok());
        assert!(validate_update_channel("beta").is_ok());
        assert!(validate_update_channel("dev").is_ok());
        assert!(validate_update_channel("nightly").is_err());
    }
}
