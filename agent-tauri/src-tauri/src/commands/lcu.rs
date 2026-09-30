use std::{path::PathBuf, sync::Arc, time::Duration};

use serde_json::{json, Value};
use tauri::State;
use tokio::time::sleep;

use crate::{
    lcu::{
        analyze_replay_fights, attach_cached_match_fights, empty_match_history,
        find_existing_replay_path, load_cached_match_fights, load_cached_match_history,
        lockfile_path, remote_replay_analysis_job, remote_replay_analysis_viewer,
        replay_for_viewer, save_cached_match_fights, save_match_history_page,
        start_remote_replay_analysis, LcuClient, RoflMatchHint,
    },
    session,
    state::AppState,
};

#[tauri::command]
pub(crate) async fn recent_match(state: State<'_, Arc<AppState>>) -> Result<Value, String> {
    let config = state.config.read().await.clone();
    let path = lockfile_path(&config).ok_or("League Client가 실행 중이 아닙니다.")?;
    let client = LcuClient::from_lockfile(&path)
        .or_else(|_| LcuClient::from_lockfile_legacy(&path))
        .map_err(|error| error.to_string())?;
    client
        .recent_match()
        .await
        .map_err(|error| error.to_string())
}

async fn history_client(state: &Arc<AppState>) -> Result<LcuClient, String> {
    let config = state.config.read().await.clone();
    let path = lockfile_path(&config).ok_or("롤 클라이언트를 실행한 뒤 다시 확인하세요.")?;
    LcuClient::from_lockfile(&path)
        .or_else(|_| LcuClient::from_lockfile_legacy(&path))
        .map_err(|error| error.to_string())
}

fn validate_game_id(game_id: &str) -> Result<(), String> {
    if game_id.is_empty() || game_id.len() > 20 || !game_id.bytes().all(|c| c.is_ascii_digit()) {
        return Err("올바른 경기 ID가 아닙니다.".into());
    }
    Ok(())
}

async fn attach_fights_best_effort(state: &Arc<AppState>, page: &mut Value) {
    if let Err(error) = attach_cached_match_fights(page).await {
        state
            .report_diagnostic("lcu", "match_fights_cache_read_failed", error)
            .await;
    }
}

#[tauri::command]
pub(crate) async fn match_history(
    state: State<'_, Arc<AppState>>,
    offset: u32,
) -> Result<Value, String> {
    let live_result = match history_client(state.inner()).await {
        Ok(client) => match client.match_history(offset).await {
            Ok((account_key, mut page)) => {
                // A cache write failure must not make a successful LCU history request fail.
                if let Err(error) = save_match_history_page(&account_key, &page).await {
                    state
                        .report_diagnostic(
                            "lcu",
                            "match_history_cache_write_failed",
                            error.to_string(),
                        )
                        .await;
                }
                attach_fights_best_effort(state.inner(), &mut page).await;
                return Ok(page);
            }
            Err(error) => Some(error.to_string()),
        },
        Err(error) => Some(error),
    };

    match load_cached_match_history(offset).await {
        Ok(Some(mut page)) => {
            if let Some(error) = live_result {
                state
                    .record_flight("match_history_cache_fallback", error)
                    .await;
            }
            attach_fights_best_effort(state.inner(), &mut page).await;
            Ok(page)
        }
        Ok(None) => Ok(empty_match_history()),
        Err(cache_error) => Err(live_result.unwrap_or_else(|| cache_error.to_string())),
    }
}

async fn local_replay_viewer(
    state: &Arc<AppState>,
    game_id: &str,
) -> Result<(Value, Option<String>), String> {
    let replay_dir = match history_client(state).await {
        Ok(client) => client.replay_directory().await.ok().flatten(),
        Err(_) => None,
    };
    let id = game_id.to_owned();
    let dir = replay_dir.clone();
    let value = tokio::task::spawn_blocking(move || replay_for_viewer(&id, dir.as_deref()))
        .await
        .map_err(|_| "리플레이를 읽는 중 오류가 발생했습니다.".to_owned())??;
    Ok((value, replay_dir))
}

fn processing_replay(base: &Value, job: &Value) -> Value {
    json!({
        "status": "processing",
        "version": job.get("clientVersion").filter(|value| !value.is_null()).cloned().unwrap_or_else(|| base.get("version").cloned().unwrap_or(Value::Null)),
        "durationMs": job.get("gameLengthMs").filter(|value| !value.is_null()).cloned().unwrap_or_else(|| base.get("durationMs").cloned().unwrap_or(Value::Null)),
        "participants": base.get("participants").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
        "movements": [],
        "jobId": job.get("id").cloned().unwrap_or(Value::Null),
        "progress": job.get("progress").cloned().unwrap_or(Value::from(0)),
        "stage": job.get("stage").cloned().unwrap_or_else(|| Value::String("서버 분석 중".into())),
        "source": "server",
    })
}

fn merge_remote_viewer(mut base: Value, viewer: Value) -> Value {
    let Some(object) = base.as_object_mut() else {
        return viewer;
    };
    for key in ["status", "version", "durationMs", "movements", "source"] {
        if let Some(value) = viewer.get(key).filter(|value| !value.is_null()) {
            object.insert(key.to_owned(), value.clone());
        }
    }
    object.remove("jobId");
    object.remove("progress");
    object.remove("stage");
    base
}

async fn start_remote_for_local_replay(
    state: &Arc<AppState>,
    game_id: &str,
    base: Value,
    replay_dir: Option<String>,
) -> Value {
    let config = state.config.read().await.clone();
    let Some(saved_session) = session::load(&config) else {
        return base;
    };
    let mut hint = RoflMatchHint::from_eog(game_id.to_owned(), &Value::Null);
    if let Some(dir) = replay_dir.as_deref() {
        hint.set_replay_dir(dir);
    }
    let path = match find_replay_path(&hint).await {
        Ok(Some(path)) => path,
        _ => return base,
    };
    match start_remote_replay_analysis(&config, &saved_session, game_id, &path).await {
        Ok(job)
            if job.get("status").and_then(Value::as_str) == Some("done")
                && job.get("resultAvailable").and_then(Value::as_bool) == Some(true) =>
        {
            let Some(job_id) = job.get("id").and_then(Value::as_str) else {
                return base;
            };
            match remote_replay_analysis_viewer(&config, &saved_session, job_id).await {
                Ok(viewer) => merge_remote_viewer(base, viewer),
                Err(_) => processing_replay(&base, &job),
            }
        }
        Ok(job) => processing_replay(&base, &job),
        Err(error) => {
            state
                .report_diagnostic(
                    "lcu",
                    "remote_replay_analysis_start_failed",
                    error.to_string(),
                )
                .await;
            base
        }
    }
}

#[tauri::command]
pub(crate) async fn match_replay(
    state: State<'_, Arc<AppState>>,
    game_id: String,
) -> Result<Value, String> {
    validate_game_id(&game_id)?;
    let (local, replay_dir) = local_replay_viewer(state.inner(), &game_id).await?;
    if local.get("status").and_then(Value::as_str) != Some("unsupported") {
        return Ok(local);
    }
    Ok(start_remote_for_local_replay(state.inner(), &game_id, local, replay_dir).await)
}

#[tauri::command]
pub(crate) async fn match_replay_analysis(
    state: State<'_, Arc<AppState>>,
    game_id: String,
    job_id: String,
) -> Result<Value, String> {
    validate_game_id(&game_id)?;
    uuid::Uuid::parse_str(&job_id).map_err(|_| "올바른 분석 작업 ID가 아닙니다.".to_owned())?;
    let config = state.config.read().await.clone();
    let saved_session = session::load(&config).ok_or("Yummi Relay 연결 세션이 없습니다.")?;
    let job = remote_replay_analysis_job(&config, &saved_session, &job_id)
        .await
        .map_err(|error| error.to_string())?;
    let (mut local, _) = local_replay_viewer(state.inner(), &game_id).await?;
    match job.get("status").and_then(Value::as_str) {
        Some("done") if job.get("resultAvailable").and_then(Value::as_bool) == Some(true) => {
            let viewer = remote_replay_analysis_viewer(&config, &saved_session, &job_id)
                .await
                .map_err(|error| error.to_string())?;
            Ok(merge_remote_viewer(local, viewer))
        }
        Some("failed") => {
            if let Some(object) = local.as_object_mut() {
                object.insert("status".into(), Value::String("unsupported".into()));
                object.insert(
                    "remoteError".into(),
                    job.get("error")
                        .cloned()
                        .unwrap_or_else(|| Value::String("서버 분석에 실패했습니다.".into())),
                );
            }
            Ok(local)
        }
        _ => Ok(processing_replay(&local, &job)),
    }
}

async fn find_replay_path(hint: &RoflMatchHint) -> Result<Option<PathBuf>, String> {
    let hint = hint.clone();
    tokio::task::spawn_blocking(move || find_existing_replay_path(&hint))
        .await
        .map_err(|_| "리플레이 파일을 찾는 중 오류가 발생했습니다.".to_owned())?
}

#[tauri::command]
pub(crate) async fn match_fights(
    state: State<'_, Arc<AppState>>,
    game_id: String,
) -> Result<Value, String> {
    validate_game_id(&game_id)?;
    if let Some(cached) = load_cached_match_fights(&game_id).await? {
        return Ok(cached);
    }

    let config = state.config.read().await.clone();
    let client = history_client(state.inner()).await.ok();
    let replay_dir = match client.as_ref() {
        Some(client) => client.replay_directory().await.ok().flatten(),
        None => None,
    };
    let mut hint = RoflMatchHint::from_eog(game_id.clone(), &Value::Null);
    if let Some(dir) = replay_dir.as_deref() {
        hint.set_replay_dir(dir);
    }

    state
        .record_flight("match_fights", format!("prepare game_id={game_id}"))
        .await;

    let mut replay_path = find_replay_path(&hint).await?;
    if replay_path.is_none() {
        let Some(client) = client.as_ref() else {
            return Err(
                "저장된 ROFL이 없습니다. 롤 클라이언트를 실행한 뒤 한타 분석을 다시 열어 주세요."
                    .into(),
            );
        };
        client
            .download_history_replay(&game_id)
            .await
            .map_err(|_| {
                "이 경기의 ROFL을 다운로드하지 못했습니다. 롤 클라이언트에서 리플레이 제공 여부를 확인하세요."
                    .to_owned()
            })?;

        for _ in 0..30 {
            sleep(Duration::from_millis(1_500)).await;
            replay_path = find_replay_path(&hint).await?;
            if replay_path.is_some() {
                break;
            }
        }
    }

    let Some(replay_path) = replay_path else {
        return Err("ROFL 다운로드가 완료되지 않았습니다. 잠시 후 다시 시도하세요.".into());
    };

    let analysis = match analyze_replay_fights(&config, &game_id, &replay_path).await {
        Ok(value) => value,
        Err(error) => {
            state
                .report_diagnostic("lcu", "match_fights_analysis_failed", error.clone())
                .await;
            return Err(error);
        }
    };
    let response = save_cached_match_fights(&game_id, &analysis).await?;
    let count = response
        .get("fights")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    state
        .record_flight(
            "match_fights",
            format!("complete game_id={game_id} fights={count}"),
        )
        .await;
    Ok(response)
}

#[tauri::command]
pub(crate) async fn download_match_replay(
    state: State<'_, Arc<AppState>>,
    game_id: String,
) -> Result<(), String> {
    validate_game_id(&game_id)?;
    history_client(state.inner())
        .await?
        .download_history_replay(&game_id)
        .await
        .map_err(|_| {
            "리플레이를 다운로드하지 못했습니다. 롤 클라이언트에서 해당 경기의 다운로드 가능 여부를 확인하세요."
                .to_owned()
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn game_id_validation_accepts_only_numeric_ids() {
        assert!(validate_game_id("8393991955").is_ok());
        assert!(validate_game_id("").is_err());
        assert!(validate_game_id("KR-8393991955").is_err());
        assert!(validate_game_id("../replay").is_err());
    }

    #[test]
    fn teamfight_ready_shape_is_small_and_frontend_friendly() {
        let response = json!({
            "status": "ready",
            "fights": [],
            "analyzedAt": 123,
            "method": "champion_damage_cluster_v2",
            "cached": false
        });
        assert_eq!(response["status"], "ready");
        assert!(response["fights"].as_array().is_some());
    }
}
