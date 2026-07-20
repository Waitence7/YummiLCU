use std::{fs, io::Read, path::Path, time::Duration};

use futures_util::StreamExt;
use reqwest::{redirect::Policy, Client, Method, RequestBuilder};
use serde_json::{json, Value};
use url::Url;

use crate::error::{AgentError, AgentResult};

const CURRENT_SUMMONER_ENDPOINT: &str = "/lol-summoner/v1/current-summoner";
const MATCH_HISTORY_ENDPOINT_PREFIX: &str = "/lol-match-history/v1/products/lol";
const OWNED_CHAMPIONS_ENDPOINT: &str = "/lol-champions/v1/owned-champions-minimal";
const MAX_LOCKFILE_BYTES: u64 = 4 * 1024;
const MAX_LCU_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const LCU_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

struct SensitiveBuffer(Vec<u8>);

impl Drop for SensitiveBuffer {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

pub(crate) struct LcuClient {
    port: u16,
    password: String,
    http: Client,
}

impl Drop for LcuClient {
    fn drop(&mut self) {
        // Replacing UTF-8 bytes with NUL preserves String invariants while clearing the secret.
        unsafe { self.password.as_bytes_mut().fill(0) };
    }
}

impl LcuClient {
    pub(crate) fn event_connection(&self) -> (u16, &str) {
        (self.port, &self.password)
    }

    pub(crate) fn from_lockfile(path: &Path) -> AgentResult<Self> {
        let metadata =
            fs::symlink_metadata(path).map_err(|_| AgentError::Lcu("lockfile 읽기 실패".into()))?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_LOCKFILE_BYTES
        {
            return Err(AgentError::Lcu("lockfile이 올바르지 않습니다.".into()));
        }
        let mut bytes = SensitiveBuffer(Vec::with_capacity(metadata.len() as usize));
        fs::File::open(path)
            .map_err(|_| AgentError::Lcu("lockfile 읽기 실패".into()))?
            .take(MAX_LOCKFILE_BYTES + 1)
            .read_to_end(&mut bytes.0)
            .map_err(|_| AgentError::Lcu("lockfile 읽기 실패".into()))?;
        if bytes.0.len() as u64 > MAX_LOCKFILE_BYTES {
            return Err(AgentError::Lcu("lockfile이 너무 큽니다.".into()));
        }
        let raw = std::str::from_utf8(&bytes.0)
            .map_err(|_| AgentError::Lcu("lockfile 문자 인코딩 오류".into()))?;
        let parts: Vec<_> = raw.trim().split(':').collect();
        if parts.len() != 5 {
            return Err(AgentError::Lcu("lockfile 형식 오류".into()));
        }
        if parts[0].is_empty() || parts[0].len() > 128 {
            return Err(AgentError::Lcu("lockfile 프로세스 이름 오류".into()));
        }
        let _process_id = parts[1]
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| AgentError::Lcu("LCU 프로세스 ID 오류".into()))?;
        let port = parts[2]
            .parse()
            .ok()
            .filter(|value: &u16| *value > 0)
            .ok_or_else(|| AgentError::Lcu("LCU 포트 오류".into()))?;
        if parts[3].is_empty()
            || parts[3].len() > 512
            || !parts[3]
                .chars()
                .all(|character| character.is_ascii_graphic())
        {
            return Err(AgentError::Lcu("LCU 인증 정보 오류".into()));
        }
        if !parts[4].eq_ignore_ascii_case("https") {
            return Err(AgentError::Lcu("LCU 프로토콜 오류".into()));
        }
        Ok(Self {
            port,
            password: parts[3].into(),
            http: Client::builder()
                .danger_accept_invalid_certs(true)
                .https_only(true)
                .redirect(Policy::none())
                .timeout(LCU_REQUEST_TIMEOUT)
                .build()
                .map_err(|_| AgentError::Lcu("LCU HTTP client 생성 실패".into()))?,
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
        let url = lcu_url(self.port, endpoint)?;
        let request = self.authenticate(self.http.request(method, url));
        let request = if let Some(value) = body {
            request.json(&value)
        } else {
            request
        };
        let response = request
            .send()
            .await
            .map_err(|_| AgentError::Lcu("LCU 요청 실패".into()))?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_LCU_RESPONSE_BYTES as u64)
        {
            return Err(AgentError::Lcu("LCU 응답이 너무 큽니다.".into()));
        }
        let mut body = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| AgentError::Lcu("LCU 응답 읽기 실패".into()))?;
            if body.len().saturating_add(chunk.len()) > MAX_LCU_RESPONSE_BYTES {
                return Err(AgentError::Lcu("LCU 응답이 너무 큽니다.".into()));
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(AgentError::Lcu(format!("LCU 요청 실패 (HTTP {status})")));
        }
        if body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&body).map_err(|_| AgentError::Lcu("LCU 응답 형식 오류".into()))
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

fn lcu_url(port: u16, endpoint: &str) -> AgentResult<Url> {
    if !endpoint.starts_with('/')
        || endpoint.starts_with("//")
        || endpoint.contains('\\')
        || endpoint.contains('#')
    {
        return Err(AgentError::Lcu("허용되지 않은 LCU endpoint".into()));
    }
    let url = Url::parse(&format!("https://127.0.0.1:{port}{endpoint}"))
        .map_err(|_| AgentError::Lcu("LCU endpoint 오류".into()))?;
    if url.scheme() != "https" || url.host_str().is_none_or(|host| host != "127.0.0.1") {
        return Err(AgentError::Lcu("LCU loopback 검증 실패".into()));
    }
    Ok(url)
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

    #[test]
    fn lockfile_rejects_invalid_port_and_protocol_without_leaking_password() {
        for contents in [
            "LeagueClientUx:123:0:super-secret:https",
            "LeagueClientUx:123:2999:super-secret:http",
        ] {
            let path = std::env::temp_dir().join(format!("yummi-{}.lock", Uuid::new_v4()));
            fs::write(&path, contents).unwrap();
            let error = LcuClient::from_lockfile(&path).err().unwrap().to_string();
            assert!(!error.contains("super-secret"));
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn oversized_lockfile_is_rejected() {
        let path = std::env::temp_dir().join(format!("yummi-{}.lock", Uuid::new_v4()));
        fs::write(&path, vec![b'x'; MAX_LOCKFILE_BYTES as usize + 1]).unwrap();
        assert!(LcuClient::from_lockfile(&path).is_err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn lcu_urls_are_fixed_to_https_loopback() {
        let url = lcu_url(2999, CURRENT_SUMMONER_ENDPOINT).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert!(lcu_url(2999, "https://example.test/steal").is_err());
        assert!(lcu_url(2999, "//example.test/steal").is_err());
    }
}
