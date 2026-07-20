use std::{sync::Arc, time::UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};

use crate::{
    config::Config,
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
    logs: Vec<String>,
    oauth_pending: bool,
    update_message: Option<String>,
    app_version: String,
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
            logs: Vec::new(),
            oauth_pending: false,
            update_message: None,
            app_version: env!("CARGO_PKG_VERSION").into(),
            downloaded_at: installed_at(),
            config,
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
    lcu_state: RwLock<LcuConnectionState>,
}

impl AppState {
    pub(crate) fn new(config: Config) -> Self {
        Self {
            config: RwLock::new(config.clone()),
            ui: Mutex::new(UiState::new(config)),
            relay: Arc::new(RelaySupervisor::new()),
            command_lock: Mutex::new(()),
            lcu_state: RwLock::new(LcuConnectionState::ClientStopped),
        }
    }

    pub(crate) async fn log(&self, app: &AppHandle, message: impl Into<String>) {
        let snapshot = {
            let mut ui = self.ui.lock().await;
            ui.logs.push(message.into());
            if ui.logs.len() > 300 {
                ui.logs.remove(0);
            }
            ui.clone()
        };
        let _ = app.emit("agent-state", snapshot);
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
        let snapshot = self.ui.lock().await.clone();
        let _ = app.emit("agent-state", snapshot);
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
                    self.ui.lock().await.lcu = next.is_ready();
                    self.emit(app).await;
                }
            }
            AgentEvent::RelayStateChanged(next) => {
                let relay_ready = matches!(
                    next,
                    RelayConnectionState::Authenticating | RelayConnectionState::Connected
                );
                self.ui.lock().await.relay = relay_ready;
                self.emit(app).await;
            }
        }
    }

    pub(crate) async fn lcu_state(&self) -> LcuConnectionState {
        *self.lcu_state.read().await
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
