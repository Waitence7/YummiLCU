use std::{fs, path::Path};

use reqwest::{Client, Method, RequestBuilder};
use serde_json::{json, Value};

use crate::error::{AgentError, AgentResult};

const CURRENT_SUMMONER_ENDPOINT: &str = "/lol-summoner/v1/current-summoner";
const MATCH_HISTORY_ENDPOINT_PREFIX: &str = "/lol-match-history/v1/products/lol";
const OWNED_CHAMPIONS_ENDPOINT: &str = "/lol-champions/v1/owned-champions-minimal";

#[derive(Clone)]
pub(crate) struct LcuClient {
    port: u16,
    password: String,
    http: Client,
}

impl LcuClient {
    pub(crate) fn event_connection(&self) -> (u16, &str) {
        (self.port, &self.password)
    }

    pub(crate) fn from_lockfile(path: &Path) -> AgentResult<Self> {
        let raw =
            fs::read_to_string(path).map_err(|_| AgentError::Lcu("lockfile 읽기 실패".into()))?;
        let parts: Vec<_> = raw.trim().split(':').collect();
        if parts.len() < 4 {
            return Err(AgentError::Lcu("lockfile 형식 오류".into()));
        }
        let port = parts[2]
            .parse()
            .map_err(|_| AgentError::Lcu("LCU 포트 오류".into()))?;
        Ok(Self {
            port,
            password: parts[3].into(),
            http: Client::builder()
                .danger_accept_invalid_certs(true)
                .build()?,
        })
    }

    fn authenticate(&self, request: RequestBuilder) -> RequestBuilder {
        request.basic_auth("riot", Some(&self.password))
    }

    pub(super) async fn request(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Value>,
    ) -> AgentResult<Value> {
        let url = format!("https://127.0.0.1:{}{}", self.port, endpoint);
        let request = self.authenticate(self.http.request(method, &url));
        let request = if let Some(value) = body {
            request.json(&value)
        } else {
            request
        };
        let response = request.send().await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(AgentError::Lcu(format!(
                "LCU {endpoint} ({status}): {}",
                text.chars().take(180).collect::<String>()
            )));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub(crate) async fn probe_logged_in(&self) -> AgentResult<()> {
        self.request(Method::GET, CURRENT_SUMMONER_ENDPOINT, None)
            .await
            .map(|_| ())
    }

    pub(crate) async fn recent_match(&self) -> AgentResult<Value> {
        let summoner = self
            .request(Method::GET, CURRENT_SUMMONER_ENDPOINT, None)
            .await?;
        let puuid = summoner
            .get("puuid")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::Lcu("소환사 PUUID를 찾지 못했습니다.".into()))?;
        let matches = self
            .request(
                Method::GET,
                &format!("{MATCH_HISTORY_ENDPOINT_PREFIX}/{puuid}/matches?begIndex=0&endIndex=1"),
                None,
            )
            .await?;
        let game = matches
            .pointer("/games/games/0")
            .cloned()
            .ok_or_else(|| AgentError::Lcu("최근 경기가 없습니다.".into()))?;
        let participant_id = game
            .get("participantIdentities")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row.pointer("/player/puuid").and_then(Value::as_str) == Some(puuid))
            })
            .and_then(|row| row.get("participantId"))
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                AgentError::Lcu("최근 경기에서 내 참가자 정보를 찾지 못했습니다.".into())
            })?;
        let participant = game
            .get("participants")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter().find(|row| {
                    row.get("participantId").and_then(Value::as_i64) == Some(participant_id)
                })
            })
            .cloned()
            .ok_or_else(|| AgentError::Lcu("최근 경기 상세 정보를 찾지 못했습니다.".into()))?;
        let stats = participant.get("stats").cloned().unwrap_or(Value::Null);
        let champion_id = participant
            .get("championId")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let champions = self
            .request(Method::GET, OWNED_CHAMPIONS_ENDPOINT, None)
            .await
            .unwrap_or(Value::Array(vec![]));
        let champion = champions
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row.get("id").and_then(Value::as_i64) == Some(champion_id))
            })
            .and_then(|row| row.get("name"))
            .cloned()
            .unwrap_or(Value::from(champion_id));
        let items = [
            "item0", "item1", "item2", "item3", "item4", "item5", "item6",
        ]
        .iter()
        .filter_map(|key| stats.get(*key).cloned())
        .collect::<Vec<_>>();

        Ok(json!({
            "champion": champion,
            "champion_id": champion_id,
            "win": stats.get("win").cloned().unwrap_or(Value::Null),
            "kills": stats.get("kills").cloned().unwrap_or(Value::Null),
            "deaths": stats.get("deaths").cloned().unwrap_or(Value::Null),
            "assists": stats.get("assists").cloned().unwrap_or(Value::Null),
            "cs": stats.get("totalMinionsKilled").cloned().unwrap_or(Value::Null),
            "gold": stats.get("goldEarned").cloned().unwrap_or(Value::Null),
            "items": items,
            "duration": game.get("gameDuration").cloned().unwrap_or(Value::Null),
            "created_at": game.get("gameCreationDate").cloned().unwrap_or(Value::Null)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn lockfile_rejects_bad_shape() {
        let path = std::env::temp_dir().join(format!("yummi-{}.lock", Uuid::new_v4()));
        fs::write(&path, "bad").unwrap();
        assert!(LcuClient::from_lockfile(&path).is_err());
        let _ = fs::remove_file(path);
    }
}
