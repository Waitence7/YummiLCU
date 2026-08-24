use std::{collections::VecDeque, sync::Arc, time::UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{watch, Mutex, RwLock};

const MAX_UI_LOGS: usize = 2_000;

use crate::{
    config::Config,
    diagnostics::FlightRecorder,
    lcu::LcuConnectionState,
    relay::supervisor::{RelayConnectionState, RelaySupervisor},
};

#[derive(Clone, Serialize)]
pub(crate) struct UiState {
    status: String,
    relay: bool,
    lcu: bool,
    discord_id: Option<u64>,
    discord_name: Option<String>,
    discord_avatar: Option<String>,
    logs: VecDeque<String>,
    oauth_pending: bool,
    update_message: Option<String>,
    app_version: String,
    release_label: String,
    release_channel: String,
    build_id: String,
    git_commit: String,
    downloaded_at: Option<u64>,
    config: Config,
}

impl UiState {
    fn new(config: Config) -> Self {
        Self {
            status: "연결 시작 → Discord 로그인".into(),
            relay: false,
            lcu: false,
            discord_id: None,
            discord_name: None,
            discord_avatar: None,
            logs: VecDeque::new(),
            oauth_pending: false,
            update_message: None,
            app_version: env!("CARGO_PKG_VERSION").into(),
            release_label: option_env!("YUMMI_AGENT_RELEASE_LABEL")
                .unwrap_or(env!("CARGO_PKG_VERSION"))
                .into(),
            release_channel: option_env!("YUMMI_AGENT_RELEASE_CHANNEL")
                .unwrap_or("stable")
                .into(),
            build_id: option_env!("YUMMI_AGENT_BUILD_ID").unwrap_or("local").into(),
            git_commit: option_env!("YUMMI_AGENT_GIT_COMMIT")
                .unwrap_or("unknown")
                .into(),
            downloaded_at: installed_at(),
            config,
        }
    }

    fn push_log(&mut self, message: String) {
        self.logs.push_back(message);
        if self.logs.len() > MAX_UI_LOGS {
            self.logs.pop_front();
        }
    }
}

fn installed_at() -> Option<u64> {
    let executable = std::env::current_exe().ok()?;
    let metadata = std::fs::metadata(executable).ok()?;
    let time = metadata.created().or_else(|_| metadata.modified()).ok()?;
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|value| value.as_secs())
}

#[derive(Clone, Debug)]
pub(crate) enum AgentEvent {
    LcuStateChanged(LcuConnectionState),
    RelayStateChanged(RelayConnectionState),
}

pub(crate) struct AppState {
    pub(crate) config: RwLock<Config>,
    pub(crate) ui: Mutex<UiState>,
    pub(crate) relay: Arc<RelaySupervisor>,
    pub(crate) command_lock: Mutex<()>,
    flight: Mutex<FlightRecorder>,
    lcu_state: RwLock<LcuConnectionState>,
    shutdown: watch::Sender<bool>,
}

impl AppState {
    pub(crate) fn new(config: Config) -> Self {
        let (shutdown, _) = watch::channel(false);
        Self {
            config: RwLock::new(config.clone()),
            ui: Mutex::new(UiState::new(config)),
            relay: Arc::new(RelaySupervisor::new()),
            command_lock: Mutex::new(()),
            flight: Mutex::new(FlightRecorder::default()),
            lcu_state: RwLock::new(LcuConnectionState::ClientStopped),
            shutdown,
        }
    }

    pub(crate) async fn log(&self, app: &AppHandle, message: impl Into<String>) {
        let snapshot = {
            let mut ui = self.ui.lock().await;
            ui.push_log(message.into());
            ui.clone()
        };
        let _ = app.emit("agent-state", snapshot);
    }

    pub(crate) async fn record_flight(&self, category: &'static str, detail: impl Into<String>) {
        self.flight.lock().await.record(category, detail);
    }

    pub(crate) async fn diagnostic_bundle(&self) -> String {
        let ui = self.ui.lock().await.clone();
        let flight = self.flight.lock().await.snapshot();
        build_diagnostic_bundle(&ui, &flight)
    }

    pub(crate) async fn set_update_message(
        &self,
        app: &AppHandle,
        message: Option<impl Into<String>>,
    ) {
        self.ui.lock().await.update_message = message.map(Into::into);
        self.emit(app).await;
    }

    pub(crate) async fn emit(&self, app: &AppHandle) {
        let snapshot = self.snapshot().await;
        let _ = app.emit("agent-state", snapshot);
    }

    pub(crate) async fn snapshot(&self) -> UiState {
        self.ui.lock().await.clone()
    }

    pub(crate) async fn bound_discord_id(&self) -> Option<u64> {
        self.ui.lock().await.discord_id
    }

    pub(crate) async fn update_config(&self, config: Config) {
        *self.config.write().await = config.clone();
        self.ui.lock().await.config = config;
    }

    pub(crate) async fn set_oauth_pending(
        &self,
        app: &AppHandle,
        pending: bool,
        status: impl Into<String>,
    ) {
        {
            let mut ui = self.ui.lock().await;
            ui.oauth_pending = pending;
            ui.status = status.into();
        }
        self.emit(app).await;
    }

    pub(crate) async fn apply_session_bound(
        &self,
        app: &AppHandle,
        discord_id: Option<u64>,
        discord_name: Option<String>,
        discord_avatar: Option<String>,
    ) {
        {
            let mut ui = self.ui.lock().await;
            ui.oauth_pending = false;
            ui.discord_id = discord_id;
            ui.discord_name = discord_name;
            ui.discord_avatar = discord_avatar;
            ui.status = "Discord 연결 완료 — LCU 확인 중…".into();
        }
        self.emit(app).await;
        self.log(app, "Discord 연결 완료").await;
    }

    pub(crate) async fn publish(&self, app: &AppHandle, event: AgentEvent) {
        match event {
            AgentEvent::LcuStateChanged(next) => {
                let changed = {
                    let mut current = self.lcu_state.write().await;
                    if *current == next {
                        false
                    } else {
                        *current = next;
                        true
                    }
                };
                if changed {
                    let label = lcu_state_label(next);
                    self.record_flight("lcu_state", label).await;
                    self.ui.lock().await.lcu = next.is_ready();
                    self.emit(app).await;
                    self.log(app, format!("LCU 상태 변경: {label}")).await;
                }
            }
            AgentEvent::RelayStateChanged(next) => {
                let relay_ready = matches!(
                    next,
                    RelayConnectionState::Authenticating | RelayConnectionState::Connected
                );
                self.record_flight("relay_state", relay_state_flight_label(next))
                    .await;
                self.ui.lock().await.relay = relay_ready;
                self.emit(app).await;
            }
        }
    }

    pub(crate) async fn lcu_state(&self) -> LcuConnectionState {
        *self.lcu_state.read().await
    }

    pub(crate) fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }

    pub(crate) fn begin_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    pub(crate) async fn mark_stopped(&self, app: &AppHandle) {
        {
            let mut ui = self.ui.lock().await;
            ui.relay = false;
            ui.lcu = false;
            ui.oauth_pending = false;
            ui.status = "중지됨".into();
        }
        *self.lcu_state.write().await = LcuConnectionState::ClientStopped;
        self.emit(app).await;
    }
}

fn build_diagnostic_bundle(ui: &UiState, flight: &[crate::diagnostics::FlightRecord]) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "Yummi LCU Agent Diagnostics");
    let _ = writeln!(out, "generated_at_ms={}", diagnostic_now_ms());
    let _ = writeln!(out, "app_version={}", ui.app_version);
    let _ = writeln!(out, "release_label={}", ui.release_label);
    let _ = writeln!(out, "release_channel={}", ui.release_channel);
    let _ = writeln!(out, "build_id={}", ui.build_id);
    let _ = writeln!(out, "git_commit={}", ui.git_commit);
    let _ = writeln!(out, "relay_connected={}", ui.relay);
    let _ = writeln!(out, "lcu_connected={}", ui.lcu);
    let _ = writeln!(out, "discord_bound={}", ui.discord_id.is_some());
    let _ = writeln!(out, "status={}", sanitize_diagnostic_line(&ui.status));
    let _ = writeln!(out);
    let _ = writeln!(out, "--- Flight Recorder ({} records) ---", flight.len());
    for record in flight {
        let _ = writeln!(
            out,
            "{} [{}] {}",
            record.at_ms,
            record.category,
            sanitize_diagnostic_line(&record.detail)
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "--- UI Logs ({} lines) ---", ui.logs.len());
    for line in &ui.logs {
        let _ = writeln!(out, "{}", sanitize_diagnostic_line(line));
    }
    out
}

fn diagnostic_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn sanitize_diagnostic_line(value: &str) -> String {
    // Diagnostics must stay useful while never echoing credential-like values.
    // Known secrets are never intentionally logged, and this is a final defense for
    // accidental key=value / JSON-like entries that may reach the UI log.
    const KEYS: [&str; 8] = [
        "password",
        "token",
        "authorization",
        "oauth_code",
        "oauthcode",
        "ws_token",
        "session_token",
        "remoting-auth-token",
    ];

    let mut output = value.to_owned();
    for key in KEYS {
        output = redact_key_value(&output, key);
    }
    output
}

fn redact_key_value(input: &str, key: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find(key) {
        let start = cursor + relative;
        out.push_str(&input[cursor..start]);
        out.push_str(&input[start..start + key.len()]);
        let mut value_start = start + key.len();
        while value_start < input.len() && input.as_bytes()[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if value_start < input.len() && matches!(input.as_bytes()[value_start], b'=' | b':') {
            let separator = input.as_bytes()[value_start] as char;
            out.push_str(&input[start + key.len()..=value_start]);
            value_start += 1;
            while value_start < input.len() && input.as_bytes()[value_start].is_ascii_whitespace() {
                out.push(' ');
                value_start += 1;
            }
            let quoted = input
                .as_bytes()
                .get(value_start)
                .copied()
                .filter(|b| matches!(b, b'\"' | b'\''));
            if let Some(quote) = quoted {
                out.push(quote as char);
                value_start += 1;
                if let Some(end) = input[value_start..].find(quote as char) {
                    out.push_str("***");
                    out.push(quote as char);
                    cursor = value_start + end + 1;
                } else {
                    out.push_str("***");
                    cursor = input.len();
                }
            } else {
                let end = input[value_start..]
                    .find(|c: char| c.is_whitespace() || matches!(c, ',' | '}' | ']' | '&'))
                    .map_or(input.len(), |offset| value_start + offset);
                out.push_str("***");
                cursor = end;
            }
            let _ = separator;
        } else {
            cursor = start + key.len();
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn relay_state_flight_label(state: RelayConnectionState) -> &'static str {
    match state {
        RelayConnectionState::Stopped => "stopped",
        RelayConnectionState::Connecting => "connecting",
        RelayConnectionState::Authenticating => "authenticating",
        RelayConnectionState::Connected => "connected",
        RelayConnectionState::Reconnecting => "reconnecting",
        RelayConnectionState::Failed => "failed",
    }
}

fn lcu_state_label(state: LcuConnectionState) -> &'static str {
    match state {
        LcuConnectionState::ClientStopped => "클라이언트 중지",
        LcuConnectionState::LockfileFound => "lockfile 발견",
        LcuConnectionState::Connecting => "연결 중",
        LcuConnectionState::Connected => "연결됨",
        LcuConnectionState::LoggedIn => "로그인됨",
        LcuConnectionState::Error => "오류",
    }
}

#[cfg(test)]
mod tests {
    use super::{build_diagnostic_bundle, sanitize_diagnostic_line, UiState, MAX_UI_LOGS};
    use crate::config::Config;

    #[test]
    fn ui_log_history_is_fifo_and_bounded() {
        let mut state = UiState::new(Config::default());
        for index in 0..(MAX_UI_LOGS + 5) {
            state.push_log(format!("log-{index}"));
        }

        assert_eq!(state.logs.len(), MAX_UI_LOGS);
        assert_eq!(state.logs.front().map(String::as_str), Some("log-5"));
        assert_eq!(
            state.logs.back().map(String::as_str),
            Some(format!("log-{}", MAX_UI_LOGS + 4).as_str())
        );
    }

    #[test]
    fn diagnostic_sanitizer_redacts_credential_values() {
        let line = r#"token=abc password: "secret" Authorization=Bearer123 keep=yes"#;
        let sanitized = sanitize_diagnostic_line(line);
        assert!(!sanitized.contains("abc"));
        assert!(!sanitized.contains("secret"));
        assert!(!sanitized.contains("Bearer123"));
        assert!(sanitized.contains("keep=yes"));
    }

    #[test]
    fn diagnostic_bundle_omits_discord_identity_and_config_secrets() {
        let mut ui = UiState::new(Config::default());
        ui.discord_id = Some(123456789);
        ui.discord_name = Some("SensitiveName".into());
        ui.logs.push_back("token=super-secret normal=ok".into());
        let bundle = build_diagnostic_bundle(&ui, &[]);
        assert!(bundle.contains("discord_bound=true"));
        assert!(!bundle.contains("123456789"));
        assert!(!bundle.contains("SensitiveName"));
        assert!(!bundle.contains("super-secret"));
        assert!(bundle.contains("normal=ok"));
    }
}
