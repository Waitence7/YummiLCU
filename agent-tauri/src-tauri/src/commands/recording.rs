use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::{
    lcu::remote_recording_context,
    recording::{self, RecordingSessionMetadata, RecordingStatus},
    session,
    state::AppState,
};

async fn report_failure(
    state: &Arc<AppState>,
    category: &'static str,
    code: &'static str,
    detail: impl AsRef<str>,
) {
    let detail = detail.as_ref();
    state
        .record_flight("recording_error", format!("{category}:{code}: {detail}"))
        .await;
    state.report_diagnostic(category, code, detail).await;
}

async fn run_blocking_status(
    state: &Arc<AppState>,
    operation: &'static str,
    code: &'static str,
    task: impl FnOnce() -> Result<RecordingStatus, String> + Send + 'static,
) -> Result<RecordingStatus, String> {
    match tokio::task::spawn_blocking(task).await {
        Ok(Ok(status)) => Ok(status),
        Ok(Err(error)) => {
            report_failure(state, "recording", code, &error).await;
            Err(error)
        }
        Err(error) => {
            let detail = format!("{operation} 작업 스레드 실패: {error}");
            report_failure(state, "recording", code, &detail).await;
            Err(detail)
        }
    }
}

#[tauri::command]
pub(crate) async fn get_recording_status(
    state: State<'_, Arc<AppState>>,
) -> Result<RecordingStatus, String> {
    run_blocking_status(
        state.inner(),
        "녹화 상태 조회",
        "status_failed",
        recording::status,
    )
    .await
}

#[tauri::command]
pub(crate) async fn list_recording_sessions(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<RecordingSessionMetadata>, String> {
    match tokio::task::spawn_blocking(recording::list_sessions).await {
        Ok(Ok(rows)) => Ok(rows),
        Ok(Err(error)) => {
            report_failure(state.inner(), "recording", "recording_list_failed", &error).await;
            Err(error)
        }
        Err(error) => {
            let detail = format!("녹화 목록 조회 작업 스레드 실패: {error}");
            report_failure(state.inner(), "recording", "recording_list_failed", &detail).await;
            Err(detail)
        }
    }
}

#[tauri::command]
pub(crate) async fn recording_game_context(
    state: State<'_, Arc<AppState>>,
    game_id: String,
) -> Result<Value, String> {
    let game_id = game_id.trim().to_owned();
    if game_id.is_empty()
        || game_id.len() > 64
        || !game_id
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || value == b'-' || value == b'_')
    {
        return Err("올바른 경기 ID가 아닙니다.".into());
    }
    let config = state.config.read().await.clone();
    let saved_session = match session::load(&config) {
        Some(session) => session,
        None => {
            let detail = format!("game_id={game_id} relay_session_missing");
            report_failure(state.inner(), "replay", "recording_context_failed", &detail).await;
            return Err("Yummi Relay 연결 세션이 없습니다.".into());
        }
    };
    match remote_recording_context(&config, &saved_session, &game_id).await {
        Ok(value) => Ok(value),
        Err(error) => {
            let detail = format!("game_id={game_id} error={error}");
            report_failure(state.inner(), "replay", "recording_context_failed", &detail).await;
            Err(error.to_string())
        }
    }
}

#[tauri::command]
pub(crate) async fn request_borderless_capture_access(
    state: State<'_, Arc<AppState>>,
) -> Result<RecordingStatus, String> {
    let status = run_blocking_status(
        state.inner(),
        "테두리 없는 캡처 권한 요청",
        "borderless_request_failed",
        recording::request_borderless_access,
    )
    .await?;
    if status.borderless_capture_requested
        && !status.borderless_capture_allowed
        && matches!(
            status.borderless_capture_status.as_str(),
            "not_declared_by_app" | "denied_by_system" | "unavailable" | "unknown"
        )
    {
        report_failure(
            state.inner(),
            "recording",
            "borderless_unavailable",
            format!("status={}", status.borderless_capture_status),
        )
        .await;
    }
    Ok(status)
}

#[tauri::command]
pub(crate) async fn start_recording(
    state: State<'_, Arc<AppState>>,
    source: Option<String>,
) -> Result<RecordingStatus, String> {
    run_blocking_status(
        state.inner(),
        "녹화 시작",
        "manual_start_failed",
        move || recording::start_recording(source.as_deref()),
    )
    .await
}

#[tauri::command]
pub(crate) async fn stop_recording(
    state: State<'_, Arc<AppState>>,
) -> Result<RecordingStatus, String> {
    run_blocking_status(
        state.inner(),
        "녹화 중지",
        "manual_stop_failed",
        recording::stop_recording,
    )
    .await
}

#[tauri::command]
pub(crate) async fn start_clip_buffer(
    state: State<'_, Arc<AppState>>,
    source: Option<String>,
) -> Result<RecordingStatus, String> {
    run_blocking_status(
        state.inner(),
        "클립 버퍼 시작",
        "clip_start_failed",
        move || recording::start_clip_buffer(source.as_deref()),
    )
    .await
}

#[tauri::command]
pub(crate) async fn stop_clip_buffer(
    state: State<'_, Arc<AppState>>,
) -> Result<RecordingStatus, String> {
    run_blocking_status(
        state.inner(),
        "클립 버퍼 중지",
        "clip_stop_failed",
        recording::stop_clip_buffer,
    )
    .await
}

#[tauri::command]
pub(crate) async fn save_last_clip(
    state: State<'_, Arc<AppState>>,
    seconds: u64,
) -> Result<RecordingStatus, String> {
    run_blocking_status(
        state.inner(),
        "최근 클립 저장",
        "clip_save_failed",
        move || recording::save_last_clip(seconds),
    )
    .await
}
