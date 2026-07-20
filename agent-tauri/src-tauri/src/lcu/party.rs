use std::collections::HashSet;
use std::time::Duration;

use reqwest::Method;
use serde_json::{json, Value};
use tokio::time::sleep;

use crate::error::AgentResult;

use super::{actions::ActionOutcome, LcuClient};

const MAX_PARTY_RIOT_IDS: usize = 20;
const INVITE_DELAY: Duration = Duration::from_millis(250);
const LOBBY_ENDPOINT: &str = "/lol-lobby/v2/lobby";
const INVITATIONS_ENDPOINT: &str = "/lol-lobby/v2/lobby/invitations";
const FRIENDS_ENDPOINT: &str = "/lol-chat/v1/friends";

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
    pub(super) async fn invite_party_members(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        let lobby = self.request(Method::GET, LOBBY_ENDPOINT, None).await;
        if !lobby.as_ref().is_ok_and(lobby_is_open) {
            return Ok(ActionOutcome::failure("로비(파티)가 열려 있지 않습니다."));
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

        let mut in_party = self.lobby_member_keys().await?;
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
            if in_party.contains(&riot_id.key()) {
                set_status(&mut statuses, &display, "in_party");
                continue;
            }

            let Some(summoner_id) = self.resolve_summoner_id(&riot_id).await? else {
                failed += 1;
                set_status(&mut statuses, &display, "invite_failed");
                details.push(format!(
                    "{display}: 닉 조회 실패 (Riot ID 확인 또는 친구 추가)"
                ));
                continue;
            };

            if self
                .request(
                    Method::POST,
                    INVITATIONS_ENDPOINT,
                    Some(json!([{"toSummonerId": summoner_id}])),
                )
                .await
                .is_ok()
            {
                invited += 1;
                set_status(&mut statuses, &display, "invited");
            } else {
                failed += 1;
                set_status(&mut statuses, &display, "invite_failed");
                details.push(format!("{display}: 초대 실패"));
            }
            sleep(INVITE_DELAY).await;
        }

        in_party = self.lobby_member_keys().await?;
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
        let mut summary = format!(
            "파티 참가 {in_party_count}명, 초대됨 {invited_count}명, 실패 {failed_count}명"
        );
        if !details.is_empty() && details.len() <= 6 {
            summary.push('\n');
            summary.push_str(&details.join("\n"));
        }
        let data = party_data(&statuses, all_in_party);
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

    pub(super) async fn check_party_members(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        let lobby = self.request(Method::GET, LOBBY_ENDPOINT, None).await;
        if !lobby.as_ref().is_ok_and(lobby_is_open) {
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
        Ok(keys)
    }

    async fn resolve_summoner_id(&self, riot_id: &RiotId) -> AgentResult<Option<i64>> {
        if let Ok(friends) = self.request(Method::GET, FRIENDS_ENDPOINT, None).await {
            if let Some(rows) = friends.as_array() {
                for friend in rows {
                    let game_name = first_string(friend, &["gameName", "riotIdGameName"]);
                    let tag_line = first_string(
                        friend,
                        &["gameTag", "tagLine", "riotIdTagline", "riotIdTagLine"],
                    );
                    if game_name.is_some_and(|value| value.eq_ignore_ascii_case(&riot_id.game_name))
                        && tag_line
                            .is_some_and(|value| value.eq_ignore_ascii_case(&riot_id.tag_line))
                    {
                        if let Some(puuid) = friend.get("puuid").and_then(Value::as_str) {
                            if let Some(id) = self.summoner_id_by_puuid(puuid).await? {
                                return Ok(Some(id));
                            }
                        }
                    }
                }
            }
        }

        let encoded_id = encode_component(&riot_id.display());
        if let Ok(summoner) = self
            .request(
                Method::GET,
                &format!("/lol-summoner/v1/summoners?name={encoded_id}"),
                None,
            )
            .await
        {
            if let Some(id) = summoner.get("summonerId").and_then(Value::as_i64) {
                return Ok(Some(id));
            }
        }

        if let Ok(aliases) = self
            .request(
                Method::GET,
                &format!("/lol-account/v1/accounts/aliases?riotId={encoded_id}"),
                None,
            )
            .await
        {
            if let Some(rows) = aliases.as_array() {
                for alias in rows {
                    if let Some(puuid) = alias.get("puuid").and_then(Value::as_str) {
                        if let Some(id) = self.summoner_id_by_puuid(puuid).await? {
                            return Ok(Some(id));
                        }
                    }
                }
            }
        }
        Ok(None)
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
            .await;
        Ok(response
            .ok()
            .and_then(|value| value.get("summonerId").and_then(Value::as_i64)))
    }
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
