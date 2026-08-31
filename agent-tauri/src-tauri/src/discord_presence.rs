use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use discord_presence::{
    models::{
        rich_presence::{ActivityType, DisplayType},
        EventData,
    },
    Client, Event,
};
use serde_json::Value;
use tauri::AppHandle;
use tokio::{sync::mpsc, time::sleep};
use uuid::Uuid;

use crate::{
    config::Config,
    lcu::{lockfile_path, LcuClient},
    state::{AppState, DiscordJoinResolution, DiscordPresenceMatchContext},
};

const DEFAULT_DISCORD_APPLICATION_ID: &str = "1491092609001722106";
const PRESENCE_POLL_INTERVAL: Duration = Duration::from_secs(4);
const DISCORD_RETRY_INTERVAL: Duration = Duration::from_secs(15);
const DISCORD_STARTUP_RETRY_DELAY: Duration = Duration::from_millis(250);
const DISCORD_STARTUP_RETRY_ATTEMPTS: usize = 6;
const DISCORD_STARTING_ERROR: &str = "Discord IPC connection is starting";
const LIVE_GAME_ENDPOINT: &str = "/liveclientdata/allgamedata";
const DOWNLOAD_URL: &str = "https://yummi.duckdns.org/download";
const YUMMI_ICON_URL: &str = "https://raw.githubusercontent.com/Waitence7/YummiLCU/main/agent-tauri/src-tauri/icons/yummibot-desktop.png";
const CHAMPION_ICON_URL_PREFIX: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/champion-icons";
const JOIN_SECRET_PREFIX: &str = "yummi:lobby:v1:";
const REQUEST_ONLY_SECRET_PREFIX: &str = "yummi:request:v1:";
const JOIN_REQUEST_MAX_DELAY: Duration = Duration::from_secs(90 * 60);
const JOIN_REQUEST_LOOKUP_RETRY: Duration = Duration::from_secs(30);
const JOIN_INVITE_RETRY: Duration = Duration::from_secs(15);
const MATCH_CONTEXT_REFRESH_INTERVAL: Duration = Duration::from_secs(15);
const PHASE_REPUBLISH_DELAY: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, Eq, PartialEq)]
struct PresenceParty {
    id: String,
    size: Option<(u32, u32)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PresenceAssets {
    large_image: String,
    large_text: String,
    small_image: Option<String>,
    small_text: Option<String>,
}

#[derive(Clone, Debug)]
struct PendingJoinRequest {
    queued_at: Instant,
    riot_id: Option<String>,
    lookup_status: Option<String>,
    last_lookup_at: Option<Instant>,
    last_invite_at: Option<Instant>,
}

#[derive(Clone, Debug)]
struct PresenceSnapshot {
    phase_key: String,
    details: String,
    state: String,
    started_at_ms: Option<u64>,
    party: Option<PresenceParty>,
    join_secret: Option<String>,
    match_join_url: Option<String>,
    opgg_url: Option<String>,
    assets: Option<PresenceAssets>,
}

impl PresenceSnapshot {
    fn same_activity(&self, other: &Self) -> bool {
        self.phase_key == other.phase_key
            && self.details == other.details
            && self.state == other.state
            && self.party == other.party
            && self.join_secret == other.join_secret
            && self.match_join_url == other.match_join_url
            && self.opgg_url == other.opgg_url
            && self.started_at_ms == other.started_at_ms
            && self.assets == other.assets
    }
}

struct PresenceSession {
    client: Option<Client>,
    join_sender: mpsc::UnboundedSender<String>,
    join_request_sender: mpsc::UnboundedSender<u64>,
    subscribed_to_join: bool,
    subscribed_to_join_request: bool,
}

impl PresenceSession {
    fn new(
        join_sender: mpsc::UnboundedSender<String>,
        join_request_sender: mpsc::UnboundedSender<u64>,
    ) -> Self {
        Self {
            client: None,
            join_sender,
            join_request_sender,
            subscribed_to_join: false,
            subscribed_to_join_request: false,
        }
    }

    fn is_connected(&self) -> bool {
        self.client.is_some() && Client::is_ready()
    }

    fn set_activity(&mut self, snapshot: &PresenceSnapshot) -> Result<(), String> {
        self.ensure_connected()?;
        self.ensure_join_subscription()?;

        let result = self
            .client
            .as_mut()
            .expect("presence client must exist after ensure_connected")
            .set_activity(|activity| {
                let mut activity = activity
                    .activity_type(ActivityType::Playing)
                    .status_display(DisplayType::Details)
                    .details(snapshot.details.clone())
                    .state(snapshot.state.clone());

                if let Some(match_join_url) = snapshot.match_join_url.as_ref() {
                    activity = activity.append_buttons(|button| {
                        button.label("방 참가하기").url(match_join_url.clone())
                    });
                } else if let Some(opgg_url) = snapshot.opgg_url.as_ref() {
                    activity = activity.append_buttons(|button| {
                        button.label("OP.GG 전적").url(opgg_url.clone())
                    });
                }
                activity = activity
                    .append_buttons(|button| button.label("앱 다운로드").url(DOWNLOAD_URL));

                if let Some(assets) = snapshot.assets.as_ref() {
                    activity = activity.assets(|builder| {
                        let mut builder = builder
                            .large_image(assets.large_image.clone())
                            .large_text(assets.large_text.clone());
                        if let Some(small_image) = assets.small_image.as_ref() {
                            builder = builder.small_image(small_image.clone());
                        }
                        if let Some(small_text) = assets.small_text.as_ref() {
                            builder = builder.small_text(small_text.clone());
                        }
                        builder
                    });
                }

                if let Some(started_at_ms) = snapshot.started_at_ms {
                    activity = activity.timestamps(|timestamps| timestamps.start(started_at_ms));
                }

                if let Some(party) = snapshot.party.as_ref() {
                    activity = activity.party(|builder| {
                        let builder = builder.id(party.id.clone());
                        if let Some(size) = party.size {
                            builder.size(size)
                        } else {
                            builder
                        }
                    });
                }

                if let Some(secret) = snapshot.join_secret.as_ref() {
                    activity = activity.secrets(|secrets| secrets.join(secret.clone()));
                }

                activity
            })
            .map(|_| ())
            .map_err(|error| error.to_string());

        if result.is_err() {
            self.disconnect();
        }
        result
    }

    fn clear(&mut self) {
        if let Some(client) = self.client.as_mut() {
            let _ = client.clear_activity();
        }
        self.disconnect();
    }

    fn ensure_connected(&mut self) -> Result<(), String> {
        if self.client.is_some() {
            return Client::is_ready()
                .then_some(())
                .ok_or_else(|| DISCORD_STARTING_ERROR.to_string());
        }

        let application_id = discord_application_id()
            .parse::<u64>()
            .map_err(|_| "Discord application ID 형식 오류".to_string())?;
        let mut client = Client::with_error_config(application_id, Duration::from_secs(2), Some(1));
        let sender = self.join_sender.clone();
        client
            .on_activity_join(move |context| {
                if let EventData::ActivityJoin(data) = context.event {
                    if let Some(secret) = data.secret {
                        let _ = sender.send(secret);
                    }
                }
            })
            .persist();

        let request_sender = self.join_request_sender.clone();
        client
            .on_activity_join_request(move |context| {
                if let EventData::ActivityJoinRequest(data) = context.event {
                    if let Some(user) = data.user {
                        if let Some(id) = user.id {
                            if let Ok(user_id) = id.parse::<u64>() {
                                let _ = request_sender.send(user_id);
                            }
                        }
                    }
                }
            })
            .persist();
        client.start();

        self.client = Some(client);
        self.subscribed_to_join = false;
        self.subscribed_to_join_request = false;
        Client::is_ready()
            .then_some(())
            .ok_or_else(|| DISCORD_STARTING_ERROR.to_string())
    }

    fn ensure_join_subscription(&mut self) -> Result<(), String> {
        let client = self
            .client
            .as_mut()
            .expect("presence client must exist before subscribing");

        if !self.subscribed_to_join {
            client
                .subscribe(Event::ActivityJoin, |args| args)
                .map_err(|error| error.to_string())?;
            self.subscribed_to_join = true;
        }
        if !self.subscribed_to_join_request {
            client
                .subscribe(Event::ActivityJoinRequest, |args| args)
                .map_err(|error| error.to_string())?;
            self.subscribed_to_join_request = true;
        }
        Ok(())
    }

    fn close_join_request(&mut self, user_id: u64) -> Result<(), String> {
        self.client
            .as_mut()
            .ok_or_else(|| "Discord IPC가 연결되어 있지 않습니다.".to_string())?
            .close_activity_request(user_id)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn disconnect(&mut self) {
        self.subscribed_to_join = false;
        self.subscribed_to_join_request = false;
        if let Some(client) = self.client.take() {
            let _ = client.shutdown();
        }
    }
}

impl Drop for PresenceSession {
    fn drop(&mut self) {
        self.clear();
    }
}

async fn set_activity_with_startup_retry(
    session: &mut PresenceSession,
    snapshot: &PresenceSnapshot,
) -> Result<(), String> {
    for attempt in 0..DISCORD_STARTUP_RETRY_ATTEMPTS {
        match session.set_activity(snapshot) {
            Ok(()) => return Ok(()),
            Err(error) if error == DISCORD_STARTING_ERROR => {
                if attempt + 1 < DISCORD_STARTUP_RETRY_ATTEMPTS {
                    sleep(DISCORD_STARTUP_RETRY_DELAY).await;
                    continue;
                }
                session.disconnect();
                return Err(
                    "Discord IPC 연결 준비 시간 초과 — Discord 실행 여부를 확인하세요".to_string(),
                );
            }
            Err(error) => return Err(error),
        }
    }
    session.disconnect();
    Err("Discord IPC 연결 준비 시간 초과".to_string())
}

pub(crate) async fn watch_discord_presence(app: AppHandle, state: Arc<AppState>) {
    let mut shutdown = state.shutdown_receiver();
    let (join_sender, mut join_receiver) = mpsc::unbounded_channel();
    let (join_request_sender, mut join_request_receiver) = mpsc::unbounded_channel();
    let mut join_resolution_receiver = state.discord_join_resolution_receiver();
    let mut session = PresenceSession::new(join_sender, join_request_sender);
    let request_party = PresenceParty {
        id: format!("yummi-presence-{}", Uuid::new_v4()),
        // Discord requires party capacity for Ask to Join. This synthetic party
        // stays request-only; real League party membership is handled by the host Agent.
        size: Some((1, 2)),
    };
    let mut last_snapshot: Option<PresenceSnapshot> = None;
    let mut retry_at = Instant::now();
    let mut last_presence_error: Option<String> = None;
    let mut champion_summary: Option<Value> = None;
    let mut pending_join_requests: HashMap<u64, PendingJoinRequest> = HashMap::new();
    let mut next_match_context_refresh = Instant::now();
    let mut delayed_phase_republish_at: Option<Instant> = None;
    let mut delayed_phase_key: Option<String> = None;

    loop {
        if *shutdown.borrow() {
            break;
        }

        while let Ok(secret) = join_receiver.try_recv() {
            if let Err(error) = handle_join_secret(&state, &secret).await {
                state
                    .record_flight("discord_presence_error", format!("join_failed: {error}"))
                    .await;
                state
                    .log(&app, format!("Discord 파티 참가 처리 실패: {error}"))
                    .await;
            }
        }
        while let Ok(user_id) = join_request_receiver.try_recv() {
            queue_join_request(&app, &state, &mut pending_join_requests, user_id).await;
        }
        while let Ok(resolution) = join_resolution_receiver.try_recv() {
            apply_join_resolution(
                &app,
                &state,
                &mut session,
                &mut pending_join_requests,
                resolution,
            )
            .await;
        }

        retry_pending_join_lookups(&state, &mut pending_join_requests).await;

        if Instant::now() >= next_match_context_refresh {
            let queued = state.request_discord_presence_context();
            state
                .record_flight(
                    "discord_presence_context",
                    format!("refresh_requested relay_receiver={queued}"),
                )
                .await;
            next_match_context_refresh = Instant::now() + MATCH_CONTEXT_REFRESH_INTERVAL;
        }

        let match_join_url = state
            .discord_presence_match()
            .await
            .as_ref()
            .and_then(guild_match_join_url);
        let config = state.config.read().await.clone();
        if let Err(error) = flush_pending_join_requests(
            &app,
            &state,
            &mut session,
            &config,
            &mut pending_join_requests,
        )
        .await
        {
            state
                .record_flight(
                    "discord_presence_error",
                    format!("join_request_flush_failed: {error}"),
                )
                .await;
            state
                .log(&app, format!("Discord 참가 요청 지연 처리 실패: {error}"))
                .await;
        }

        let snapshot = detect_presence(
            &config,
            &mut champion_summary,
            &request_party,
            match_join_url,
        )
        .await;
        let phase_changed = match (&last_snapshot, &snapshot) {
            (Some(previous), Some(current)) => previous.phase_key != current.phase_key,
            (None, Some(_)) => true,
            _ => false,
        };
        let changed = match (&last_snapshot, &snapshot) {
            (Some(previous), Some(current)) => !previous.same_activity(current),
            (None, None) => false,
            _ => true,
        };
        let now = Instant::now();
        let should_retry = snapshot.is_some() && !session.is_connected() && now >= retry_at;
        // Discord does not expose activity priority. As a best-effort nudge, resend
        // Yummi shortly after League changes phase, once the first publish succeeded.
        let delayed_republish_due = session.is_connected()
            && delayed_phase_republish_at.is_some_and(|at| now >= at)
            && snapshot.as_ref().is_some_and(|current| {
                delayed_phase_key.as_deref() == Some(current.phase_key.as_str())
            });

        if changed || should_retry || delayed_republish_due {
            match snapshot.as_ref() {
                Some(current) => {
                    match set_activity_with_startup_retry(&mut session, current).await {
                        Ok(()) => {
                            if delayed_republish_due {
                                state
                                    .record_flight(
                                        "discord_presence",
                                        format!("phase_republish_sent phase={}", current.phase_key),
                                    )
                                    .await;
                                delayed_phase_republish_at = None;
                                delayed_phase_key = None;
                            } else if phase_changed {
                                delayed_phase_republish_at =
                                    Some(Instant::now() + PHASE_REPUBLISH_DELAY);
                                delayed_phase_key = Some(current.phase_key.clone());
                                state
                                    .record_flight(
                                        "discord_presence",
                                        format!(
                                            "phase_republish_scheduled phase={} delay_ms={}",
                                            current.phase_key,
                                            PHASE_REPUBLISH_DELAY.as_millis()
                                        ),
                                    )
                                    .await;
                            }
                            if last_presence_error.take().is_some() {
                                state
                                    .record_flight("discord_presence", "connection_recovered")
                                    .await;
                                state.log(&app, "Discord Rich Presence 연결 복구").await;
                            }
                        }
                        Err(error) => {
                            retry_at = Instant::now() + DISCORD_RETRY_INTERVAL;
                            if last_presence_error.as_deref() != Some(error.as_str()) {
                                state
                                    .record_flight(
                                        "discord_presence_error",
                                        format!("set_activity_failed: {error}"),
                                    )
                                    .await;
                                state
                                    .log(&app, format!("Discord Rich Presence 오류: {error}"))
                                    .await;
                                last_presence_error = Some(error);
                            }
                        }
                    }
                }
                None => {
                    session.clear();
                    last_presence_error = None;
                    delayed_phase_republish_at = None;
                    delayed_phase_key = None;
                }
            }
            last_snapshot = snapshot;
        }

        let next_wait = if session.is_connected() {
            delayed_phase_republish_at
                .map(|at| {
                    at.saturating_duration_since(Instant::now())
                        .min(PRESENCE_POLL_INTERVAL)
                })
                .unwrap_or(PRESENCE_POLL_INTERVAL)
        } else {
            PRESENCE_POLL_INTERVAL
        };

        tokio::select! {
            _ = sleep(next_wait) => {}
            Some(secret) = join_receiver.recv() => {
                if let Err(error) = handle_join_secret(&state, &secret).await {
                    state
                        .record_flight("discord_presence_error", format!("join_failed: {error}"))
                        .await;
                    state
                        .log(&app, format!("Discord 파티 참가 처리 실패: {error}"))
                        .await;
                }
            }
            Some(user_id) = join_request_receiver.recv() => {
                queue_join_request(&app, &state, &mut pending_join_requests, user_id).await;
            }
            resolution = join_resolution_receiver.recv() => {
                if let Ok(resolution) = resolution {
                    apply_join_resolution(
                        &app,
                        &state,
                        &mut session,
                        &mut pending_join_requests,
                        resolution,
                    )
                    .await;
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }

    session.clear();
}

async fn handle_join_secret(state: &AppState, secret: &str) -> Result<(), String> {
    // Outside Lobby we publish a request-only secret so Discord can keep the
    // Ask to Join affordance visible. It must never be forwarded to the LCU.
    if parse_request_only_secret(secret).is_some() {
        return Ok(());
    }

    let party_id =
        parse_join_secret(secret).ok_or_else(|| "Discord join secret 형식 오류".to_string())?;
    let config = state.config.read().await.clone();
    let path = lockfile_path(&config).ok_or_else(|| "LCU lockfile을 찾을 수 없음".to_string())?;
    let client = LcuClient::from_lockfile(&path)
        .or_else(|_| LcuClient::from_lockfile_legacy(&path))
        .map_err(|error| format!("LCU 연결 준비 실패: {error}"))?;
    client
        .join_discord_party(party_id)
        .await
        .map_err(|error| format!("LCU 파티 참가 요청 실패: {error}"))?;
    Ok(())
}

async fn queue_join_request(
    app: &AppHandle,
    state: &AppState,
    pending: &mut HashMap<u64, PendingJoinRequest>,
    user_id: u64,
) {
    if pending.contains_key(&user_id) {
        return;
    }

    pending.insert(
        user_id,
        PendingJoinRequest {
            queued_at: Instant::now(),
            riot_id: None,
            lookup_status: None,
            last_lookup_at: None,
            last_invite_at: None,
        },
    );
    request_join_lookup(state, pending, user_id).await;
    state
        .record_flight(
            "discord_presence_join_request",
            format!("queued_until_lobby user_id={user_id}"),
        )
        .await;
    state
        .log(
            app,
            "Discord 참가 요청 대기 — Riot ID 확인 후 League 로비에서 초대",
        )
        .await;
}

async fn request_join_lookup(
    state: &AppState,
    pending: &mut HashMap<u64, PendingJoinRequest>,
    user_id: u64,
) {
    let Some(request) = pending.get_mut(&user_id) else {
        return;
    };
    request.last_lookup_at = Some(Instant::now());
    let queued = state.request_discord_join_resolution(user_id);
    state
        .record_flight(
            "discord_presence_join_request",
            format!("lookup_queued user_id={user_id} relay_receiver={queued}"),
        )
        .await;
}

async fn retry_pending_join_lookups(
    state: &AppState,
    pending: &mut HashMap<u64, PendingJoinRequest>,
) {
    let now = Instant::now();
    let retry_ids = pending
        .iter()
        .filter_map(|(user_id, request)| {
            if request.riot_id.is_some()
                || request.lookup_status.as_deref() == Some("nickname_missing")
            {
                return None;
            }
            let due = request
                .last_lookup_at
                .is_none_or(|last| now.duration_since(last) >= JOIN_REQUEST_LOOKUP_RETRY);
            due.then_some(*user_id)
        })
        .collect::<Vec<_>>();

    for user_id in retry_ids {
        request_join_lookup(state, pending, user_id).await;
    }
}

async fn apply_join_resolution(
    app: &AppHandle,
    state: &AppState,
    session: &mut PresenceSession,
    pending: &mut HashMap<u64, PendingJoinRequest>,
    resolution: DiscordJoinResolution,
) {
    let Some(request) = pending.get_mut(&resolution.requester_discord_id) else {
        return;
    };

    let previous_status = request.lookup_status.clone();
    request.lookup_status = Some(resolution.status.clone());

    if resolution.status == "resolved" {
        if let Some(riot_id) = resolution.riot_id.filter(|value| valid_riot_id(value)) {
            request.riot_id = Some(riot_id);
            request.last_invite_at = None;
            state
                .record_flight(
                    "discord_presence_join_request",
                    format!(
                        "riot_id_resolved user_id={}",
                        resolution.requester_discord_id
                    ),
                )
                .await;
            if previous_status.as_deref() != Some("resolved") {
                state
                    .log(app, "Discord 참가 요청 Riot ID 확인 완료 — 로비 대기")
                    .await;
            }
            return;
        }
    }

    request.riot_id = None;
    state
        .record_flight(
            "discord_presence_join_request",
            format!(
                "riot_id_unresolved user_id={} status={}",
                resolution.requester_discord_id, resolution.status
            ),
        )
        .await;

    if resolution.status == "nickname_missing" {
        let user_id = resolution.requester_discord_id;
        pending.remove(&user_id);
        let _ = session.close_join_request(user_id);
        state
            .record_flight(
                "discord_presence_join_request",
                format!("closed_nickname_missing user_id={user_id}"),
            )
            .await;
        if previous_status.as_deref() != Some("nickname_missing") {
            state
                .log(
                    app,
                    "Discord 참가 요청 종료 — 요청자의 Yummi 닉네임이 설정되어 있지 않음",
                )
                .await;
        }
    }
}

async fn flush_pending_join_requests(
    app: &AppHandle,
    state: &AppState,
    session: &mut PresenceSession,
    config: &Config,
    pending: &mut HashMap<u64, PendingJoinRequest>,
) -> Result<(), String> {
    let now = Instant::now();
    let expired = pending
        .iter()
        .filter_map(|(user_id, request)| {
            (now.duration_since(request.queued_at) >= JOIN_REQUEST_MAX_DELAY).then_some(*user_id)
        })
        .collect::<Vec<_>>();

    for user_id in expired {
        pending.remove(&user_id);
        let _ = session.close_join_request(user_id);
        state
            .record_flight(
                "discord_presence_join_request",
                format!("expired_before_lobby user_id={user_id}"),
            )
            .await;
    }

    let invite_ids = pending
        .iter()
        .filter_map(|(user_id, request)| {
            let riot_id = request.riot_id.as_ref()?;
            let due = request
                .last_invite_at
                .is_none_or(|last| now.duration_since(last) >= JOIN_INVITE_RETRY);
            due.then_some((*user_id, riot_id.clone()))
        })
        .collect::<Vec<_>>();
    if invite_ids.is_empty() {
        return Ok(());
    }

    let Some(path) = lockfile_path(config) else {
        return Ok(());
    };
    let Ok(client) =
        LcuClient::from_lockfile(&path).or_else(|_| LcuClient::from_lockfile_legacy(&path))
    else {
        return Ok(());
    };

    let phase = client.gameflow_phase().await.unwrap_or_default();
    if phase != "Lobby" {
        return Ok(());
    }

    let mut invited_count = 0_usize;
    let mut first_error: Option<String> = None;
    for (user_id, riot_id) in invite_ids {
        if let Some(request) = pending.get_mut(&user_id) {
            request.last_invite_at = Some(Instant::now());
        }

        match client.invite_discord_requester(&riot_id).await {
            Ok(outcome) if outcome.ok => {
                pending.remove(&user_id);
                let _ = session.close_join_request(user_id);
                invited_count += 1;
                state
                    .record_flight(
                        "discord_presence_join_request",
                        format!("league_invite_sent_after_lobby user_id={user_id}"),
                    )
                    .await;
            }
            Ok(outcome) => {
                state
                    .record_flight(
                        "discord_presence_join_request",
                        format!("league_invite_rejected user_id={user_id}"),
                    )
                    .await;
                if first_error.is_none() {
                    first_error = Some(outcome.message);
                }
            }
            Err(error) => {
                state
                    .record_flight(
                        "discord_presence_join_request",
                        format!("league_invite_error user_id={user_id}"),
                    )
                    .await;
                if first_error.is_none() {
                    first_error = Some(error.to_string());
                }
            }
        }
    }

    if invited_count > 0 {
        state
            .log(
                app,
                format!(
                    "Discord 참가 요청 {invited_count}건 처리 — 요청자 앱 없이 League 파티 초대 전송"
                ),
            )
            .await;
    }

    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(())
}

fn valid_riot_id(value: &str) -> bool {
    let value = value.trim();
    let Some(separator) = value.rfind('#') else {
        return false;
    };
    !value[..separator].trim().is_empty()
        && !value[separator + 1..].trim().is_empty()
        && value.len() <= 128
}

fn riot_id_from_identity(value: &Value) -> Option<String> {
    if let Some(riot_id) = value
        .get("riotId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|riot_id| valid_riot_id(riot_id))
    {
        return Some(riot_id.to_string());
    }

    let game_name = value
        .get("gameName")
        .or_else(|| value.get("riotIdGameName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let tag_line = value
        .get("tagLine")
        .or_else(|| value.get("riotIdTagLine"))
        .or_else(|| value.get("riotIdTagline"))
        .or_else(|| value.get("gameTag"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let riot_id = format!("{game_name}#{tag_line}");
    valid_riot_id(&riot_id).then_some(riot_id)
}

fn opgg_url_from_identity(value: &Value) -> Option<String> {
    riot_id_from_identity(value).and_then(|riot_id| opgg_url_for_riot_id(&riot_id))
}

fn opgg_url_for_riot_id(riot_id: &str) -> Option<String> {
    let riot_id = riot_id.trim();
    let separator = riot_id.rfind('#')?;
    let game_name = riot_id[..separator].trim();
    let tag_line = riot_id[separator + 1..].trim();
    if game_name.is_empty() || tag_line.is_empty() || riot_id.len() > 128 {
        return None;
    }

    let slug = format!("{game_name}-{tag_line}");
    let mut url = url::Url::parse("https://www.op.gg/summoners/kr").ok()?;
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.push(&slug);
    }
    Some(url.to_string())
}

fn guild_match_join_url(context: &DiscordPresenceMatchContext) -> Option<String> {
    if !matches!(
        context.status.as_str(),
        "TEAMING"
            | "LOBBY_WAITING"
            | "LOBBY_SYNCED"
            | "CHAMP_SELECT"
            | "IN_GAME"
            | "GAME_ENDED"
            | "RESULT_PENDING"
    )
        || context.discord_guild_id.is_empty()
        || !context
            .discord_guild_id
            .chars()
            .all(|character| character.is_ascii_digit())
        || context.invite_code.is_empty()
        || context.invite_code.len() > 64
    {
        return None;
    }

    let mut url = url::Url::parse("https://yummi.duckdns.org/").ok()?;
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.push("guilds");
        segments.push(&context.discord_guild_id);
        segments.push("m");
        segments.push(&context.invite_code);
    }
    Some(url.to_string())
}

async fn detect_presence(
    config: &Config,
    champion_summary: &mut Option<Value>,
    request_party: &PresenceParty,
    match_join_url: Option<String>,
) -> Option<PresenceSnapshot> {
    if let Some(path) = lockfile_path(config) {
        if let Ok(client) =
            LcuClient::from_lockfile(&path).or_else(|_| LcuClient::from_lockfile_legacy(&path))
        {
            if let Ok(phase) = client.gameflow_phase().await {
                if phase == "InProgress" {
                    if let Ok(live_game) = LcuClient::live_game_request(LIVE_GAME_ENDPOINT).await {
                        if champion_summary.is_none() {
                            *champion_summary = client
                                .champion_summary()
                                .await
                                .ok()
                                .filter(Value::is_array);
                        }
                        let gameflow_session = client.gameflow_session().await.ok();
                        let opgg_url = client
                            .current_summoner()
                            .await
                            .ok()
                            .and_then(|summoner| opgg_url_from_identity(&summoner));
                        return Some(in_progress_snapshot(
                            &live_game,
                            gameflow_session.as_ref(),
                            champion_summary.as_ref(),
                            request_party,
                            match_join_url.clone(),
                            opgg_url,
                        ));
                    }
                }
                let party = if phase == "Lobby" {
                    client
                        .discord_party_info()
                        .await
                        .ok()
                        .flatten()
                        .map(|party| PresenceParty {
                            // Keep Discord's activity party synthetic so clicking
                            // Ask to Join never depends on the requester's Agent.
                            id: request_party.id.clone(),
                            size: party.size.or(request_party.size),
                        })
                        .unwrap_or_else(|| request_party.clone())
                } else {
                    request_party.clone()
                };
                let opgg_url = client
                    .current_summoner()
                    .await
                    .ok()
                    .and_then(|summoner| opgg_url_from_identity(&summoner));
                return phase_snapshot(
                    &phase,
                    Some(party),
                    Some(request_only_secret(&request_party.id)),
                    match_join_url.clone(),
                    opgg_url,
                );
            }
        }
    }

    LcuClient::live_game_request(LIVE_GAME_ENDPOINT)
        .await
        .ok()
        .map(|live_game| {
            in_progress_snapshot(
                &live_game,
                None,
                champion_summary.as_ref(),
                request_party,
                match_join_url.clone(),
                None,
            )
        })
}

fn yummi_assets() -> PresenceAssets {
    PresenceAssets {
        large_image: YUMMI_ICON_URL.into(),
        large_text: "Yummi LCU Agent".into(),
        small_image: None,
        small_text: None,
    }
}

fn phase_snapshot(
    phase: &str,
    party: Option<PresenceParty>,
    join_secret: Option<String>,
    match_join_url: Option<String>,
    opgg_url: Option<String>,
) -> Option<PresenceSnapshot> {
    let details = match phase {
        "None" | "ClientStopped" => return None,
        "Lobby" => "로비에 있는 중",
        "Matchmaking" => "매칭 검색 중",
        "ReadyCheck" => "매치 수락 대기 중",
        "ChampSelect" => "챔피언 선택 중",
        "GameStart" => "게임 시작 중",
        "InProgress" => "게임 진행 중",
        "Reconnect" => "게임 재접속 대기 중",
        "WaitingForStats" | "PreEndOfGame" | "EndOfGame" => "게임 결과 확인 중",
        "TerminatedInError" => "League Client 복구 중",
        _ => "League Client 사용 중",
    };

    Some(PresenceSnapshot {
        phase_key: phase.to_string(),
        details: details.into(),
        state: "League of Legends".into(),
        started_at_ms: None,
        party,
        join_secret,
        match_join_url,
        opgg_url,
        assets: Some(yummi_assets()),
    })
}

fn in_progress_snapshot(
    live_game: &Value,
    gameflow_session: Option<&Value>,
    champion_summary: Option<&Value>,
    request_party: &PresenceParty,
    match_join_url: Option<String>,
    opgg_url: Option<String>,
) -> PresenceSnapshot {
    let raw_mode = live_game
        .pointer("/gameData/gameMode")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mode = game_mode_label(raw_mode);
    let queue_id = gameflow_session.and_then(queue_id_from_gameflow);
    let details = queue_id
        .and_then(queue_label)
        .map(|queue| format!("{queue} 플레이 중"))
        .unwrap_or_else(|| {
            if mode == "League of Legends" {
                "게임 진행 중".into()
            } else {
                format!("{mode} 플레이 중")
            }
        });
    let elapsed_seconds = live_game
        .pointer("/gameData/gameTime")
        .and_then(Value::as_f64)
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0);

    let active_player = active_player_row(live_game);
    let opgg_url = opgg_url.or_else(|| active_player.and_then(opgg_url_from_identity));
    let champion_alias = active_player
        .and_then(|player| player.get("championName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let champion = champion_alias.and_then(|alias| {
        champion_summary.and_then(|summary| champion_metadata(summary, alias))
    });
    let champion_name = champion
        .as_ref()
        .map(|(_, name)| name.as_str())
        .or(champion_alias);

    let state = match (active_player, champion_name) {
        (Some(player), Some(name)) => {
            let scores = player.get("scores");
            match scores.and_then(kda_from_scores) {
                Some((kills, deaths, assists)) => {
                    format!("{name} · {kills} / {deaths} / {assists}")
                }
                None => name.to_string(),
            }
        }
        _ => mode.into(),
    };

    let assets = match champion {
        Some((champion_id, champion_name)) => Some(PresenceAssets {
            large_image: champion_icon_url(champion_id),
            large_text: champion_name,
            small_image: Some(YUMMI_ICON_URL.into()),
            small_text: Some("Yummi LCU Agent".into()),
        }),
        None => Some(yummi_assets()),
    };

    PresenceSnapshot {
        phase_key: "InProgress".into(),
        details,
        state,
        started_at_ms: elapsed_seconds.and_then(activity_started_at_ms),
        party: Some(request_party.clone()),
        join_secret: Some(request_only_secret(&request_party.id)),
        match_join_url,
        opgg_url,
        assets,
    }
}

fn queue_id_from_gameflow(session: &Value) -> Option<i64> {
    [
        "/gameData/queue/id",
        "/gameData/queueId",
        "/gameConfig/queueId",
        "/queue/id",
    ]
    .iter()
    .find_map(|path| session.pointer(path).and_then(Value::as_i64))
}

fn queue_label(queue_id: i64) -> Option<&'static str> {
    match queue_id {
        0 => Some("커스텀"),
        400 | 430 => Some("일반"),
        420 => Some("솔로랭크"),
        440 => Some("자유랭크"),
        450 => Some("칼바람"),
        490 => Some("빠른대전"),
        318 | 900 | 1900 => Some("우르프"),
        870 => Some("봇 대전: 입문"),
        880 => Some("봇 대전: 초보"),
        890 => Some("봇 대전: 중급"),
        1020 => Some("단일 챔피언"),
        1300 => Some("넥서스 블리츠"),
        1400 => Some("궁극기 주문서"),
        1700 | 1710 => Some("아레나"),
        1750 => Some("3인 아레나"),
        2400 => Some("아수라장"),
        _ => None,
    }
}

fn active_player_row(live_game: &Value) -> Option<&Value> {
    let active = live_game.get("activePlayer")?;
    let active_keys = player_identity_keys(active);
    if active_keys.is_empty() {
        return None;
    }
    live_game
        .get("allPlayers")
        .or_else(|| live_game.get("players"))
        .and_then(Value::as_array)?
        .iter()
        .find(|player| {
            let player_keys = player_identity_keys(player);
            active_keys
                .iter()
                .any(|active_key| player_keys.iter().any(|key| key == active_key))
        })
}

fn player_identity_keys(player: &Value) -> Vec<String> {
    let mut keys = ["riotId", "summonerName", "riotIdGameName", "gameName"]
        .iter()
        .filter_map(|key| player.get(*key).and_then(Value::as_str))
        .map(normalize_identity)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    let game_name = player
        .get("riotIdGameName")
        .or_else(|| player.get("gameName"))
        .and_then(Value::as_str);
    let tag_line = player
        .get("riotIdTagLine")
        .or_else(|| player.get("riotIdTagline"))
        .or_else(|| player.get("tagLine"))
        .and_then(Value::as_str);
    if let (Some(game_name), Some(tag_line)) = (game_name, tag_line) {
        let riot_id = normalize_identity(&format!("{game_name}#{tag_line}"));
        if !riot_id.is_empty() {
            keys.push(riot_id);
        }
    }
    keys.sort_unstable();
    keys.dedup();
    keys
}

fn normalize_identity(value: &str) -> String {
    value.trim().to_lowercase()
}

fn champion_metadata(summary: &Value, alias: &str) -> Option<(u64, String)> {
    summary.as_array()?.iter().find_map(|champion| {
        let champion_alias = champion.get("alias").and_then(Value::as_str)?;
        if !champion_alias.eq_ignore_ascii_case(alias) {
            return None;
        }
        let champion_id = champion.get("id").and_then(Value::as_u64)?;
        if champion_id == 0 {
            return None;
        }
        let name = champion
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(alias)
            .to_string();
        Some((champion_id, name))
    })
}

fn kda_from_scores(scores: &Value) -> Option<(u64, u64, u64)> {
    Some((
        scores.get("kills")?.as_u64()?,
        scores.get("deaths")?.as_u64()?,
        scores.get("assists")?.as_u64()?,
    ))
}

fn champion_icon_url(champion_id: u64) -> String {
    format!("{CHAMPION_ICON_URL_PREFIX}/{champion_id}.png")
}

fn activity_started_at_ms(elapsed_seconds: f64) -> Option<u64> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    let elapsed_ms = Duration::from_secs_f64(elapsed_seconds).as_millis();
    u64::try_from(now_ms.saturating_sub(elapsed_ms)).ok()
}

fn join_secret(party_id: &str) -> String {
    format!("{JOIN_SECRET_PREFIX}{party_id}")
}

fn request_only_secret(party_id: &str) -> String {
    format!("{REQUEST_ONLY_SECRET_PREFIX}{party_id}")
}

fn parse_join_secret(secret: &str) -> Option<&str> {
    parse_prefixed_party_secret(secret, JOIN_SECRET_PREFIX)
}

fn parse_request_only_secret(secret: &str) -> Option<&str> {
    parse_prefixed_party_secret(secret, REQUEST_ONLY_SECRET_PREFIX)
}

fn parse_prefixed_party_secret<'a>(secret: &'a str, prefix: &str) -> Option<&'a str> {
    let party_id = secret.strip_prefix(prefix)?.trim();
    if party_id.is_empty()
        || party_id.len() > 128
        || !party_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return None;
    }
    Some(party_id)
}

fn game_mode_label(raw: &str) -> &str {
    match raw.trim().to_ascii_uppercase().as_str() {
        "CLASSIC" => "소환사의 협곡",
        "ARAM" => "무작위 총력전",
        "CHERRY" => "아레나",
        "STRAWBERRY" => "집중포화",
        "URF" => "URF",
        "ULTBOOK" => "궁극기 주문서",
        "ONEFORALL" => "단일 챔피언",
        "NEXUSBLITZ" => "넥서스 블리츠",
        "TUTORIAL" => "튜토리얼",
        _ => "League of Legends",
    }
}

fn discord_application_id() -> &'static str {
    option_env!("YUMMI_DISCORD_APPLICATION_ID").unwrap_or(DEFAULT_DISCORD_APPLICATION_ID)
}

#[cfg(test)]
mod tests {
    use super::{
        activity_started_at_ms, game_mode_label, in_progress_snapshot, join_secret,
        guild_match_join_url, parse_join_secret, parse_request_only_secret, phase_snapshot,
        request_only_secret, PresenceParty, YUMMI_ICON_URL,
    };
    use serde_json::json;

    #[test]
    fn maps_known_gameflow_phases() {
        let matchmaking = phase_snapshot("Matchmaking", None, None, None, None).unwrap();
        assert_eq!(matchmaking.phase_key, "Matchmaking");
        assert_eq!(matchmaking.details, "매칭 검색 중");
        assert_eq!(
            phase_snapshot("ChampSelect", None, None, None, None).unwrap().details,
            "챔피언 선택 중"
        );
        assert!(phase_snapshot("None", None, None, None, None).is_none());
    }

    #[test]
    fn lobby_can_publish_join_party() {
        let snapshot = phase_snapshot(
            "Lobby",
            Some(PresenceParty {
                id: "party-123".into(),
                size: Some((2, 5)),
            }),
            Some(join_secret("party-123")),
            None,
            None,
        )
        .unwrap();
        assert_eq!(snapshot.party.unwrap().id, "party-123");
        assert_eq!(
            snapshot.join_secret.as_deref(),
            Some("yummi:lobby:v1:party-123")
        );
    }

    #[test]
    fn join_secret_is_versioned_and_validated() {
        let secret = join_secret("abc-123_def");
        assert_eq!(parse_join_secret(&secret), Some("abc-123_def"));
        assert!(parse_join_secret("other:lobby:v1:abc").is_none());
        assert!(parse_join_secret("yummi:lobby:v1:../../bad").is_none());

        let request_secret = request_only_secret("yummi-presence-test");
        assert_eq!(
            parse_request_only_secret(&request_secret),
            Some("yummi-presence-test")
        );
        assert!(parse_join_secret(&request_secret).is_none());
    }

    #[test]
    fn live_game_uses_queue_champion_kda_and_champion_assets() {
        let live_game = json!({
            "gameData": {"gameMode": "CLASSIC", "gameTime": 125.0},
            "activePlayer": {"riotId": "Player#KR1"},
            "allPlayers": [{
                "riotId": "Player#KR1",
                "championName": "Ahri",
                "scores": {"kills": 7, "deaths": 2, "assists": 9}
            }]
        });
        let gameflow = json!({"gameData": {"queue": {"id": 420}}});
        let champions = json!([{"id": 103, "alias": "Ahri", "name": "아리"}]);
        let request_party = PresenceParty {
            id: "yummi-presence-test".into(),
            size: None,
        };
        let snapshot = in_progress_snapshot(
            &live_game,
            Some(&gameflow),
            Some(&champions),
            &request_party,
            None,
            None,
        );

        assert_eq!(snapshot.phase_key, "InProgress");
        assert_eq!(snapshot.details, "솔로랭크 플레이 중");
        assert_eq!(snapshot.state, "아리 · 7 / 2 / 9");
        assert!(snapshot.started_at_ms.is_some());
        assert_eq!(snapshot.party.as_ref().map(|party| party.id.as_str()), Some("yummi-presence-test"));
        assert_eq!(
            snapshot.join_secret.as_deref(),
            Some("yummi:request:v1:yummi-presence-test")
        );
        assert_eq!(
            snapshot.opgg_url.as_deref(),
            Some("https://www.op.gg/summoners/kr/Player-KR1")
        );
        let assets = snapshot.assets.unwrap();
        assert!(assets.large_image.ends_with("/103.png"));
        assert_eq!(assets.large_text, "아리");
        assert_eq!(assets.small_image.as_deref(), Some(YUMMI_ICON_URL));
        assert_eq!(assets.small_text.as_deref(), Some("Yummi LCU Agent"));
    }

    #[test]
    fn guild_match_context_builds_dashboard_url() {
        let url = guild_match_join_url(&crate::state::DiscordPresenceMatchContext {
            discord_guild_id: "123456789".into(),
            invite_code: "ABCDE".into(),
            status: "IN_GAME".into(),
        });
        assert_eq!(
            url.as_deref(),
            Some("https://yummi.duckdns.org/guilds/123456789/m/ABCDE")
        );
    }

    #[test]
    fn opgg_url_is_built_from_riot_id_and_encoded() {
        assert_eq!(
            super::opgg_url_for_riot_id("Player Name#KR1").as_deref(),
            Some("https://www.op.gg/summoners/kr/Player%20Name-KR1")
        );
        assert!(super::opgg_url_for_riot_id("missing-tag").is_none());
    }

    #[test]
    fn maps_common_modes_without_exposing_unknown_values() {
        assert_eq!(game_mode_label("classic"), "소환사의 협곡");
        assert_eq!(game_mode_label("CHERRY"), "아레나");
        assert_eq!(game_mode_label("future-secret-mode"), "League of Legends");
    }

    #[test]
    fn elapsed_timer_produces_epoch_milliseconds() {
        let started = activity_started_at_ms(60.0).unwrap();
        assert!(started > 1_000_000_000_000);
    }
}
