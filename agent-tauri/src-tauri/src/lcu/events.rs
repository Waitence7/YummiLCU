use futures_util::SinkExt;
use reqwest::Method;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, watch};
use tokio::time::{sleep, timeout, Duration, Instant};
use tokio_tungstenite::{
    connect_async_tls_with_config,
    tungstenite::{
        client::IntoClientRequest, http::header::AUTHORIZATION, protocol::WebSocketConfig, Message,
    },
    Connector,
};

use crate::{config::Config, error::AgentError};

use super::{discover_lockfile, LcuClient, LcuIdentity, LockfileDiscovery};

const GAMEFLOW_PHASE: &str = "/lol-gameflow/v1/gameflow-phase";
const READY_CHECK: &str = "/lol-matchmaking/v1/ready-check";
const CHAMP_SELECT: &str = "/lol-champ-select/v1/session";
const LOBBY: &str = "/lol-lobby/v2/lobby";
const EOG_STATS: &str = "/lol-end-of-game/v1/eog-stats-block";
const GAMEFLOW_SESSION: &str = "/lol-gameflow/v1/session";
const LIVE_GAME_DATA: &str = "/liveclientdata/allgamedata";
const LIVE_GAME_EVENTS: &str = "/liveclientdata/eventdata";
const LCU_SOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const LCU_SOCKET_RETRY_DELAY: Duration = Duration::from_secs(5);
const LIVE_GAME_POLL_INTERVAL: Duration = Duration::from_secs(1);
const GAMEFLOW_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(8);
const LOCKFILE_DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);
const LOCKFILE_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(2);
const RECENT_MATCH_VERIFICATION_TIMEOUT: Duration = Duration::from_secs(3);
const EOG_RECOVERY_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const EOG_POSTGAME_RECOVERY_GRACE: Duration = Duration::from_secs(90);
const LIVE_GAME_PARTICIPANT_COUNT: usize = 10;
const MAX_LCU_EVENT_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(crate) struct LcuEventPoller {
    gameflow: Option<String>,
    last_gameflow_emit: Option<Instant>,
    ready_check: Option<String>,
    champ_select: Option<String>,
    party: Option<String>,
    participant: Option<String>,
    live_game: Option<String>,
    live_respawn_samples: HashMap<String, u8>,
    last_live_game_poll: Option<Instant>,
    // None keeps legacy agents compatible until Relay sends the first control message.
    live_game_polling: Option<bool>,
    eog_sent: bool,
    last_eog_attempt: Option<Instant>,
    eog_recovery_started_at: Option<Instant>,
    eog_attempt_count: u32,
    eog_expected_game_id: Option<String>,
    diagnostics: Vec<String>,
    last_lockfile_diagnostics: Vec<String>,
    cached_lockfile_discovery: Option<LockfileDiscovery>,
    last_lockfile_discovery: Option<Instant>,
    lockfile_discovery_slow: bool,
    lockfile_available: Option<bool>,
    lcu_available: Option<bool>,
    live_client_available: Option<bool>,
    live_game_id: Option<String>,
    connection_identity: Option<LcuIdentity>,
    connection_generation: u64,
    schema_warnings: HashSet<&'static str>,
}

impl LcuEventPoller {
    fn reset_lcu_snapshot(&mut self) {
        self.gameflow = None;
        self.last_gameflow_emit = None;
        self.ready_check = None;
        self.champ_select = None;
        self.party = None;
        self.participant = None;
        self.live_respawn_samples.clear();
        self.eog_sent = false;
        self.last_eog_attempt = None;
        self.eog_recovery_started_at = None;
        self.eog_attempt_count = 0;
        self.eog_expected_game_id = None;
        self.lcu_available = None;
        self.schema_warnings.clear();
    }

    fn observe_connection(&mut self, identity: LcuIdentity) {
        if self.connection_identity == Some(identity) {
            return;
        }
        self.connection_generation = self.connection_generation.wrapping_add(1);
        self.connection_identity = Some(identity);
        self.reset_lcu_snapshot();
        self.diagnostic(format!(
            "LCU 연결 세대 #{} 시작: pid={}, port={}",
            self.connection_generation, identity.process_id, identity.port
        ));
    }

    fn observe_disconnected(&mut self) {
        if self.connection_identity.take().is_none() {
            return;
        }
        self.connection_generation = self.connection_generation.wrapping_add(1);
        self.reset_lcu_snapshot();
        self.diagnostic(format!(
            "LCU 연결 세대 #{} 종료 — 다음 연결은 새 상태로 초기화",
            self.connection_generation
        ));
    }

    pub(crate) fn set_live_game_polling(&mut self, enabled: bool) {
        if self.live_game_polling != Some(enabled) {
            self.live_game_polling = Some(enabled);
            self.last_live_game_poll = None;
            if !enabled {
                self.live_game = None;
                self.live_game_id = None;
                self.live_respawn_samples.clear();
            }
        }
    }

    fn diagnostic(&mut self, message: impl Into<String>) {
        self.diagnostics.push(message.into());
        if self.diagnostics.len() > 64 {
            self.diagnostics.remove(0);
        }
    }

    fn observe_schema(&mut self, endpoint: &'static str, healthy: bool) {
        if healthy {
            if self.schema_warnings.remove(endpoint) {
                self.diagnostic(format!("LCU schema canary 복구: {endpoint}"));
            }
        } else if self.schema_warnings.insert(endpoint) {
            self.diagnostic(format!("LCU schema canary 변경 감지: {endpoint}"));
        }
    }

    pub(crate) fn take_diagnostics(&mut self) -> Vec<String> {
        std::mem::take(&mut self.diagnostics)
    }

    pub(crate) async fn watch_socket(
        config: Config,
        changed: mpsc::Sender<()>,
        mut stop: watch::Receiver<bool>,
    ) {
        loop {
            if *stop.borrow() {
                return;
            }
            let Some(discovery) = discover_lockfile_nonblocking(&config).await else {
                if !wait_for_socket_retry(&mut stop).await {
                    return;
                }
                continue;
            };
            let Some(path) = discovery.path else {
                if !wait_for_socket_retry(&mut stop).await {
                    return;
                }
                continue;
            };
            let Ok(client) =
                LcuClient::from_lockfile(&path).or_else(|_| LcuClient::from_lockfile_legacy(&path))
            else {
                if !wait_for_socket_retry(&mut stop).await {
                    return;
                }
                continue;
            };
            let (port, password) = client.event_connection();
            let mut request = match format!("wss://127.0.0.1:{port}").into_client_request() {
                Ok(request) => request,
                Err(_) => return,
            };
            let mut credentials = format!("riot:{password}");
            let mut token = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                credentials.as_bytes(),
            );
            let header = format!("Basic {token}").parse();
            // Both values are ASCII, so NUL replacement keeps each String valid before drop.
            unsafe {
                credentials.as_bytes_mut().fill(0);
                token.as_bytes_mut().fill(0);
            }
            drop(client);
            let Ok(header) = header else {
                return;
            };
            request.headers_mut().insert(AUTHORIZATION, header);
            let mut builder = native_tls::TlsConnector::builder();
            // Riot's local LCU certificate is self-signed. Host and process identity
            // are constrained separately before these credentials are used.
            builder.danger_accept_invalid_certs(true);
            let Ok(tls) = builder.build() else {
                return;
            };
            let connected = connect_async_tls_with_config(
                request,
                Some(
                    WebSocketConfig::default()
                        .max_message_size(Some(MAX_LCU_EVENT_MESSAGE_BYTES))
                        .max_frame_size(Some(MAX_LCU_EVENT_MESSAGE_BYTES)),
                ),
                false,
                Some(Connector::NativeTls(tls)),
            );
            let Ok(Ok((mut socket, _))) = timeout(LCU_SOCKET_CONNECT_TIMEOUT, connected).await
            else {
                if !wait_for_socket_retry(&mut stop).await {
                    return;
                }
                continue;
            };
            if socket
                .send(Message::Text("[5,\"OnJsonApiEvent\"]".into()))
                .await
                .is_err()
            {
                if !wait_for_socket_retry(&mut stop).await {
                    return;
                }
                continue;
            }
            loop {
                tokio::select! {
                    changed_stop = stop.changed() => { if changed_stop.is_err() || *stop.borrow() { return; } }
                    message = futures_util::StreamExt::next(&mut socket) => match message {
                        Some(Ok(Message::Text(text))) if is_lcu_event(text.as_str()) => {
                            if changed.is_closed() { return; }
                            let _ = changed.try_send(());
                        }
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        _ => {}
                    }
                }
            }
            if !wait_for_socket_retry(&mut stop).await {
                return;
            }
        }
    }

    pub(crate) async fn poll(
        &mut self,
        config: &Config,
        poll_lcu: bool,
    ) -> Vec<(&'static str, Value)> {
        let mut events = Vec::new();

        // Live Client Data is independent of the League Client lockfile. This is
        // especially important for spectators and PC cafes where LCU discovery or
        // gameflow may be unavailable while the local game API is still running.
        if self.live_game_polling.unwrap_or(true) {
            let should_poll = self
                .last_live_game_poll
                .is_none_or(|last| last.elapsed() >= LIVE_GAME_POLL_INTERVAL);
            if should_poll {
                self.last_live_game_poll = Some(Instant::now());
                match LcuClient::live_game_request(LIVE_GAME_DATA).await {
                    Ok(value) => {
                        self.observe_schema(
                            "live_client_data",
                            value.get("gameData").is_some_and(Value::is_object)
                                && value.get("allPlayers").is_some_and(Value::is_array),
                        );
                        if self.live_client_available != Some(true) {
                            self.diagnostic("Live Client Data API 연결됨 (127.0.0.1:2999)");
                        }
                        self.live_client_available = Some(true);
                        let live_events_response =
                            LcuClient::live_game_request(LIVE_GAME_EVENTS).await.ok();
                        if let Some(payload) =
                            live_game_payload(&value, live_events_response.as_ref())
                        {
                            let game_id = payload
                                .pointer("/game/id")
                                .map(Value::to_string)
                                .unwrap_or_else(|| "unknown".into());
                            if self.live_game_id.as_deref() != Some(game_id.as_str()) {
                                self.live_respawn_samples.clear();
                                self.diagnostic(format!(
                                    "관전 게임 감지: game_id={game_id}, 참가자={}명, 이벤트={}건, active_player={}",
                                    payload
                                        .get("participants")
                                        .and_then(Value::as_array)
                                        .map_or(0, Vec::len),
                                    payload
                                        .get("events")
                                        .and_then(Value::as_array)
                                        .map_or(0, Vec::len),
                                    if payload.get("active_player").is_some_and(|v| !v.is_null()) {
                                        "yes"
                                    } else {
                                        "no (관전자)"
                                    }
                                ));
                                self.live_game_id = Some(game_id);
                            }
                            push_changed(
                                &mut self.live_game,
                                live_game_fingerprint(&payload, &mut self.live_respawn_samples),
                                "live_game_update",
                                payload,
                                &mut events,
                            );
                        } else {
                            self.live_game = None;
                            self.live_respawn_samples.clear();
                        }
                    }
                    Err(error) => {
                        if self.live_client_available != Some(false) {
                            self.diagnostic(format!("Live Client Data API 응답 없음: {error}"));
                        }
                        self.live_client_available = Some(false);
                        self.live_game_id = None;
                        self.live_game = None;
                    }
                }
            }
        }

        // The 1s worker tick exists to keep Live Client Data responsive. LCU
        // itself is event-driven through OnJsonApiEvent, with an 8s recovery
        // poll supplied by the supervisor in case an event is missed.
        if !poll_lcu {
            return events;
        }

        let should_refresh_lockfile = self
            .last_lockfile_discovery
            .is_none_or(|last| last.elapsed() >= LOCKFILE_DISCOVERY_INTERVAL);
        if should_refresh_lockfile {
            self.last_lockfile_discovery = Some(Instant::now());
            match discover_lockfile_nonblocking(config).await {
                Some(discovery) => {
                    if self.lockfile_discovery_slow {
                        self.diagnostic("LCU lockfile 탐색 응답 정상화");
                    }
                    self.lockfile_discovery_slow = false;
                    self.cached_lockfile_discovery = Some(discovery);
                }
                None => {
                    if !self.lockfile_discovery_slow {
                        self.diagnostic(
                            "LCU lockfile 탐색이 2초를 초과해 이번 주기는 건너뜀 — Relay 처리는 계속함",
                        );
                    }
                    self.lockfile_discovery_slow = true;
                }
            }
        }

        let Some(discovery) = self.cached_lockfile_discovery.clone() else {
            return events;
        };
        if self.last_lockfile_diagnostics != discovery.diagnostics {
            for message in &discovery.diagnostics {
                self.diagnostic(message.clone());
            }
            self.last_lockfile_diagnostics = discovery.diagnostics.clone();
        }
        let Some(path) = discovery.path else {
            if self.lockfile_available != Some(false) {
                self.diagnostic("LCU lockfile 없음 — 관전 API 독립 조회만 계속함");
            }
            self.observe_disconnected();
            self.lockfile_available = Some(false);
            self.lcu_available = Some(false);
            return events;
        };
        if discovery.legacy_fallback && self.lockfile_available != Some(true) {
            self.diagnostic("LCU lockfile 엄격 검증 실패 — 과거 파일 탐색 방식으로 fallback");
        }
        if self.lockfile_available != Some(true) {
            self.diagnostic(format!("LCU lockfile 발견: {}", path.display()));
        }
        self.lockfile_available = Some(true);
        let Ok(client) =
            LcuClient::from_lockfile(&path).or_else(|_| LcuClient::from_lockfile_legacy(&path))
        else {
            if self.lcu_available != Some(false) {
                self.diagnostic("LCU lockfile/PID 검증 실패 — 관전 API 독립 조회는 계속함");
            }
            self.lcu_available = Some(false);
            return events;
        };
        self.observe_connection(client.identity());

        let phase_value = match client.request(Method::GET, GAMEFLOW_PHASE, None).await {
            Ok(value) => value,
            Err(error) => {
                if self.lcu_available != Some(false) {
                    self.diagnostic(format!("LCU gameflow API 응답 실패: {error}"));
                }
                self.lcu_available = Some(false);
                return events;
            }
        };
        self.observe_schema("gameflow_phase", phase_value.is_string());
        let Some(phase) = phase_value.as_str().map(str::to_owned) else {
            if self.lcu_available != Some(false) {
                self.diagnostic("LCU gameflow API 응답 형식 오류");
            }
            self.lcu_available = Some(false);
            return events;
        };
        if self.lcu_available != Some(true) {
            self.diagnostic("LCU gameflow API 연결됨");
        }
        self.lcu_available = Some(true);
        let previous_phase = self.gameflow.clone();
        if previous_phase.as_deref() != Some(phase.as_str()) {
            self.diagnostic(format!(
                "게임 단계 변경: {} -> {phase}",
                previous_phase.as_deref().unwrap_or("초기")
            ));
        }
        push_gameflow_snapshot(
            &mut self.gameflow,
            &mut self.last_gameflow_emit,
            phase.clone(),
            json!({"phase": phase, "lcu_ready": true}),
            &mut events,
        );
        if phase == "InProgress" && previous_phase.as_deref() != Some("InProgress") {
            self.eog_sent = false;
            self.last_eog_attempt = None;
            self.eog_recovery_started_at = None;
            self.eog_attempt_count = 0;
            self.eog_expected_game_id = None;
        }
        if is_eog_phase(&phase) && self.eog_recovery_started_at.is_none() {
            self.eog_recovery_started_at = Some(Instant::now());
            self.eog_attempt_count = 0;
            self.diagnostic(format!(
                "EOG 복구 시작: phase={phase} previous_phase={} live_game_id={}",
                previous_phase.as_deref().unwrap_or("none"),
                self.live_game_id.as_deref().unwrap_or("unknown")
            ));
        }

        if should_attempt_eog_recovery(
            &phase,
            previous_phase.as_deref(),
            self.eog_sent,
            self.last_eog_attempt,
            self.eog_recovery_started_at,
            self.eog_expected_game_id.as_deref(),
        ) {
            let attempt_started = Instant::now();
            self.last_eog_attempt = Some(attempt_started);
            self.eog_attempt_count = self.eog_attempt_count.saturating_add(1);
            let recovery_age_ms = self
                .eog_recovery_started_at
                .map(|started| started.elapsed().as_millis() as u64)
                .unwrap_or(0);
            let payload = eog_payload(
                &client,
                &phase,
                self.eog_expected_game_id.as_deref(),
                self.eog_attempt_count,
                recovery_age_ms,
            )
            .await;

            if self.eog_expected_game_id.is_none() {
                self.eog_expected_game_id = json_scalar_id(payload.get("gameId"));
            }
            let evidence_source = eog_result_evidence_source(&payload);
            self.eog_sent = evidence_source.is_some();
            let diagnostics = payload.get("eogDiagnostics").and_then(Value::as_object);
            let eog_status = diagnostics
                .and_then(|value| value.get("eogRequestStatus"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let eog_error = diagnostics
                .and_then(|value| value.get("eogRequestError"))
                .and_then(Value::as_str)
                .unwrap_or("none");
            let recent_status = diagnostics
                .and_then(|value| value.get("recentMatchRequestStatus"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let recent_game_id = diagnostics
                .and_then(|value| value.get("recentMatchGameId"))
                .and_then(|value| json_scalar_id(Some(value)))
                .unwrap_or_else(|| "unknown".into());
            let participants = payload
                .get("participants")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            let recent_participants = payload
                .pointer("/recentMatch/participants")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            self.diagnostic(format!(
                "EOG 시도 #{}: phase={} age_ms={} expected_game_id={} game_id={} eog_status={} eog_error={} participants={} recent_status={} recent_game_id={} recent_participants={} evidence={} elapsed_ms={}",
                self.eog_attempt_count,
                phase,
                recovery_age_ms,
                self.eog_expected_game_id.as_deref().unwrap_or("unknown"),
                json_scalar_id(payload.get("gameId")).as_deref().unwrap_or("unknown"),
                eog_status,
                eog_error,
                participants,
                recent_status,
                recent_game_id,
                recent_participants,
                evidence_source.unwrap_or("none"),
                attempt_started.elapsed().as_millis(),
            ));

            // Every attempt is archived by the generic EOG path for diagnostics.
            // Only evidence-bearing payloads are promoted to guild-match result ingest.
            events.push(("match_eog", payload.clone()));
            if self.eog_sent {
                events.push(("guild_match_eog", payload));
            }
        }

        if !self.eog_sent
            && self
                .eog_recovery_started_at
                .is_some_and(|started| started.elapsed() > EOG_POSTGAME_RECOVERY_GRACE)
        {
            self.diagnostic(format!(
                "EOG 복구 시간 초과: attempts={} expected_game_id={} phase={} grace_sec={}",
                self.eog_attempt_count,
                self.eog_expected_game_id.as_deref().unwrap_or("unknown"),
                phase,
                EOG_POSTGAME_RECOVERY_GRACE.as_secs()
            ));
            self.last_eog_attempt = None;
            self.eog_recovery_started_at = None;
            self.eog_attempt_count = 0;
            self.eog_expected_game_id = None;
        } else if !self.eog_sent
            && matches!(phase.as_str(), "ChampSelect" | "Matchmaking" | "GameStart")
            && self.eog_recovery_started_at.is_some()
        {
            self.diagnostic(format!(
                "새 게임 단계 진입으로 EOG 복구 종료: attempts={} expected_game_id={} phase={}",
                self.eog_attempt_count,
                self.eog_expected_game_id.as_deref().unwrap_or("unknown"),
                phase
            ));
            self.last_eog_attempt = None;
            self.eog_recovery_started_at = None;
            self.eog_attempt_count = 0;
            self.eog_expected_game_id = None;
        }

        let plan = phase_poll_plan(&phase);
        clear_inactive_phase_snapshots(self, &phase, plan, &mut events);

        if plan.ready_check {
            match client.request(Method::GET, READY_CHECK, None).await {
                Ok(value) => {
                    self.observe_schema(
                        "ready_check",
                        value.is_object()
                            && value.get("state").is_some_and(Value::is_string)
                            && value.get("playerResponse").is_some_and(Value::is_string),
                    );
                    let payload = ready_check_payload(&value);
                    push_changed(
                        &mut self.ready_check,
                        fingerprint(&payload),
                        "ready_check_update",
                        payload,
                        &mut events,
                    );
                }
                Err(error) => {
                    if !is_expected_missing_lcu_endpoint(&error) {
                        self.diagnostic(format!("LCU ready_check API 응답 실패: {error}"));
                    }
                }
            }
        }
        if plan.champ_select {
            match client.request(Method::GET, CHAMP_SELECT, None).await {
                Ok(value) => {
                    self.observe_schema(
                        "champ_select",
                        value.is_object()
                            && value.get("timer").is_some_and(Value::is_object)
                            && value.get("actions").is_some_and(Value::is_array),
                    );
                    let mut payload = champ_select_payload(&value);
                    if payload.get("is_spectating").and_then(Value::as_bool) == Some(true) {
                        let champ_select_picks = spectator_champ_select_picks(&value);
                        let gameflow_picks =
                            match client.request(Method::GET, GAMEFLOW_SESSION, None).await {
                                Ok(gameflow_session) => spectator_gameflow_picks(&gameflow_session),
                                Err(_) => None,
                            };
                        if let Some(observer_picks) =
                            prefer_more_complete_picks(champ_select_picks, gameflow_picks)
                        {
                            payload["observer_picks"] = observer_picks;
                        }
                    }
                    push_changed(
                        &mut self.champ_select,
                        champ_select_fingerprint(&payload),
                        "champ_select_update",
                        payload,
                        &mut events,
                    );
                }
                Err(error) => {
                    if !is_expected_missing_lcu_endpoint(&error) {
                        self.diagnostic(format!("LCU champ_select API 응답 실패: {error}"));
                    }
                }
            }
        }
        if plan.lobby {
            match client.request(Method::GET, LOBBY, None).await {
                Ok(value) => {
                    self.observe_schema(
                        "lobby",
                        value.is_object() && value.get("members").is_some_and(Value::is_array),
                    );
                    let payload = party_payload(&value);
                    push_changed(
                        &mut self.party,
                        fingerprint(&payload),
                        "party_lobby_update",
                        payload.clone(),
                        &mut events,
                    );
                    let status = participant_status(&phase, &payload);
                    push_changed(
                        &mut self.participant,
                        fingerprint(&status),
                        "participant_status_update",
                        status,
                        &mut events,
                    );
                }
                Err(error) => {
                    if !is_expected_missing_lcu_endpoint(&error) {
                        self.diagnostic(format!("LCU lobby API 응답 실패: {error}"));
                    }
                }
            }
        }
        events
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PhasePollPlan {
    ready_check: bool,
    champ_select: bool,
    lobby: bool,
}

fn phase_poll_plan(phase: &str) -> PhasePollPlan {
    match phase {
        "Lobby" => PhasePollPlan {
            lobby: true,
            ..PhasePollPlan::default()
        },
        "Matchmaking" => PhasePollPlan {
            ready_check: true,
            lobby: true,
            ..PhasePollPlan::default()
        },
        // ChampSelect can become available a few milliseconds before the
        // gameflow phase event arrives, so probe it while ReadyCheck is active.
        // This is event/8s driven now, not a 1s 404 loop.
        "ReadyCheck" => PhasePollPlan {
            ready_check: true,
            champ_select: true,
            lobby: true,
        },
        "ChampSelect" | "GameStart" => PhasePollPlan {
            champ_select: true,
            ..PhasePollPlan::default()
        },
        _ => PhasePollPlan::default(),
    }
}

fn clear_inactive_phase_snapshots(
    poller: &mut LcuEventPoller,
    phase: &str,
    plan: PhasePollPlan,
    events: &mut Vec<(&'static str, Value)>,
) {
    if !plan.ready_check && poller.ready_check.take().is_some() {
        events.push((
            "ready_check_update",
            json!({"active": false, "state": "", "player_response": ""}),
        ));
    }
    if !plan.champ_select && poller.champ_select.take().is_some() {
        events.push(("champ_select_update", json!({"active": false})));
    }
    if !plan.lobby && poller.party.take().is_some() {
        events.push((
            "party_lobby_update",
            json!({"in_lobby": false, "riot_ids_in_party": []}),
        ));
    }

    // Participant status is primarily phase-derived. Keep it fresh even when
    // the lobby endpoint is intentionally not queried for the current phase.
    if !plan.lobby {
        let status =
            participant_status(phase, &json!({"in_lobby": false, "riot_ids_in_party": []}));
        push_changed(
            &mut poller.participant,
            fingerprint(&status),
            "participant_status_update",
            status,
            events,
        );
    }
}

fn is_expected_missing_lcu_endpoint(error: &AgentError) -> bool {
    matches!(
        error,
        AgentError::Lcu(message) if message.contains("HTTP 404 Not Found")
    )
}

async fn discover_lockfile_nonblocking(config: &Config) -> Option<LockfileDiscovery> {
    let config = config.clone();
    match timeout(
        LOCKFILE_DISCOVERY_TIMEOUT,
        tokio::task::spawn_blocking(move || discover_lockfile(&config)),
    )
    .await
    {
        Ok(Ok(discovery)) => Some(discovery),
        Ok(Err(_)) | Err(_) => None,
    }
}

async fn wait_for_socket_retry(stop: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        _ = sleep(LCU_SOCKET_RETRY_DELAY) => !*stop.borrow(),
        changed = stop.changed() => changed.is_ok() && !*stop.borrow(),
    }
}

fn has_consistent_eog_result_participants(value: Option<&Value>) -> bool {
    let Some(rows) = value.and_then(Value::as_array) else {
        return false;
    };
    let mut valid_count = 0usize;
    let mut blue_result: Option<bool> = None;
    let mut red_result: Option<bool> = None;

    for row in rows {
        let Some(row) = row.as_object() else {
            continue;
        };
        let has_name = row
            .get("gameName")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        let has_tag = row
            .get("tagLine")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        let Some(team_id) = row.get("teamId").and_then(Value::as_i64) else {
            continue;
        };
        let Some(won) = row.get("won").and_then(Value::as_bool) else {
            continue;
        };
        if !has_name || !has_tag || (team_id != 100 && team_id != 200) {
            continue;
        }
        let slot = if team_id == 100 {
            &mut blue_result
        } else {
            &mut red_result
        };
        if let Some(existing) = *slot {
            if existing != won {
                return false;
            }
        }
        *slot = Some(won);
        valid_count += 1;
    }

    valid_count >= 9
        && blue_result.is_some()
        && red_result.is_some()
        && blue_result != red_result
}

fn json_scalar_id(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn is_eog_phase(phase: &str) -> bool {
    matches!(phase, "PreEndOfGame" | "EndOfGame" | "WaitingForStats")
}

fn eog_result_evidence_source(payload: &Value) -> Option<&'static str> {
    let expected_game_id = json_scalar_id(payload.get("expectedGameId"));
    let payload_game_id = json_scalar_id(payload.get("gameId"));
    let game_id_matches_expected = match (&expected_game_id, &payload_game_id) {
        (Some(expected), Some(actual)) => expected == actual,
        (Some(_), None) => false,
        (None, _) => true,
    };
    if game_id_matches_expected && has_consistent_eog_result_participants(payload.get("participants")) {
        return Some("eog_stats");
    }

    let recent = payload.get("recentMatch").and_then(Value::as_object)?;
    if !has_consistent_eog_result_participants(recent.get("participants")) {
        return None;
    }
    let authoritative_game_id = expected_game_id.or(payload_game_id)?;
    let recent_game_id = json_scalar_id(recent.get("gameId"))?;
    (authoritative_game_id == recent_game_id).then_some("recent_match")
}

fn has_usable_eog_result_evidence(payload: &Value) -> bool {
    eog_result_evidence_source(payload).is_some()
}

fn should_attempt_eog_recovery(
    phase: &str,
    previous_phase: Option<&str>,
    eog_sent: bool,
    last_attempt: Option<Instant>,
    recovery_started_at: Option<Instant>,
    expected_game_id: Option<&str>,
) -> bool {
    if eog_sent {
        return false;
    }
    let in_eog_phase = is_eog_phase(phase);
    let postgame_grace = matches!(phase, "Lobby" | "None")
        && recovery_started_at.is_some_and(|started| started.elapsed() <= EOG_POSTGAME_RECOVERY_GRACE)
        && expected_game_id.is_some();
    if !in_eog_phase && !postgame_grace {
        return false;
    }
    if in_eog_phase
        && !matches!(
            previous_phase,
            None
                | Some("InProgress")
                | Some("PreEndOfGame")
                | Some("EndOfGame")
                | Some("WaitingForStats")
        )
        && recovery_started_at.is_none()
    {
        return false;
    }
    // EndOfGame is the strongest signal that eog-stats should now be ready.
    // Retry immediately on the transition even if WaitingForStats was queried moments ago.
    if phase == "EndOfGame" && previous_phase != Some("EndOfGame") {
        return true;
    }
    match last_attempt {
        None => true,
        Some(attempt) => Instant::now().duration_since(attempt) >= EOG_RECOVERY_RETRY_INTERVAL,
    }
}

async fn eog_payload(
    client: &LcuClient,
    phase: &str,
    expected_game_id: Option<&str>,
    attempt: u32,
    recovery_age_ms: u64,
) -> Value {
    let mut none_reasons = Vec::new();
    let query_eog_stats = is_eog_phase(phase);
    let (eog, eog_request_status, eog_request_error) = if query_eog_stats {
        match client.request(Method::GET, EOG_STATS, None).await {
            Ok(value) => (Some(value), "ok", None),
            Err(error) => {
                none_reasons.push("eog_stats_request_failed");
                (None, "error", Some(error.to_string()))
            }
        }
    } else {
        none_reasons.push("eog_stats_skipped_postgame_recovery");
        (None, "skipped", None)
    };

    let (session, session_request_status, session_request_error) = match client
        .request(Method::GET, GAMEFLOW_SESSION, None)
        .await
    {
        Ok(value) => (Some(value), "ok", None),
        Err(error) => {
            none_reasons.push("gameflow_session_request_failed");
            (None, "error", Some(error.to_string()))
        }
    };

    let (recent_history, recent_request_status, recent_request_error) = match timeout(
        RECENT_MATCH_VERIFICATION_TIMEOUT,
        client.recent_match_verification(),
    )
    .await
    {
        Ok(Ok(value)) => (Some(value), "ok", None),
        Ok(Err(error)) => {
            none_reasons.push("recent_match_request_failed");
            (None, "error", Some(error.to_string()))
        }
        Err(_) => {
            none_reasons.push("recent_match_request_timeout");
            (None, "timeout", Some("recent match verification timeout".into()))
        }
    };
    let recent_match = recent_history
        .as_ref()
        .and_then(|value| value.get("latest"))
        .cloned();
    let recent_matches = recent_history
        .as_ref()
        .and_then(|value| value.get("matches"))
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let recent_game_id = recent_match
        .as_ref()
        .and_then(|value| json_scalar_id(value.get("gameId")));

    let mut participants = Vec::new();
    let mut missing_name_count = 0;
    let mut missing_tag_count = 0;
    let teams = eog
        .as_ref()
        .and_then(|value| value.get("teams"))
        .and_then(Value::as_array);
    if let Some(teams) = teams {
        for team in teams {
            let winning = team.get("isWinningTeam").and_then(Value::as_bool);
            for player in team
                .get("players")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(name) = player
                    .get("riotIdGameName")
                    .or_else(|| player.get("summonerName"))
                    .and_then(Value::as_str)
                else {
                    missing_name_count += 1;
                    continue;
                };
                let tag = player
                    .get("riotIdTagline")
                    .or_else(|| player.get("riotIdTagLine"))
                    .and_then(Value::as_str);
                if tag.is_none() {
                    missing_tag_count += 1;
                }
                participants.push(json!({
                    "gameName": name,
                    "tagLine": tag,
                    "teamId": player.get("teamId").cloned().unwrap_or(Value::Null),
                    "won": player.get("win").cloned().or_else(|| winning.map(Value::Bool)).unwrap_or(Value::Null)
                }));
            }
        }
    }
    if teams.is_none() {
        none_reasons.push("eog_stats_teams_missing");
    }
    if missing_name_count > 0 {
        none_reasons.push("participant_name_missing");
    }
    if missing_tag_count > 0 {
        none_reasons.push("participant_riot_tag_missing");
    }
    if participants.len() < 2 {
        none_reasons.push("participants_less_than_2");
    }

    // Keep only EOG fields Yummi consumes; detailed diagnostics are bounded strings/counts.
    // Riot's complete EOG/session objects are intentionally not forwarded.
    let eog_game_id = eog
        .as_ref()
        .and_then(|value| value.get("gameId").or_else(|| value.get("reportGameId")))
        .and_then(|value| json_scalar_id(Some(value)));
    let session_game_id = session
        .as_ref()
        .and_then(|value| value.pointer("/gameData/gameId"))
        .and_then(|value| json_scalar_id(Some(value)));
    let game_id = eog_game_id
        .clone()
        .or_else(|| expected_game_id.map(str::to_owned))
        .or_else(|| session_game_id.clone());
    let end_of_game_timestamp = eog
        .as_ref()
        .and_then(|value| value.get("endOfGameTimestamp"))
        .cloned()
        .unwrap_or(Value::Null);
    let captured_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut payload = json!({
        "source":"lcu-agent",
        "gameflowPhase": phase,
        "capturedAt": captured_at_ms.to_string(),
        "capturedAtMs": captured_at_ms,
        "gameId": game_id,
        "expectedGameId": expected_game_id,
        "participants": participants,
        "recentMatch": recent_match,
        "recentMatches": recent_matches,
        "eog_none_reason": (!none_reasons.is_empty()).then(|| none_reasons.join(",")),
        "eogStats": {
            "gameId": eog_game_id,
            "endOfGameTimestamp": end_of_game_timestamp
        },
        "eogDiagnostics": {
            "attempt": attempt,
            "recoveryAgeMs": recovery_age_ms,
            "phase": phase,
            "endpoint": EOG_STATS,
            "eogRequestStatus": eog_request_status,
            "eogRequestError": eog_request_error,
            "sessionRequestStatus": session_request_status,
            "sessionRequestError": session_request_error,
            "recentMatchRequestStatus": recent_request_status,
            "recentMatchRequestError": recent_request_error,
            "expectedGameId": expected_game_id,
            "eogGameId": eog_game_id,
            "sessionGameId": session_game_id,
            "recentMatchGameId": recent_game_id,
            "teamCount": teams.map_or(0, Vec::len),
            "participantCount": participants.len(),
            "recentParticipantCount": recent_match
                .as_ref()
                .and_then(|value| value.get("participants"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        }
    });
    let evidence_source = eog_result_evidence_source(&payload);
    if let Some(diagnostics) = payload
        .get_mut("eogDiagnostics")
        .and_then(Value::as_object_mut)
    {
        diagnostics.insert("evidenceSource".into(), json!(evidence_source));
        diagnostics.insert("usableEvidence".into(), json!(evidence_source.is_some()));
    }
    payload
}

fn is_lcu_event(text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let Some(parts) = value.as_array() else {
        return false;
    };
    parts.first().and_then(Value::as_i64) == Some(8)
        && parts.get(1).and_then(Value::as_str) == Some("OnJsonApiEvent")
        && parts
            .get(2)
            .and_then(|payload| payload.get("uri"))
            .and_then(Value::as_str)
            .is_some_and(|uri| matches!(uri, GAMEFLOW_PHASE | READY_CHECK | CHAMP_SELECT | LOBBY))
}

fn push_gameflow_snapshot(
    previous: &mut Option<String>,
    last_emit: &mut Option<Instant>,
    next: String,
    payload: Value,
    events: &mut Vec<(&'static str, Value)>,
) {
    let now = Instant::now();
    let changed = previous.as_deref() != Some(next.as_str());
    let snapshot_due = last_emit
        .is_none_or(|last| now.duration_since(last) >= GAMEFLOW_SNAPSHOT_INTERVAL);
    *previous = Some(next);
    if changed || snapshot_due {
        *last_emit = Some(now);
        events.push(("gameflow_update", payload));
    }
}


fn push_changed(
    previous: &mut Option<String>,
    next: String,
    message_type: &'static str,
    payload: Value,
    events: &mut Vec<(&'static str, Value)>,
) {
    if previous.as_deref() != Some(&next) {
        *previous = Some(next);
        events.push((message_type, payload));
    }
}

fn fingerprint(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn champ_select_fingerprint(value: &Value) -> String {
    let mut stable = value.clone();
    if let Some(object) = stable.as_object_mut() {
        object.remove("timer_ms");
        if let Some(timer) = object.get_mut("timer").and_then(Value::as_object_mut) {
            timer.remove("remaining_ms");
            timer.remove("captured_at_ms");
        }
    }
    fingerprint(&stable)
}

fn wall_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn live_game_fingerprint(
    value: &Value,
    respawn_samples: &mut HashMap<String, u8>,
) -> String {
    let mut stable = value.clone();
    if let Some(object) = stable.as_object_mut() {
        // Wall-clock / continuously advancing game clock values must not make an
        // otherwise identical live state look changed. The original payload still
        // carries these timestamps when a meaningful state change is emitted.
        object.remove("captured_at_ms");
        object.remove("match_created_at_ms");

        // active_player is local-Agent-only data (not shared match state), and its
        // continuously changing current_gold would otherwise force an update almost
        // every poll. Keep it in the transmitted payload, but ignore it for dedupe.
        object.remove("active_player");

        if let Some(game) = object.get_mut("game").and_then(Value::as_object_mut) {
            game.remove("time_seconds");
            game.remove("game_time");
        }

        if let Some(participants) = object
            .get_mut("participants")
            .and_then(Value::as_array_mut)
        {
            let mut alive_keys = Vec::new();
            for (index, participant) in participants.iter_mut().enumerate() {
                let Some(row) = participant.as_object_mut() else {
                    continue;
                };
                let key = participant_fingerprint_key(row, index);
                let is_dead = row
                    .get("is_dead")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                // ward_score and creep_score remain in emitted payloads, but changes
                // to either value alone are too noisy to trigger a network update.
                // Item slot/order/metadata changes are also ignored for dedupe; only
                // the item id + count multiset is fingerprinted.
                row.remove("ward_score");
                row.remove("creep_score");
                normalize_live_items(row);

                // respawn_timer is a continuously decreasing float. Preserve exactly
                // two post-death samples in the outgoing payload, but dedupe on a
                // bounded sample phase (1 -> 2 -> 2...) so the countdown itself does
                // not cause endless updates. is_dead still catches the actual revive.
                row.remove("respawn_timer");
                if is_dead {
                    let sample = respawn_samples.entry(key).or_insert(0);
                    *sample = (*sample + 1).min(2);
                    row.insert(
                        "respawn_sample_phase".to_string(),
                        Value::from(*sample),
                    );
                } else {
                    alive_keys.push(key);
                }
            }
            for key in alive_keys {
                respawn_samples.remove(&key);
            }
        }
    }
    fingerprint(&stable)
}

fn participant_fingerprint_key(
    participant: &serde_json::Map<String, Value>,
    index: usize,
) -> String {
    for field in ["riot_id", "summoner_name", "riot_id_game_name"] {
        if let Some(value) = participant
            .get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return format!("{field}:{value}");
        }
    }
    format!("index:{index}")
}

fn normalize_live_items(participant: &mut serde_json::Map<String, Value>) {
    let Some(items) = participant.get_mut("items").and_then(Value::as_array_mut) else {
        return;
    };

    let mut normalized = items
        .iter()
        .map(|item| {
            let id = item
                .get("item_id")
                .filter(|value| !value.is_null())
                .or_else(|| item.get("id"))
                .cloned()
                .unwrap_or(Value::Null);
            let count = item
                .get("count")
                .cloned()
                .unwrap_or_else(|| Value::from(1));
            json!({"id": id, "count": count})
        })
        .collect::<Vec<_>>();

    normalized.sort_by_cached_key(|item| {
        (
            item.get("id")
                .map(|value| value.to_string())
                .unwrap_or_default(),
            item.get("count")
                .map(|value| value.to_string())
                .unwrap_or_default(),
        )
    });
    *items = normalized;
}

fn ready_check_payload(value: &Value) -> Value {
    let state = value.get("state").and_then(Value::as_str).unwrap_or("");
    let player_response = value
        .get("playerResponse")
        .and_then(Value::as_str)
        .unwrap_or("");
    let active = matches!(state, "InProgress" | "Waiting")
        && matches!(player_response, "" | "None" | "Pending");
    json!({"active": active, "state": state, "player_response": player_response})
}

fn champ_select_payload(value: &Value) -> Value {
    let Some(session) = value.as_object() else {
        return json!({"active": false});
    };
    let local_cell_id = session
        .get("localPlayerCellId")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let actions = session
        .get("actions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
        .map(|action| {
            json!({
                "id": action.get("id").cloned().unwrap_or(Value::Null),
                "type": action.get("type").cloned().unwrap_or(Value::String(String::new())),
                "champion_id": action.get("championId").cloned().unwrap_or(Value::from(0)),
                "completed": action.get("completed").and_then(Value::as_bool).unwrap_or(false),
                "is_ally_action": action.get("isAllyAction").and_then(Value::as_bool).unwrap_or(false),
                "is_in_progress": action.get("isInProgress").and_then(Value::as_bool).unwrap_or(false),
                "actor_cell_id": action.get("actorCellId").cloned().unwrap_or(Value::from(-1)),
            })
        })
        .collect::<Vec<_>>();
    let current_action = actions.iter().find(|action| {
        action.get("is_ally_action").and_then(Value::as_bool) == Some(true)
            && action.get("is_in_progress").and_then(Value::as_bool) == Some(true)
            && action
                .get("actor_cell_id")
                .and_then(Value::as_i64)
                .is_none_or(|cell| cell < 0 || cell == local_cell_id)
    });
    let timer = session.get("timer");
    let remaining_ms = timer
        .and_then(|timer| timer.get("adjustedTimeLeftInPhase"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let total_ms = timer
        .and_then(|timer| timer.get("totalTimeInPhase"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let captured_at_ms = wall_clock_ms();
    let phase_end_at_ms = timer
        .and_then(|timer| timer.get("phaseEndTimeInEpochMs"))
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .or_else(|| {
            timer
                .and_then(|timer| timer.get("internalNowInEpochMs"))
                .and_then(Value::as_u64)
                .filter(|value| *value > 0)
                .map(|now| now.saturating_add(remaining_ms as u64))
        });
    json!({
        "active": true,
        "phase": timer.and_then(|timer| timer.get("phase")).and_then(Value::as_str).unwrap_or(""),
        "timer_ms": remaining_ms,
        "timer": {
            "remaining_ms": remaining_ms,
            "total_ms": total_ms,
            "captured_at_ms": captured_at_ms,
            "phase_end_at_ms": phase_end_at_ms,
            "authoritative_epoch": phase_end_at_ms.is_some(),
        },
        "local_cell_id": local_cell_id,
        "is_spectating": session.get("isSpectating").and_then(Value::as_bool).unwrap_or(false),
        "my_team": team_payload(session.get("myTeam")),
        "their_team": team_payload(session.get("theirTeam")),
        "actions": actions,
        "current_action": current_action.cloned().unwrap_or(Value::Null),
    })
}

fn spectator_member_side(member: &Value) -> Option<&'static str> {
    match member.get("team").and_then(Value::as_i64) {
        Some(1 | 100) => Some("blue"),
        Some(2 | 200) => Some("red"),
        _ => None,
    }
}

fn push_unique_pick(picks: &mut Vec<Value>, champion_id: u64) {
    if champion_id == 0
        || picks
            .iter()
            .any(|value| value.as_u64() == Some(champion_id))
    {
        return;
    }
    picks.push(Value::from(champion_id));
}

fn spectator_champ_select_picks(session: &Value) -> Option<Value> {
    let mut blue = Vec::new();
    let mut red = Vec::new();
    let mut cell_sides: Vec<(i64, &'static str)> = Vec::new();
    for team_key in ["myTeam", "theirTeam"] {
        for member in session
            .get(team_key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(side) = spectator_member_side(member) else {
                continue;
            };
            if let Some(cell_id) = member.get("cellId").and_then(Value::as_i64) {
                cell_sides.push((cell_id, side));
            }
            if let Some(champion_id) = positive_champion_id(member) {
                push_unique_pick(
                    if side == "blue" { &mut blue } else { &mut red },
                    champion_id,
                );
            }
        }
    }
    for action in session
        .get("actions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
    {
        if action.get("completed").and_then(Value::as_bool) != Some(true)
            || action.get("type").and_then(Value::as_str) != Some("pick")
        {
            continue;
        }
        let Some(champion_id) = positive_champion_id(action) else {
            continue;
        };
        let Some(actor_cell_id) = action.get("actorCellId").and_then(Value::as_i64) else {
            continue;
        };
        let Some((_, side)) = cell_sides
            .iter()
            .find(|(cell_id, _)| *cell_id == actor_cell_id)
        else {
            continue;
        };
        push_unique_pick(
            if *side == "blue" { &mut blue } else { &mut red },
            champion_id,
        );
    }
    if blue.is_empty() && red.is_empty() {
        return None;
    }
    Some(json!({"blue": blue, "red": red}))
}

fn prefer_more_complete_picks(primary: Option<Value>, fallback: Option<Value>) -> Option<Value> {
    if primary.is_none() {
        return fallback;
    }
    if fallback.is_none() {
        return primary;
    }
    let primary = primary.unwrap_or_default();
    let fallback = fallback.unwrap_or_default();
    let choose = |side: &str| {
        let first = primary
            .get(side)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let second = fallback
            .get(side)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if second.len() > first.len() {
            second
        } else {
            first
        }
    };
    let blue = choose("blue");
    let red = choose("red");
    if blue.is_empty() && red.is_empty() {
        None
    } else {
        Some(json!({"blue": blue, "red": red}))
    }
}

fn spectator_identity_keys(value: &Value) -> Vec<String> {
    let mut keys = Vec::new();
    for field in [
        "summonerInternalName",
        "puuid",
        "summonerId",
        "summonerName",
        "riotId",
        "riotIdGameName",
    ] {
        let Some(raw) = value.get(field) else {
            continue;
        };
        let text = match raw {
            Value::String(value) => value.trim().to_ascii_lowercase(),
            Value::Number(value) => value.to_string(),
            _ => String::new(),
        };
        if !text.is_empty() && !keys.contains(&text) {
            keys.push(text);
        }
    }
    keys
}

fn positive_champion_id(value: &Value) -> Option<u64> {
    value
        .get("championId")
        .and_then(Value::as_u64)
        .filter(|champion_id| *champion_id > 0)
}

fn spectator_team_picks(team: Option<&Value>, selections: &[Value]) -> Vec<Value> {
    let mut picks = Vec::new();
    for member in team.and_then(Value::as_array).into_iter().flatten() {
        let champion_id = positive_champion_id(member).or_else(|| {
            let member_keys = spectator_identity_keys(member);
            if member_keys.is_empty() {
                return None;
            }
            selections.iter().find_map(|selection| {
                let selection_keys = spectator_identity_keys(selection);
                if member_keys
                    .iter()
                    .any(|member_key| selection_keys.contains(member_key))
                {
                    positive_champion_id(selection)
                } else {
                    None
                }
            })
        });
        if let Some(champion_id) = champion_id {
            picks.push(Value::from(champion_id));
        }
    }
    picks
}

fn spectator_gameflow_picks(session: &Value) -> Option<Value> {
    let game_data = session.get("gameData")?;
    let selections = game_data
        .get("playerChampionSelections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let blue = spectator_team_picks(game_data.get("teamOne"), &selections);
    let red = spectator_team_picks(game_data.get("teamTwo"), &selections);
    if blue.is_empty() && red.is_empty() {
        return None;
    }
    Some(json!({
        "blue": blue,
        "red": red,
    }))
}

fn team_payload(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|member| json!({
            "cell_id": member.get("cellId").cloned().unwrap_or(Value::from(0)),
            "summoner_name": member.get("summonerName").cloned().unwrap_or(Value::String(String::new())),
            "assigned_position": member.get("assignedPosition").cloned().unwrap_or(Value::String(String::new())),
            "champion_id": member.get("championId").cloned().unwrap_or(Value::from(0)),
            "champion_pick_intent": member.get("championPickIntent").cloned().unwrap_or(Value::from(0)),
            "team": member.get("team").cloned().unwrap_or(Value::Null),
        }))
        .collect()
}

fn party_payload(value: &Value) -> Value {
    let members = value.get("members").and_then(Value::as_array);
    let in_lobby = value
        .pointer("/gameConfig/queueId")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        > 0
        || members.is_some_and(|members| !members.is_empty());
    let riot_ids_in_party = members
        .into_iter()
        .flatten()
        .filter_map(|member| {
            let name = member
                .get("riotIdGameName")
                .or_else(|| member.get("gameName"))?
                .as_str()?;
            let tag = member
                .get("riotIdTagLine")
                .or_else(|| member.get("riotIdTagline"))
                .or_else(|| member.get("gameTag"))?
                .as_str()?;
            Some(format!("{name}#{tag}"))
        })
        .collect::<Vec<_>>();
    json!({"in_lobby": in_lobby, "riot_ids_in_party": riot_ids_in_party})
}

fn participant_status(phase: &str, party: &Value) -> Value {
    let status = match phase {
        "InProgress" | "PreEndOfGame" => "in_game",
        "ChampSelect" => "champ_select",
        "Lobby" if party.get("in_lobby").and_then(Value::as_bool) == Some(true) => "lobby",
        _ => "waiting",
    };
    json!({"status": status, "phase": phase, "game_started_at_ms": Value::Null, "lcu_ready": true, "agent_online": true})
}

fn live_game_payload(value: &Value, events: Option<&Value>) -> Option<Value> {
    live_game_payload_at(value, events, unix_now_ms())
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn live_game_payload_at(
    value: &Value,
    events: Option<&Value>,
    captured_at_ms: u64,
) -> Option<Value> {
    let game_data = value.get("gameData")?.as_object()?;
    let players = value
        .get("allPlayers")
        .or_else(|| value.get("players"))
        .and_then(Value::as_array)?;
    if players.len() != LIVE_GAME_PARTICIPANT_COUNT {
        return None;
    }

    let participants = players.iter().map(live_player_payload).collect::<Vec<_>>();
    let match_created_at_ms = game_data
        .get("gameTime")
        .and_then(Value::as_f64)
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(|seconds| captured_at_ms.saturating_sub((seconds * 1000.0).round() as u64));
    Some(json!({
        "source": "lcu-agent",
        "client_mode": live_client_mode(value.get("activePlayer")),
        "phase": "InProgress",
        "captured_at_ms": captured_at_ms,
        "match_created_at_ms": match_created_at_ms,
        "game": {
            "id": game_data.get("gameId").cloned().unwrap_or(Value::Null),
            "game_id": game_data.get("gameId").cloned().unwrap_or(Value::Null),
            "mode": game_data.get("gameMode").cloned().unwrap_or(Value::Null),
            "game_mode": game_data.get("gameMode").cloned().unwrap_or(Value::Null),
            "map": game_data.get("mapName").cloned().unwrap_or(Value::Null),
            "map_name": game_data.get("mapName").cloned().unwrap_or(Value::Null),
            "map_number": game_data.get("mapNumber").cloned().unwrap_or(Value::Null),
            "terrain": game_data.get("mapTerrain").cloned().unwrap_or(Value::Null),
            "time_seconds": game_data.get("gameTime").cloned().unwrap_or(Value::Null),
            "game_time": game_data.get("gameTime").cloned().unwrap_or(Value::Null)
        },
        "active_player": active_player_payload(value.get("activePlayer")),
        "participants": participants,
        "events": live_events_payload(events),
    }))
}

fn live_client_mode(value: Option<&Value>) -> &'static str {
    let Some(player) = value.and_then(Value::as_object) else {
        return "spectator";
    };
    let has_identity = ["summonerName", "riotId", "riotIdGameName", "gameName"]
        .iter()
        .any(|key| {
            player
                .get(*key)
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        });
    if has_identity {
        "player"
    } else {
        "spectator"
    }
}

fn live_events_payload(value: Option<&Value>) -> Vec<Value> {
    let Some(events) = value
        .and_then(|value| value.get("Events").or_else(|| value.get("events")))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    events
        .iter()
        .filter_map(|event| {
            let name = event
                .get("EventName")
                .or_else(|| event.get("eventName"))
                .and_then(Value::as_str)?;
            let assisters = event
                .get("Assisters")
                .or_else(|| event.get("assisters"))
                .and_then(Value::as_array)
                .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            Some(json!({
                "id": event.get("EventID").or_else(|| event.get("eventId")).cloned().unwrap_or(Value::Null),
                "name": name,
                "time_seconds": event.get("EventTime").or_else(|| event.get("eventTime")).cloned().unwrap_or(Value::Null),
                "killer_name": event.get("KillerName").or_else(|| event.get("killerName")).cloned().unwrap_or(Value::Null),
                "victim_name": event.get("VictimName").or_else(|| event.get("victimName")).cloned().unwrap_or(Value::Null),
                "killer_team": event.get("KillerTeam").or_else(|| event.get("killerTeam")).cloned().unwrap_or(Value::Null),
                "victim_team": event.get("VictimTeam").or_else(|| event.get("victimTeam")).cloned().unwrap_or(Value::Null),
                "team": event.get("Team").or_else(|| event.get("team")).cloned().unwrap_or(Value::Null),
                "killer_champion": event.get("KillerChampion").or_else(|| event.get("killerChampion")).cloned().unwrap_or(Value::Null),
                "victim_champion": event.get("VictimChampion").or_else(|| event.get("victimChampion")).cloned().unwrap_or(Value::Null),
                "multi_kill": event.get("MultiKill").or_else(|| event.get("multiKill")).or_else(|| event.get("Multikill")).cloned().unwrap_or(Value::Null),
                "assisters": assisters,
                "dragon_type": event.get("DragonType").or_else(|| event.get("dragonType")).cloned().unwrap_or(Value::Null),
                "turret_killed": event.get("TurretKilled").or_else(|| event.get("turretKilled")).cloned().unwrap_or(Value::Null),
                "inhibitor_killed": event.get("InhibKilled").or_else(|| event.get("inhibKilled")).cloned().unwrap_or(Value::Null),
                "monster_type": event.get("MonsterType").or_else(|| event.get("monsterType")).cloned().unwrap_or(Value::Null)
            }))
        })
        .collect()
}

fn live_player_payload(player: &Value) -> Value {
    let scores = player.get("scores");
    let items = player
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(7)
        .map(|item| {
            json!({
                "id": item.get("itemID").or_else(|| item.get("id")).cloned().unwrap_or(Value::Null),
                "item_id": item.get("itemID").or_else(|| item.get("id")).cloned().unwrap_or(Value::Null),
                "name": item.get("displayName").or_else(|| item.get("name")).cloned().unwrap_or(Value::Null),
                "display_name": item.get("displayName").or_else(|| item.get("name")).cloned().unwrap_or(Value::Null),
                "count": item.get("count").cloned().unwrap_or(Value::from(1)),
                "can_use": item.get("canUse").cloned().unwrap_or(Value::Null),
                "consumable": item.get("consumable").cloned().unwrap_or(Value::Null),
                "price": item.get("price").cloned().unwrap_or(Value::Null),
                "slot": item.get("slot").cloned().unwrap_or(Value::Null)
            })
        })
        .collect::<Vec<_>>();
    json!({
        "summoner_name": player.get("summonerName").cloned().unwrap_or(Value::Null),
        "riot_id": player.get("riotId").or_else(|| player.get("riot_id")).cloned().unwrap_or(Value::Null),
        "riot_id_game_name": player.get("riotIdGameName").or_else(|| player.get("gameName")).cloned().unwrap_or(Value::Null),
        "riot_id_tag_line": player.get("riotIdTagLine").or_else(|| player.get("tagLine")).cloned().unwrap_or(Value::Null),
        "champion_name": player.get("championName").cloned().unwrap_or(Value::Null),
        "raw_champion_name": player.get("rawChampionName").cloned().unwrap_or(Value::Null),
        "team": player.get("team").cloned().unwrap_or(Value::Null),
        "position": player.get("position").cloned().unwrap_or(Value::Null),
        "is_bot": player.get("isBot").cloned().unwrap_or(Value::Bool(false)),
        "is_dead": player.get("isDead").cloned().unwrap_or(Value::Bool(false)),
        "respawn_timer": player.get("respawnTimer").cloned().unwrap_or(Value::Null),
        "level": player.get("level").cloned().unwrap_or(Value::Null),
        "skin_id": player.get("skinID").cloned().unwrap_or(Value::Null),
        "gold": player.get("gold").or_else(|| player.get("currentGold")).cloned().unwrap_or(Value::Null),
        "kills": scores.and_then(|value| value.get("kills")).cloned().unwrap_or(Value::from(0)),
        "deaths": scores.and_then(|value| value.get("deaths")).cloned().unwrap_or(Value::from(0)),
        "assists": scores.and_then(|value| value.get("assists")).cloned().unwrap_or(Value::from(0)),
        "creep_score": scores.and_then(|value| value.get("creepScore")).cloned().unwrap_or(Value::from(0)),
        "ward_score": scores.and_then(|value| value.get("wardScore")).cloned().unwrap_or(Value::from(0)),
        "runes": player.get("runes").cloned().unwrap_or(Value::Null),
        "items": items,
        "summoner_spells": player.get("summonerSpells").cloned().unwrap_or(Value::Null)
    })
}

fn active_player_payload(value: Option<&Value>) -> Value {
    let Some(player) = value else {
        return Value::Null;
    };
    let scores = player.get("scores");
    json!({
        "summoner_name": player.get("summonerName").cloned().unwrap_or(Value::Null),
        "riot_id": player.get("riotId").or_else(|| player.get("riot_id")).cloned().unwrap_or(Value::Null),
        "riot_id_game_name": player.get("riotIdGameName").or_else(|| player.get("gameName")).cloned().unwrap_or(Value::Null),
        "riot_id_tag_line": player.get("riotIdTagLine").or_else(|| player.get("tagLine")).cloned().unwrap_or(Value::Null),
        "level": player.get("level").cloned().unwrap_or(Value::Null),
        "current_gold": player.get("currentGold").cloned().unwrap_or(Value::Null),
        "kills": scores.and_then(|value| value.get("kills")).cloned().unwrap_or(Value::from(0)),
        "deaths": scores.and_then(|value| value.get("deaths")).cloned().unwrap_or(Value::from(0)),
        "assists": scores.and_then(|value| value.get("assists")).cloned().unwrap_or(Value::from(0)),
        "creep_score": scores.and_then(|value| value.get("creepScore")).cloned().unwrap_or(Value::from(0)),
        "ward_score": scores.and_then(|value| value.get("wardScore")).cloned().unwrap_or(Value::from(0)),
        "summoner_spells": player.get("summonerSpells").cloned().unwrap_or(Value::Null)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameflow_snapshot_reemits_unchanged_phase_after_interval() {
        let mut previous = Some("ChampSelect".to_string());
        let mut last_emit = Some(Instant::now() - GAMEFLOW_SNAPSHOT_INTERVAL);
        let mut events = Vec::new();

        push_gameflow_snapshot(
            &mut previous,
            &mut last_emit,
            "ChampSelect".to_string(),
            json!({"phase": "ChampSelect", "lcu_ready": true}),
            &mut events,
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "gameflow_update");
        assert_eq!(events[0].1["phase"], "ChampSelect");

        events.clear();
        push_gameflow_snapshot(
            &mut previous,
            &mut last_emit,
            "ChampSelect".to_string(),
            json!({"phase": "ChampSelect", "lcu_ready": true}),
            &mut events,
        );
        assert!(events.is_empty());

        push_gameflow_snapshot(
            &mut previous,
            &mut last_emit,
            "InProgress".to_string(),
            json!({"phase": "InProgress", "lcu_ready": true}),
            &mut events,
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1["phase"], "InProgress");
    }

    #[test]
    fn eog_recovery_accepts_late_connection_and_retries_after_interval() {
        assert!(should_attempt_eog_recovery(
            "WaitingForStats",
            None,
            false,
            None,
            Some(Instant::now()),
            None,
        ));
        assert!(!should_attempt_eog_recovery(
            "WaitingForStats",
            Some("WaitingForStats"),
            false,
            Some(Instant::now()),
            Some(Instant::now()),
            Some("200"),
        ));
    }

    #[test]
    fn eog_recovery_immediately_retries_when_end_of_game_arrives() {
        assert!(should_attempt_eog_recovery(
            "EndOfGame",
            Some("WaitingForStats"),
            false,
            Some(Instant::now()),
            Some(Instant::now()),
            Some("200"),
        ));
    }

    #[test]
    fn eog_recovery_continues_into_lobby_with_known_game_id() {
        assert!(should_attempt_eog_recovery(
            "Lobby",
            Some("WaitingForStats"),
            false,
            None,
            Some(Instant::now()),
            Some("200"),
        ));
        assert!(!should_attempt_eog_recovery(
            "Lobby",
            Some("WaitingForStats"),
            false,
            None,
            Some(Instant::now()),
            None,
        ));
    }

    #[test]
    fn eog_recovery_retries_until_recent_match_matches_game_id() {
        let stale = json!({
            "gameId": 200,
            "participants": [],
            "recentMatch": {
                "gameId": 199,
                "participants": (0..10).map(|i| json!({
                    "gameName": format!("Player{i}"),
                    "tagLine": "KR1",
                    "teamId": if i < 5 { 100 } else { 200 },
                    "won": i < 5
                })).collect::<Vec<_>>()
            }
        });
        assert!(!has_usable_eog_result_evidence(&stale));

        let current = json!({
            "gameId": 200,
            "participants": [],
            "recentMatch": {
                "gameId": 200,
                "participants": (0..10).map(|i| json!({
                    "gameName": format!("Player{i}"),
                    "tagLine": "KR1",
                    "teamId": if i < 5 { 100 } else { 200 },
                    "won": i < 5
                })).collect::<Vec<_>>()
            }
        });
        assert!(has_usable_eog_result_evidence(&current));
        assert_eq!(eog_result_evidence_source(&current), Some("recent_match"));

        let unknown_game = json!({
            "participants": [],
            "recentMatch": current["recentMatch"].clone()
        });
        assert!(!has_usable_eog_result_evidence(&unknown_game));

        let mismatched_expected = json!({
            "expectedGameId": 201,
            "gameId": 200,
            "participants": (0..10).map(|i| json!({
                "gameName": format!("Player{i}"),
                "tagLine": "KR1",
                "teamId": if i < 5 { 100 } else { 200 },
                "won": i < 5
            })).collect::<Vec<_>>()
        });
        assert!(!has_usable_eog_result_evidence(&mismatched_expected));
    }

    #[test]
    fn connection_generation_resets_lcu_snapshots_on_identity_change() {
        let mut poller = LcuEventPoller::default();
        poller.gameflow = Some("Lobby".into());
        poller.ready_check = Some("pending".into());
        poller.eog_sent = true;

        poller.observe_connection(LcuIdentity {
            process_id: 10,
            port: 5000,
        });
        assert_eq!(poller.connection_generation, 1);
        assert!(poller.gameflow.is_none());
        assert!(poller.ready_check.is_none());
        assert!(!poller.eog_sent);

        poller.gameflow = Some("ChampSelect".into());
        poller.observe_connection(LcuIdentity {
            process_id: 10,
            port: 5000,
        });
        assert_eq!(poller.connection_generation, 1);
        assert_eq!(poller.gameflow.as_deref(), Some("ChampSelect"));

        poller.observe_connection(LcuIdentity {
            process_id: 11,
            port: 5001,
        });
        assert_eq!(poller.connection_generation, 2);
        assert!(poller.gameflow.is_none());
    }

    #[test]
    fn disconnect_forces_next_connection_to_get_a_fresh_generation() {
        let mut poller = LcuEventPoller::default();
        let identity = LcuIdentity {
            process_id: 10,
            port: 5000,
        };
        poller.observe_connection(identity);
        poller.gameflow = Some("Lobby".into());
        poller.observe_disconnected();
        assert_eq!(poller.connection_generation, 2);
        assert!(poller.connection_identity.is_none());
        assert!(poller.gameflow.is_none());

        poller.observe_connection(identity);
        assert_eq!(poller.connection_generation, 3);
    }

    #[test]
    fn schema_canary_reports_drift_once_and_recovery_once() {
        let mut poller = LcuEventPoller::default();
        poller.observe_schema("champ_select", false);
        poller.observe_schema("champ_select", false);
        assert_eq!(poller.take_diagnostics().len(), 1);
        poller.observe_schema("champ_select", true);
        let recovered = poller.take_diagnostics();
        assert_eq!(recovered.len(), 1);
        assert!(recovered[0].contains("복구"));
    }

    #[test]
    fn ready_check_matches_legacy_active_rule() {
        assert_eq!(
            ready_check_payload(&json!({"state":"InProgress","playerResponse":"Pending"}))
                ["active"],
            true
        );
        assert_eq!(
            ready_check_payload(&json!({"state":"Accepted","playerResponse":"Accepted"}))["active"],
            false
        );
    }

    #[test]
    fn champ_select_payload_preserves_relay_keys() {
        let payload = champ_select_payload(
            &json!({"localPlayerCellId": 1, "timer":{"phase":"BAN_PICK","adjustedTimeLeftInPhase":12000}, "actions":[[{"id":7,"type":"pick","championId":10,"isAllyAction":true,"isInProgress":true,"actorCellId":1}]]}),
        );
        assert_eq!(payload["active"], true);
        assert_eq!(payload["is_spectating"], false);
        assert_eq!(payload["actions"][0]["champion_id"], 10);
        assert_eq!(payload["current_action"]["id"], 7);
        assert_eq!(payload["timer"]["remaining_ms"], 12000);
        assert!(payload["timer"]["captured_at_ms"].as_u64().is_some());
    }

    #[test]
    fn spectator_champ_select_picks_uses_member_team_and_actions() {
        let picks = spectator_champ_select_picks(&json!({
            "myTeam": [
                {"cellId": 1, "team": 1, "championId": 103},
                {"cellId": 2, "team": 1, "championId": 0}
            ],
            "theirTeam": [
                {"cellId": 6, "team": 2, "championId": 86}
            ],
            "actions": [[
                {"actorCellId": 2, "type": "pick", "completed": true, "championId": 22}
            ]]
        }))
        .unwrap();
        assert_eq!(picks["blue"], json!([103, 22]));
        assert_eq!(picks["red"], json!([86]));
    }

    #[test]
    fn spectator_gameflow_picks_maps_both_teams() {
        let picks = spectator_gameflow_picks(&json!({
            "gameData": {
                "teamOne": [
                    {"summonerInternalName": "blue-1"},
                    {"summonerInternalName": "blue-2", "championId": 22}
                ],
                "teamTwo": [
                    {"summonerInternalName": "red-1"}
                ],
                "playerChampionSelections": [
                    {"summonerInternalName": "blue-1", "championId": 103},
                    {"summonerInternalName": "red-1", "championId": 86}
                ]
            }
        }))
        .unwrap();
        assert_eq!(picks["blue"], json!([103, 22]));
        assert_eq!(picks["red"], json!([86]));
    }

    #[test]
    fn spectator_pick_fallback_prefers_more_complete_side() {
        let picks = prefer_more_complete_picks(
            Some(json!({"blue": [103], "red": [86, 51]})),
            Some(json!({"blue": [103, 22, 13], "red": [86]})),
        )
        .unwrap();
        assert_eq!(picks["blue"], json!([103, 22, 13]));
        assert_eq!(picks["red"], json!([86, 51]));
    }

    #[test]
    fn champ_select_timer_uses_lcu_epoch_when_available() {
        let payload = champ_select_payload(&json!({
            "localPlayerCellId": 1,
            "timer": {
                "phase": "BAN_PICK",
                "adjustedTimeLeftInPhase": 5000,
                "internalNowInEpochMs": 100000
            },
            "actions": []
        }));
        assert_eq!(payload["timer"]["phase_end_at_ms"], 105000);
        assert_eq!(payload["timer"]["authoritative_epoch"], true);
    }

    #[test]
    fn live_game_payload_includes_participant_kda_and_items_without_raw_objects() {
        let payload = live_game_payload_at(&json!({
            "gameData": {"gameId": 42, "gameMode": "CLASSIC", "gameTime": 120.5, "futureSecret": "do-not-forward"},
            "activePlayer": {"summonerName": "Me", "scores": {"kills": 2, "deaths": 1, "assists": 3}, "futureSecret": "do-not-forward"},
            "allPlayers": [{
                "summonerName": "Me", "riotId": "Me#KR1", "riotIdGameName": "Me", "riotIdTagLine": "KR1",
                "championName": "Ahri", "rawChampionName": "game_character_displayname_Ahri", "team": "ORDER",
                "position": "MIDDLE", "respawnTimer": 0.0, "skinID": 123, "runes": {"keystone": {"id": 8112}},
                "scores": {"kills": 2, "deaths": 1, "assists": 3, "creepScore": 80},
                "items": [{"itemID": 1056, "displayName": "Doran's Ring", "count": 1, "slot": 0, "canUse": true, "futureSecret": "do-not-forward"}],
                "futureSecret": "do-not-forward"
            },
            {"summonerName":"P2"},{"summonerName":"P3"},{"summonerName":"P4"},{"summonerName":"P5"},
            {"summonerName":"P6"},{"summonerName":"P7"},{"summonerName":"P8"},{"summonerName":"P9"},{"summonerName":"P10"}]
        }), None, 1_000_000)
        .unwrap();
        assert_eq!(payload["client_mode"], "player");
        assert_eq!(payload["game"]["id"], 42);
        assert_eq!(payload["captured_at_ms"], 1_000_000);
        assert_eq!(payload["match_created_at_ms"], 879_500);
        assert_eq!(payload["participants"][0]["kills"], 2);
        assert_eq!(payload["participants"][0]["deaths"], 1);
        assert_eq!(payload["participants"][0]["assists"], 3);
        assert_eq!(payload["participants"][0]["items"][0]["id"], 1056);
        assert_eq!(payload["participants"][0]["riot_id"], "Me#KR1");
        assert_eq!(payload["participants"][0]["position"], "MIDDLE");
        assert_eq!(payload["participants"][0]["items"][0]["slot"], 0);
        assert!(payload["game"].get("raw").is_none());
        assert!(payload["participants"][0].get("raw").is_none());
        assert!(payload["participants"][0]["items"][0].get("raw").is_none());
        assert!(payload["participants"][0].get("futureSecret").is_none());
    }

    #[test]
    fn live_game_payload_includes_kill_and_objective_events_without_raw_event() {
        let payload = live_game_payload(
            &json!({
                "gameData": {"gameId": 42, "gameTime": 120.5},
                "allPlayers": [
                    {"summonerName": "Me", "team": "ORDER"},
                    {"summonerName":"P2"},{"summonerName":"P3"},{"summonerName":"P4"},{"summonerName":"P5"},
                    {"summonerName":"P6"},{"summonerName":"P7"},{"summonerName":"P8"},{"summonerName":"P9"},{"summonerName":"P10"}
                ]
            }),
            Some(&json!({"Events": [
                {"EventID": 1, "EventName": "ChampionKill", "EventTime": 61.2, "KillerName": "Me", "VictimName": "Enemy", "MultiKill": 2, "Assisters": ["Ally"], "futureSecret": "do-not-forward"},
                {"EventID": 2, "EventName": "DragonKill", "EventTime": 90.0, "DragonType": "EarthDragon"}
            ]})),
        )
        .unwrap();
        assert_eq!(payload["events"][0]["killer_name"], "Me");
        assert_eq!(payload["events"][0]["victim_name"], "Enemy");
        assert_eq!(payload["events"][0]["multi_kill"], 2);
        assert!(payload["events"][0].get("raw").is_none());
        assert!(payload["events"][0].get("futureSecret").is_none());
        assert_eq!(payload["events"][1]["dragon_type"], "EarthDragon");
    }

    #[test]
    fn phase_poll_plan_queries_only_relevant_endpoints() {
        assert_eq!(phase_poll_plan("None"), PhasePollPlan::default());
        assert_eq!(
            phase_poll_plan("Lobby"),
            PhasePollPlan {
                lobby: true,
                ..PhasePollPlan::default()
            }
        );
        assert_eq!(
            phase_poll_plan("Matchmaking"),
            PhasePollPlan {
                ready_check: true,
                lobby: true,
                ..PhasePollPlan::default()
            }
        );
        assert_eq!(
            phase_poll_plan("ReadyCheck"),
            PhasePollPlan {
                ready_check: true,
                champ_select: true,
                lobby: true,
            }
        );
        assert_eq!(
            phase_poll_plan("ChampSelect"),
            PhasePollPlan {
                champ_select: true,
                ..PhasePollPlan::default()
            }
        );
        assert_eq!(phase_poll_plan("InProgress"), PhasePollPlan::default());
    }

    #[test]
    fn inactive_phase_snapshots_are_cleared_once() {
        let mut poller = LcuEventPoller {
            ready_check: Some("ready".into()),
            champ_select: Some("champ".into()),
            party: Some("party".into()),
            participant: Some("participant".into()),
            ..LcuEventPoller::default()
        };
        let mut events = Vec::new();

        clear_inactive_phase_snapshots(
            &mut poller,
            "InProgress",
            PhasePollPlan::default(),
            &mut events,
        );

        assert!(poller.ready_check.is_none());
        assert!(poller.champ_select.is_none());
        assert!(poller.party.is_none());
        assert_eq!(
            events
                .iter()
                .filter(|(kind, _)| *kind == "ready_check_update")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|(kind, _)| *kind == "champ_select_update")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|(kind, _)| *kind == "party_lobby_update")
                .count(),
            1
        );
        assert!(events.iter().any(|(kind, payload)| {
            *kind == "participant_status_update" && payload["status"] == "in_game"
        }));
    }

    #[test]
    fn spectator_live_game_payload_does_not_require_active_player() {
        let payload = live_game_payload(
            &json!({
                "gameData": {"gameId": 84, "gameMode": "CLASSIC", "gameTime": 300.0},
                "allPlayers": [
                    {"summonerName": "Blue", "championName": "Ahri", "team": "ORDER"},
                    {"summonerName": "Red", "championName": "Garen", "team": "CHAOS"},
                    {"summonerName":"P3"},{"summonerName":"P4"},{"summonerName":"P5"},{"summonerName":"P6"},
                    {"summonerName":"P7"},{"summonerName":"P8"},{"summonerName":"P9"},{"summonerName":"P10"}
                ]
            }),
            None,
        )
        .unwrap();

        assert_eq!(payload["client_mode"], "spectator");
        assert_eq!(payload["game"]["id"], 84);
        assert!(payload["active_player"].is_null());
        assert_eq!(payload["participants"].as_array().unwrap().len(), 10);
    }

    #[test]
    fn live_game_payload_rejects_partial_participant_lists() {
        assert!(live_game_payload(
            &json!({
                "gameData": {"gameId": 42, "gameTime": 1.0},
                "allPlayers": [{"summonerName": "OnlyOne"}]
            }),
            None,
        )
        .is_none());
    }

    #[test]
    fn live_game_fingerprint_ignores_only_advancing_clock_fields() {
        let first = json!({
            "captured_at_ms": 1_000,
            "match_created_at_ms": 500,
            "game": {"id": 42, "time_seconds": 500.1, "game_time": 500.1},
            "participants": [{"kills": 1, "deaths": 0, "assists": 2, "gold": 4200}],
            "events": [{"id": 7, "name": "ChampionKill", "time_seconds": 499.9}]
        });
        let second = json!({
            "captured_at_ms": 2_000,
            "match_created_at_ms": 501,
            "game": {"id": 42, "time_seconds": 501.1, "game_time": 501.1},
            "participants": [{"kills": 1, "deaths": 0, "assists": 2, "gold": 4200}],
            "events": [{"id": 7, "name": "ChampionKill", "time_seconds": 499.9}]
        });
        assert_eq!(
            live_game_fingerprint(&first, &mut HashMap::new()),
            live_game_fingerprint(&second, &mut HashMap::new())
        );
    }

    #[test]
    fn live_game_fingerprint_changes_for_meaningful_state_updates() {
        let baseline = json!({
            "captured_at_ms": 1_000,
            "game": {"id": 42, "time_seconds": 500.1},
            "participants": [{"kills": 1, "deaths": 0, "assists": 2, "gold": 4200}],
            "events": []
        });
        let kda_changed = json!({
            "captured_at_ms": 2_000,
            "game": {"id": 42, "time_seconds": 501.1},
            "participants": [{"kills": 2, "deaths": 0, "assists": 2, "gold": 4200}],
            "events": []
        });
        let event_changed = json!({
            "captured_at_ms": 2_000,
            "game": {"id": 42, "time_seconds": 501.1},
            "participants": [{"kills": 1, "deaths": 0, "assists": 2, "gold": 4200}],
            "events": [{"id": 8, "name": "ChampionKill", "time_seconds": 500.8}]
        });
        assert_ne!(
            live_game_fingerprint(&baseline, &mut HashMap::new()),
            live_game_fingerprint(&kda_changed, &mut HashMap::new())
        );
        assert_ne!(
            live_game_fingerprint(&baseline, &mut HashMap::new()),
            live_game_fingerprint(&event_changed, &mut HashMap::new())
        );
    }
    #[test]
    fn live_game_fingerprint_ignores_active_player_changes() {
        let mut samples = HashMap::new();
        let first = json!({
            "game": {"id": 42},
            "active_player": {"current_gold": 100.0, "level": 5},
            "participants": []
        });
        let second = json!({
            "game": {"id": 42},
            "active_player": {"current_gold": 123.4, "level": 6},
            "participants": []
        });
        assert_eq!(
            live_game_fingerprint(&first, &mut samples),
            live_game_fingerprint(&second, &mut samples)
        );
    }

    #[test]
    fn live_game_fingerprint_ignores_ward_score_changes() {
        let first = json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "ward_score": 1.25,
                "creep_score": 40
            }]
        });
        let second = json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "ward_score": 99.75,
                "creep_score": 40
            }]
        });

        assert_eq!(
            live_game_fingerprint(&first, &mut HashMap::new()),
            live_game_fingerprint(&second, &mut HashMap::new())
        );
    }

    #[test]
    fn live_game_fingerprint_ignores_creep_score_changes() {
        let first = json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "creep_score": 40,
                "kills": 1
            }]
        });
        let second = json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "creep_score": 130,
                "kills": 1
            }]
        });

        assert_eq!(
            live_game_fingerprint(&first, &mut HashMap::new()),
            live_game_fingerprint(&second, &mut HashMap::new())
        );
    }

    #[test]
    fn live_game_fingerprint_normalizes_item_slot_order_and_metadata() {
        let first = json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "items": [
                    {"id": 1056, "item_id": 1056, "count": 1, "slot": 0, "name": "A", "price": 400, "can_use": false},
                    {"id": 2003, "item_id": 2003, "count": 2, "slot": 1, "name": "Potion", "price": 50, "can_use": true}
                ]
            }]
        });
        let second = json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "items": [
                    {"id": 2003, "item_id": 2003, "count": 2, "slot": 5, "name": "Changed", "price": 999, "can_use": false},
                    {"id": 1056, "item_id": 1056, "count": 1, "slot": 3, "name": "Changed", "price": 0, "can_use": true}
                ]
            }]
        });

        assert_eq!(
            live_game_fingerprint(&first, &mut HashMap::new()),
            live_game_fingerprint(&second, &mut HashMap::new())
        );
    }

    #[test]
    fn live_game_fingerprint_detects_item_id_or_count_changes() {
        let baseline = json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "items": [{"id": 1056, "item_id": 1056, "count": 1, "slot": 0}]
            }]
        });
        let id_changed = json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "items": [{"id": 1052, "item_id": 1052, "count": 1, "slot": 0}]
            }]
        });
        let count_changed = json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "items": [{"id": 1056, "item_id": 1056, "count": 2, "slot": 0}]
            }]
        });

        let baseline_fp = live_game_fingerprint(&baseline, &mut HashMap::new());
        assert_ne!(
            baseline_fp,
            live_game_fingerprint(&id_changed, &mut HashMap::new())
        );
        assert_ne!(
            baseline_fp,
            live_game_fingerprint(&count_changed, &mut HashMap::new())
        );
    }

    #[test]
    fn live_game_fingerprint_sends_two_respawn_samples_then_ignores_countdown() {
        let mut samples = HashMap::new();
        let dead = |timer| json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "is_dead": true,
                "deaths": 1,
                "respawn_timer": timer
            }],
            "events": []
        });

        let first = live_game_fingerprint(&dead(30.5), &mut samples);
        let second = live_game_fingerprint(&dead(29.5), &mut samples);
        let third = live_game_fingerprint(&dead(28.5), &mut samples);
        let fourth = live_game_fingerprint(&dead(27.5), &mut samples);

        assert_ne!(first, second);
        assert_eq!(second, third);
        assert_eq!(third, fourth);
    }

    #[test]
    fn live_game_fingerprint_revive_resets_respawn_sampling() {
        let mut samples = HashMap::new();
        let dead = |timer, deaths| json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "is_dead": true,
                "deaths": deaths,
                "respawn_timer": timer
            }]
        });
        let alive = json!({
            "game": {"id": 42},
            "participants": [{
                "riot_id": "Player#KR1",
                "is_dead": false,
                "deaths": 1,
                "respawn_timer": 0
            }]
        });

        let _ = live_game_fingerprint(&dead(30.0, 1), &mut samples);
        let second_dead = live_game_fingerprint(&dead(29.0, 1), &mut samples);
        let revived = live_game_fingerprint(&alive, &mut samples);
        assert_ne!(second_dead, revived);

        let next_death_first = live_game_fingerprint(&dead(35.0, 2), &mut samples);
        let next_death_second = live_game_fingerprint(&dead(34.0, 2), &mut samples);
        assert_ne!(next_death_first, next_death_second);
    }

}
