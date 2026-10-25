use std::{
    fs,
    io::{Read, Write},
    net::IpAddr,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use url::{Host, Url};
use uuid::Uuid;

use crate::{
    diagnostics::write_bootstrap_error,
    error::{AgentError, AgentResult},
};

const STABLE_UPDATE_MANIFEST_URL: &str = "https://yummi.duckdns.org/agent/version.json";
const BETA_UPDATE_MANIFEST_URL: &str =
    "https://yummi.duckdns.org/agent/releases/tauri/beta/version.json";
const DEV_UPDATE_MANIFEST_URL: &str =
    "https://yummi.duckdns.org/agent/releases/tauri/dev/version.json";
const CURRENT_CONFIG_SCHEMA_VERSION: u32 = 1;

fn embedded_release_channel() -> &'static str {
    match option_env!("YUMMI_AGENT_RELEASE_CHANNEL").unwrap_or("stable") {
        "beta" => "beta",
        "dev" => "dev",
        _ => "stable",
    }
}

fn public_update_manifest_url(channel: &str) -> &'static str {
    match channel.trim() {
        "beta" => BETA_UPDATE_MANIFEST_URL,
        "dev" => DEV_UPDATE_MANIFEST_URL,
        _ => STABLE_UPDATE_MANIFEST_URL,
    }
}

fn apply_schema_migrations(config: &mut Config, source_schema_version: u32) {
    // Schema v1 makes automatic update checks/install the default for existing
    // installations. Once v1 is persisted, later explicit user choices remain
    // untouched instead of being forced back on at every startup.
    if source_schema_version < 1 {
        config.check_updates_on_startup = true;
        config.auto_update_enabled = true;
    }
    config.config_schema_version = CURRENT_CONFIG_SCHEMA_VERSION;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Config {
    pub config_schema_version: u32,
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
    pub tray_hide_effect: String,
    pub tray_effect_playback_rate: f64,
    /// Sliding multiplier for custom Windows window inertia. None means no friction (infinity).
    pub window_glide_strength: Option<f64>,
    pub window_free_rotation: bool,
    pub ui_test_mode: bool,
}

impl Default for Config {
    fn default() -> Self {
        let update_channel = embedded_release_channel();
        Self {
            config_schema_version: CURRENT_CONFIG_SCHEMA_VERSION,
            relay_public_base_url: "https://yummi.duckdns.org".into(),
            auth_poll_interval_ms: 1500,
            lockfile_path: None,
            prevent_queue_after_dodge: true,
            apply_default_status_on_connect: true,
            auto_accept_match: false,
            follow_league_client: true,
            update_manifest_url: Some(public_update_manifest_url(update_channel).into()),
            check_updates_on_startup: true,
            auto_update_enabled: true,
            update_channel: update_channel.into(),
            saved_session_max_age_days: 14,
            // The agent is a tray/background process. It must be running after
            // Windows login even when its main window is not visible.
            run_at_windows_startup: true,
            tray_hide_effect: "book-return-v2".into(),
            tray_effect_playback_rate: 1.0,
            window_glide_strength: Some(1.0),
            window_free_rotation: false,
            ui_test_mode: false,
        }
    }
}

impl Config {
    const MAX_CONFIG_BYTES: u64 = 64 * 1024;

    fn path() -> PathBuf {
        match std::env::current_exe() {
            Ok(path) => path
                .parent()
                .map(ToOwned::to_owned)
                .unwrap_or_default()
                .join("agent.json"),
            Err(error) => {
                eprintln!("[yummi config] current executable path lookup failed: {error}");
                PathBuf::from("agent.json")
            }
        }
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
        self.update_channel = self.update_channel.trim().to_ascii_lowercase();
        self.tray_hide_effect = self.tray_hide_effect.trim().to_ascii_lowercase();
        self.update_manifest_url = self
            .update_manifest_url
            .as_ref()
            .map(|value| Self::secure_url(value));
        if !cfg!(debug_assertions) && validate_update_channel(&self.update_channel).is_ok() {
            self.update_manifest_url =
                Some(public_update_manifest_url(&self.update_channel).into());
        }
    }

    pub(crate) fn load() -> Self {
        let path = Self::path();
        let raw_config = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() > Self::MAX_CONFIG_BYTES
                {
                    write_bootstrap_error("config_load_rejected invalid_file_shape_or_size");
                    None
                } else {
                    let mut bytes = Vec::new();
                    match fs::File::open(&path).and_then(|file| {
                        file.take(Self::MAX_CONFIG_BYTES + 1)
                            .read_to_end(&mut bytes)
                    }) {
                        Ok(_) if bytes.len() as u64 <= Self::MAX_CONFIG_BYTES => {
                            match String::from_utf8(bytes) {
                                Ok(raw) => Some(raw),
                                Err(error) => {
                                    write_bootstrap_error(&format!(
                                        "config_load_utf8_failed error={error}"
                                    ));
                                    None
                                }
                            }
                        }
                        Ok(_) => {
                            write_bootstrap_error("config_load_rejected file_too_large");
                            None
                        }
                        Err(error) => {
                            write_bootstrap_error(&format!("config_load_io_failed error={error}"));
                            None
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                write_bootstrap_error(&format!("config_metadata_failed error={error}"));
                None
            }
        };
        let source_schema_version = raw_config
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .and_then(|value| {
                value
                    .get("ConfigSchemaVersion")
                    .and_then(|version| version.as_u64())
            })
            .and_then(|version| u32::try_from(version).ok())
            .unwrap_or(0);
        let mut config = match raw_config.as_deref() {
            Some(raw) => match serde_json::from_str::<Self>(raw) {
                Ok(config) => config,
                Err(error) => {
                    write_bootstrap_error(&format!("config_parse_failed error={error}"));
                    Self::default()
                }
            },
            None => Self::default(),
        };

        apply_schema_migrations(&mut config, source_schema_version);
        config.normalize();
        // The desktop agent is intentionally a background tray service. Older
        // installs may still contain RunAtWindowsStartup=false from the former
        // visible-window behavior, so migrate that value in memory.
        config.run_at_windows_startup = true;
        let defaults = Self::default();
        if validate_relay_base_url(&config.relay_public_base_url, cfg!(debug_assertions)).is_err() {
            write_bootstrap_error("config_relay_url_invalid fallback=default");
            config.relay_public_base_url = defaults.relay_public_base_url;
        }
        if validate_tray_hide_effect(&config.tray_hide_effect).is_err() {
            write_bootstrap_error("config_tray_effect_invalid fallback=default");
            config.tray_hide_effect = defaults.tray_hide_effect.clone();
        }
        if validate_tray_effect_playback_rate(config.tray_effect_playback_rate).is_err() {
            write_bootstrap_error("config_tray_effect_rate_invalid fallback=default");
            config.tray_effect_playback_rate = defaults.tray_effect_playback_rate;
        }
        if validate_window_glide_strength(config.window_glide_strength).is_err() {
            write_bootstrap_error("config_window_glide_invalid fallback=default");
            config.window_glide_strength = defaults.window_glide_strength;
        }
        if validate_update_channel(&config.update_channel).is_err() {
            write_bootstrap_error("config_update_channel_invalid fallback=default");
            config.update_channel = defaults.update_channel.clone();
            config.update_manifest_url = defaults.update_manifest_url.clone();
        }
        if validate_update_url(
            config.update_manifest_url.as_deref(),
            cfg!(debug_assertions),
        )
        .is_err()
        {
            write_bootstrap_error("config_update_url_invalid fallback=default");
            config.update_manifest_url = defaults.update_manifest_url.clone();
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
            .map_err(|error| AgentError::Config(format!("설정 직렬화 실패: {error}")))?;
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
        if let Err(error) = result {
            let cleanup = fs::remove_file(&temporary).err();
            return Err(AgentError::Config(match cleanup {
                Some(cleanup_error) => {
                    format!("설정 저장 실패: {error}; 임시 파일 정리 실패: {cleanup_error}")
                }
                None => format!("설정 저장 실패: {error}"),
            }));
        }
        Ok(())
    }

    pub(crate) fn validate(&self) -> AgentResult<()> {
        validate_relay_base_url(&self.relay_public_base_url, cfg!(debug_assertions))?;
        validate_update_channel(&self.update_channel)?;
        validate_tray_hide_effect(&self.tray_hide_effect)?;
        validate_tray_effect_playback_rate(self.tray_effect_playback_rate)?;
        validate_window_glide_strength(self.window_glide_strength)?;
        validate_update_url(self.update_manifest_url.as_deref(), cfg!(debug_assertions))?;
        if !cfg!(debug_assertions)
            && self.update_manifest_url.as_deref()
                != Some(public_update_manifest_url(&self.update_channel))
        {
            return Err(AgentError::Config(
                "업데이트 채널과 공식 manifest URL이 일치하지 않습니다.".into(),
            ));
        }
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
            .map_err(|_| AgentError::Relay("Relay WebSocket URL scheme 변환 실패".into()))?;
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

    pub(crate) fn replay_upload_target_url(
        &self,
        session_id: &str,
        game_id: &str,
    ) -> AgentResult<String> {
        let mut url = validate_relay_base_url(&self.relay_public_base_url, cfg!(debug_assertions))?;
        url.set_path("/lcu/replay-upload-target");
        url.set_query(None);
        url.query_pairs_mut()
            .append_pair("session_id", session_id)
            .append_pair("game_id", game_id);
        Ok(url.into())
    }

    pub(crate) fn replay_upload_url(&self, session_id: &str, game_id: &str) -> AgentResult<String> {
        let mut url = validate_relay_base_url(&self.relay_public_base_url, cfg!(debug_assertions))?;
        url.set_path("/lcu/replay-upload");
        url.set_query(None);
        url.query_pairs_mut()
            .append_pair("session_id", session_id)
            .append_pair("game_id", game_id);
        Ok(url.into())
    }

    pub(crate) fn replay_analysis_url(
        &self,
        session_id: &str,
        game_id: &str,
    ) -> AgentResult<String> {
        let mut url = validate_relay_base_url(&self.relay_public_base_url, cfg!(debug_assertions))?;
        url.set_path("/lcu/replay-analysis");
        url.set_query(None);
        url.query_pairs_mut()
            .append_pair("session_id", session_id)
            .append_pair("game_id", game_id);
        Ok(url.into())
    }

    pub(crate) fn replay_analysis_job_url(
        &self,
        session_id: &str,
        job_id: &str,
    ) -> AgentResult<String> {
        let mut url = validate_relay_base_url(&self.relay_public_base_url, cfg!(debug_assertions))?;
        url.set_path("/lcu/replay-analysis-job");
        url.set_query(None);
        url.query_pairs_mut()
            .append_pair("session_id", session_id)
            .append_pair("job_id", job_id);
        Ok(url.into())
    }

    pub(crate) fn replay_analysis_viewer_url(
        &self,
        session_id: &str,
        job_id: &str,
    ) -> AgentResult<String> {
        let mut url = validate_relay_base_url(&self.relay_public_base_url, cfg!(debug_assertions))?;
        url.set_path("/lcu/replay-analysis-viewer");
        url.set_query(None);
        url.query_pairs_mut()
            .append_pair("session_id", session_id)
            .append_pair("job_id", job_id);
        Ok(url.into())
    }
}

pub(crate) fn validate_tray_hide_effect(raw: &str) -> AgentResult<()> {
    match raw.trim() {
        "fold" | "jelly" | "pixels" | "cat" | "glass" | "swirl" | "suction" | "page-curl"
        | "book-return" | "book-return-v2" | "curtain" | "shards" | "fade" | "none" => Ok(()),
        _ => Err(AgentError::Config(
            "트레이 전환 효과가 올바르지 않습니다.".into(),
        )),
    }
}

pub(crate) fn validate_tray_effect_playback_rate(rate: f64) -> AgentResult<()> {
    if rate.is_finite() && (0.1..=4.0).contains(&rate) {
        Ok(())
    } else {
        Err(AgentError::Config(
            "트레이 전환 효과 속도는 0.1배에서 4배 사이여야 합니다.".into(),
        ))
    }
}

pub(crate) fn validate_window_glide_strength(strength: Option<f64>) -> AgentResult<()> {
    match strength {
        None => Ok(()),
        Some(value) if value.is_finite() && value >= 0.0 => Ok(()),
        _ => Err(AgentError::Config(
            "창 미끄러짐 강도는 0 이상의 값 또는 ∞여야 합니다.".into(),
        )),
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
        .map_err(|error| AgentError::Config(format!("Relay URL이 올바르지 않습니다: {error}")))?;
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
    let url = Url::parse(raw.trim()).map_err(|error| {
        AgentError::Config(format!("업데이트 URL이 올바르지 않습니다: {error}"))
    })?;
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
    if !allow_custom
        && ![
            STABLE_UPDATE_MANIFEST_URL,
            BETA_UPDATE_MANIFEST_URL,
            DEV_UPDATE_MANIFEST_URL,
        ]
        .contains(&url.as_str())
    {
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
        assert_eq!(config.update_channel, embedded_release_channel());
        assert_eq!(config.saved_session_max_age_days, 14);
        assert_eq!(config.tray_hide_effect, "book-return-v2");
        assert_eq!(config.tray_effect_playback_rate, 1.0);
        assert_eq!(config.window_glide_strength, Some(1.0));
        assert!(!config.window_free_rotation);
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
    fn replay_upload_target_url_includes_game_id() {
        let config = Config {
            relay_public_base_url: "https://relay.example".into(),
            ..Config::default()
        };
        assert_eq!(
            config
                .replay_upload_target_url("session", "KR-123")
                .unwrap(),
            "https://relay.example/lcu/replay-upload-target?session_id=session&game_id=KR-123"
        );
    }

    #[test]
    fn production_update_url_is_limited_to_official_channel_manifests() {
        for url in [
            STABLE_UPDATE_MANIFEST_URL,
            BETA_UPDATE_MANIFEST_URL,
            DEV_UPDATE_MANIFEST_URL,
        ] {
            assert!(validate_update_url(Some(url), false).is_ok());
        }
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
    fn legacy_config_migrates_automatic_updates_on_once() {
        let mut config = Config {
            check_updates_on_startup: false,
            auto_update_enabled: false,
            ..Config::default()
        };

        apply_schema_migrations(&mut config, 0);

        assert!(config.check_updates_on_startup);
        assert!(config.auto_update_enabled);
        assert_eq!(config.config_schema_version, CURRENT_CONFIG_SCHEMA_VERSION);
    }

    #[test]
    fn current_schema_preserves_explicit_auto_update_choice() {
        let mut config = Config {
            check_updates_on_startup: false,
            auto_update_enabled: false,
            ..Config::default()
        };

        apply_schema_migrations(&mut config, CURRENT_CONFIG_SCHEMA_VERSION);

        assert!(!config.check_updates_on_startup);
        assert!(!config.auto_update_enabled);
    }

    #[test]
    fn tray_hide_effect_is_limited_to_known_effects() {
        for effect in [
            "fold",
            "jelly",
            "pixels",
            "cat",
            "glass",
            "swirl",
            "suction",
            "page-curl",
            "book-return",
            "book-return-v2",
            "curtain",
            "shards",
            "fade",
            "none",
        ] {
            assert!(validate_tray_hide_effect(effect).is_ok());
        }
        assert!(validate_tray_hide_effect("shader-experiment").is_err());
    }

    #[test]
    fn window_glide_strength_accepts_zero_finite_values_and_infinity() {
        assert!(validate_window_glide_strength(Some(0.0)).is_ok());
        assert!(validate_window_glide_strength(Some(1.0)).is_ok());
        assert!(validate_window_glide_strength(Some(250.0)).is_ok());
        assert!(validate_window_glide_strength(None).is_ok());
        assert!(validate_window_glide_strength(Some(-0.1)).is_err());
        assert!(validate_window_glide_strength(Some(f64::INFINITY)).is_err());
        assert!(validate_window_glide_strength(Some(f64::NAN)).is_err());
    }

    #[test]
    fn tray_effect_playback_rate_is_bounded() {
        assert!(validate_tray_effect_playback_rate(0.1).is_ok());
        assert!(validate_tray_effect_playback_rate(1.0).is_ok());
        assert!(validate_tray_effect_playback_rate(4.0).is_ok());
        assert!(validate_tray_effect_playback_rate(0.09).is_err());
        assert!(validate_tray_effect_playback_rate(4.01).is_err());
        assert!(validate_tray_effect_playback_rate(f64::NAN).is_err());
    }

    #[test]
    fn update_channel_is_limited_to_known_release_tracks() {
        assert!(validate_update_channel("stable").is_ok());
        assert!(validate_update_channel("beta").is_ok());
        assert!(validate_update_channel("dev").is_ok());
        assert!(validate_update_channel("nightly").is_err());
    }
}
