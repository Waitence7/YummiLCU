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

async fn report_match_history_warnings(
    state: &Arc<AppState>,
    offset: u32,
    target: Option<&str>,
    warnings: &[String],
) {
    if warnings.is_empty() {
        return;
    }
    let sample = warnings
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ");
    let detail = format!(
        "offset={offset} target={} warning_count={} sample={sample}",
        if target.is_some() { "searched" } else { "self" },
        warnings.len()
    );
    state
        .record_flight("match_history_warning", detail.clone())
        .await;
    state
        .report_diagnostic("lcu", "match_history_partial_detail", detail)
        .await;
}

#[tauri::command]
pub(crate) async fn match_history(
    state: State<'_, Arc<AppState>>,
    offset: u32,
    riot_id: Option<String>,
) -> Result<Value, String> {
    let requested_riot_id = riot_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    // 다른 사람 전적은 현재 사용자의 로컬 캐시를 덮어쓰거나 fallback하지 않는다.
    if let Some(target) = requested_riot_id.as_deref() {
        let client = history_client(state.inner()).await.map_err(|error| error)?;
        return match client.match_history(offset, Some(target)).await {
            Ok((_account_key, mut page, warnings)) => {
                report_match_history_warnings(state.inner(), offset, Some(target), &warnings).await;
                attach_fights_best_effort(state.inner(), &mut page).await;
                state
                    .record_flight(
                        "match_history_search",
                        format!(
                            "target={target} offset={offset} matches={}",
                            page.get("matches")
                                .and_then(Value::as_array)
                                .map_or(0, Vec::len)
                        ),
                    )
                    .await;
                Ok(page)
            }
            Err(error) => {
                let local_detail = format!("target={target} offset={offset} error={error}");
                state
                    .record_flight("match_history_search_failed", local_detail)
                    .await;
                state
                    .report_diagnostic(
                        "lcu",
                        "match_history_player_search_failed",
                        format!("offset={offset} error={error}"),
                    )
                    .await;
                Err(error.to_string())
            }
        };
    }

    let live_result = match history_client(state.inner()).await {
        Ok(client) => match client.match_history(offset, None).await {
            Ok((account_key, mut page, warnings)) => {
                report_match_history_warnings(state.inner(), offset, None, &warnings).await;
                // 내 전적만 로컬 캐시에 저장한다.
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
                    .record_flight("match_history_cache_fallback", error.clone())
                    .await;
                state
                    .report_diagnostic("lcu", "match_history_live_failed", error)
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
        Ok(client) => match client.replay_directory().await {
            Ok(value) => value,
            Err(error) => {
                state
                    .report_diagnostic(
                        "lcu",
                        "replay_directory_lookup_failed",
                        format!("game_id={game_id} error={error}"),
                    )
                    .await;
                None
            }
        },
        Err(error) => {
            state
                .report_diagnostic(
                    "lcu",
                    "replay_history_client_unavailable",
                    format!("game_id={game_id} error={error}"),
                )
                .await;
            None
        }
    };
    let id = game_id.to_owned();
    let dir = replay_dir.clone();
    let value =
        match tokio::task::spawn_blocking(move || replay_for_viewer(&id, dir.as_deref())).await {
            Ok(result) => result?,
            Err(error) => {
                let detail = format!("game_id={game_id} replay worker failed: {error}");
                state
                    .report_diagnostic("lcu", "replay_viewer_worker_failed", &detail)
                    .await;
                return Err("리플레이를 읽는 중 오류가 발생했습니다.".to_owned());
            }
        };
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
        Ok(None) => {
            state
                .report_diagnostic(
                    "lcu",
                    "remote_replay_local_file_missing",
                    format!("game_id={game_id}"),
                )
                .await;
            return base;
        }
        Err(error) => {
            state
                .report_diagnostic(
                    "lcu",
                    "remote_replay_local_file_lookup_failed",
                    format!("game_id={game_id} error={error}"),
                )
                .await;
            return base;
        }
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
                Err(error) => {
                    state
                        .report_diagnostic(
                            "lcu",
                            "remote_replay_viewer_fetch_failed",
                            format!("game_id={game_id} job_id={job_id} error={error}"),
                        )
                        .await;
                    processing_replay(&base, &job)
                }
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
    let job = match remote_replay_analysis_job(&config, &saved_session, &job_id).await {
        Ok(job) => job,
        Err(error) => {
            let detail = format!("game_id={game_id} job_id={job_id} error={error}");
            state
                .report_diagnostic("lcu", "remote_replay_job_fetch_failed", &detail)
                .await;
            return Err(error.to_string());
        }
    };
    let (mut local, _) = local_replay_viewer(state.inner(), &game_id).await?;
    match job.get("status").and_then(Value::as_str) {
        Some("done") if job.get("resultAvailable").and_then(Value::as_bool) == Some(true) => {
            let viewer = match remote_replay_analysis_viewer(&config, &saved_session, &job_id).await
            {
                Ok(viewer) => viewer,
                Err(error) => {
                    let detail = format!("game_id={game_id} job_id={job_id} error={error}");
                    state
                        .report_diagnostic("lcu", "remote_replay_viewer_fetch_failed", &detail)
                        .await;
                    return Err(error.to_string());
                }
            };
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
        .map_err(|error| format!("리플레이 파일 탐색 worker 실패: {error}"))?
}

async fn ensure_replay_downloaded(
    state: &Arc<AppState>,
    client: &LcuClient,
    game_id: &str,
    hint: &RoflMatchHint,
    context: &'static str,
    game_version: Option<&str>,
) -> Result<PathBuf, String> {
    let version = game_version
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");

    match find_replay_path(hint).await {
        Ok(Some(path)) => {
            state
                .record_flight(
                    "replay_download",
                    format!(
                        "existing context={context} game_id={game_id} version={version} file={}",
                        path.file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("replay.rofl")
                    ),
                )
                .await;
            return Ok(path);
        }
        Ok(None) => {}
        Err(error) => {
            let detail = format!(
                "context={context} game_id={game_id} version={version} phase=initial_path_check error={error}"
            );
            state
                .record_flight("replay_download_error", detail.clone())
                .await;
            state
                .report_diagnostic("lcu", "replay_download_path_check_failed", &detail)
                .await;
            return Err(error);
        }
    }

    let endpoint = format!("/lol-replays/v1/rofls/{game_id}/download/graceful");
    state
        .record_flight(
            "replay_download",
            format!(
                "request context={context} game_id={game_id} version={version} endpoint={endpoint}"
            ),
        )
        .await;

    if let Err(error) = client.download_history_replay(game_id).await {
        let detail =
            format!("context={context} game_id={game_id} version={version} endpoint={endpoint} error={error}");
        state
            .record_flight("replay_download_error", detail.clone())
            .await;
        state
            .report_diagnostic("lcu", "replay_download_request_failed", &detail)
            .await;
        return Err(
            "리플레이 다운로드 요청이 실패했습니다. 자세한 LCU 오류를 진단 로그에 기록했습니다."
                .to_owned(),
        );
    }

    state
        .record_flight(
            "replay_download",
            format!("accepted context={context} game_id={game_id} version={version} endpoint={endpoint}"),
        )
        .await;

    const POLL_MS: u64 = 1_500;
    const ATTEMPTS: u64 = 30;
    for attempt in 1..=ATTEMPTS {
        sleep(Duration::from_millis(POLL_MS)).await;
        match find_replay_path(hint).await {
            Ok(Some(path)) => {
                state
                    .record_flight(
                        "replay_download",
                        format!(
                            "ready context={context} game_id={game_id} version={version} wait_ms={} file={}",
                            attempt * POLL_MS,
                            path.file_name()
                                .and_then(|value| value.to_str())
                                .unwrap_or("replay.rofl")
                        ),
                    )
                    .await;
                return Ok(path);
            }
            Ok(None) => {}
            Err(error) => {
                let detail = format!(
                    "context={context} game_id={game_id} version={version} wait_ms={} error={error}",
                    attempt * POLL_MS
                );
                state
                    .record_flight("replay_download_error", detail.clone())
                    .await;
                state
                    .report_diagnostic("lcu", "replay_download_path_check_failed", &detail)
                    .await;
                return Err(error);
            }
        }
    }

    let waited_ms = POLL_MS * ATTEMPTS;
    let detail = format!(
        "context={context} game_id={game_id} version={version} endpoint={endpoint} request_accepted=true file_ready=false waited_ms={waited_ms}"
    );
    state
        .record_flight("replay_download_timeout", detail.clone())
        .await;
    state
        .report_diagnostic("lcu", "replay_download_timeout", &detail)
        .await;
    Err(format!(
        "리플레이 다운로드 요청은 수락됐지만 {waited_ms}ms 안에 ROFL 파일이 생성되지 않았습니다. 진단 로그에 기록했습니다."
    ))
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
    let client = match history_client(state.inner()).await {
        Ok(client) => Some(client),
        Err(error) => {
            state
                .report_diagnostic(
                    "lcu",
                    "match_fights_client_unavailable",
                    format!("game_id={game_id} error={error}"),
                )
                .await;
            None
        }
    };
    let replay_dir = match client.as_ref() {
        Some(client) => match client.replay_directory().await {
            Ok(value) => value,
            Err(error) => {
                state
                    .report_diagnostic(
                        "lcu",
                        "match_fights_replay_directory_failed",
                        format!("game_id={game_id} error={error}"),
                    )
                    .await;
                None
            }
        },
        None => None,
    };
    let mut hint = RoflMatchHint::from_eog(game_id.clone(), &Value::Null);
    if let Some(dir) = replay_dir.as_deref() {
        hint.set_replay_dir(dir);
    }

    state
        .record_flight("match_fights", format!("prepare game_id={game_id}"))
        .await;

    let replay_path = match find_replay_path(&hint).await? {
        Some(path) => path,
        None => {
            let Some(client) = client.as_ref() else {
                return Err(
                    "저장된 ROFL이 없습니다. 롤 클라이언트를 실행한 뒤 한타 분석을 다시 열어 주세요."
                        .into(),
                );
            };
            ensure_replay_downloaded(state.inner(), client, &game_id, &hint, "match_fights", None)
                .await?
        }
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
    game_version: Option<String>,
) -> Result<(), String> {
    validate_game_id(&game_id)?;
    let client = match history_client(state.inner()).await {
        Ok(client) => client,
        Err(error) => {
            let detail = format!(
                "context=match_replay game_id={game_id} version={} error={error}",
                game_version.as_deref().unwrap_or("unknown")
            );
            state
                .record_flight("replay_download_error", detail.clone())
                .await;
            state
                .report_diagnostic("lcu", "replay_download_client_unavailable", &detail)
                .await;
            return Err(error);
        }
    };

    let replay_dir = match client.replay_directory().await {
        Ok(value) => value,
        Err(error) => {
            let detail = format!(
                "context=match_replay game_id={game_id} version={} replay_directory_error={error}",
                game_version.as_deref().unwrap_or("unknown")
            );
            state
                .record_flight("replay_download_warning", detail.clone())
                .await;
            state
                .report_diagnostic("lcu", "replay_directory_lookup_failed", &detail)
                .await;
            None
        }
    };

    let mut hint = RoflMatchHint::from_eog(game_id.clone(), &Value::Null);
    if let Some(dir) = replay_dir.as_deref() {
        hint.set_replay_dir(dir);
    }
    ensure_replay_downloaded(
        state.inner(),
        &client,
        &game_id,
        &hint,
        "match_replay",
        game_version.as_deref(),
    )
    .await?;
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
