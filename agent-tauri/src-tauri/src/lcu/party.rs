use std::collections::{HashMap, HashSet};
use std::time::Duration;

use reqwest::Method;
use serde_json::{json, Value};
use tokio::time::{sleep, timeout};

use crate::error::{AgentError, AgentResult};

use super::{actions::ActionOutcome, LcuClient};

const MAX_PARTY_RIOT_IDS: usize = 20;
const INVITE_DELAY: Duration = Duration::from_millis(250);
const LOBBY_ENDPOINT: &str = "/lol-lobby/v2/lobby";
const INVITATIONS_ENDPOINT: &str = "/lol-lobby/v2/lobby/invitations";
const FRIENDS_ENDPOINT: &str = "/lol-chat/v1/friends";
const FRIEND_REQUESTS_ENDPOINT: &str = "/lol-chat/v2/friend-requests";
const ALIAS_BATCH_ENDPOINT: &str = "/lol-summoner/v1/summoners/aliases";
const PLAYER_PARTY_ENDPOINT: &str = "/lol-lobby/v1/parties/player";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscordPartyInfo {
    pub(crate) id: String,
    pub(crate) size: Option<(u32, u32)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RiotId {
    game_name: String,
    tag_line: String,
}

impl RiotId {
    fn parse(raw: &str) -> Option<Self> {
        let value = raw.trim();
        let separator = value.rfind('#')?;
        let game_name = value[..separator].trim();
        let tag_line = value[separator + 1..].trim();
        if game_name.is_empty() || tag_line.is_empty() {
            return None;
        }
        Some(Self {
            game_name: game_name.into(),
            tag_line: tag_line.into(),
        })
    }

    fn display(&self) -> String {
        format!("{}#{}", self.game_name, self.tag_line)
    }

    fn key(&self) -> String {
        self.display().to_lowercase()
    }
}

impl LcuClient {
    pub(crate) async fn discord_party_info(&self) -> AgentResult<Option<DiscordPartyInfo>> {
        let lobby = self.request(Method::GET, LOBBY_ENDPOINT, None).await?;
        if !lobby_is_open(&lobby) {
            return Ok(None);
        }

        let party_id = first_string(&lobby, &["partyId", "id"])
            .map(str::to_owned)
            .or_else(|| {
                lobby
                    .get("localMember")
                    .and_then(|member| first_string(member, &["partyId"]))
                    .map(str::to_owned)
            });
        let party_id = if let Some(id) = party_id.filter(|id| valid_party_id(id)) {
            Some(id)
        } else {
            let party = self
                .request(Method::GET, PLAYER_PARTY_ENDPOINT, None)
                .await?;
            first_string(&party, &["partyId", "id"])
                .map(str::to_owned)
                .filter(|id| valid_party_id(id))
        };
        let Some(id) = party_id else {
            return Ok(None);
        };

        let current = lobby
            .get("members")
            .and_then(Value::as_array)
            .map(|members| members.len() as u32)
            .unwrap_or(1)
            .max(1);
        let maximum = lobby
            .pointer("/gameConfig/maxLobbySize")
            .and_then(Value::as_u64)
            .or_else(|| {
                lobby
                    .pointer("/gameConfig/maxPartySize")
                    .and_then(Value::as_u64)
            })
            .or_else(|| lobby.get("maxLobbySize").and_then(Value::as_u64))
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value >= current);

        Ok(Some(DiscordPartyInfo {
            id,
            size: maximum.map(|maximum| (current, maximum)),
        }))
    }

    pub(crate) async fn join_discord_party(&self, party_id: &str) -> AgentResult<()> {
        if !valid_party_id(party_id) {
            return Err(crate::error::AgentError::Lcu(
                "Discord 참가 정보가 올바르지 않습니다.".into(),
            ));
        }
        let encoded = encode_component(party_id);
        self.request(
            Method::POST,
            &format!("/lol-lobby/v2/party/{encoded}/join"),
            None,
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn invite_discord_requester(
        &self,
        riot_id: &str,
    ) -> AgentResult<ActionOutcome> {
        self.invite_party_members(&json!({"riot_ids": [riot_id]}))
            .await
    }

    pub(super) async fn invite_party_members(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        let lobby = match timeout(Duration::from_secs(3), self.request(Method::GET, LOBBY_ENDPOINT, None)).await {
            Ok(result) => result?,
            Err(_) => return Ok(ActionOutcome::failure("로비 조회 시간 초과")),
        };
        if !lobby_is_open(&lobby) {
            return Ok(ActionOutcome::failure("로비(파티)가 열려 있지 않습니다."));
        }
        let mut party_id = lobby_party_id(&lobby);
        if party_id.is_none() {
            if let Ok(Ok(party)) = timeout(Duration::from_secs(2), self.request(Method::GET, PLAYER_PARTY_ENDPOINT, None)).await {
                party_id = first_string(&party, &["partyId", "id"]).filter(|id| valid_party_id(id)).map(str::to_owned);
            }
        }
        if let Some(expected) = payload.get("expected_party_id") {
            if expected.as_str().filter(|id| valid_party_id(id)) != party_id.as_deref() || party_id.is_none() {
                return Ok(ActionOutcome::failure("기존 로비와 일치하지 않아 추가 초대를 중단했습니다."));
            }
        }

        let riot_ids = payload_string_array(payload, "riot_ids");
        let mut check_ids = payload_string_array(payload, "check_riot_ids");
        if check_ids.is_empty() {
            check_ids.clone_from(&riot_ids);
        }
        if riot_ids.is_empty() && check_ids.is_empty() {
            return Ok(ActionOutcome::failure("초대할 Riot ID가 없습니다."));
        }
        if riot_ids.len() > MAX_PARTY_RIOT_IDS {
            return Ok(ActionOutcome::failure(format!(
                "초대는 최대 {MAX_PARTY_RIOT_IDS}명까지 가능합니다."
            )));
        }

        let mut in_party = lobby_member_keys_from(&lobby);
        let alias_targets = invite_alias_targets(&riot_ids, &in_party);
        let alias_started = std::time::Instant::now();
        let (alias_ids, alias_status, alias_outcome) = if alias_targets.is_empty() {
            (HashMap::new(), None, "skipped")
        } else {
            match timeout(
                Duration::from_secs(3),
                self.request(Method::POST, ALIAS_BATCH_ENDPOINT, Some(alias_batch_body(&alias_targets))),
            ).await {
                Ok(Ok(response)) if response.is_array() => (
                    parse_alias_batch(&alias_targets, &response), Some(200), "received"
                ),
                Ok(Ok(_)) => (HashMap::new(), Some(200), "invalid_response"),
                Ok(Err(error)) => (HashMap::new(), lookup_http_status(&error), "failed"),
                Err(_) => (HashMap::new(), None, "timeout"),
            }
        };
        let alias_elapsed_ms = alias_started.elapsed().as_millis() as u64;
        let alias_batch = json!({
            "requested": alias_targets.len(),
            "resolved": alias_ids.len(),
            "http_status": alias_status,
            "outcome": alias_outcome,
            "elapsed_ms": alias_elapsed_ms,
        });
        let mut processed = HashSet::new();
        let mut friends: Option<Value> = None;
        let mut lookup_failed_ids = Vec::new();
        let mut lookup_results = Vec::new();
        let mut invited = 0_usize;
        let mut failed = 0_usize;
        let mut details = Vec::new();
        let mut statuses: Vec<(String, &'static str)> = Vec::new();

        for raw in &riot_ids {
            let Some(riot_id) = RiotId::parse(raw) else {
                failed += 1;
                set_status(&mut statuses, raw, "invite_failed");
                details.push(format!("{raw}: Riot ID 형식 오류"));
                continue;
            };
            let display = riot_id.display();
            if !processed.insert(riot_id.key()) {
                continue;
            }
            if in_party.contains(&riot_id.key()) {
                set_status(&mut statuses, &display, "in_party");
                continue;
            }

            let started = std::time::Instant::now();
            let lookup = if let Some(&id) = alias_ids.get(&riot_id.key()) {
                Ok(Ok((id, "aliases_batch")))
            } else if alias_status == Some(429) {
                // A batch-level rate limit must not fan out into more requests.
                Ok(Err(Some(429)))
            } else {
                timeout(Duration::from_secs(6), self.resolve_summoner_id(&riot_id, &mut friends)).await
            };
            let (summoner_id, method) = match lookup {
                Ok(Ok((id, method))) => (id, method),
                other => {
                    failed += 1;
                    set_status(&mut statuses, &display, "invite_failed");
                    let status = match other { Ok(Err(status)) => status, _ => None };
                    lookup_failed_ids.push(json!({"riot_id": display, "http_status": status, "elapsed_ms": started.elapsed().as_millis() as u64}));
                    details.push(format!("{display}: 계정 조회 실패"));
                    continue;
                }
            };

            let lookup_elapsed_ms = if method == "aliases_batch" {
                alias_elapsed_ms
            } else {
                started.elapsed().as_millis() as u64
            };
            lookup_results.push(json!({"riot_id": display, "method": method, "elapsed_ms": lookup_elapsed_ms}));
            match timeout(Duration::from_secs(4), self.request(
                    Method::POST,
                    INVITATIONS_ENDPOINT,
                    Some(json!([{"toSummonerId": summoner_id}])),
                )).await
            {
                Ok(Ok(_)) => {
                    invited += 1;
                    set_status(&mut statuses, &display, "invited");
                }
                Ok(Err(error)) => {
                    failed += 1;
                    if lookup_http_status(&error).is_some_and(|status| (400..500).contains(&status)) {
                        set_status(&mut statuses, &display, "invite_failed");
                        details.push(format!("{display}: 초대 실패"));
                    } else {
                        set_status(&mut statuses, &display, "invite_unknown");
                        details.push(format!("{display}: 초대 발송 여부 미확인"));
                    }
                }
                Err(_) => {
                    failed += 1;
                    set_status(&mut statuses, &display, "invite_unknown");
                    details.push(format!("{display}: 초대 응답 시간 초과 (발송 여부 미확인)"));
                }
            }
            sleep(INVITE_DELAY).await;
        }

        if let Ok(Ok(current)) = timeout(Duration::from_secs(3), self.lobby_member_keys()).await {
            in_party = current;
        }
        for raw in &check_ids {
            if let Some(riot_id) = RiotId::parse(raw) {
                if in_party.contains(&riot_id.key()) {
                    set_status(&mut statuses, &riot_id.display(), "in_party");
                }
            }
        }

        let all_in_party = !check_ids.is_empty()
            && check_ids.iter().all(|raw| {
                RiotId::parse(raw).is_some_and(|riot_id| in_party.contains(&riot_id.key()))
            });
        let in_party_count = statuses
            .iter()
            .filter(|(_, status)| *status == "in_party")
            .count();
        let invited_count = statuses
            .iter()
            .filter(|(_, status)| *status == "invited")
            .count();
        let failed_count = statuses
            .iter()
            .filter(|(_, status)| *status == "invite_failed")
            .count();
        let unknown_count = statuses.iter().filter(|(_, status)| *status == "invite_unknown").count();
        let mut summary = format!(
            "파티 참가 {in_party_count}명, 초대됨 {invited_count}명, 실패 {failed_count}명, 결과 미확인 {unknown_count}명"
        );
        if !details.is_empty() && details.len() <= 6 {
            summary.push('\n');
            summary.push_str(&details.join("\n"));
        }
        let mut data = party_data(&statuses, all_in_party);
        data["party_id"] = json!(party_id);
        data["lookup_failed_ids"] = json!(lookup_failed_ids);
        data["lookup_results"] = json!(lookup_results);
        data["alias_batch"] = alias_batch;
        let ok = all_in_party
            || invited > 0
            || statuses
                .iter()
                .any(|(_, status)| matches!(*status, "in_party" | "invited"));
        if !ok && failed > 0 {
            Ok(ActionOutcome::with_data(false, summary, data))
        } else {
            Ok(ActionOutcome::with_data(true, summary, data))
        }
    }

    pub(super) async fn kick_party_member(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        let summoner_id = payload
            .get("summoner_id")
            .and_then(Value::as_i64)
            .filter(|value| *value > 0)
            .ok_or_else(|| AgentError::Lcu("summoner_id가 필요합니다.".into()))?;
        self.request(
            Method::POST,
            &format!("/lol-lobby/v2/lobby/members/{summoner_id}/kick"),
            None,
        )
        .await?;
        Ok(ActionOutcome::success("파티원 강퇴 완료"))
    }

    pub(super) async fn check_party_members(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        let lobby = self.request(Method::GET, LOBBY_ENDPOINT, None).await?;
        if !lobby_is_open(&lobby) {
            return Ok(ActionOutcome::failure("로비(파티)가 열려 있지 않습니다."));
        }

        let check_ids = payload_string_array(payload, "check_riot_ids");
        if check_ids.is_empty() {
            return Ok(ActionOutcome::failure("확인할 Riot ID가 없습니다."));
        }
        if check_ids.len() > MAX_PARTY_RIOT_IDS {
            return Ok(ActionOutcome::failure(format!(
                "확인은 최대 {MAX_PARTY_RIOT_IDS}명까지 가능합니다."
            )));
        }

        let in_party = self.lobby_member_keys().await?;
        let mut statuses = Vec::new();
        for raw in &check_ids {
            if let Some(riot_id) = RiotId::parse(raw) {
                if in_party.contains(&riot_id.key()) {
                    statuses.push((riot_id.display(), "in_party"));
                }
            }
        }
        let all_in_party = check_ids
            .iter()
            .all(|raw| RiotId::parse(raw).is_some_and(|riot_id| in_party.contains(&riot_id.key())));
        Ok(ActionOutcome::with_data(
            true,
            format!("파티 참가 {}/{}명", statuses.len(), check_ids.len()),
            party_data(&statuses, all_in_party),
        ))
    }

    async fn lobby_member_keys(&self) -> AgentResult<HashSet<String>> {
        let lobby = self.request(Method::GET, LOBBY_ENDPOINT, None).await?;
        Ok(lobby_member_keys_from(&lobby))
    }

    // Read only the requested roster accounts; never return the friend's list itself.
    pub(super) async fn match_friend_presence(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        let Some(targets) = friend_presence_targets(payload) else {
            return Ok(ActionOutcome::failure("내전 계정 목록이 올바르지 않습니다."));
        };
        let response = match timeout(Duration::from_secs(3), self.request(Method::GET, FRIENDS_ENDPOINT, None)).await {
            Ok(Ok(value)) if value.is_array() => value,
            _ => return Ok(ActionOutcome::failure("친구 온라인 상태를 확인하지 못했습니다.")),
        };
        let online = online_match_friends(&targets, &response);
        let offline = match_friend_ids(&targets, &response, true);
        let known = match_friend_ids(&targets, &response, false);
        let pending = match timeout(Duration::from_secs(2), self.request(Method::GET, FRIEND_REQUESTS_ENDPOINT, None)).await {
            Ok(Ok(value)) if value.as_array().is_some_and(|rows| rows.iter().all(|row| first_string(row, &["gameName", "riotIdGameName"]).is_some() && first_string(row, &["gameTag", "tagLine", "riotIdTagline", "riotIdTagLine"]).is_some())) => Some(match_friend_ids(&targets, &value, false)),
            _ => None,
        };
        let addable: Vec<String> = match pending {
            Some(pending) if response.as_array().is_some_and(|rows| rows.iter().all(|row| first_string(row, &["gameName", "riotIdGameName"]).is_some() && first_string(row, &["gameTag", "tagLine", "riotIdTagline", "riotIdTagLine"]).is_some())) => targets.iter().filter(|id| !known.contains(*id) && !pending.contains(*id)).cloned().collect(),
            _ => Vec::new(),
        };
        Ok(ActionOutcome::with_data(true, "내전 친구 상태 확인", json!({"online_riot_ids": online, "offline_riot_ids": offline, "addable_riot_ids": addable})))
    }

    pub(super) async fn add_match_friend(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        if payload.get("confirmed").and_then(Value::as_bool) != Some(true) || payload.get("riot_ids").and_then(Value::as_array).map(Vec::len) != Some(1) {
            return Ok(ActionOutcome::failure("명시적으로 확인한 친구 요청만 보낼 수 있습니다."));
        }
        let Some(targets) = friend_presence_targets(payload).filter(|ids| ids.len() == 1) else {
            return Ok(ActionOutcome::failure("친구 추가 대상이 올바르지 않습니다."));
        };
        let Some(raw) = payload.get("riot_ids").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str) else {
            return Ok(ActionOutcome::failure("친구 추가 대상이 올바르지 않습니다."));
        };
        let Some(id) = RiotId::parse(raw) else { return Ok(ActionOutcome::failure("Riot ID 오류")); };
        for endpoint in [FRIENDS_ENDPOINT, FRIEND_REQUESTS_ENDPOINT] {
            let rows = match timeout(Duration::from_secs(2), self.request(Method::GET, endpoint, None)).await {
                Ok(Ok(value)) if value.as_array().is_some_and(|rows| rows.iter().all(|row| first_string(row, &["gameName", "riotIdGameName"]).is_some() && first_string(row, &["gameTag", "tagLine", "riotIdTagline", "riotIdTagLine"]).is_some())) => value,
                _ => return Ok(ActionOutcome::failure("친구 또는 요청 목록을 확인하지 못했습니다.")),
            };
            if !match_friend_ids(&targets, &rows, false).is_empty() {
                return Ok(ActionOutcome::failure("이미 친구이거나 대기 중인 친구 요청이 있습니다."));
            }
        }
        match timeout(Duration::from_secs(4), self.request(Method::POST, FRIEND_REQUESTS_ENDPOINT,
            Some(json!({"gameName":id.game_name,"tagLine":id.tag_line})))).await {
            Ok(Ok(_)) => Ok(ActionOutcome::success("친구 요청을 보냈습니다.")),
            _ => Ok(ActionOutcome::failure("친구 요청 결과를 확인하지 못했습니다. 롤에서 확인해 주세요.")),
        }
    }

    /// Diagnostic only: bounded name lookups, no friends, lobby changes or invitations.
    pub(super) async fn resolve_riot_identity(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        let Some(ids) = identity_lookup_ids(payload) else {
            return Ok(ActionOutcome::failure("Riot ID 1~5개가 필요합니다."));
        };
        let mut rows = Vec::with_capacity(ids.len());
        let mut resolved = 0;
        for riot_id in ids {
            let requested = riot_id.display();
            let endpoint = format!(
                "/lol-summoner/v1/summoners?name={}",
                encode_component(&requested)
            );
            let started = std::time::Instant::now();
            let mut row = match timeout(
                Duration::from_secs(4),
                self.request(Method::GET, &endpoint, None),
            )
            .await
            {
                Ok(Ok(summoner)) => {
                    let row = identity_lookup_result(&riot_id, &summoner);
                    if row["ok"].as_bool() == Some(true) {
                        resolved += 1;
                    }
                    row
                }
                Ok(Err(error)) => {
                    // Never return raw upstream bodies (which may include private fields).
                    let text = error.to_string();
                    let http_status = text.split_whitespace().find_map(|part| {
                        part.strip_prefix("http_status=")?.parse::<u16>().ok()
                    });
                    json!({"requested_riot_id": requested, "ok": false, "status": "lookup_failed", "http_status": http_status})
                }
                Err(_) => json!({"requested_riot_id": requested, "ok": false, "status": "timeout"}),
            };
            row["elapsed_ms"] = json!(started.elapsed().as_millis() as u64);
            rows.push(row);
        }
        Ok(ActionOutcome::with_data(
            true,
            format!("Riot ID 조회 {resolved}/{}", rows.len()),
            json!({"results": rows, "resolved": resolved}),
        ))
    }

    // Normal path: one name lookup. Retry only a server error; load friends lazily once.
    async fn resolve_summoner_id(&self, riot_id: &RiotId, friends: &mut Option<Value>) -> Result<(i64, &'static str), Option<u16>> {
        let endpoint = format!("/lol-summoner/v1/summoners?name={}", encode_component(&riot_id.display()));
        let mut last_status = None;
        for attempt in 0..3 {
            match timeout(Duration::from_millis(if attempt == 0 { 2000 } else { 800 }), self.request(Method::GET, &endpoint, None)).await {
                Ok(Ok(summoner)) => {
                    let name = first_string(&summoner, &["gameName", "riotIdGameName"]);
                    let tag = first_string(&summoner, &["tagLine", "riotIdTagLine", "riotIdTagline", "gameTag"]);
                    // Some older LCU responses omit identity fields; reject a contradictory ID when present.
                    let mismatch = name.zip(tag).is_some_and(|(name, tag)| format!("{name}#{tag}").to_lowercase() != riot_id.key());
                    if !mismatch {
                        if let Some(id) = summoner.get("summonerId").and_then(Value::as_i64).filter(|id| *id > 0) {
                            return Ok((id, "name"));
                        }
                    }
                    break;
                }
                Ok(Err(error)) => {
                    last_status = lookup_http_status(&error);
                    if last_status == Some(429) { return Err(last_status); }
                    if last_status != Some(500) || attempt == 2 { break; }
                    sleep(Duration::from_millis(150)).await;
                }
                Err(_) => break,
            }
        }
        if friends.is_none() {
            // Mark attempted before await so cancellation does not repeat the fetch.
            *friends = Some(Value::Null);
            if let Ok(Ok(value)) = timeout(Duration::from_millis(1200), self.request(Method::GET, FRIENDS_ENDPOINT, None)).await {
                *friends = Some(value);
            }
        }
        if let Some(rows) = friends.as_ref().and_then(Value::as_array) {
            for friend in rows {
                let name = first_string(friend, &["gameName", "riotIdGameName"]);
                let tag = first_string(friend, &["gameTag", "tagLine", "riotIdTagline", "riotIdTagLine"]);
                if name.zip(tag).is_some_and(|(name, tag)| format!("{name}#{tag}").to_lowercase() == riot_id.key()) {
                    if let Some(puuid) = first_string(friend, &["puuid"]) {
                        if let Ok(Ok(Some(id))) = timeout(Duration::from_millis(1200), self.summoner_id_by_puuid(puuid)).await {
                            if id > 0 { return Ok((id, "friends")); }
                        }
                    }
                }
            }
        }
        Err(last_status)
    }

    async fn summoner_id_by_puuid(&self, puuid: &str) -> AgentResult<Option<i64>> {
        if puuid.trim().is_empty() {
            return Ok(None);
        }
        let response = self
            .request(
                Method::GET,
                &format!(
                    "/lol-summoner/v1/summoners-by-puuid-cached/{}",
                    encode_component(puuid)
                ),
                None,
            )
            .await?;
        Ok(response.get("summonerId").and_then(Value::as_i64))
    }
}

// The batch response is not guaranteed to preserve input order or include every
// requested account. Accept only positively identified, unambiguous rows.
fn friend_presence_targets(payload: &Value) -> Option<HashSet<String>> {
    let ids = payload.get("riot_ids")?.as_array()?;
    if ids.is_empty() || ids.len() > 50 { return None; }
    ids.iter().map(|raw| {
        let raw = raw.as_str()?;
        if raw.len() > 128 || raw.matches('#').count() != 1 || raw.chars().any(char::is_control) { return None; }
        Some(RiotId::parse(raw)?.key())
    }).collect()
}

fn match_friend_ids(targets: &HashSet<String>, rows: &Value, offline_only: bool) -> Vec<String> {
    let mut ids = HashSet::new();
    for row in rows.as_array().into_iter().flatten() {
        if offline_only && row.get("availability").and_then(Value::as_str) != Some("offline") { continue; }
        let name = first_string(row, &["gameName", "riotIdGameName"]);
        let tag = first_string(row, &["gameTag", "tagLine", "riotIdTagline", "riotIdTagLine"]);
        if let Some((name,tag)) = name.zip(tag) {
            if let Some(id) = RiotId::parse(&format!("{name}#{tag}")) {
                if targets.contains(&id.key()) { ids.insert(id.key()); }
            }
        }
    }
    let mut out: Vec<_> = ids.into_iter().collect();
    out.sort();
    out
}

fn online_match_friends(targets: &HashSet<String>, friends: &Value) -> Vec<String> {
    let mut online = HashSet::new();
    for row in friends.as_array().into_iter().flatten() {
        // Mobile-only, offline, invisible, and unknown values are not proof of a
        // League desktop session. Share positive observations only.
        if !matches!(row.get("availability").and_then(Value::as_str), Some("chat" | "away" | "dnd")) { continue; }
        // League presence is required; generic Riot chat / other games are not sufficient.
        let product = first_string(row, &["product"]);
        if !product.is_some_and(|p| p.eq_ignore_ascii_case("league_of_legends") || p.eq_ignore_ascii_case("leagueoflegends")) { continue; }
        let name = first_string(row, &["gameName", "riotIdGameName"]);
        let tag = first_string(row, &["gameTag", "tagLine", "riotIdTagline", "riotIdTagLine"]);
        let Some((name, tag)) = name.zip(tag) else { continue; };
        let Some(id) = RiotId::parse(&format!("{name}#{tag}")) else { continue; };
        if targets.contains(&id.key()) { online.insert(id.key()); }
    }
    let mut values: Vec<String> = online.into_iter().collect();
    values.sort();
    values
}

fn invite_alias_targets(raw_ids: &[String], in_party: &HashSet<String>) -> Vec<RiotId> {
    let mut seen = HashSet::new();
    raw_ids.iter().filter_map(|raw| {
        let id = RiotId::parse(raw)?;
        let key = id.key();
        if in_party.contains(&key) || !seen.insert(key) { return None; }
        Some(id)
    }).collect()
}

fn alias_batch_body(targets: &[RiotId]) -> Value {
    json!(targets.iter().map(|id| json!({
        "gameName": id.game_name,
        "tagLine": id.tag_line,
    })).collect::<Vec<_>>())
}

fn parse_alias_batch(targets: &[RiotId], response: &Value) -> HashMap<String, i64> {
    let requested: HashSet<String> = targets.iter().map(RiotId::key).collect();
    let mut resolved = HashMap::new();
    let mut ambiguous = HashSet::new();
    let mut owners: HashMap<i64, String> = HashMap::new();
    let Some(rows) = response.as_array() else { return resolved; };
    for row in rows {
        let name = first_string(row, &["gameName", "riotIdGameName"]);
        let tag = first_string(row, &["tagLine", "riotIdTagLine", "riotIdTagline", "gameTag"]);
        let Some((name, tag)) = name.zip(tag) else { continue; };
        let key = format!("{name}#{tag}").to_lowercase();
        if !requested.contains(&key) { continue; }
        let Some(id) = row.get("summonerId").and_then(Value::as_i64).filter(|id| *id > 0) else { continue; };
        if let Some(previous) = resolved.insert(key.clone(), id) {
            if previous != id { ambiguous.insert(key.clone()); }
        }
        if let Some(previous_key) = owners.insert(id, key.clone()) {
            if previous_key != key {
                ambiguous.insert(previous_key);
                ambiguous.insert(key);
            }
        }
    }
    resolved.retain(|key, _| !ambiguous.contains(key));
    resolved
}

fn lobby_party_id(lobby: &Value) -> Option<String> {
    first_string(lobby, &["partyId", "id"])
        .or_else(|| lobby.get("localMember").and_then(|member| first_string(member, &["partyId"])))
        .filter(|id| valid_party_id(id))
        .map(str::to_owned)
}

fn lobby_member_keys_from(lobby: &Value) -> HashSet<String> {
    let mut keys = HashSet::new();
    if let Some(members) = lobby.get("members").and_then(Value::as_array) {
        for member in members {
            let game_name =
                first_string(member, &["riotIdGameName", "gameName", "summonerName"]);
            let tag_line = first_string(
                member,
                &["riotIdTagline", "riotIdTagLine", "gameTag", "tagLine"],
            );
            if let (Some(game_name), Some(tag_line)) = (game_name, tag_line) {
                keys.insert(
                    RiotId {
                        game_name: game_name.into(),
                        tag_line: tag_line.into(),
                    }
                    .key(),
                );
            }
        }
    }
    keys
}

fn lookup_http_status(error: &AgentError) -> Option<u16> {
    error.to_string().split_whitespace().find_map(|part| part.strip_prefix("http_status=")?.parse().ok())
}

fn identity_lookup_ids(payload: &Value) -> Option<Vec<RiotId>> {
    let values = payload.get("riot_ids")?.as_array()?;
    if values.is_empty() || values.len() > 5 {
        return None;
    }
    values
        .iter()
        .map(|value| {
            let raw = value.as_str()?;
            if raw.len() > 256 || raw.matches('#').count() != 1 || raw.chars().any(char::is_control) {
                return None;
            }
            RiotId::parse(raw)
        })
        .collect()
}

fn identity_lookup_result(requested: &RiotId, summoner: &Value) -> Value {
    let name = first_string(summoner, &["gameName", "riotIdGameName"]);
    let tag = first_string(summoner, &["tagLine", "riotIdTagLine", "riotIdTagline", "gameTag"]);
    let actual = name.zip(tag).map(|(name, tag)| format!("{name}#{tag}"));
    let puuid = first_string(summoner, &["puuid"]).filter(|value| value.len() <= 256);
    let summoner_id = summoner.get("summonerId").and_then(Value::as_i64).filter(|id| *id > 0);
    let matches = actual.as_deref().is_some_and(|id| id.to_lowercase() == requested.key());
    if !matches || puuid.is_none() || summoner_id.is_none() {
        return json!({"requested_riot_id": requested.display(), "ok": false, "status": "identity_unverified"});
    }
    json!({"requested_riot_id": requested.display(), "riot_id": actual, "puuid": puuid, "summoner_id": summoner_id, "ok": true, "status": "resolved", "http_status": 200})
}

fn valid_party_id(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn lobby_is_open(value: &Value) -> bool {
    if value.is_null() {
        return false;
    }
    let queue_id = value
        .pointer("/gameConfig/queueId")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let members = value
        .get("members")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    queue_id > 0 || members > 0
}

fn payload_string_array(payload: &Value, key: &str) -> Vec<String> {
    payload
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn first_string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn set_status(statuses: &mut Vec<(String, &'static str)>, riot_id: &str, status: &'static str) {
    if let Some(existing) = statuses
        .iter_mut()
        .find(|(value, _)| value.eq_ignore_ascii_case(riot_id))
    {
        existing.1 = status;
    } else {
        statuses.push((riot_id.into(), status));
    }
}

fn party_data(statuses: &[(String, &'static str)], all_in_party: bool) -> Value {
    json!({
        "members": statuses
            .iter()
            .map(|(riot_id, status)| json!({"riot_id": riot_id, "status": status}))
            .collect::<Vec<_>>(),
        "all_in_party": all_in_party,
    })
}

fn encode_component(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friend_presence_only_reports_requested_positive_league_accounts() {
        let targets = friend_presence_targets(&json!({"riot_ids":["A#KR1","B#KR1","C#KR1","D#KR1"]})).unwrap();
        let friends = json!([
            {"gameName":"A","gameTag":"KR1","availability":"chat","product":"league_of_legends"},
            {"gameName":"B","gameTag":"KR1","availability":"offline","product":"league_of_legends"},
            {"gameName":"C","gameTag":"KR1","availability":"mobile","product":"league_of_legends"},
            {"gameName":"D","gameTag":"KR1","availability":"chat","product":"valorant"},
            {"gameName":"Outside","gameTag":"KR1","availability":"chat","product":"league_of_legends"}
        ]);
        assert_eq!(online_match_friends(&targets, &friends), vec!["a#kr1".to_owned()]);
        assert_eq!(match_friend_ids(&targets, &friends, true), vec!["b#kr1".to_owned()]);
        assert_eq!(match_friend_ids(&targets, &friends, false), vec!["a#kr1".to_owned(),"b#kr1".to_owned(),"c#kr1".to_owned(),"d#kr1".to_owned()]);

        assert!(friend_presence_targets(&json!({"riot_ids":["A#B#C"]})).is_none());
    }

    #[test]
    fn aliases_batch_excludes_existing_and_duplicate_members_and_encodes_names_separately() {
        let raw = vec!["A#KR1".into(), "a#kr1".into(), "Already#KR1".into(), "한 글#KR2".into(), "invalid".into()];
        let existing = HashSet::from(["already#kr1".to_owned()]);
        let targets = invite_alias_targets(&raw, &existing);
        assert_eq!(alias_batch_body(&targets), json!([
            {"gameName": "A", "tagLine": "KR1"},
            {"gameName": "한 글", "tagLine": "KR2"}
        ]));
    }

    #[test]
    fn aliases_batch_matches_by_riot_id_not_array_order_and_ignores_unknown_rows() {
        let targets = [RiotId::parse("A#KR1").unwrap(), RiotId::parse("B#KR2").unwrap(), RiotId::parse("C#KR3").unwrap()];
        let response = json!([
            {"gameName": "b", "tagLine": "kr2", "summonerId": 22},
            {"gameName": "Other", "tagLine": "KR1", "summonerId": 99},
            {"gameName": "A", "tagLine": "KR1", "summonerId": 11},
            {"gameName": "C", "tagLine": "KR3", "summonerId": 0},
            {"summonerId": 33}
        ]);
        let resolved = parse_alias_batch(&targets, &response);
        assert_eq!(resolved, HashMap::from([("a#kr1".to_owned(), 11), ("b#kr2".to_owned(), 22)]));
        assert!(parse_alias_batch(&targets, &json!({"errorCode": "RPC_ERROR"})).is_empty());
    }

    #[test]
    fn aliases_batch_rejects_conflicting_identity_mappings() {
        let targets = [RiotId::parse("A#KR1").unwrap(), RiotId::parse("B#KR2").unwrap(), RiotId::parse("C#KR3").unwrap()];
        let response = json!([
            {"gameName": "A", "tagLine": "KR1", "summonerId": 11},
            {"gameName": "A", "tagLine": "KR1", "summonerId": 12},
            {"gameName": "B", "tagLine": "KR2", "summonerId": 11},
            {"gameName": "C", "tagLine": "KR3", "summonerId": 33},
            {"gameName": "c", "tagLine": "kr3", "summonerId": 33}
        ]);
        assert_eq!(parse_alias_batch(&targets, &response), HashMap::from([("c#kr3".to_owned(), 33)]));
    }

    #[test]
    fn diagnostic_identity_requires_bounded_ids_and_verified_response() {
        assert!(identity_lookup_ids(&json!({"riot_ids": ["Player#KR1"]})).is_some());
        assert!(identity_lookup_ids(&json!({"riot_ids": ["Player#KR1", 3]})).is_none());
        assert!(identity_lookup_ids(&json!({"riot_ids": vec!["Player#KR1"; 6]})).is_none());
        let target = RiotId::parse("Player#KR1").unwrap();
        let result = identity_lookup_result(&target, &json!({"gameName": "player", "tagLine": "KR1", "puuid": "sample", "summonerId": 42, "privateField": "do not return"}));
        assert_eq!(result["ok"], true);
        assert_eq!(result["puuid"], "sample");
        assert!(result.get("privateField").is_none());
        let wrong = identity_lookup_result(&target, &json!({"gameName": "Other", "tagLine": "KR1", "puuid": "sample", "summonerId": 42}));
        assert_eq!(wrong["ok"], false);
        assert!(wrong.get("puuid").is_none());
    }

    #[test]
    fn riot_id_uses_last_hash_and_normalized_key() {
        let riot_id = RiotId::parse("  Game#Name#KR1  ").unwrap();
        assert_eq!(riot_id.game_name, "Game#Name");
        assert_eq!(riot_id.tag_line, "KR1");
        assert_eq!(riot_id.key(), "game#name#kr1");
        assert!(RiotId::parse("missing-tag").is_none());
    }

    #[test]
    fn party_result_keeps_relay_member_schema() {
        let data = party_data(&[("Player#KR1".into(), "invited")], false);
        assert_eq!(data["members"][0]["riot_id"], "Player#KR1");
        assert_eq!(data["members"][0]["status"], "invited");
        assert_eq!(data["all_in_party"], false);
    }

    #[test]
    fn invite_payload_is_limited_and_trimmed() {
        let payload = json!({"riot_ids": [" Player#KR1 ", "", 7]});
        assert_eq!(payload_string_array(&payload, "riot_ids"), ["Player#KR1"]);
        assert_eq!(MAX_PARTY_RIOT_IDS, 20);
    }
}
