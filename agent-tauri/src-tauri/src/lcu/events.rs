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
}

impl LcuEventPoller {
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
        let Some(path) = lockfile_path(config) else {
            return Vec::new();
        };
        let Ok(client) = LcuClient::from_lockfile(&path) else {
            return Vec::new();
        };

        let Ok(phase_value) = client.request(Method::GET, GAMEFLOW_PHASE, None).await else {
            return Vec::new();
        };
        let Some(phase) = phase_value.as_str().map(str::to_owned) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        let previous_phase = self.gameflow.clone();
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

        if phase == "InProgress" {
            let should_poll = self
                .last_live_game_poll
                .is_none_or(|last| last.elapsed() >= LIVE_GAME_POLL_INTERVAL);
            if should_poll {
                self.last_live_game_poll = Some(Instant::now());
                if let Ok(value) = client.request(Method::GET, LIVE_GAME_DATA, None).await {
                    if let Some(payload) = live_game_payload(&value) {
                        push_changed(
                            &mut self.live_game,
                            live_game_fingerprint(&payload),
                            "live_game_update",
                            payload,
                            &mut events,
                        );
                    }
                }
            }
        } else {
            self.live_game = None;
            self.last_live_game_poll = None;
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

fn live_game_payload(value: &Value) -> Option<Value> {
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
            "mode": game_data.get("gameMode").cloned().unwrap_or(Value::Null),
            "map": game_data.get("mapName").cloned().unwrap_or(Value::Null),
            "map_number": game_data.get("mapNumber").cloned().unwrap_or(Value::Null),
            "terrain": game_data.get("mapTerrain").cloned().unwrap_or(Value::Null),
            "time_seconds": game_data.get("gameTime").cloned().unwrap_or(Value::Null),
        },
        "active_player": active_player_payload(value.get("activePlayer")),
        "participants": participants,
    }))
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
                "name": item.get("displayName").or_else(|| item.get("name")).cloned().unwrap_or(Value::Null),
                "count": item.get("count").cloned().unwrap_or(Value::from(1)),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "summoner_name": player.get("summonerName").cloned().unwrap_or(Value::Null),
        "champion_name": player.get("championName").cloned().unwrap_or(Value::Null),
        "team": player.get("team").cloned().unwrap_or(Value::Null),
        "position": player.get("position").cloned().unwrap_or(Value::Null),
        "is_bot": player.get("isBot").cloned().unwrap_or(Value::Bool(false)),
        "is_dead": player.get("isDead").cloned().unwrap_or(Value::Bool(false)),
        "level": player.get("level").cloned().unwrap_or(Value::Null),
        "gold": player.get("gold").or_else(|| player.get("currentGold")).cloned().unwrap_or(Value::Null),
        "kills": scores.and_then(|value| value.get("kills")).cloned().unwrap_or(Value::from(0)),
        "deaths": scores.and_then(|value| value.get("deaths")).cloned().unwrap_or(Value::from(0)),
        "assists": scores.and_then(|value| value.get("assists")).cloned().unwrap_or(Value::from(0)),
        "creep_score": scores.and_then(|value| value.get("creepScore")).cloned().unwrap_or(Value::from(0)),
        "ward_score": scores.and_then(|value| value.get("wardScore")).cloned().unwrap_or(Value::from(0)),
        "items": items,
        "summoner_spells": player.get("summonerSpells").cloned().unwrap_or(Value::Null),
    })
}

fn active_player_payload(value: Option<&Value>) -> Value {
    let Some(player) = value else {
        return Value::Null;
    };
    let scores = player.get("scores");
    json!({
        "summoner_name": player.get("summonerName").cloned().unwrap_or(Value::Null),
        "level": player.get("level").cloned().unwrap_or(Value::Null),
        "current_gold": player.get("currentGold").cloned().unwrap_or(Value::Null),
        "kills": scores.and_then(|value| value.get("kills")).cloned().unwrap_or(Value::from(0)),
        "deaths": scores.and_then(|value| value.get("deaths")).cloned().unwrap_or(Value::from(0)),
        "assists": scores.and_then(|value| value.get("assists")).cloned().unwrap_or(Value::from(0)),
        "creep_score": scores.and_then(|value| value.get("creepScore")).cloned().unwrap_or(Value::from(0)),
        "champion_stats": player.get("championStats").cloned().unwrap_or(Value::Null),
        "summoner_spells": player.get("summonerSpells").cloned().unwrap_or(Value::Null),
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
                "summonerName": "Me", "championName": "Ahri", "team": "ORDER",
                "scores": {"kills": 2, "deaths": 1, "assists": 3, "creepScore": 80},
                "items": [{"itemID": 1056, "displayName": "Doran's Ring", "count": 1}]
            }]
        }))
        .unwrap();
        assert_eq!(payload["game"]["id"], 42);
        assert_eq!(payload["participants"][0]["kills"], 2);
        assert_eq!(payload["participants"][0]["deaths"], 1);
        assert_eq!(payload["participants"][0]["assists"], 3);
        assert_eq!(payload["participants"][0]["items"][0]["id"], 1056);
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
