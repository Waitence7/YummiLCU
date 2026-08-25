use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::{
    sync::{broadcast, mpsc, watch, Mutex},
    task::JoinHandle,
    time::{interval, interval_at, sleep, timeout, Instant, MissedTickBehavior},
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
};

use crate::{
    error::{AgentError, AgentResult},
    lcu::{lockfile_path, LcuClient, LcuEventPoller},
    platform::{launch_league_client, open_login_url},
    session,
    state::{AgentEvent, AppState},
};

use super::{
    command_auth,
    protocol::{
        Action, AgentEventMessage, AgentHelloMessage, AuthMessage, CommandResult, IncomingMessage,
        OAuthCodeMessage, PongMessage, UnexpectedErrorReport, MAX_RELAY_MESSAGE_BYTES,
    },
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
const SESSION_RESTORE_TIMEOUT: Duration = Duration::from_secs(5);
const LCU_EVENT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const MAX_DURABLE_REPLAY_EVENTS: usize = 64;
const DURABLE_REPLAY_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct SerializedAgentEvent {
    event_id: String,
    text: String,
}

#[derive(Default)]
struct DurableReplayBuffer {
    pending: VecDeque<SerializedAgentEvent>,
}

impl DurableReplayBuffer {
    fn track(&mut self, event: SerializedAgentEvent) -> bool {
        if self
            .pending
            .iter()
            .any(|pending| pending.event_id == event.event_id)
        {
            return false;
        }
        let dropped = if self.pending.len() >= MAX_DURABLE_REPLAY_EVENTS {
            self.pending.pop_front();
            true
        } else {
            false
        };
        self.pending.push_back(event);
        dropped
    }

    fn ack(&mut self, event_id: &str) -> bool {
        let before = self.pending.len();
        self.pending.retain(|event| event.event_id != event_id);
        self.pending.len() != before
    }

    fn snapshot(&self) -> Vec<SerializedAgentEvent> {
        self.pending.iter().cloned().collect()
    }
}

struct LcuSocketWatch {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Drop for LcuSocketWatch {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.task.abort();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RelayConnectionState {
    Stopped,
    Connecting,
    Authenticating,
    Connected,
    Reconnecting,
    Failed,
}

struct SupervisorControl {
    generation: u64,
    stop_tx: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<()>>,
    oauth_tx: Option<mpsc::Sender<String>>,
    connection_state: RelayConnectionState,
}

pub(crate) struct RelaySupervisor {
    control: Mutex<SupervisorControl>,
}

impl RelaySupervisor {
    pub(crate) fn new() -> Self {
        Self {
            control: Mutex::new(SupervisorControl {
                generation: 0,
                stop_tx: None,
                task: None,
                oauth_tx: None,
                connection_state: RelayConnectionState::Stopped,
            }),
        }
    }

    pub(crate) async fn is_running(&self) -> bool {
        let control = self.control.lock().await;
        control
            .task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    }

    pub(crate) async fn start(app: AppHandle, state: Arc<AppState>) -> AgentResult<()> {
        let (generation, stop_rx) = {
            let mut control = state.relay.control.lock().await;
            if control
                .task
                .as_ref()
                .is_some_and(|task| !task.is_finished())
            {
                return Ok(());
            }
            control.task.take();
            control.generation = control.generation.wrapping_add(1);
            let generation = control.generation;
            let (stop_tx, stop_rx) = watch::channel(false);
            control.stop_tx = Some(stop_tx);
            control.oauth_tx = None;
            control.connection_state = RelayConnectionState::Connecting;
            (generation, stop_rx)
        };

        state
            .publish(
                &app,
                AgentEvent::RelayStateChanged(RelayConnectionState::Connecting),
            )
            .await;
        state.log(&app, "Relay 연결 시작").await;

        let task_state = state.clone();
        let task_app = app.clone();
        let error_reports = state.unexpected_error_receiver();
        let task = tokio::spawn(async move {
            run_supervisor(
                task_app.clone(),
                task_state.clone(),
                generation,
                stop_rx,
                error_reports,
            )
            .await;
            task_state
                .relay
                .finish(&task_app, &task_state, generation)
                .await;
        });

        let mut control = state.relay.control.lock().await;
        if control.generation == generation {
            control.task = Some(task);
        } else {
            task.abort();
        }
        Ok(())
    }

    pub(crate) async fn stop(app: &AppHandle, state: &Arc<AppState>) {
        let task = {
            let mut control = state.relay.control.lock().await;
            control.generation = control.generation.wrapping_add(1);
            if let Some(stop_tx) = control.stop_tx.take() {
                let _ = stop_tx.send(true);
            }
            control.oauth_tx = None;
            control.connection_state = RelayConnectionState::Stopped;
            control.task.take()
        };

        if let Some(mut task) = task {
            if timeout(Duration::from_secs(2), &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
        }
        state
            .publish(
                app,
                AgentEvent::RelayStateChanged(RelayConnectionState::Stopped),
            )
            .await;
        state.mark_stopped(app).await;
    }

    pub(crate) async fn restart(app: AppHandle, state: Arc<AppState>) -> AgentResult<()> {
        Self::stop(&app, &state).await;
        Self::start(app, state).await
    }

    pub(crate) async fn submit_oauth_code(&self, mut code: String) -> AgentResult<()> {
        let sender = self.control.lock().await.oauth_tx.clone();
        let Some(sender) = sender else {
            unsafe { code.as_bytes_mut().fill(0) };
            return Err(AgentError::Relay("먼저 연결을 시작하세요.".into()));
        };
        match sender.send(code).await {
            Ok(()) => Ok(()),
            Err(mut error) => {
                unsafe { error.0.as_bytes_mut().fill(0) };
                Err(AgentError::Relay("Relay 연결이 없습니다.".into()))
            }
        }
    }

    async fn set_connection_state(
        &self,
        app: &AppHandle,
        state: &AppState,
        generation: u64,
        next: RelayConnectionState,
    ) {
        let changed = {
            let mut control = self.control.lock().await;
            if control.generation != generation || control.connection_state == next {
                false
            } else {
                control.connection_state = next;
                true
            }
        };
        if changed {
            state
                .publish(app, AgentEvent::RelayStateChanged(next))
                .await;
            state
                .log(app, format!("Relay 상태 변경: {}", relay_state_label(next)))
                .await;
        }
    }

    async fn set_oauth_sender(&self, generation: u64, sender: Option<mpsc::Sender<String>>) {
        let mut control = self.control.lock().await;
        if control.generation == generation {
            control.oauth_tx = sender;
        }
    }

    async fn finish(&self, app: &AppHandle, state: &AppState, generation: u64) {
        let should_publish = {
            let mut control = self.control.lock().await;
            if control.generation != generation {
                false
            } else {
                control.stop_tx = None;
                control.oauth_tx = None;
                control.connection_state = RelayConnectionState::Stopped;
                true
            }
        };
        if should_publish {
            state
                .publish(
                    app,
                    AgentEvent::RelayStateChanged(RelayConnectionState::Stopped),
                )
                .await;
        }
    }
}

async fn run_supervisor(
    app: AppHandle,
    state: Arc<AppState>,
    generation: u64,
    mut stop_rx: watch::Receiver<bool>,
    mut error_reports: broadcast::Receiver<UnexpectedErrorReport>,
) {
    let config = state.config.read().await.clone();
    let saved_session = session::load(&config);
    let mut needs_login = saved_session.is_none();
    let mut session = saved_session.unwrap_or_else(|| session::create(&config));
    let mut attempt = 0_u32;
    let mut durable_replay = DurableReplayBuffer::default();

    loop {
        if *stop_rx.borrow() {
            break;
        }
        state
            .relay
            .set_connection_state(&app, &state, generation, RelayConnectionState::Connecting)
            .await;

        match connect_once(
            &app,
            &state,
            generation,
            &config,
            &mut session,
            &mut stop_rx,
            &mut needs_login,
            &mut durable_replay,
            &mut error_reports,
        )
        .await
        {
            Ok(ConnectionEnd::Stopped) => break,
            Ok(ConnectionEnd::Closed { authenticated }) => {
                if authenticated {
                    attempt = 0;
                }
                state.log(&app, "Relay 연결 종료 — 재연결 대기").await;
            }
            Err(error) => {
                state
                    .relay
                    .set_connection_state(&app, &state, generation, RelayConnectionState::Failed)
                    .await;
                state.log(&app, format!("Relay 오류: {error}")).await;
            }
        }
        state.relay.set_oauth_sender(generation, None).await;

        if !should_reconnect(*stop_rx.borrow()) {
            break;
        }
        state
            .relay
            .set_connection_state(&app, &state, generation, RelayConnectionState::Reconnecting)
            .await;
        attempt = attempt.saturating_add(1);
        if !wait_for_retry(&mut stop_rx, backoff_delay(attempt)).await {
            break;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConnectionEnd {
    Stopped,
    Closed { authenticated: bool },
}

async fn connect_once(
    app: &AppHandle,
    state: &Arc<AppState>,
    generation: u64,
    config: &crate::config::Config,
    session: &mut session::Session,
    stop_rx: &mut watch::Receiver<bool>,
    needs_login: &mut bool,
    durable_replay: &mut DurableReplayBuffer,
    error_reports: &mut broadcast::Receiver<UnexpectedErrorReport>,
) -> AgentResult<ConnectionEnd> {
    let url = config.ws_url(&session.session_id)?;
    let connection = tokio::select! {
        changed = stop_rx.changed() => {
            let _ = changed;
            return Ok(ConnectionEnd::Stopped);
        }
        result = timeout(
            CONNECT_TIMEOUT,
            connect_async_with_config(
                url,
                Some(
                    WebSocketConfig::default()
                        .max_message_size(Some(MAX_RELAY_MESSAGE_BYTES))
                        .max_frame_size(Some(MAX_RELAY_MESSAGE_BYTES)),
                ),
                false,
            ),
        ) => result,
    };
    let (mut websocket, _) = connection
        .map_err(|_| AgentError::Relay("Relay 연결 시간 초과".into()))?
        .map_err(|_| AgentError::Relay("Relay TLS/WebSocket 연결 실패".into()))?;
    state.log(app, "Relay WebSocket 연결 성공").await;

    websocket
        .send(Message::Text(
            serde_json::to_string(&AuthMessage::new(&session.ws_token))?.into(),
        ))
        .await
        .map_err(|_| AgentError::Relay("Relay 인증 메시지 전송 실패".into()))?;
    state.log(app, "Relay 인증 메시지 전송 완료").await;
    if *stop_rx.borrow() {
        let _ = websocket.close(None).await;
        return Ok(ConnectionEnd::Stopped);
    }
    let hello = AgentHelloMessage::new(state.lcu_state().await.is_ready());
    websocket
        .send(Message::Text(serde_json::to_string(&hello)?.into()))
        .await
        .map_err(|_| AgentError::Relay("Relay hello 전송 실패".into()))?;
    state
        .log(
            app,
            format!(
                "Agent hello 전송: lcu_ready={}",
                state.lcu_state().await.is_ready()
            ),
        )
        .await;
    state
        .relay
        .set_connection_state(app, state, generation, RelayConnectionState::Authenticating)
        .await;

    let (oauth_tx, mut oauth_rx) = mpsc::channel::<String>(1);
    state
        .relay
        .set_oauth_sender(generation, Some(oauth_tx))
        .await;
    state
        .set_oauth_pending(
            app,
            *needs_login,
            if *needs_login {
                "브라우저에서 Discord 로그인 중…"
            } else {
                "저장된 Discord 세션 복원 중…"
            },
        )
        .await;
    if *needs_login {
        open_login_url(app, &config.login_url(&session.session_id)?)?;
    }
    let auth_started = Instant::now();
    let mut login_opened = *needs_login;
    let _ = session::save(session);

    let mut heartbeat = interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    heartbeat.tick().await;
    let mut watchdog = interval(Duration::from_secs(1));
    watchdog.set_missed_tick_behavior(MissedTickBehavior::Skip);
    watchdog.tick().await;
    let mut durable_replay_tick = interval_at(
        Instant::now() + DURABLE_REPLAY_INTERVAL,
        DURABLE_REPLAY_INTERVAL,
    );
    durable_replay_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut awaiting_pong: Option<Instant> = None;
    let mut lcu_events = LcuEventPoller::default();
    let mut live_game_announced = false;
    let mut lcu_event_poll = interval(LCU_EVENT_POLL_INTERVAL);
    lcu_event_poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    lcu_event_poll.tick().await;
    let mut session_bound = false;
    let mut durable_replay_enabled = false;
    let mut unexpected_error_reports_enabled = false;
    let (lcu_event_tx, mut lcu_event_rx) = mpsc::channel(8);
    let mut lcu_socket_watch: Option<LcuSocketWatch> = None;

    loop {
        tokio::select! {
            changed = stop_rx.changed() => {
                let _ = changed;
                let _ = websocket.close(None).await;
                state.relay.set_oauth_sender(generation, None).await;
                return Ok(ConnectionEnd::Stopped);
            }
            code = oauth_rx.recv() => {
                if let Some(mut code) = code {
                    state.log(app, "Discord OAuth 코드 전송 시작").await;
                    let message = serde_json::to_string(&OAuthCodeMessage::new(&code))?;
                    let sent = websocket.send(Message::Text(message.into())).await;
                    // The code is ASCII, so zeroing its bytes keeps the String valid before drop.
                    unsafe { code.as_bytes_mut().fill(0) };
                    sent.map_err(|_| AgentError::Relay("OAuth 코드 전송 실패".into()))?;
                    state.log(app, "Discord OAuth 코드 전송 완료").await;
                }
            }
            message = websocket.next() => {
                match message {
                    Some(Ok(Message::Text(text))) if text.as_str() == "ping" => {
                        websocket.send(Message::Text(
                            serde_json::to_string(&PongMessage::new())?.into()
                        )).await.map_err(|_| AgentError::Relay("Relay pong 전송 실패".into()))?;
                    }
                    Some(Ok(Message::Text(text))) => {
                        let incoming = IncomingMessage::parse(text.as_str())
                            .map_err(|message| AgentError::Relay(message.into()))?;
                        match &incoming {
                            IncomingMessage::Pong => awaiting_pong = None,
                            IncomingMessage::EventAck { event_id } => {
                                if durable_replay.ack(event_id) {
                                    state.record_flight("relay_ack", format!("event_id={event_id}")).await;
                                }
                            }
                            IncomingMessage::ServerHello { protocol_version, capabilities } => {
                                durable_replay_enabled = *protocol_version >= 1
                                    && capabilities.get("event_ack") == Some(&true)
                                    && capabilities.get("durable_event_replay") == Some(&true);
                                unexpected_error_reports_enabled = *protocol_version >= 1
                                    && capabilities.get("unexpected_error_reports") == Some(&true);
                                state
                                    .record_flight(
                                        "protocol",
                                        format!(
                                            "server_protocol={} durable_replay={} error_reports={}",
                                            protocol_version,
                                            durable_replay_enabled,
                                            unexpected_error_reports_enabled,
                                        ),
                                    )
                                    .await;
                                if durable_replay_enabled && session_bound {
                                    let replayed = replay_durable_events(&mut websocket, durable_replay).await?;
                                    if replayed > 0 {
                                        state.log(app, format!("미확인 EOG 이벤트 {replayed}건 재전송")).await;
                                    }
                                }
                            }
                            IncomingMessage::SessionBound { discord_id, .. } => {
                                let Some(discord_id) = *discord_id else {
                                    return Err(AgentError::Relay(
                                        "Relay session_bound에 Discord ID가 없습니다.".into(),
                                    ));
                                };
                                if let Err(error) = session::pin_discord_id(session, discord_id) {
                                    let _ = session::remove();
                                    *needs_login = true;
                                    state
                                        .set_oauth_pending(
                                            app,
                                            true,
                                            "저장된 Discord 계정과 다른 계정이 감지되어 연결을 차단했습니다. 다시 로그인하세요.",
                                        )
                                        .await;
                                    state
                                        .record_flight("security", "discord_binding_mismatch")
                                        .await;
                                    state.log(app, format!("Discord 바인딩 pin 차단: {error}")).await;
                                    return Ok(ConnectionEnd::Stopped);
                                }
                                *needs_login = false;
                                handle_incoming(app, state, &mut websocket, incoming, false, &mut lcu_events).await?;
                                session_bound = true;
                                if durable_replay_enabled {
                                    let replayed = replay_durable_events(&mut websocket, durable_replay).await?;
                                    if replayed > 0 {
                                        state.log(app, format!("미확인 EOG 이벤트 {replayed}건 재전송")).await;
                                    }
                                }
                                state.log(app, "Relay 세션 인증 완료 — LCU 이벤트 감시 시작").await;
                                state.relay.set_oauth_sender(generation, None).await;
                                if lcu_socket_watch.is_none() {
                                    let (stop_tx, stop_rx) = watch::channel(false);
                                    let event_config = config.clone();
                                    let event_tx = lcu_event_tx.clone();
                                    let task = tokio::spawn(async move {
                                        LcuEventPoller::watch_socket(event_config, event_tx, stop_rx).await;
                                    });
                                    lcu_socket_watch = Some(LcuSocketWatch { stop: stop_tx, task });
                                }
                                state.relay.set_connection_state(
                                    app,
                                    state,
                                    generation,
                                    RelayConnectionState::Connected,
                                ).await;
                            }
                            _ => handle_incoming(
                                app,
                                state,
                                &mut websocket,
                                incoming,
                                session_bound,
                                &mut lcu_events,
                            ).await?,
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        websocket.send(Message::Pong(payload)).await
                            .map_err(|_| AgentError::Relay("Relay pong 전송 실패".into()))?;
                    }
                    Some(Ok(Message::Pong(_))) => awaiting_pong = None,
                    Some(Ok(Message::Close(_))) | None => {
                        state.relay.set_oauth_sender(generation, None).await;
                        return Ok(ConnectionEnd::Closed { authenticated: session_bound });
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => {
                        state.relay.set_oauth_sender(generation, None).await;
                        return Err(AgentError::Relay("Relay 메시지 수신 실패".into()));
                    }
                }
            }
            _ = durable_replay_tick.tick(), if session_bound && durable_replay_enabled => {
                let replayed = replay_durable_events(&mut websocket, durable_replay).await?;
                if replayed > 0 {
                    state.log(app, format!("ACK 대기 EOG 이벤트 {replayed}건 재전송")).await;
                }
            }
            report = error_reports.recv(), if session_bound && unexpected_error_reports_enabled => {
                match report {
                    Ok(report) => {
                        let message = serde_json::to_string(&report)?;
                        websocket.send(Message::Text(message.into())).await
                            .map_err(|_| AgentError::Relay("예상치 못한 오류 보고 전송 실패".into()))?;
                        state.record_flight("error_report", "unexpected_error_sent").await;
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        state.record_flight(
                            "error_report",
                            format!("queue_lagged skipped={skipped}"),
                        ).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err(AgentError::Relay("오류 보고 큐가 종료되었습니다.".into()));
                    }
                }
            }
            _ = heartbeat.tick() => {
                if awaiting_pong.is_none() {
                    websocket.send(Message::Text("ping".into())).await
                        .map_err(|_| AgentError::Relay("Relay heartbeat 전송 실패".into()))?;
                    awaiting_pong = Some(Instant::now());
                    state.log(app, "Relay heartbeat 전송").await;
                }
            }
            _ = watchdog.tick() => {
                if heartbeat_timed_out(awaiting_pong, Instant::now()) {
                    state.relay.set_oauth_sender(generation, None).await;
                    return Err(AgentError::Relay("Relay heartbeat 응답 시간 초과".into()));
                }
                if should_start_login(session_bound, login_opened, auth_started.elapsed()) {
                    *needs_login = true;
                    login_opened = true;
                    state.set_oauth_pending(app, true, "저장된 세션이 만료되어 Discord 재인증이 필요합니다.").await;
                    open_login_url(app, &config.login_url(&session.session_id)?)?;
                }
            }
            _ = lcu_event_poll.tick(), if session_bound => {
                let config = state.config.read().await.clone();
                for (message_type, data) in lcu_events.poll(&config).await {
                    let event_log = event_summary(message_type, &data);
                    state.record_flight("lcu_event", event_log.clone()).await;
                    let live_participant_count = (message_type == "live_game_update" && !live_game_announced)
                        .then(|| data.get("participants").and_then(Value::as_array).map_or(0, Vec::len));
                    let Some(event) = serialize_agent_event(message_type, data)? else {
                        state.log(app, "LCU 이벤트가 Relay 크기 제한을 초과해 생략됨").await;
                        continue;
                    };
                    if durable_replay_enabled && is_durable_event(message_type) {
                        if durable_replay.track(event.clone()) {
                            state.log(app, "EOG replay buffer가 가득 차 가장 오래된 이벤트를 제거함").await;
                        }
                    }
                    websocket.send(Message::Text(event.text.into())).await
                        .map_err(|_| AgentError::Relay("Relay 이벤트 전송 실패".into()))?;
                    state.log(app, format!("Relay 이벤트 전송: {event_log}")).await;
                    if let Some(count) = live_participant_count {
                        live_game_announced = true;
                        state.log(app, format!("실시간 관전 데이터 서버 전송 확인 ({count}명)")).await;
                    }
                }
                for diagnostic in lcu_events.take_diagnostics() {
                    state.record_flight("lcu_diagnostic", diagnostic.clone()).await;
                    state.log(app, format!("LCU 진단: {diagnostic}")).await;
                }
            }
            Some(()) = lcu_event_rx.recv(), if session_bound => {
                let config = state.config.read().await.clone();
                for (message_type, data) in lcu_events.poll(&config).await {
                    let event_log = event_summary(message_type, &data);
                    state.record_flight("lcu_event", event_log.clone()).await;
                    let live_participant_count = (message_type == "live_game_update" && !live_game_announced)
                        .then(|| data.get("participants").and_then(Value::as_array).map_or(0, Vec::len));
                    let Some(event) = serialize_agent_event(message_type, data)? else {
                        state.log(app, "LCU 이벤트가 Relay 크기 제한을 초과해 생략됨").await;
                        continue;
                    };
                    if durable_replay_enabled && is_durable_event(message_type) {
                        if durable_replay.track(event.clone()) {
                            state.log(app, "EOG replay buffer가 가득 차 가장 오래된 이벤트를 제거함").await;
                        }
                    }
                    websocket.send(Message::Text(event.text.into())).await
                        .map_err(|_| AgentError::Relay("Relay 이벤트 전송 실패".into()))?;
                    state.log(app, format!("Relay 이벤트 전송: {event_log}")).await;
                    if let Some(count) = live_participant_count {
                        live_game_announced = true;
                        state.log(app, format!("실시간 관전 데이터 서버 전송 확인 ({count}명)")).await;
                    }
                }
                for diagnostic in lcu_events.take_diagnostics() {
                    state.record_flight("lcu_diagnostic", diagnostic.clone()).await;
                    state.log(app, format!("LCU 진단: {diagnostic}")).await;
                }
            }
        }
    }
}

async fn handle_incoming<S>(
    app: &AppHandle,
    state: &Arc<AppState>,
    websocket: &mut S,
    incoming: IncomingMessage,
    session_bound: bool,
    lcu_events: &mut LcuEventPoller,
) -> AgentResult<()>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    match incoming {
        IncomingMessage::Command {
            action,
            request_id,
            payload,
        } => {
            if !command_is_authorized(session_bound) {
                let result =
                    CommandResult::failure(request_id, "Relay 인증이 완료되지 않았습니다.");
                websocket
                    .send(Message::Text(serde_json::to_string(&result)?.into()))
                    .await
                    .map_err(|_| AgentError::Relay("Relay 응답 전송 실패".into()))?;
                state.log(app, "인증 전 Relay 명령 차단").await;
                return Ok(());
            }

            let payload = match command_auth::verify_command(
                &action,
                &payload,
                state.bound_discord_id().await,
            )
            .await
            {
                Ok(payload) => payload,
                Err(error) => {
                    let result = CommandResult::failure(request_id, "LCU 명령 인증 실패");
                    websocket
                        .send(Message::Text(serde_json::to_string(&result)?.into()))
                        .await
                        .map_err(|_| AgentError::Relay("Relay 응답 전송 실패".into()))?;
                    state
                        .log(app, format!("Relay LCU 명령 서명 차단: {error}"))
                        .await;
                    return Ok(());
                }
            };

            let parsed_action = Action::parse(&action);
            let action_label = parsed_action.map(Action::as_str).unwrap_or("unknown");
            let result = match parsed_action {
                None => CommandResult::failure(request_id, "unknown action"),
                Some(Action::LaunchClient) => {
                    let (ok, message) = launch_league_client();
                    CommandResult::from_parts(request_id, ok, message, json!({}))
                }
                Some(Action::Ping) => CommandResult::success(request_id, "pong", json!({})),
                Some(action) => {
                    let config = state.config.read().await.clone();
                    if let Some(path) = lockfile_path(&config) {
                        let _guard = state.command_lock.lock().await;
                        match LcuClient::from_lockfile(&path) {
                            Ok(client) => match client
                                .execute_action(action, &payload, &config)
                                .await
                            {
                                Ok(outcome) => CommandResult::from_parts(
                                    request_id,
                                    outcome.ok,
                                    outcome.message,
                                    outcome.data,
                                ),
                                Err(error) => CommandResult::failure(request_id, error.to_string()),
                            },
                            Err(error) => CommandResult::failure(request_id, error.to_string()),
                        }
                    } else {
                        CommandResult::failure(request_id, "LCU 미연결")
                    }
                }
            };
            state
                .record_flight(
                    "command",
                    format!(
                        "action={action_label} result={}",
                        if result.is_ok() { "ok" } else { "error" }
                    ),
                )
                .await;
            websocket
                .send(Message::Text(serde_json::to_string(&result)?.into()))
                .await
                .map_err(|_| AgentError::Relay("Relay 응답 전송 실패".into()))?;
            if !parsed_action.is_some_and(Action::is_background) {
                let result_label = if result.is_ok() { "성공" } else { "실패" };
                state
                    .log(app, format!("명령 완료: {action_label} ({result_label})"))
                    .await;
            }
        }
        IncomingMessage::SessionBound {
            discord_id,
            discord_name,
            username,
            discord_avatar,
            avatar_url,
        } => {
            state
                .apply_session_bound(
                    app,
                    discord_id,
                    discord_name.or(username),
                    safe_avatar_url(discord_avatar.or(avatar_url)),
                )
                .await;
        }
        IncomingMessage::LiveGamePolling { enabled } => {
            if session_bound {
                lcu_events.set_live_game_polling(enabled);
                state
                    .log(
                        app,
                        if enabled {
                            "Relay live game polling 활성화"
                        } else {
                            "Relay live game polling 비활성화 — 구독자 없음"
                        },
                    )
                    .await;
            }
        }
        IncomingMessage::Pong
        | IncomingMessage::ServerHello { .. }
        | IncomingMessage::EventAck { .. }
        | IncomingMessage::Unknown => {}
    }
    Ok(())
}

fn command_is_authorized(session_bound: bool) -> bool {
    session_bound
}

fn safe_avatar_url(value: Option<String>) -> Option<String> {
    let value = value?;
    let url = url::Url::parse(&value).ok()?;
    (url.scheme() == "https"
        && url.host_str() == Some("cdn.discordapp.com")
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(value)
}

fn serialize_agent_event(
    message_type: &'static str,
    data: Value,
) -> AgentResult<Option<SerializedAgentEvent>> {
    let message = AgentEventMessage::new(message_type, data);
    let event_id = message.event_id().to_owned();
    let text = serde_json::to_string(&message)?;
    Ok((text.len() <= MAX_RELAY_MESSAGE_BYTES).then_some(SerializedAgentEvent { event_id, text }))
}

fn is_durable_event(message_type: &str) -> bool {
    matches!(message_type, "match_eog" | "guild_match_eog")
}

async fn replay_durable_events<S>(
    websocket: &mut S,
    replay: &DurableReplayBuffer,
) -> AgentResult<usize>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let events = replay.snapshot();
    for event in &events {
        websocket
            .send(Message::Text(event.text.clone().into()))
            .await
            .map_err(|_| AgentError::Relay("Relay EOG 이벤트 재전송 실패".into()))?;
    }
    Ok(events.len())
}

fn relay_state_label(state: RelayConnectionState) -> &'static str {
    match state {
        RelayConnectionState::Stopped => "중지됨",
        RelayConnectionState::Connecting => "연결 중",
        RelayConnectionState::Authenticating => "인증 중",
        RelayConnectionState::Connected => "연결됨",
        RelayConnectionState::Reconnecting => "재연결 대기",
        RelayConnectionState::Failed => "실패",
    }
}

fn event_summary(message_type: &str, data: &Value) -> String {
    match message_type {
        "live_game_update" => format!(
            "live_game_update game_id={} participants={} events={} active_player={}",
            data.pointer("/game/id")
                .map(Value::to_string)
                .unwrap_or_else(|| "unknown".into()),
            data.get("participants")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            data.get("events")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            if data.get("active_player").is_some_and(|v| !v.is_null()) {
                "yes"
            } else {
                "no"
            }
        ),
        "gameflow_update" => format!(
            "gameflow_update phase={}",
            data.pointer("/phase")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        "match_eog" | "guild_match_eog" => format!(
            "{message_type} participants={} none_reason={}",
            data.get("participants")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            data.get("eog_none_reason")
                .and_then(Value::as_str)
                .unwrap_or("none")
        ),
        "ready_check_update" => format!(
            "ready_check_update active={}",
            data.get("active").and_then(Value::as_bool).unwrap_or(false)
        ),
        "champ_select_update" => format!(
            "champ_select_update active={} current_action={}",
            data.get("active").and_then(Value::as_bool).unwrap_or(false),
            data.get("current_action").is_some_and(|v| !v.is_null())
        ),
        "party_lobby_update" => format!(
            "party_lobby_update members={}",
            data.get("members")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        ),
        "participant_status_update" => format!(
            "participant_status_update status={} phase={}",
            data.get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
            data.get("phase")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        other => other.to_owned(),
    }
}

fn should_reconnect(manual_stop: bool) -> bool {
    !manual_stop
}

fn heartbeat_timed_out(awaiting_since: Option<Instant>, now: Instant) -> bool {
    awaiting_since.is_some_and(|started| now.duration_since(started) >= HEARTBEAT_TIMEOUT)
}

fn should_start_login(session_bound: bool, login_opened: bool, elapsed: Duration) -> bool {
    !session_bound && !login_opened && elapsed >= SESSION_RESTORE_TIMEOUT
}

fn backoff_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(5);
    let base = Duration::from_secs((1_u64 << exponent).min(MAX_BACKOFF.as_secs()));
    let jitter_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_millis()
        % 500;
    (base + Duration::from_millis(u64::from(jitter_ms))).min(MAX_BACKOFF)
}

async fn wait_for_retry(stop_rx: &mut watch::Receiver<bool>, delay: Duration) -> bool {
    tokio::select! {
        _ = sleep(delay) => true,
        changed = stop_rx.changed() => {
            changed.is_ok() && !*stop_rx.borrow()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_close_reconnects_unless_manually_stopped() {
        assert!(should_reconnect(false));
        assert!(!should_reconnect(true));
    }

    #[test]
    fn heartbeat_timeout_uses_last_unanswered_ping() {
        let now = Instant::now();
        assert!(!heartbeat_timed_out(None, now));
        assert!(!heartbeat_timed_out(
            Some(now - HEARTBEAT_TIMEOUT + Duration::from_millis(1)),
            now
        ));
        assert!(heartbeat_timed_out(Some(now - HEARTBEAT_TIMEOUT), now));
    }

    #[test]
    fn expired_saved_session_falls_back_to_login_once() {
        assert!(!should_start_login(false, false, Duration::from_secs(4)));
        assert!(should_start_login(false, false, SESSION_RESTORE_TIMEOUT));
        assert!(!should_start_login(false, true, Duration::from_secs(30)));
        assert!(!should_start_login(true, false, Duration::from_secs(30)));
    }

    #[test]
    fn reconnect_backoff_is_capped() {
        assert!(backoff_delay(1) <= Duration::from_millis(1_500));
        assert!(backoff_delay(20) <= MAX_BACKOFF);
    }

    #[test]
    fn commands_require_session_binding() {
        assert!(!command_is_authorized(false));
        assert!(command_is_authorized(true));
    }

    #[test]
    fn relay_avatar_is_limited_to_discord_cdn() {
        assert!(safe_avatar_url(Some("file:///C:/secret.png".into())).is_none());
        assert!(safe_avatar_url(Some("http://cdn.discordapp.com/avatar.png".into())).is_none());
        assert!(safe_avatar_url(Some("https://cdn.example/avatar.png".into())).is_none());
        assert_eq!(
            safe_avatar_url(Some("https://cdn.discordapp.com/avatar.png".into())).as_deref(),
            Some("https://cdn.discordapp.com/avatar.png")
        );
    }

    #[test]
    fn durable_replay_buffer_deduplicates_and_acks() {
        let mut replay = DurableReplayBuffer::default();
        let event = SerializedAgentEvent {
            event_id: "event-1".into(),
            text: "payload".into(),
        };
        assert!(!replay.track(event.clone()));
        assert!(!replay.track(event));
        assert_eq!(replay.snapshot().len(), 1);
        assert!(replay.ack("event-1"));
        assert!(replay.snapshot().is_empty());
    }

    #[test]
    fn oversized_lcu_event_is_not_forwarded_to_relay() {
        let data = json!({"value": "x".repeat(MAX_RELAY_MESSAGE_BYTES)});
        assert!(serialize_agent_event("gameflow_update", data)
            .unwrap()
            .is_none());
    }
}
