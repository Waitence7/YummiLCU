//! Local, opt-in LCU preferences. Never depend on a connected Relay socket.
use std::{path::PathBuf, sync::Arc, time::Duration};

use reqwest::Method;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use tauri::AppHandle;
use tokio::time::{sleep, timeout, Instant};

use crate::{blocking_io::BlockingIo, state::AppState};
use super::{LcuClient, LcuIdentity};

const READY_CHECK: &str = "/lol-matchmaking/v1/ready-check";
const ACCEPT: &str = "/lol-matchmaking/v1/ready-check/accept";
const SEARCH: &str = "/lol-lobby/v2/lobby/matchmaking/search";
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_secs(1);
static LOCAL_PREFERENCE_DISK_IO: BlockingIo = BlockingIo::new();

#[derive(Default)]
struct PreferencesState {
    identity: Option<LcuIdentity>,
    phase: Option<String>,
    pending_since: Option<Instant>,
    accept_attempts: u8,
    accepted: bool,
    last_accept_attempt: Option<Instant>,
}

fn ready_pending(value: &Value) -> bool {
    matches!(value.get("state").and_then(Value::as_str), Some("InProgress" | "Waiting"))
        && matches!(value.get("playerResponse").and_then(Value::as_str), Some("None" | "Pending"))
}

fn ready_phase(phase: &str) -> bool {
    matches!(phase, "ReadyCheck" | "Matchmaking")
}

fn dodge_transition(previous: Option<&str>, next: &str) -> bool {
    previous == Some("ChampSelect") && matches!(next, "Lobby" | "None" | "Matchmaking")
}

impl PreferencesState {
    fn reset_ready(&mut self) {
        self.pending_since = None;
        self.accept_attempts = 0;
        self.accepted = false;
        self.last_accept_attempt = None;
    }

    fn observe_pending(&mut self, pending: bool, enabled: bool, now: Instant) -> bool {
        if !pending || !enabled {
            self.reset_ready();
            return false;
        }
        let since = *self.pending_since.get_or_insert(now);
        !self.accepted && self.accept_attempts < 2
            && now.duration_since(since) >= Duration::from_secs(1)
            && self.last_accept_attempt.is_none_or(|last| now.duration_since(last) >= Duration::from_secs(2))
    }
}

pub(crate) async fn watch_local_preferences(app: AppHandle, state: Arc<AppState>) {
    let mut shutdown = state.shutdown_receiver();
    let mut local = PreferencesState::default();
    let mut cached_client: Option<(PathBuf, Instant, LcuClient)> = None;
    loop {
        if *shutdown.borrow() { return; }
        // A manual Stop/Logout is authoritative. A network outage is not: the
        // Relay supervisor remains running while it reconnects.
        if state.relay.is_running().await && !crate::session::auto_login_suppressed() {
            tick(&app, &state, &mut local, &mut cached_client).await;
        } else {
            local = PreferencesState::default();
            cached_client = None;
        }
        tokio::select! {
            _ = sleep(TICK) => {},
            changed = shutdown.changed() => { if changed.is_err() || *shutdown.borrow() { return; } }
        }
    }
}

async fn tick(app: &AppHandle, state: &Arc<AppState>, local: &mut PreferencesState, cached_client: &mut Option<(PathBuf, Instant, LcuClient)>) {
    let config = state.config.read().await.clone();
    if !config.auto_accept_match && !config.prevent_queue_after_dodge {
        *local = PreferencesState::default();
        *cached_client = None;
        return;
    }
    let Some(path) = state.lcu_lockfile_path() else {
        *local = PreferencesState::default();
        *cached_client = None;
        return;
    };
    if cached_client.as_ref().is_none_or(|(old, at, _)| *old != path || at.elapsed() >= Duration::from_secs(5)) {
        let load_path = path.clone();
        let result = LOCAL_PREFERENCE_DISK_IO.run(IO_TIMEOUT, move || {
            LcuClient::from_lockfile(&load_path).or_else(|_| LcuClient::from_lockfile_legacy(&load_path))
        }).await;
        *cached_client = match result {
            Ok(Ok(client)) => Some((path, Instant::now(), client)),
            _ => None,
        };
    }
    let Some((_, _, client)) = cached_client.as_ref() else { return; };
    if local.identity != Some(client.identity()) {
        *local = PreferencesState { identity: Some(client.identity()), ..Default::default() };
    }
    let Ok(Ok(phase)) = timeout(IO_TIMEOUT, client.gameflow_phase()).await else { return; };
    let was_dodge = dodge_transition(local.phase.as_deref(), &phase);
    // A returned matchmaking queue is a new ready-check episode, even when
    // the old endpoint is no longer available.
    if local.phase.as_deref() == Some("ReadyCheck") && phase == "Matchmaking" {
        local.reset_ready();
    }
    local.phase = Some(phase.clone());

    if config.auto_accept_match && ready_phase(&phase) {
        match timeout(IO_TIMEOUT, client.request(Method::GET, READY_CHECK, None)).await {
            Ok(Ok(value)) => {
                let now = Instant::now();
                if local.observe_pending(ready_pending(&value), true, now) {
                    if let Ok(_guard) = timeout(Duration::from_millis(250), state.command_lock.lock()).await {
                        let enabled = state.config.read().await.auto_accept_match;
                        if enabled && state.relay.is_running().await && !crate::session::auto_login_suppressed()
                            && matches!(timeout(IO_TIMEOUT, client.request(Method::GET, READY_CHECK, None)).await, Ok(Ok(ref current)) if ready_pending(current))
                            && state.config.read().await.auto_accept_match
                        {
                            local.accept_attempts += 1;
                            local.last_accept_attempt = Some(Instant::now());
                            match timeout(IO_TIMEOUT, client.request(Method::POST, ACCEPT, None)).await {
                                Ok(Ok(_)) => {
                                    local.accepted = true;
                                    state.log(app, "매치 자동 수락 완료").await;
                                }
                                Ok(Err(error)) => {
                                    state.log(app, format!("매치 자동 수락 실패: {error}")).await;
                                }
                                Err(_) => state.log(app, "매치 자동 수락 응답 시간 초과").await,
                            }
                        }
                    }
                }
            }
            Ok(Err(error)) if error.to_string().contains("http_status=404") => local.reset_ready(),
            _ => {},
        }
    } else {
        local.reset_ready();
    }

    if was_dodge && config.prevent_queue_after_dodge {
        if let Ok(_guard) = timeout(Duration::from_millis(250), state.command_lock.lock()).await {
            if state.config.read().await.prevent_queue_after_dodge && state.relay.is_running().await
                && matches!(timeout(IO_TIMEOUT, client.gameflow_phase()).await, Ok(Ok(ref current)) if matches!(current.as_str(), "Lobby" | "None" | "Matchmaking"))
                && state.config.read().await.prevent_queue_after_dodge
            {
                match timeout(IO_TIMEOUT, client.request(Method::DELETE, SEARCH, None)).await {
                    Ok(Ok(_)) => state.log(app, "챔피언 선택 종료 → 매칭 자동 재시작 방지").await,
                    Ok(Err(error)) if error.to_string().contains("http_status=404") => {},
                    Ok(Err(error)) => state.log(app, format!("매칭 재시작 방지 실패: {error}")).await,
                    Err(_) => state.log(app, "매칭 재시작 방지 응답 시간 초과").await,
                }
            }
        }
    }


}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_requires_explicit_pending_response() {
        assert!(ready_pending(&json!({"state":"InProgress","playerResponse":"None"})));
        assert!(ready_pending(&json!({"state":"Waiting","playerResponse":"Pending"})));
        for response in ["Accepted", "Declined", ""] {
            assert!(!ready_pending(&json!({"state":"InProgress","playerResponse":response})));
        }
        assert!(!ready_pending(&json!({"state":"Invalid","playerResponse":"None"})));
        assert!(!ready_pending(&json!({"state":"InProgress"})));
    }

    #[test]
    fn ready_check_is_opt_in_debounced_and_bounded() {
        let start = Instant::now();
        let mut state = PreferencesState::default();
        assert!(!state.observe_pending(true, false, start));
        assert!(!state.observe_pending(true, true, start));
        assert!(state.observe_pending(true, true, start + Duration::from_secs(1)));
        state.accept_attempts = 1;
        state.last_accept_attempt = Some(start + Duration::from_secs(1));
        assert!(!state.observe_pending(true, true, start + Duration::from_secs(2)));
        assert!(state.observe_pending(true, true, start + Duration::from_secs(3)));
        state.accepted = true;
        assert!(!state.observe_pending(true, true, start + Duration::from_secs(4)));
        assert!(!state.observe_pending(false, true, start + Duration::from_secs(5)));
        assert_eq!(state.accept_attempts, 0);
    }

    #[test]
    fn dodge_only_on_draft_exit_not_normal_game_completion() {
        assert!(dodge_transition(Some("ChampSelect"), "Lobby"));
        assert!(dodge_transition(Some("ChampSelect"), "Matchmaking"));
        assert!(!dodge_transition(Some("ChampSelect"), "GameStart"));
        assert!(!dodge_transition(Some("EndOfGame"), "Lobby"));
        assert!(!dodge_transition(None, "Lobby"));
    }
}
