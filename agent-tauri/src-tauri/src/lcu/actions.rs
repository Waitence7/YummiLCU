use std::time::Duration;

use reqwest::Method;
use serde_json::{json, Value};
use tokio::time::sleep;

use crate::{
    config::Config,
    error::{AgentError, AgentResult},
    relay::protocol::Action,
};

use super::LcuClient;

const NORMAL_DRAFT_QUEUE_ID: i64 = 400;
const RANKED_SOLO_QUEUE_ID: i64 = 420;
const LOBBY_RETRY_COUNT: usize = 6;
const LOBBY_SETTLE_DELAY: Duration = Duration::from_millis(400);
const LOBBY_RETRY_DELAY: Duration = Duration::from_millis(2_500);

const CURRENT_SUMMONER: &str = "/lol-summoner/v1/current-summoner";
const MATCHMAKING_SEARCH: &str = "/lol-lobby/v2/lobby/matchmaking/search";
const LOBBY: &str = "/lol-lobby/v2/lobby";
const READY_CHECK_ACCEPT: &str = "/lol-matchmaking/v1/ready-check/accept";
const READY_CHECK_DECLINE: &str = "/lol-matchmaking/v1/ready-check/decline";
const GAMEFLOW_RECONNECT: &str = "/lol-gameflow/v1/reconnect";
const GAMEFLOW_DODGE: &str = "/lol-gameflow/v1/session/dodge";
const PARTY_READY: &str = "/lol-lobby/v1/parties/ready";
const CHAMP_REROLL: &str = "/lol-champ-select/v1/session/my-selection/reroll";
const MY_CHAMP_SELECTION: &str = "/lol-champ-select/v1/session/my-selection";
const PERK_PAGES: &str = "/lol-perks/v1/pages";
const CURRENT_PERK_PAGE: &str = "/lol-perks/v1/currentpage";
const PROCESS_QUIT: &str = "/process-control/v1/process/quit";
const CHAT_ME: &str = "/lol-chat/v1/me";

#[derive(Debug)]
pub(crate) struct ActionOutcome {
    pub(crate) ok: bool,
    pub(crate) message: String,
    pub(crate) data: Value,
}

impl ActionOutcome {
    pub(super) fn success(message: impl Into<String>) -> Self {
        Self::with_data(true, message, json!({}))
    }

    pub(super) fn failure(message: impl Into<String>) -> Self {
        Self::with_data(false, message, json!({}))
    }

    pub(super) fn with_data(ok: bool, message: impl Into<String>, data: Value) -> Self {
        Self {
            ok,
            message: message.into(),
            data,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QueueKind {
    NormalDraft,
    RankedSolo,
}

impl QueueKind {
    const fn id(self) -> i64 {
        match self {
            Self::NormalDraft => NORMAL_DRAFT_QUEUE_ID,
            Self::RankedSolo => RANKED_SOLO_QUEUE_ID,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::NormalDraft => "일반(비공개)",
            Self::RankedSolo => "솔랭",
        }
    }
}

impl LcuClient {
    async fn create_queue_lobby(
        &self,
        queue: QueueKind,
        start_search: bool,
    ) -> AgentResult<ActionOutcome> {
        for attempt in 1..=LOBBY_RETRY_COUNT {
            let _ = self.request(Method::DELETE, MATCHMAKING_SEARCH, None).await;
            let _ = self.request(Method::DELETE, LOBBY, None).await;
            if self
                .request(Method::POST, LOBBY, Some(json!({"queueId": queue.id()})))
                .await
                .is_ok()
            {
                if !start_search {
                    return Ok(ActionOutcome::success(format!(
                        "{} 로비 생성",
                        queue.label()
                    )));
                }
                sleep(LOBBY_SETTLE_DELAY).await;
                if self
                    .request(Method::POST, MATCHMAKING_SEARCH, None)
                    .await
                    .is_ok()
                {
                    return Ok(ActionOutcome::success(format!(
                        "{} 매칭 시작",
                        queue.label()
                    )));
                }
            }
            if attempt < LOBBY_RETRY_COUNT {
                sleep(LOBBY_RETRY_DELAY).await;
            }
        }
        Err(AgentError::Lcu(if start_search {
            "매칭 시작 실패".into()
        } else {
            format!("로비 생성 실패 (queue {})", queue.id())
        }))
    }

    pub(crate) async fn execute_action(
        &self,
        action: Action,
        payload: &Value,
        config: &Config,
    ) -> AgentResult<ActionOutcome> {
        match action {
            Action::CreateRankedLobby => {
                return self.create_queue_lobby(QueueKind::RankedSolo, false).await;
            }
            Action::CreateNormalLobby => {
                return self.create_queue_lobby(QueueKind::NormalDraft, false).await;
            }
            Action::PlayRankedSolo => {
                return self.create_queue_lobby(QueueKind::RankedSolo, true).await;
            }
            Action::PlayNormalDraft => {
                return self.create_queue_lobby(QueueKind::NormalDraft, true).await;
            }
            Action::Dodge => return self.dodge(config).await,
            Action::SetSummonerSpells => return self.set_summoner_spells(payload).await,
            Action::ListRunePages => return self.list_rune_pages().await,
            Action::SetRunePage => return self.set_rune_page(payload).await,
            Action::GetCurrentRunePage => return self.get_current_rune_page().await,
            Action::UpdateRunePage => return self.update_rune_page(payload).await,
            Action::ClaimAllRewards => return self.claim_all_rewards().await,
            Action::InvitePartyMembers => return self.invite_party_members(payload).await,
            Action::CheckPartyMembers => return self.check_party_members(payload).await,
            _ => {}
        }

        let (method, endpoint, body) = match action {
            Action::Ping => (Method::GET, CURRENT_SUMMONER.to_owned(), None),
            Action::AcceptMatch => (Method::POST, READY_CHECK_ACCEPT.to_owned(), None),
            Action::DeclineMatch => (Method::POST, READY_CHECK_DECLINE.to_owned(), None),
            Action::Reconnect => (Method::POST, GAMEFLOW_RECONNECT.to_owned(), None),
            Action::QueueStart => (Method::POST, MATCHMAKING_SEARCH.to_owned(), None),
            Action::QueueCancel => (Method::DELETE, MATCHMAKING_SEARCH.to_owned(), None),
            Action::LeaveLobby => (Method::DELETE, LOBBY.to_owned(), None),
            Action::PartyReady => (
                Method::PUT,
                PARTY_READY.to_owned(),
                Some(json!({"ready": true})),
            ),
            Action::ChampReroll => (Method::POST, CHAMP_REROLL.to_owned(), None),
            Action::QuitClient => (Method::POST, PROCESS_QUIT.to_owned(), None),
            Action::SetStatus => {
                let text = payload.get("text").and_then(Value::as_str).unwrap_or("");
                if text.len() > 128 {
                    return Ok(ActionOutcome::failure("상태 메시지가 너무 깁니다."));
                }
                (
                    Method::PUT,
                    CHAT_ME.to_owned(),
                    Some(json!({"statusMessage": text})),
                )
            }
            Action::ResetStatus => (
                Method::PUT,
                CHAT_ME.to_owned(),
                Some(json!({"statusMessage": "𝗬𝘂𝗺𝗺𝗶 𝗖𝗹𝗶𝗲𝗻𝘁"})),
            ),
            Action::ChampSelect => {
                let action_id = payload_i64(payload, "action_id")
                    .filter(|value| *value >= 0)
                    .ok_or_else(|| AgentError::Lcu("action_id가 필요합니다.".into()))?;
                let champion_id = payload_i64(payload, "champion_id")
                    .filter(|value| *value > 0)
                    .ok_or_else(|| AgentError::Lcu("champion_id가 필요합니다.".into()))?;
                (
                    Method::PATCH,
                    format!("/lol-champ-select/v1/session/actions/{action_id}"),
                    Some(json!({"championId": champion_id})),
                )
            }
            _ => {
                return Ok(ActionOutcome::failure(format!(
                    "action '{}'은 추가 LCU 매핑이 필요합니다.",
                    action.as_str()
                )));
            }
        };
        self.request(method, &endpoint, body).await?;
        Ok(ActionOutcome::success("완료"))
    }

    async fn dodge(&self, config: &Config) -> AgentResult<ActionOutcome> {
        if self
            .request(Method::POST, GAMEFLOW_DODGE, None)
            .await
            .is_err()
        {
            return Ok(ActionOutcome::failure("닷지 실패"));
        }
        if config.prevent_queue_after_dodge {
            let _ = self.request(Method::DELETE, MATCHMAKING_SEARCH, None).await;
            Ok(ActionOutcome::success("닷지 + 매칭 중지"))
        } else {
            Ok(ActionOutcome::success("닷지 완료"))
        }
    }

    async fn set_summoner_spells(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        let spell1_id = payload_i64(payload, "spell1_id").filter(|value| *value > 0);
        let spell2_id = payload_i64(payload, "spell2_id").filter(|value| *value > 0);
        let (Some(spell1_id), Some(spell2_id)) = (spell1_id, spell2_id) else {
            return Ok(ActionOutcome::failure("spell1_id/spell2_id가 필요합니다."));
        };
        if spell1_id == spell2_id {
            return Ok(ActionOutcome::failure("서로 다른 스펠을 선택하세요."));
        }
        self.request(
            Method::PATCH,
            MY_CHAMP_SELECTION,
            Some(json!({"spell1Id": spell1_id, "spell2Id": spell2_id})),
        )
        .await?;
        Ok(ActionOutcome::success("스펠 변경 완료"))
    }

    async fn list_rune_pages(&self) -> AgentResult<ActionOutcome> {
        let response = self.request(Method::GET, PERK_PAGES, None).await?;
        let pages = response.as_array().cloned().unwrap_or_default();
        let data = pages
            .iter()
            .filter_map(|page| {
                let id = page.get("id").and_then(Value::as_i64)?;
                (id > 0).then(|| {
                    json!({
                        "id": id,
                        "name": page.get("name").and_then(Value::as_str).unwrap_or(""),
                        "current": page.get("current").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
            })
            .take(25)
            .collect::<Vec<_>>();
        Ok(ActionOutcome::with_data(
            true,
            format!("{}개 룬 페이지", pages.len()),
            json!({"pages": data}),
        ))
    }

    async fn set_rune_page(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        let Some(page_id) = payload_i64(payload, "page_id").filter(|value| *value > 0) else {
            return Ok(ActionOutcome::failure("page_id가 필요합니다."));
        };
        self.request(Method::PUT, PERK_PAGES, Some(json!({"id": page_id})))
            .await?;
        Ok(ActionOutcome::success("룬 페이지 변경 완료"))
    }

    async fn get_current_rune_page(&self) -> AgentResult<ActionOutcome> {
        let page = self.request(Method::GET, CURRENT_PERK_PAGE, None).await?;
        let Some(id) = page
            .get("id")
            .and_then(Value::as_i64)
            .filter(|value| *value > 0)
        else {
            return Ok(ActionOutcome::failure("현재 룬 페이지를 읽지 못했습니다."));
        };
        Ok(ActionOutcome::with_data(
            true,
            "현재 룬 페이지",
            json!({
                "id": id,
                "name": page.get("name").and_then(Value::as_str).unwrap_or(""),
                "primary_style_id": page.get("primaryStyleId").and_then(Value::as_i64).unwrap_or(0),
                "sub_style_id": page.get("subStyleId").and_then(Value::as_i64).unwrap_or(0),
                "selected_perk_ids": page.get("selectedPerkIds").cloned().unwrap_or_else(|| json!([])),
                "current": page.get("current").and_then(Value::as_bool).unwrap_or(false),
            }),
        ))
    }

    async fn update_rune_page(&self, payload: &Value) -> AgentResult<ActionOutcome> {
        let Some(page_id) = payload_i64(payload, "page_id").filter(|value| *value > 0) else {
            return Ok(ActionOutcome::failure("page_id가 필요합니다."));
        };
        let primary_style_id = payload_i64(payload, "primary_style_id").filter(|value| *value > 0);
        let sub_style_id = payload_i64(payload, "sub_style_id").filter(|value| *value > 0);
        let (Some(primary_style_id), Some(sub_style_id)) = (primary_style_id, sub_style_id) else {
            return Ok(ActionOutcome::failure(
                "primary_style_id/sub_style_id가 필요합니다.",
            ));
        };
        let selected_perk_ids = payload_positive_i64_array(payload, "selected_perk_ids");
        if selected_perk_ids.len() != 9 {
            return Ok(ActionOutcome::failure(
                "selected_perk_ids는 9개여야 합니다.",
            ));
        }
        let name = payload
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Yummi");
        self.request(
            Method::PUT,
            &format!("{PERK_PAGES}/{page_id}"),
            Some(json!({
                "id": page_id,
                "name": name,
                "primaryStyleId": primary_style_id,
                "subStyleId": sub_style_id,
                "selectedPerkIds": selected_perk_ids,
                "current": true,
            })),
        )
        .await?;
        Ok(ActionOutcome::success("룬 구성 저장 완료"))
    }
}

fn payload_i64(payload: &Value, key: &str) -> Option<i64> {
    let value = payload.get(key)?;
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
}

fn payload_positive_i64_array(payload: &Value, key: &str) -> Vec<i64> {
    payload
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
        })
        .filter(|value| *value > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_types_keep_relay_compatible_ids() {
        assert_eq!(QueueKind::NormalDraft.id(), 400);
        assert_eq!(QueueKind::RankedSolo.id(), 420);
    }

    #[test]
    fn rune_payload_accepts_existing_string_ids() {
        let payload = json!({
            "page_id": "42",
            "selected_perk_ids": [1, "2", 3, 4, 5, 6, 7, 8, 9]
        });
        assert_eq!(payload_i64(&payload, "page_id"), Some(42));
        assert_eq!(
            payload_positive_i64_array(&payload, "selected_perk_ids"),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9]
        );
    }

    #[test]
    fn action_outcome_preserves_relay_data() {
        let outcome = ActionOutcome::with_data(true, "현재 룬 페이지", json!({"id": 7}));
        assert!(outcome.ok);
        assert_eq!(outcome.message, "현재 룬 페이지");
        assert_eq!(outcome.data["id"], 7);
    }
}
