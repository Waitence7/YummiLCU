use futures_util::SinkExt;
use reqwest::Method;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, watch};
use tokio::time::{sleep, timeout, Duration, Instant};
use tokio_tungstenite::{
    connect_async_tls_with_config,
    tungstenite::{
        client::IntoClientRequest, http::header::AUTHORIZATION, protocol::WebSocketConfig, Message,
    },
    Connector,
};

use crate::config::Config;

use super::{lockfile_path, LcuClient};

const GAMEFLOW_PHASE: &str = "/lol-gameflow/v1/gameflow-phase";
const READY_CHECK: &str = "/lol-matchmaking/v1/ready-check";
const CHAMP_SELECT: &str = "/lol-champ-select/v1/session";
const LOBBY: &str = "/lol-lobby/v2/lobby";
const EOG_STATS: &str = "/lol-end-of-game/v1/eog-stats-block";
const GAMEFLOW_SESSION: &str = "/lol-gameflow/v1/session";
const LIVE_GAME_DATA: &str = "/liveclientdata/allgamedata";
const LIVE_GAME_EVENTS: &str = "/liveclientdata/eventdata";
const LCU_SOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const LCU_SOCKET_RETRY_DELAY: Duration = Duration::from_secs(1);
const LIVE_GAME_POLL_INTERVAL: Duration = Duration::from_secs(3);
const MAX_LCU_EVENT_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(crate) struct LcuEventPoller {
    gameflow: Option<String>,
    ready_check: Option<String>,
    champ_select: Option<String>,
    party: Option<String>,
    participant: Option<String>,
    live_game: Option<String>,
    last_live_game_poll: Option<Instant>,
    eog_sent: bool,
    diagnostics: Vec<String>,
    lockfile_available: Option<bool>,
    lcu_available: Option<bool>,
    live_client_available: Option<bool>,
    live_game_id: Option<String>,
}

impl LcuEventPoller {
    fn diagnostic(&mut self, message: impl Into<String>) {
        self.diagnostics.push(message.into());
        if self.diagnostics.len() > 64 {
            self.diagnostics.remove(0);
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
            let Some(path) = lockfile_path(&config) else {
                if !wait_for_socket_retry(&mut stop).await {
                    return;
                }
                continue;
            };
            let Ok(client) = LcuClient::from_lockfile(&path) else {
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
    pub(crate) async fn poll(&mut self, config: &Config) -> Vec<(&'static str, Value)> {
        let mut events = Vec::new();

        // Live Client Data is independent of the League Client lockfile. This is
        // especially important for spectators and PC cafes where LCU discovery or
        // gameflow may be unavailable while the local game API is still running.
        let should_poll = self
            .last_live_game_poll
            .is_none_or(|last| last.elapsed() >= LIVE_GAME_POLL_INTERVAL);
        if should_poll {
            self.last_live_game_poll = Some(Instant::now());
            match LcuClient::live_game_request(LIVE_GAME_DATA).await {
                Ok(value) => {
                    if self.live_client_available != Some(true) {
                        self.diagnostic("Live Client Data API 연결됨 (127.0.0.1:2999)");
                    }
                    self.live_client_available = Some(true);
                    let live_events_response =
                        LcuClient::live_game_request(LIVE_GAME_EVENTS).await.ok();
                    if let Some(payload) = live_game_payload(&value, live_events_response.as_ref())
                    {
                        let game_id = payload
                            .pointer("/game/id")
                            .map(Value::to_string)
                            .unwrap_or_else(|| "unknown".into());
                        if self.live_game_id.as_deref() != Some(game_id.as_str()) {
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
                            live_game_fingerprint(&payload),
                            "live_game_update",
                            payload,
                            &mut events,
                        );
                    } else {
                        self.live_game = None;
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

        let Some(path) = lockfile_path(config) else {
            if self.lockfile_available != Some(false) {
                self.diagnostic("LCU lockfile 없음 — 관전 API 독립 조회만 계속함");
            }
            self.lockfile_available = Some(false);
            self.lcu_available = Some(false);
            return events;
        };
        if self.lockfile_available != Some(true) {
            self.diagnostic(format!("LCU lockfile 발견: {}", path.display()));
        }
        self.lockfile_available = Some(true);
        let Ok(client) = LcuClient::from_lockfile(&path) else {
            if self.lcu_available != Some(false) {
                self.diagnostic("LCU lockfile 읽기 실패 — 관전 API 독립 조회는 계속함");
            }
            self.lcu_available = Some(false);
            return events;
        };

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
        push_changed(
            &mut self.gameflow,
            phase.clone(),
            "gameflow_update",
            json!({"phase": phase, "lcu_ready": true}),
            &mut events,
        );
        if matches!(
            phase.as_str(),
            "PreEndOfGame" | "EndOfGame" | "WaitingForStats"
        ) && !self.eog_sent
            && matches!(
                previous_phase.as_deref(),
                Some("InProgress") | Some("PreEndOfGame")
            )
        {
            if let Some(payload) = eog_payload(&client, &phase).await {
                events.push(("match_eog", payload.clone()));
                events.push(("guild_match_eog", payload));
                self.eog_sent = true;
            }
        } else if matches!(
            phase.as_str(),
            "Lobby" | "None" | "ChampSelect" | "Matchmaking"
        ) {
            self.eog_sent = false;
        }

        if let Ok(value) = client.request(Method::GET, READY_CHECK, None).await {
            let payload = ready_check_payload(&value);
            push_changed(
                &mut self.ready_check,
                fingerprint(&payload),
                "ready_check_update",
                payload,
                &mut events,
            );
        }
        if let Ok(value) = client.request(Method::GET, CHAMP_SELECT, None).await {
            let payload = champ_select_payload(&value);
            push_changed(
                &mut self.champ_select,
                fingerprint(&payload),
                "champ_select_update",
                payload,
                &mut events,
            );
        }
        if let Ok(value) = client.request(Method::GET, LOBBY, None).await {
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
        events
    }
}

async fn wait_for_socket_retry(stop: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        _ = sleep(LCU_SOCKET_RETRY_DELAY) => !*stop.borrow(),
        changed = stop.changed() => changed.is_ok() && !*stop.borrow(),
    }
}

async fn eog_payload(client: &LcuClient, phase: &str) -> Option<Value> {
    let eog = client.request(Method::GET, EOG_STATS, None).await.ok();
    let session = client
        .request(Method::GET, GAMEFLOW_SESSION, None)
        .await
        .ok();
    let mut participants = Vec::new();
    if let Some(teams) = eog
        .as_ref()
        .and_then(|value| value.get("teams"))
        .and_then(Value::as_array)
    {
        for team in teams {
            let winning = team.get("isWinningTeam").and_then(Value::as_bool);
            for player in team
                .get("players")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = player
                    .get("riotIdGameName")
                    .or_else(|| player.get("summonerName"))
                    .and_then(Value::as_str)?;
                let tag = player
                    .get("riotIdTagline")
                    .or_else(|| player.get("riotIdTagLine"))
                    .and_then(Value::as_str)?;
                participants.push(json!({"gameName": name, "tagLine": tag, "teamId": player.get("teamId").cloned().unwrap_or(Value::Null), "won": player.get("win").cloned().or_else(|| winning.map(Value::Bool)).unwrap_or(Value::Null)}));
            }
        }
    }
    (participants.len() >= 2).then(|| json!({"source":"lcu-agent","gameflowPhase":phase,"capturedAt":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs().to_string(),"participants":participants,"eogStats":eog,"gameflowSession":session}))
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

fn live_game_fingerprint(value: &Value) -> String {
    let mut stable = value.clone();
    if let Some(object) = stable.as_object_mut() {
        object.remove("captured_at_ms");
    }
    fingerprint(&stable)
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
    json!({
        "active": true,
        "phase": timer.and_then(|timer| timer.get("phase")).and_then(Value::as_str).unwrap_or(""),
        "timer_ms": timer.and_then(|timer| timer.get("adjustedTimeLeftInPhase")).and_then(Value::as_i64).unwrap_or(0),
        "local_cell_id": local_cell_id,
        "my_team": team_payload(session.get("myTeam")),
        "their_team": team_payload(session.get("theirTeam")),
        "actions": actions,
        "current_action": current_action.cloned().unwrap_or(Value::Null),
    })
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
    let game_data = value.get("gameData")?.as_object()?;
    let players = value
        .get("allPlayers")
        .or_else(|| value.get("players"))
        .and_then(Value::as_array)?;
    if players.is_empty() {
        return None;
    }

    let participants = players
        .iter()
        .take(10)
        .map(live_player_payload)
        .collect::<Vec<_>>();
    Some(json!({
        "source": "lcu-agent",
        "phase": "InProgress",
        "captured_at_ms": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
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
            "game_time": game_data.get("gameTime").cloned().unwrap_or(Value::Null),
            "raw": game_data.clone(),
        },
        "active_player": active_player_payload(value.get("activePlayer")),
        "participants": participants,
        "events": live_events_payload(events),
    }))
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
                "monster_type": event.get("MonsterType").or_else(|| event.get("monsterType")).cloned().unwrap_or(Value::Null),
                "raw": event.clone(),
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
                "raw_description": item.get("rawDescription").cloned().unwrap_or(Value::Null),
                "raw_display_name": item.get("rawDisplayName").cloned().unwrap_or(Value::Null),
                "slot": item.get("slot").cloned().unwrap_or(Value::Null),
                "raw": item.clone(),
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
        "scores": scores.cloned().unwrap_or(Value::Null),
        "runes": player.get("runes").cloned().unwrap_or(Value::Null),
        "items": items,
        "summoner_spells": player.get("summonerSpells").cloned().unwrap_or(Value::Null),
        "raw": player.clone(),
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
        "scores": scores.cloned().unwrap_or(Value::Null),
        "abilities": player.get("abilities").cloned().unwrap_or(Value::Null),
        "champion_stats": player.get("championStats").cloned().unwrap_or(Value::Null),
        "full_runes": player.get("fullRunes").cloned().unwrap_or(Value::Null),
        "runes": player.get("runes").cloned().unwrap_or(Value::Null),
        "summoner_spells": player.get("summonerSpells").cloned().unwrap_or(Value::Null),
        "raw": player.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(payload["actions"][0]["champion_id"], 10);
        assert_eq!(payload["current_action"]["id"], 7);
    }

    #[test]
    fn live_game_payload_includes_participant_kda_and_items() {
        let payload = live_game_payload(&json!({
            "gameData": {"gameId": 42, "gameMode": "CLASSIC", "gameTime": 120.5},
            "activePlayer": {"summonerName": "Me", "scores": {"kills": 2, "deaths": 1, "assists": 3}},
            "allPlayers": [{
                "summonerName": "Me", "riotId": "Me#KR1", "riotIdGameName": "Me", "riotIdTagLine": "KR1",
                "championName": "Ahri", "rawChampionName": "game_character_displayname_Ahri", "team": "ORDER",
                "position": "MIDDLE", "respawnTimer": 0.0, "skinID": 123, "runes": {"keystone": {"id": 8112}},
                "scores": {"kills": 2, "deaths": 1, "assists": 3, "creepScore": 80},
                "items": [{"itemID": 1056, "displayName": "Doran's Ring", "count": 1, "slot": 0, "canUse": true}]
            }]
        }), None)
        .unwrap();
        assert_eq!(payload["game"]["id"], 42);
        assert_eq!(payload["participants"][0]["kills"], 2);
        assert_eq!(payload["participants"][0]["deaths"], 1);
        assert_eq!(payload["participants"][0]["assists"], 3);
        assert_eq!(payload["participants"][0]["items"][0]["id"], 1056);
        assert_eq!(payload["participants"][0]["riot_id"], "Me#KR1");
        assert_eq!(payload["participants"][0]["position"], "MIDDLE");
        assert_eq!(payload["participants"][0]["items"][0]["slot"], 0);
    }

    #[test]
    fn live_game_payload_includes_kill_and_objective_events() {
        let payload = live_game_payload(
            &json!({
                "gameData": {"gameId": 42, "gameTime": 120.5},
                "allPlayers": [{"summonerName": "Me", "team": "ORDER"}]
            }),
            Some(&json!({"Events": [
                {"EventID": 1, "EventName": "ChampionKill", "EventTime": 61.2, "KillerName": "Me", "VictimName": "Enemy", "MultiKill": 2, "Assisters": ["Ally"]},
                {"EventID": 2, "EventName": "DragonKill", "EventTime": 90.0, "DragonType": "EarthDragon"}
            ]})),
        )
        .unwrap();
        assert_eq!(payload["events"][0]["killer_name"], "Me");
        assert_eq!(payload["events"][0]["victim_name"], "Enemy");
        assert_eq!(payload["events"][0]["multi_kill"], 2);
        assert_eq!(payload["events"][0]["raw"]["EventName"], "ChampionKill");
        assert_eq!(payload["events"][1]["dragon_type"], "EarthDragon");
    }

    #[test]
    fn spectator_live_game_payload_does_not_require_active_player() {
        let payload = live_game_payload(
            &json!({
                "gameData": {"gameId": 84, "gameMode": "CLASSIC", "gameTime": 300.0},
                "allPlayers": [
                    {"summonerName": "Blue", "championName": "Ahri", "team": "ORDER"},
                    {"summonerName": "Red", "championName": "Garen", "team": "CHAOS"}
                ]
            }),
            None,
        )
        .unwrap();

        assert_eq!(payload["game"]["id"], 84);
        assert!(payload["active_player"].is_null());
        assert_eq!(payload["participants"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn live_game_fingerprint_ignores_capture_timestamp() {
        let first = json!({"captured_at_ms": 1, "game": {"id": 42}});
        let second = json!({"captured_at_ms": 2, "game": {"id": 42}});
        assert_eq!(
            live_game_fingerprint(&first),
            live_game_fingerprint(&second)
        );
    }
}
