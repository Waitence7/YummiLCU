use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::{
    sync::{mpsc, watch, Mutex},
    task::JoinHandle,
    time::{interval, sleep, timeout, Instant, MissedTickBehavior},
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

use super::protocol::{
    Action, AgentEventMessage, AgentHelloMessage, AuthMessage, CommandResult, IncomingMessage,
    OAuthCodeMessage, PongMessage, MAX_RELAY_MESSAGE_BYTES,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
const SESSION_RESTORE_TIMEOUT: Duration = Duration::from_secs(5);
const LCU_EVENT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

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
        let task = tokio::spawn(async move {
            run_supervisor(task_app.clone(), task_state.clone(), generation, stop_rx).await;
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
) {
    let config = state.config.read().await.clone();
    let saved_session = session::load(&config);
    let mut needs_login = saved_session.is_none();
    let session = saved_session.unwrap_or_else(|| session::create(&config));
    let mut attempt = 0_u32;

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
            &session,
            &mut stop_rx,
            &mut needs_login,
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
    session: &session::Session,
    stop_rx: &mut watch::Receiver<bool>,
    needs_login: &mut bool,
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

    websocket
        .send(Message::Text(
            serde_json::to_string(&AuthMessage::new(&session.ws_token))?.into(),
        ))
        .await
        .map_err(|_| AgentError::Relay("Relay 인증 메시지 전송 실패".into()))?;
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
    let mut awaiting_pong: Option<Instant> = None;
    let mut lcu_events = LcuEventPoller::default();
    let mut lcu_event_poll = interval(LCU_EVENT_POLL_INTERVAL);
    lcu_event_poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    lcu_event_poll.tick().await;
    let mut session_bound = false;
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
                    let message = serde_json::to_string(&OAuthCodeMessage::new(&code))?;
                    let sent = websocket.send(Message::Text(message.into())).await;
                    // The code is ASCII, so zeroing its bytes keeps the String valid before drop.
                    unsafe { code.as_bytes_mut().fill(0) };
                    sent.map_err(|_| AgentError::Relay("OAuth 코드 전송 실패".into()))?;
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
                        match incoming {
                            IncomingMessage::Pong => awaiting_pong = None,
                            IncomingMessage::SessionBound { .. } => {
                                *needs_login = false;
                                handle_incoming(app, state, &mut websocket, incoming, false).await?;
                                session_bound = true;
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
            _ = heartbeat.tick() => {
                if awaiting_pong.is_none() {
                    websocket.send(Message::Text("ping".into())).await
                        .map_err(|_| AgentError::Relay("Relay heartbeat 전송 실패".into()))?;
                    awaiting_pong = Some(Instant::now());
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
                    let Some(message) = serialize_agent_event(message_type, data)? else {
                        state.log(app, "LCU 이벤트가 Relay 크기 제한을 초과해 생략됨").await;
                        continue;
                    };
                    websocket.send(Message::Text(message.into())).await
                        .map_err(|_| AgentError::Relay("Relay 이벤트 전송 실패".into()))?;
                }
            }
            Some(()) = lcu_event_rx.recv(), if session_bound => {
                let config = state.config.read().await.clone();
                for (message_type, data) in lcu_events.poll(&config).await {
                    let Some(message) = serialize_agent_event(message_type, data)? else {
                        state.log(app, "LCU 이벤트가 Relay 크기 제한을 초과해 생략됨").await;
                        continue;
                    };
                    websocket.send(Message::Text(message.into())).await
                        .map_err(|_| AgentError::Relay("Relay 이벤트 전송 실패".into()))?;
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
            let action_label = Action::parse(&action)
                .map(Action::as_str)
                .unwrap_or("unknown");
            let result = match Action::parse(&action) {
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
            websocket
                .send(Message::Text(serde_json::to_string(&result)?.into()))
                .await
                .map_err(|_| AgentError::Relay("Relay 응답 전송 실패".into()))?;
            state.log(app, format!("명령 실행: {action_label}")).await;
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
        IncomingMessage::Pong | IncomingMessage::Unknown => {}
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
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(value)
}

fn serialize_agent_event(message_type: &'static str, data: Value) -> AgentResult<Option<String>> {
    let message = serde_json::to_string(&AgentEventMessage::new(message_type, data))?;
    Ok((message.len() <= MAX_RELAY_MESSAGE_BYTES).then_some(message))
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
    fn relay_avatar_is_limited_to_https_urls() {
        assert!(safe_avatar_url(Some("file:///C:/secret.png".into())).is_none());
        assert!(safe_avatar_url(Some("http://cdn.example/avatar.png".into())).is_none());
        assert_eq!(
            safe_avatar_url(Some("https://cdn.example/avatar.png".into())).as_deref(),
            Some("https://cdn.example/avatar.png")
        );
    }

    #[test]
    fn oversized_lcu_event_is_not_forwarded_to_relay() {
        let data = json!({"value": "x".repeat(MAX_RELAY_MESSAGE_BYTES)});
        assert!(serialize_agent_event("gameflow_update", data)
            .unwrap()
            .is_none());
    }
}
