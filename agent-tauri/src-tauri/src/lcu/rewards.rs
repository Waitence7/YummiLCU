use reqwest::Method;
use serde_json::{json, Value};

use crate::error::AgentResult;

use super::{actions::ActionOutcome, LcuClient};

const LOOT_NOTIFICATIONS: &str = "/lol-loot/v1/player-loot-notifications";
const MILESTONES: &str = "/lol-loot/v1/milestones";
const MISSIONS: &str = "/lol-missions/v1/missions";
const EVENTS: &str = "/lol-event-hub/v1/events";
const PLAYER_LOOT: &str = "/lol-loot/v1/player-loot";

impl LcuClient {
    pub(super) async fn claim_all_rewards(&self) -> AgentResult<ActionOutcome> {
        let mut lines = Vec::new();
        let mut claimed = 0_usize;
        claimed += self.claim_loot_notifications(&mut lines).await;
        claimed += self.claim_milestones(&mut lines).await;
        claimed += self.claim_missions(&mut lines).await;
        claimed += self.claim_event_hub(&mut lines).await;
        claimed += self.redeem_player_loot(&mut lines).await;

        if lines.is_empty() {
            return Ok(ActionOutcome::success("수령할 보상을 찾지 못했습니다."));
        }
        Ok(ActionOutcome::success(format!(
            "보상 처리 완료 (성공 {claimed}건)\n{}",
            lines.into_iter().take(12).collect::<Vec<_>>().join("\n")
        )))
    }

    async fn claim_loot_notifications(&self, lines: &mut Vec<String>) -> usize {
        let Ok(value) = self.request(Method::GET, LOOT_NOTIFICATIONS, None).await else {
            return 0;
        };
        let mut claimed = 0;
        for item in value.as_array().into_iter().flatten() {
            let Some(id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            if self
                .request(
                    Method::POST,
                    &format!("{LOOT_NOTIFICATIONS}/{}/acknowledge", encode_component(id)),
                    None,
                )
                .await
                .is_ok()
            {
                claimed += 1;
                lines.push(format!("알림: {}", truncate(id, 20)));
            }
        }
        claimed
    }

    async fn claim_milestones(&self, lines: &mut Vec<String>) -> usize {
        let Ok(value) = self.request(Method::GET, MILESTONES, None).await else {
            return 0;
        };
        let mut claimed = 0;
        for milestone in value.as_array().into_iter().flatten() {
            let Some(id) = milestone.get("id").and_then(Value::as_str) else {
                continue;
            };
            if self
                .request(
                    Method::POST,
                    &format!("{MILESTONES}/{}/claim", encode_component(id)),
                    None,
                )
                .await
                .is_ok()
            {
                claimed += 1;
                lines.push(format!("마일스톤: {id}"));
            }
        }
        claimed
    }

    async fn claim_missions(&self, lines: &mut Vec<String>) -> usize {
        let Ok(value) = self.request(Method::GET, MISSIONS, None).await else {
            return 0;
        };
        let mut claimed = 0;
        for mission in value.as_array().into_iter().flatten() {
            let status = mission.get("status").and_then(Value::as_str);
            if !matches!(status, Some("COMPLETED" | "COMPLETE")) {
                continue;
            }
            let Some(id) = mission.get("id").and_then(Value::as_str) else {
                continue;
            };
            if self
                .request(
                    Method::PUT,
                    &format!("/lol-missions/v1/player/{}", encode_component(id)),
                    Some(json!({})),
                )
                .await
                .is_ok()
            {
                claimed += 1;
                lines.push(format!("미션: {}", truncate(id, 24)));
            }
        }
        claimed
    }

    async fn claim_event_hub(&self, lines: &mut Vec<String>) -> usize {
        let Ok(value) = self.request(Method::GET, EVENTS, None).await else {
            return 0;
        };
        let mut claimed = 0;
        for event in value.as_array().into_iter().flatten() {
            let Some(id) = event.get("eventId").and_then(Value::as_str) else {
                continue;
            };
            if self
                .request(
                    Method::POST,
                    &format!("{EVENTS}/{}/reward-track/claim-all", encode_component(id)),
                    None,
                )
                .await
                .is_ok()
            {
                claimed += 1;
                lines.push(format!("이벤트: {id}"));
            }
        }
        claimed
    }

    async fn redeem_player_loot(&self, lines: &mut Vec<String>) -> usize {
        let Ok(value) = self.request(Method::GET, PLAYER_LOOT, None).await else {
            return 0;
        };
        let mut claimed = 0;
        for loot in value.as_array().into_iter().flatten() {
            if !loot_is_redeemable(loot) {
                continue;
            }
            let Some(name) = loot.get("lootName").and_then(Value::as_str) else {
                continue;
            };
            if self
                .request(
                    Method::POST,
                    &format!("{PLAYER_LOOT}/{}/redeem", encode_component(name)),
                    None,
                )
                .await
                .is_ok()
            {
                claimed += 1;
                if lines.len() < 8 {
                    lines.push(format!("루트: {name}"));
                }
            }
        }
        claimed
    }
}

fn loot_is_redeemable(loot: &Value) -> bool {
    if loot.get("redeemableStatus").and_then(Value::as_str) == Some("REDEEMABLE") {
        return true;
    }
    loot.get("isRevealed").and_then(Value::as_bool) != Some(false)
}

fn encode_component(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reward_redemption_matches_existing_agent_rules() {
        assert!(loot_is_redeemable(
            &json!({"redeemableStatus": "REDEEMABLE", "isRevealed": false})
        ));
        assert!(!loot_is_redeemable(
            &json!({"redeemableStatus": "LOCKED", "isRevealed": false})
        ));
        assert!(loot_is_redeemable(
            &json!({"redeemableStatus": "LOCKED", "isRevealed": true})
        ));
    }

    #[test]
    fn reward_identifiers_are_path_encoded() {
        assert_eq!(encode_component("mission/a b"), "mission%2Fa%20b");
        assert_eq!(truncate("가나다라마바사", 4), "가나다라");
    }
}
