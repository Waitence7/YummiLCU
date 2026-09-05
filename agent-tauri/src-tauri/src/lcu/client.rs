use std::{fs, io::Read, path::Path, time::Duration};

use futures_util::StreamExt;
use reqwest::{redirect::Policy, Client, Method, RequestBuilder};
use serde_json::{json, Value};
use url::Url;

use crate::error::{AgentError, AgentResult};

const CURRENT_SUMMONER_ENDPOINT: &str = "/lol-summoner/v1/current-summoner";
const GAMEFLOW_PHASE_ENDPOINT: &str = "/lol-gameflow/v1/gameflow-phase";
const GAMEFLOW_SESSION_ENDPOINT: &str = "/lol-gameflow/v1/session";
const CHAMPION_SUMMARY_ENDPOINT: &str = "/lol-game-data/assets/v1/champion-summary.json";
const MATCH_HISTORY_ENDPOINT_PREFIX: &str = "/lol-match-history/v1/products/lol";
const OWNED_CHAMPIONS_ENDPOINT: &str = "/lol-champions/v1/owned-champions-minimal";
const MAX_LOCKFILE_BYTES: u64 = 4 * 1024;
const MAX_LCU_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const LCU_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const LIVE_CLIENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

struct SensitiveBuffer(Vec<u8>);

impl Drop for SensitiveBuffer {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LcuIdentity {
    pub(crate) process_id: u32,
    pub(crate) port: u16,
}

pub(crate) struct LcuClient {
    process_id: u32,
    port: u16,
    password: String,
    http: Client,
}

fn compact_match_history_game(game: &Value) -> Option<Value> {
    let game_participants = game
        .get("participants")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let participants = game
        .get("participantIdentities")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let participant_id = row.get("participantId").and_then(Value::as_i64)?;
                    let player = row.get("player")?;
                    let mut game_name = player
                        .get("gameName")
                        .or_else(|| player.get("riotIdGameName"))
                        .and_then(Value::as_str);
                    let mut tag_line = player
                        .get("tagLine")
                        .or_else(|| player.get("riotIdTagLine"))
                        .or_else(|| player.get("riotIdTagline"))
                        .and_then(Value::as_str);
                    if game_name.is_none() || tag_line.is_none() {
                        if let Some(summoner_name) =
                            player.get("summonerName").and_then(Value::as_str)
                        {
                            if let Some((name, tag)) = summoner_name.rsplit_once('#') {
                                if game_name.is_none() {
                                    game_name = Some(name);
                                }
                                if tag_line.is_none() {
                                    tag_line = Some(tag);
                                }
                            }
                        }
                    }
                    let game_name = game_name?.trim();
                    let tag_line = tag_line?.trim();
                    if game_name.is_empty() || tag_line.is_empty() {
                        return None;
                    }
                    let stats_row = game_participants.iter().find(|candidate| {
                        candidate.get("participantId").and_then(Value::as_i64)
                            == Some(participant_id)
                    });
                    let team_id = stats_row
                        .and_then(|candidate| candidate.get("teamId"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let won = stats_row
                        .and_then(|candidate| candidate.get("stats"))
                        .and_then(|stats| stats.get("win"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    Some(json!({
                        "gameName": game_name,
                        "tagLine": tag_line,
                        "teamId": team_id,
                        "won": won
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(json!({
        "gameId": game.get("gameId").cloned().unwrap_or(Value::Null),
        "gameCreation": game.get("gameCreation").cloned().unwrap_or(Value::Null),
        "gameCreationDate": game.get("gameCreationDate").cloned().unwrap_or(Value::Null),
        "gameDuration": game.get("gameDuration").cloned().unwrap_or(Value::Null),
        "participants": participants
    }))
}

impl std::fmt::Debug for LcuClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LcuClient")
            .field("process_id", &self.process_id)
            .field("port", &self.port)
            .field("password", &"***")
            .finish_non_exhaustive()
    }
}

impl Drop for LcuClient {
    fn drop(&mut self) {
        // Replacing UTF-8 bytes with NUL preserves String invariants while clearing the secret.
        unsafe { self.password.as_bytes_mut().fill(0) };
    }
}

impl LcuClient {
    pub(crate) const fn identity(&self) -> LcuIdentity {
        LcuIdentity {
            process_id: self.process_id,
            port: self.port,
        }
    }

    pub(crate) fn event_connection(&self) -> (u16, &str) {
        (self.port, &self.password)
    }

    pub(crate) fn from_lockfile(path: &Path) -> AgentResult<Self> {
        Self::from_lockfile_inner(path, true)
    }

    /// Compatibility path for installations where the lockfile is valid but
    /// Windows process/path identity checks cannot be completed. File shape,
    /// process label, PID/port, credentials and HTTPS protocol are still
    /// validated before any local LCU request is made.
    pub(crate) fn from_lockfile_legacy(path: &Path) -> AgentResult<Self> {
        Self::from_lockfile_inner(path, false)
    }

    fn from_lockfile_inner(path: &Path, validate_process_identity: bool) -> AgentResult<Self> {
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
        if !valid_lcu_process_label(parts[0]) {
            return Err(AgentError::Lcu("lockfile 프로세스 이름 오류".into()));
        }
        let process_id = parts[1]
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| AgentError::Lcu("LCU 프로세스 ID 오류".into()))?;
        if validate_process_identity {
            validate_lcu_process(path, process_id)?;
        }
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
            process_id,
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
        read_json_response(request, "LCU").await
    }

    pub(crate) async fn live_game_request(endpoint: &str) -> AgentResult<Value> {
        let url = live_client_url(endpoint)?;
        let http = Client::builder()
            .danger_accept_invalid_certs(true)
            .https_only(true)
            .redirect(Policy::none())
            .timeout(LIVE_CLIENT_REQUEST_TIMEOUT)
            .build()
            .map_err(|_| AgentError::Lcu("Live Client Data HTTP client 생성 실패".into()))?;
        read_json_response(http.get(url), "Live Client Data").await
    }

    pub(crate) async fn probe_live_game() -> AgentResult<()> {
        Self::live_game_request("/liveclientdata/allgamedata")
            .await
            .map(|_| ())
    }

    pub(crate) async fn probe_logged_in(&self) -> AgentResult<()> {
        self.request(Method::GET, CURRENT_SUMMONER_ENDPOINT, None)
            .await
            .map(|_| ())
    }

    pub(crate) async fn gameflow_phase(&self) -> AgentResult<String> {
        self.request(Method::GET, GAMEFLOW_PHASE_ENDPOINT, None)
            .await?
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| AgentError::Lcu("게임 진행 상태 응답이 올바르지 않습니다.".into()))
    }

    pub(crate) async fn current_summoner(&self) -> AgentResult<Value> {
        self.request(Method::GET, CURRENT_SUMMONER_ENDPOINT, None).await
    }

    pub(crate) async fn gameflow_session(&self) -> AgentResult<Value> {
        self.request(Method::GET, GAMEFLOW_SESSION_ENDPOINT, None).await
    }

    pub(crate) async fn champion_summary(&self) -> AgentResult<Value> {
        self.request(Method::GET, CHAMPION_SUMMARY_ENDPOINT, None).await
    }

    pub(crate) async fn recent_match_verification(&self) -> AgentResult<Value> {
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
                &format!("{MATCH_HISTORY_ENDPOINT_PREFIX}/{puuid}/matches?begIndex=0&endIndex=5"),
                None,
            )
            .await?;
        let games = matches
            .pointer("/games/games")
            .and_then(Value::as_array)
            .ok_or_else(|| AgentError::Lcu("최근 경기 목록을 찾지 못했습니다.".into()))?;
        let compacted = games
            .iter()
            .take(5)
            .filter_map(compact_match_history_game)
            .collect::<Vec<_>>();
        if compacted.is_empty() {
            return Err(AgentError::Lcu("최근 경기가 없습니다.".into()));
        }

        Ok(json!({
            "latest": compacted.first().cloned().unwrap_or(Value::Null),
            "matches": compacted
        }))
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

fn valid_lcu_process_label(value: &str) -> bool {
    value.eq_ignore_ascii_case("LeagueClient")
        || value.eq_ignore_ascii_case("LeagueClient.exe")
        || value.eq_ignore_ascii_case("LeagueClientUx")
        || value.eq_ignore_ascii_case("LeagueClientUx.exe")
}

fn valid_lcu_executable_name(value: &str) -> bool {
    value.eq_ignore_ascii_case("LeagueClient.exe")
        || value.eq_ignore_ascii_case("LeagueClientUx.exe")
}

#[cfg(windows)]
fn validate_lcu_process(lockfile: &Path, process_id: u32) -> AgentResult<()> {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::CloseHandle,
            System::Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
        .map_err(|_| AgentError::Lcu("LCU 프로세스를 확인할 수 없습니다.".into()))?;
    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    let query = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(handle) };
    query.map_err(|_| AgentError::Lcu("LCU 프로세스 경로 확인 실패".into()))?;

    let executable = String::from_utf16(&buffer[..length as usize])
        .map_err(|_| AgentError::Lcu("LCU 프로세스 경로 오류".into()))?;
    let executable = std::path::PathBuf::from(executable);
    if !executable
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(valid_lcu_executable_name)
    {
        return Err(AgentError::Lcu(
            "lockfile PID와 프로세스 이름이 일치하지 않습니다.".into(),
        ));
    }

    let process_dir = executable
        .parent()
        .and_then(|path| path.canonicalize().ok())
        .ok_or_else(|| AgentError::Lcu("League Client 설치 경로 확인 실패".into()))?;
    let lockfile_dir = lockfile
        .parent()
        .and_then(|path| path.canonicalize().ok())
        .ok_or_else(|| AgentError::Lcu("lockfile 경로 확인 실패".into()))?;
    let same_install_dir = process_dir == lockfile_dir
        || lockfile_dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("Game"))
            && lockfile_dir
                .parent()
                .is_some_and(|parent| parent == process_dir);
    if !same_install_dir {
        return Err(AgentError::Lcu(
            "lockfile과 League Client 실행 경로가 일치하지 않습니다.".into(),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn validate_lcu_process(_: &Path, _: u32) -> AgentResult<()> {
    // Production Agent is Windows-only. Non-Windows builds are used for CI/tests,
    // where Win32 process identity cannot be verified.
    Ok(())
}

async fn read_json_response(request: RequestBuilder, service: &str) -> AgentResult<Value> {
    let response = request.send().await.map_err(|error| {
        AgentError::Lcu(format!(
            "{service} 요청 실패 ({})",
            reqwest_error_kind(&error)
        ))
    })?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_LCU_RESPONSE_BYTES as u64)
    {
        return Err(AgentError::Lcu(format!(
            "{service} 응답이 너무 큽니다. (HTTP {status})"
        )));
    }
    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|error| {
            AgentError::Lcu(format!(
                "{service} 응답 읽기 실패 (HTTP {status}, {})",
                reqwest_error_kind(&error)
            ))
        })?;
        if body.len().saturating_add(chunk.len()) > MAX_LCU_RESPONSE_BYTES {
            return Err(AgentError::Lcu(format!(
                "{service} 응답이 너무 큽니다. (HTTP {status})"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let detail = compact_lcu_error_detail(&body)
            .map(|detail| format!(", {detail}"))
            .unwrap_or_default();
        return Err(AgentError::Lcu(format!(
            "{service} 요청 실패 (HTTP {status}{detail})"
        )));
    }
    if body.is_empty() {
        return Ok(Value::Null);
    }
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| AgentError::Lcu(format!("{service} 응답 형식 오류 (HTTP {status})")))?;
    if is_lcu_error_envelope(&value) {
        let detail = compact_lcu_error_detail(&body)
            .map(|detail| format!(": {detail}"))
            .unwrap_or_default();
        return Err(AgentError::Lcu(format!(
            "{service} 요청 실패 (LCU 오류 응답){detail}"
        )));
    }
    Ok(value)
}

fn reqwest_error_kind(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_request() {
        "request"
    } else if error.is_body() {
        "body"
    } else if error.is_decode() {
        "decode"
    } else {
        "transport"
    }
}

fn compact_lcu_error_detail(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let object = value.as_object()?;
    let mut fields = Vec::new();
    for key in ["errorCode", "httpStatus", "message"] {
        let Some(value) = object.get(key) else {
            continue;
        };
        let rendered = match value {
            Value::String(value) => sanitize_lcu_error_text(value),
            Value::Number(value) => value.to_string(),
            Value::Bool(value) => value.to_string(),
            _ => continue,
        };
        if !rendered.is_empty() {
            fields.push(format!("{key}={rendered}"));
        }
    }
    (!fields.is_empty()).then(|| fields.join(" "))
}

fn sanitize_lcu_error_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}

fn is_lcu_error_envelope(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.get("errorCode").is_some()
        || object
            .get("httpStatus")
            .and_then(Value::as_u64)
            .is_some_and(|status| status >= 400)
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

fn live_client_url(endpoint: &str) -> AgentResult<Url> {
    if !matches!(
        endpoint,
        "/liveclientdata/allgamedata" | "/liveclientdata/eventdata"
    ) {
        return Err(AgentError::Lcu(
            "허용되지 않은 Live Client Data endpoint".into(),
        ));
    }
    Url::parse(&format!("https://127.0.0.1:2999{endpoint}"))
        .map_err(|_| AgentError::Lcu("Live Client Data endpoint 오류".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn recognizes_current_lcu_lockfile_process_labels() {
        assert!(valid_lcu_process_label("LeagueClient"));
        assert!(valid_lcu_process_label("LeagueClient.exe"));
        assert!(valid_lcu_process_label("LeagueClientUx"));
        assert!(valid_lcu_process_label("LeagueClientUx.exe"));
        assert!(!valid_lcu_process_label("RiotClientUx"));
        assert!(valid_lcu_executable_name("LeagueClient.exe"));
        assert!(valid_lcu_executable_name("LeagueClientUx.exe"));
        assert!(!valid_lcu_executable_name("RiotClientUx.exe"));
    }

    #[test]
    fn lockfile_rejects_bad_shape() {
        let path = std::env::temp_dir().join(format!("yummi-{}.lock", Uuid::new_v4()));
        fs::write(&path, "bad").unwrap();
        assert!(LcuClient::from_lockfile(&path).is_err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn lockfile_rejects_invalid_port_protocol_and_process_label_without_leaking_password() {
        for contents in [
            "LeagueClientUx:123:0:super-secret:https",
            "LeagueClientUx:123:2999:super-secret:http",
            "FakeClient:123:2999:super-secret:https",
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

    #[test]
    fn debug_output_redacts_lcu_password() {
        let client = LcuClient {
            process_id: 123,
            port: 4567,
            password: "super-secret".into(),
            http: Client::new(),
        };
        let rendered = format!("{client:?}");
        assert!(rendered.contains("123"));
        assert!(rendered.contains("4567"));
        assert!(!rendered.contains("super-secret"));
        assert!(rendered.contains("***"));
    }

    #[test]
    fn detects_lcu_error_envelopes_without_treating_normal_payloads_as_errors() {
        assert!(is_lcu_error_envelope(
            &json!({"errorCode":"RPC_ERROR", "message":"x"})
        ));
        assert!(is_lcu_error_envelope(&json!({"httpStatus":500})));
        assert!(!is_lcu_error_envelope(&json!({"httpStatus":204})));
        assert!(!is_lcu_error_envelope(&json!({"phase":"Lobby"})));
        assert!(!is_lcu_error_envelope(&Value::Null));
    }

    #[test]
    fn lcu_error_details_are_bounded_and_structured() {
        let detail = compact_lcu_error_detail(
            br#"{"errorCode":"RPC_ERROR","httpStatus":404,"message":"  stats   not ready  "}"#,
        )
        .unwrap();
        assert!(detail.contains("errorCode=RPC_ERROR"));
        assert!(detail.contains("httpStatus=404"));
        assert!(detail.contains("message=stats not ready"));
        assert!(sanitize_lcu_error_text(&"x".repeat(500)).len() <= 160);
    }

    #[test]
    fn live_client_urls_are_fixed_to_game_client_port() {
        let url = live_client_url("/liveclientdata/allgamedata").unwrap();
        assert_eq!(
            url.as_str(),
            "https://127.0.0.1:2999/liveclientdata/allgamedata"
        );
        assert!(live_client_url("https://example.test/steal").is_err());
        assert!(live_client_url("/lol-summoner/v1/current-summoner").is_err());
    }
}
