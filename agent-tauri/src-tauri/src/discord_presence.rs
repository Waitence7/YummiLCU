use std::{
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

use crate::{
    config::Config,
    lcu::{lockfile_path, LcuClient},
    state::AppState,
};

const DEFAULT_DISCORD_APPLICATION_ID: &str = "1491092609001722106";
const PRESENCE_POLL_INTERVAL: Duration = Duration::from_secs(4);
const DISCORD_RETRY_INTERVAL: Duration = Duration::from_secs(15);
const DISCORD_STARTUP_RETRY_DELAY: Duration = Duration::from_millis(250);
const DISCORD_STARTUP_RETRY_ATTEMPTS: usize = 6;
const DISCORD_STARTING_ERROR: &str = "Discord IPC connection is starting";
const LIVE_GAME_ENDPOINT: &str = "/liveclientdata/allgamedata";
const DOWNLOAD_URL: &str = "https://yummi.duckdns.org/download";
const JOIN_SECRET_PREFIX: &str = "yummi:lobby:v1:";

#[derive(Clone, Debug, Eq, PartialEq)]
struct PresenceParty {
    id: String,
    size: Option<(u32, u32)>,
}

#[derive(Clone, Debug)]
struct PresenceSnapshot {
    details: String,
    state: String,
    started_at_ms: Option<u64>,
    party: Option<PresenceParty>,
}

impl PresenceSnapshot {
    fn same_activity(&self, other: &Self) -> bool {
        self.details == other.details
            && self.state == other.state
            && self.party == other.party
            && self.started_at_ms == other.started_at_ms
    }
}

struct PresenceSession {
    client: Option<Client>,
    join_sender: mpsc::UnboundedSender<String>,
    subscribed_to_join: bool,
}

impl PresenceSession {
    fn new(join_sender: mpsc::UnboundedSender<String>) -> Self {
        Self {
            client: None,
            join_sender,
            subscribed_to_join: false,
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
                    .state(snapshot.state.clone())
                    .append_buttons(|button| button.label("앱 다운로드").url(DOWNLOAD_URL));

                if let Some(started_at_ms) = snapshot.started_at_ms {
                    activity = activity.timestamps(|timestamps| timestamps.start(started_at_ms));
                }

                if let Some(party) = snapshot.party.as_ref() {
                    activity = activity
                        .party(|builder| {
                            let builder = builder.id(party.id.clone());
                            if let Some(size) = party.size {
                                builder.size(size)
                            } else {
                                builder
                            }
                        })
                        .secrets(|secrets| secrets.join(join_secret(&party.id)));
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
        client.start();

        self.client = Some(client);
        self.subscribed_to_join = false;
        Client::is_ready()
            .then_some(())
            .ok_or_else(|| DISCORD_STARTING_ERROR.to_string())
    }

    fn ensure_join_subscription(&mut self) -> Result<(), String> {
        if self.subscribed_to_join {
            return Ok(());
        }
        self.client
            .as_mut()
            .expect("presence client must exist before subscribing")
            .subscribe(Event::ActivityJoin, |args| args)
            .map_err(|error| error.to_string())?;
        self.subscribed_to_join = true;
        Ok(())
    }

    fn disconnect(&mut self) {
        self.subscribed_to_join = false;
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
    let mut session = PresenceSession::new(join_sender);
    let mut last_snapshot: Option<PresenceSnapshot> = None;
    let mut retry_at = Instant::now();
    let mut last_presence_error: Option<String> = None;

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

        let config = state.config.read().await.clone();
        let snapshot = detect_presence(&config).await;
        let changed = match (&last_snapshot, &snapshot) {
            (Some(previous), Some(current)) => !previous.same_activity(current),
            (None, None) => false,
            _ => true,
        };
        let should_retry =
            snapshot.is_some() && !session.is_connected() && Instant::now() >= retry_at;

        if changed || should_retry {
            match snapshot.as_ref() {
                Some(current) => {
                    match set_activity_with_startup_retry(&mut session, current).await {
                        Ok(()) => {
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
                }
            }
            last_snapshot = snapshot;
        }

        tokio::select! {
            _ = sleep(PRESENCE_POLL_INTERVAL) => {}
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

async fn detect_presence(config: &Config) -> Option<PresenceSnapshot> {
    if let Some(path) = lockfile_path(config) {
        if let Ok(client) =
            LcuClient::from_lockfile(&path).or_else(|_| LcuClient::from_lockfile_legacy(&path))
        {
            if let Ok(phase) = client.gameflow_phase().await {
                if phase == "InProgress" {
                    if let Ok(live_game) = LcuClient::live_game_request(LIVE_GAME_ENDPOINT).await {
                        return Some(in_progress_snapshot(&live_game));
                    }
                }
                let party = if phase == "Lobby" {
                    client
                        .discord_party_info()
                        .await
                        .ok()
                        .flatten()
                        .map(|party| PresenceParty {
                            id: party.id,
                            size: party.size,
                        })
                } else {
                    None
                };
                return phase_snapshot(&phase, party);
            }
        }
    }

    LcuClient::live_game_request(LIVE_GAME_ENDPOINT)
        .await
        .ok()
        .map(|live_game| in_progress_snapshot(&live_game))
}

fn phase_snapshot(phase: &str, party: Option<PresenceParty>) -> Option<PresenceSnapshot> {
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
        details: details.into(),
        state: "League of Legends".into(),
        started_at_ms: None,
        party,
    })
}

fn in_progress_snapshot(live_game: &Value) -> PresenceSnapshot {
    let mode = live_game
        .pointer("/gameData/gameMode")
        .and_then(Value::as_str)
        .map(game_mode_label)
        .unwrap_or("League of Legends");
    let elapsed_seconds = live_game
        .pointer("/gameData/gameTime")
        .and_then(Value::as_f64)
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0);

    PresenceSnapshot {
        details: "게임 진행 중".into(),
        state: mode.into(),
        started_at_ms: elapsed_seconds.and_then(activity_started_at_ms),
        party: None,
    }
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

fn parse_join_secret(secret: &str) -> Option<&str> {
    let party_id = secret.strip_prefix(JOIN_SECRET_PREFIX)?.trim();
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
        parse_join_secret, phase_snapshot, PresenceParty,
    };
    use serde_json::json;

    #[test]
    fn maps_known_gameflow_phases() {
        assert_eq!(
            phase_snapshot("Matchmaking", None).unwrap().details,
            "매칭 검색 중"
        );
        assert_eq!(
            phase_snapshot("ChampSelect", None).unwrap().details,
            "챔피언 선택 중"
        );
        assert!(phase_snapshot("None", None).is_none());
    }

    #[test]
    fn lobby_can_publish_join_party() {
        let snapshot = phase_snapshot(
            "Lobby",
            Some(PresenceParty {
                id: "party-123".into(),
                size: Some((2, 5)),
            }),
        )
        .unwrap();
        assert_eq!(snapshot.party.unwrap().id, "party-123");
    }

    #[test]
    fn join_secret_is_versioned_and_validated() {
        let secret = join_secret("abc-123_def");
        assert_eq!(parse_join_secret(&secret), Some("abc-123_def"));
        assert!(parse_join_secret("other:lobby:v1:abc").is_none());
        assert!(parse_join_secret("yummi:lobby:v1:../../bad").is_none());
    }

    #[test]
    fn live_game_uses_mode_and_elapsed_timer() {
        let snapshot = in_progress_snapshot(&json!({
            "gameData": {"gameMode": "ARAM", "gameTime": 125.0}
        }));
        assert_eq!(snapshot.details, "게임 진행 중");
        assert_eq!(snapshot.state, "무작위 총력전");
        assert!(snapshot.started_at_ms.is_some());
        assert!(snapshot.party.is_none());
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
