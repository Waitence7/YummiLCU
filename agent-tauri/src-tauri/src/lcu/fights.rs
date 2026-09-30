use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{header, Client, StatusCode};
use serde_json::{json, Map, Value};
use tokio::{
    sync::Mutex,
    time::{sleep, Instant},
};
use tokio_util::io::ReaderStream;
use url::Url;
use uuid::Uuid;

use crate::config::Config;

const FIGHT_CACHE_SCHEMA: u64 = 1;
const MAX_FIGHT_CACHE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FIGHT_CACHE_MATCHES: usize = 200;
const MAX_ANALYZE_FILE_BYTES: u64 = 128 * 1024 * 1024;
const ANALYSIS_POLL_INTERVAL: Duration = Duration::from_millis(1_500);
const ANALYSIS_DEADLINE: Duration = Duration::from_secs(20 * 60);

static FIGHT_CACHE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn cache_lock() -> &'static Mutex<()> {
    FIGHT_CACHE_LOCK.get_or_init(|| Mutex::new(()))
}

fn fight_cache_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("YummiAgent")
        .join("match-fights-cache.json")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

async fn read_fight_cache(path: &Path) -> Result<Option<Value>, String> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("한타 캐시 상태 확인 실패: {error}")),
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_FIGHT_CACHE_BYTES
    {
        return Ok(None);
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| format!("한타 캐시 읽기 실패: {error}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("한타 캐시 JSON 오류: {error}"))?;
    if value.get("schema").and_then(Value::as_u64) != Some(FIGHT_CACHE_SCHEMA)
        || value.get("entries").and_then(Value::as_object).is_none()
    {
        return Ok(None);
    }
    Ok(Some(value))
}

fn cached_entry_response(entry: &Value) -> Option<Value> {
    let fights = entry.get("fights")?.as_array()?;
    Some(json!({
        "status": "ready",
        "fights": fights,
        "analyzedAt": entry.get("analyzedAt").cloned().unwrap_or(Value::Null),
        "method": entry.get("method").cloned().unwrap_or(Value::Null),
        "cached": true,
    }))
}

pub(crate) async fn load_cached_match_fights(game_id: &str) -> Result<Option<Value>, String> {
    let Some(cache) = read_fight_cache(&fight_cache_path()).await? else {
        return Ok(None);
    };
    Ok(cache
        .get("entries")
        .and_then(Value::as_object)
        .and_then(|entries| entries.get(game_id))
        .and_then(cached_entry_response))
}

fn merge_fights_into_history(page: &mut Value, cache: &Value) {
    let Some(entries) = cache.get("entries").and_then(Value::as_object) else {
        return;
    };
    let Some(matches) = page.get_mut("matches").and_then(Value::as_array_mut) else {
        return;
    };
    for entry in matches {
        let Some(game_id) = entry.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let Some(cached) = entries.get(&game_id) else {
            continue;
        };
        let Some(object) = entry.as_object_mut() else {
            continue;
        };
        if let Some(fights) = cached.get("fights") {
            object.insert("fights".into(), fights.clone());
        }
        if let Some(analyzed_at) = cached.get("analyzedAt") {
            object.insert("fightsAnalyzedAt".into(), analyzed_at.clone());
        }
        if let Some(method) = cached.get("method") {
            object.insert("fightAnalysisMethod".into(), method.clone());
        }
    }
}

pub(crate) async fn attach_cached_match_fights(page: &mut Value) -> Result<(), String> {
    let Some(cache) = read_fight_cache(&fight_cache_path()).await? else {
        return Ok(());
    };
    merge_fights_into_history(page, &cache);
    Ok(())
}

pub(crate) async fn save_cached_match_fights(
    game_id: &str,
    analysis: &Value,
) -> Result<Value, String> {
    let fights = analysis
        .get("fights")
        .and_then(Value::as_array)
        .ok_or_else(|| "ROFL 한타 분석 응답에 fights가 없습니다.".to_owned())?
        .clone();
    let method = analysis
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("champion_damage_cluster_v2")
        .to_owned();
    let analyzed_at = now_ms();

    let _guard = cache_lock().lock().await;
    let path = fight_cache_path();
    let mut cache = read_fight_cache(&path)
        .await?
        .unwrap_or_else(|| json!({"schema": FIGHT_CACHE_SCHEMA, "entries": {}}));

    if cache.get("entries").and_then(Value::as_object).is_none() {
        cache["entries"] = Value::Object(Map::new());
    }
    let entries = cache
        .get_mut("entries")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "한타 캐시 구조 오류".to_owned())?;
    entries.insert(
        game_id.to_owned(),
        json!({
            "fights": fights,
            "analyzedAt": analyzed_at,
            "method": method.clone(),
        }),
    );

    if entries.len() > MAX_FIGHT_CACHE_MATCHES {
        let mut ages = entries
            .iter()
            .map(|(id, value)| {
                (
                    id.clone(),
                    value.get("analyzedAt").and_then(Value::as_u64).unwrap_or(0),
                )
            })
            .collect::<Vec<_>>();
        ages.sort_by_key(|(_, analyzed_at)| *analyzed_at);
        let remove_count = ages.len().saturating_sub(MAX_FIGHT_CACHE_MATCHES);
        for (id, _) in ages.into_iter().take(remove_count) {
            entries.remove(&id);
        }
    }

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("한타 캐시 폴더 생성 실패: {error}"))?;
    }
    let bytes =
        serde_json::to_vec(&cache).map_err(|error| format!("한타 캐시 직렬화 실패: {error}"))?;
    if bytes.len() as u64 > MAX_FIGHT_CACHE_BYTES {
        return Err("한타 캐시 크기가 허용 범위를 벗어났습니다.".into());
    }
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    tokio::fs::write(&temp, bytes)
        .await
        .map_err(|error| format!("한타 캐시 쓰기 실패: {error}"))?;
    match tokio::fs::remove_file(&path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(format!("기존 한타 캐시 삭제 실패: {error}"));
        }
    }
    tokio::fs::rename(&temp, &path)
        .await
        .map_err(|error| format!("한타 캐시 교체 실패: {error}"))?;

    Ok(json!({
        "status": "ready",
        "fights": analysis.get("fights").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
        "analyzedAt": analyzed_at,
        "method": method,
        "cached": false,
    }))
}

fn endpoint(config: &Config, path: &str) -> Result<Url, String> {
    config.validate().map_err(|error| error.to_string())?;
    let mut url = Url::parse(config.relay_public_base_url.trim())
        .map_err(|_| "ROFL 분석 서버 주소가 올바르지 않습니다.".to_owned())?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn safe_server_error(value: &Value) -> String {
    value
        .get("error")
        .or_else(|| value.get("stage"))
        .and_then(Value::as_str)
        .map(|value| value.chars().take(300).collect::<String>())
        .unwrap_or_else(|| "ROFL 서버 분석 실패".into())
}

async fn stable_replay_size(path: &Path) -> Result<u64, String> {
    let first = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| format!("ROFL 파일을 찾지 못했습니다: {error}"))?;
    if !first.is_file() || first.file_type().is_symlink() {
        return Err("ROFL 파일 형식이 올바르지 않습니다.".into());
    }
    if first.len() == 0 || first.len() > MAX_ANALYZE_FILE_BYTES {
        return Err("ROFL 파일 크기가 분석 허용 범위를 벗어났습니다.".into());
    }
    sleep(Duration::from_millis(500)).await;
    let second = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| format!("ROFL 파일 상태를 다시 확인하지 못했습니다: {error}"))?;
    if second.len() != first.len() {
        return Err("ROFL 파일이 아직 저장 중입니다. 잠시 후 다시 시도하세요.".into());
    }
    Ok(second.len())
}

pub(crate) async fn analyze_replay_fights(
    config: &Config,
    game_id: &str,
    replay_path: &Path,
) -> Result<Value, String> {
    let file_size = stable_replay_size(replay_path).await?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(5 * 60))
        .build()
        .map_err(|error| format!("ROFL 분석용 HTTP 클라이언트 생성 실패: {error}"))?;

    let analyze_url = endpoint(config, "/api/rofl/analyze")?;
    let file = tokio::fs::File::open(replay_path)
        .await
        .map_err(|error| format!("ROFL 파일을 열 수 없습니다: {error}"))?;
    let body = reqwest::Body::wrap_stream(ReaderStream::new(file));
    let submit = client
        .post(analyze_url)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header("x-rofl-filename", format!("KR-{game_id}.rofl"))
        .body(body)
        .send()
        .await
        .map_err(|error| format!("ROFL 분석 서버 업로드 실패: {error}"))?;
    let submit_status = submit.status();
    let submit_json: Value = submit
        .json()
        .await
        .map_err(|error| format!("ROFL 분석 서버 응답을 읽지 못했습니다: {error}"))?;
    if submit_status != StatusCode::OK && submit_status != StatusCode::ACCEPTED {
        return Err(safe_server_error(&submit_json));
    }

    let job_id = submit_json
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "ROFL 분석 작업 ID가 없습니다.".to_owned())?;
    Uuid::parse_str(job_id)
        .map_err(|_| "ROFL 분석 작업 ID 형식이 올바르지 않습니다.".to_owned())?;

    let job_path = format!("/api/rofl/jobs/{job_id}");
    let fights_path = format!("/api/rofl/jobs/{job_id}/fights");
    let job_url = endpoint(config, &job_path)?;
    let fights_url = endpoint(config, &fights_path)?;
    let deadline = Instant::now() + ANALYSIS_DEADLINE;

    loop {
        let status = client
            .get(job_url.clone())
            .send()
            .await
            .map_err(|error| format!("ROFL 분석 상태 조회 실패: {error}"))?;
        if !status.status().is_success() {
            return Err(format!(
                "ROFL 분석 상태 조회 실패: HTTP {}",
                status.status()
            ));
        }
        let job: Value = status
            .json()
            .await
            .map_err(|error| format!("ROFL 분석 상태 응답을 읽지 못했습니다: {error}"))?;
        match job.get("status").and_then(Value::as_str) {
            Some("done") => {
                let response = client
                    .get(fights_url.clone())
                    .send()
                    .await
                    .map_err(|error| format!("한타 분석 결과 조회 실패: {error}"))?;
                if !response.status().is_success() {
                    return Err(format!(
                        "한타 분석 결과 조회 실패: HTTP {}",
                        response.status()
                    ));
                }
                let analysis: Value = response
                    .json()
                    .await
                    .map_err(|error| format!("한타 분석 결과를 읽지 못했습니다: {error}"))?;
                if analysis.get("complete").and_then(Value::as_bool) == Some(false) {
                    return Err(
                        "ROFL 전체 해독이 끝나지 않아 한타 결과를 확정할 수 없습니다.".into(),
                    );
                }
                return Ok(analysis);
            }
            Some("failed") => return Err(safe_server_error(&job)),
            _ => {}
        }

        if Instant::now() >= deadline {
            return Err("ROFL 한타 분석 시간이 초과되었습니다. 잠시 후 다시 시도하세요.".into());
        }
        sleep(ANALYSIS_POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_merge_attaches_only_matching_fights() {
        let mut page = json!({
            "matches": [
                {"id": "100", "players": []},
                {"id": "200", "players": []}
            ]
        });
        let cache = json!({
            "schema": FIGHT_CACHE_SCHEMA,
            "entries": {
                "100": {
                    "fights": [{"id": "fight-1"}],
                    "analyzedAt": 123,
                    "method": "champion_damage_cluster_v2"
                },
                "999": {
                    "fights": [{"id": "wrong"}],
                    "analyzedAt": 999
                }
            }
        });
        merge_fights_into_history(&mut page, &cache);
        assert_eq!(page["matches"][0]["fights"][0]["id"], "fight-1");
        assert_eq!(page["matches"][0]["fightsAnalyzedAt"], 123);
        assert!(page["matches"][1].get("fights").is_none());
    }

    #[test]
    fn endpoint_keeps_analysis_on_configured_origin() {
        let config = Config {
            relay_public_base_url: "https://relay.example:9443".into(),
            ..Config::default()
        };
        let url = endpoint(&config, "/api/rofl/analyze").unwrap();
        assert_eq!(url.as_str(), "https://relay.example:9443/api/rofl/analyze");
    }

    #[test]
    fn cached_response_requires_a_fights_array() {
        assert!(cached_entry_response(&json!({"fights": [], "analyzedAt": 1})).is_some());
        assert!(cached_entry_response(&json!({"analyzedAt": 1})).is_none());
    }

    #[test]
    fn fight_cache_pruning_order_is_stable() {
        let entries = (0..5)
            .map(|index| (index.to_string(), json!({"analyzedAt": index})))
            .collect::<std::collections::HashMap<_, _>>();
        let mut ages = entries
            .iter()
            .map(|(id, value)| (id.clone(), value["analyzedAt"].as_u64().unwrap_or(0)))
            .collect::<Vec<_>>();
        ages.sort_by_key(|(_, analyzed_at)| *analyzed_at);
        assert_eq!(ages[0].0, "0");
    }
}
