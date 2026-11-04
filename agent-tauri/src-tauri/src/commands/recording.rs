use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::{
    lcu::remote_recording_context,
    recording::{self, RecordingSessionMetadata, RecordingStatus},
    session,
    state::{AppState, GameBarToast, GAMEBAR_TOAST_STICKY_EXPIRES_AT_MS},
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

pub(crate) fn sync_recording_overlay_toast(state: &Arc<AppState>, status: &RecordingStatus) {
    const TOAST_ID: &str = "recording.capture.active";
    if !status.recording {
        state.remove_gamebar_toast(TOAST_ID);
        return;
    }
    let body = if status.auto_game_recording {
        match status.auto_game_id.as_deref() {
            Some(game_id) if !game_id.is_empty() => format!("자동 게임 녹화 · {game_id}"),
            _ => "자동 게임 녹화".into(),
        }
    } else if status.recording_source == "leagueWindow" {
        "게임 창 녹화".into()
    } else {
        "화면 전체 녹화".into()
    };
    state.push_gamebar_toast(GameBarToast {
        id: TOAST_ID.into(),
        source: "recording.capture".into(),
        title: "녹화 중".into(),
        body,
        tone: "error".into(),
        timestamp_ms: 0,
        expires_at_ms: GAMEBAR_TOAST_STICKY_EXPIRES_AT_MS,
    });
}

#[tauri::command]
pub(crate) async fn get_recording_status(
    state: State<'_, Arc<AppState>>,
) -> Result<RecordingStatus, String> {
    let status = run_blocking_status(
        state.inner(),
        "녹화 상태 조회",
        "status_failed",
        recording::status,
    )
    .await?;
    sync_recording_overlay_toast(state.inner(), &status);
    Ok(status)
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
    let detail = format!(
        "status={} allowed={} error={}",
        status.borderless_capture_status,
        status.borderless_capture_allowed,
        status
            .borderless_capture_detail
            .as_deref()
            .unwrap_or("none")
    );
    state
        .record_flight("recording", format!("borderless_access_result {detail}"))
        .await;
    // Record every user-initiated attempt, including successful authorization
    // and DeniedByUser, to distinguish system policy from user choices.
    state
        .report_diagnostic("recording", "borderless_access_result", &detail)
        .await;
    if !status.borderless_capture_allowed {
        report_failure(
            state.inner(),
            "recording",
            "borderless_unavailable",
            &detail,
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
    // A single click starts both the permission flow and manual recording.
    // Permission is optional: denied/missing borderless capability must not
    // prevent ordinary WGC capture with its default system border.
    match run_blocking_status(
        state.inner(),
        "일반 녹화 시작 전 테두리 없는 캡처 권한 요청",
        "manual_borderless_request_failed",
        recording::request_borderless_access,
    )
    .await
    {
        Ok(access) => {
            let detail = format!(
                "status={} allowed={} error={}",
                access.borderless_capture_status,
                access.borderless_capture_allowed,
                access
                    .borderless_capture_detail
                    .as_deref()
                    .unwrap_or("none")
            );
            state
                .record_flight("recording", format!("manual_start_borderless {detail}"))
                .await;
            if !access.borderless_capture_allowed {
                state
                    .report_diagnostic("recording", "manual_borderless_unavailable", &detail)
                    .await;
            }
        }
        Err(error) => {
            state
                .record_flight(
                    "recording_error",
                    format!("manual_start_borderless_request_failed {error}"),
                )
                .await;
            // run_blocking_status already records a structured error.
        }
    }
    let status = run_blocking_status(
        state.inner(),
        "녹화 시작",
        "manual_start_failed",
        move || recording::start_recording(source.as_deref()),
    )
    .await?;
    sync_recording_overlay_toast(state.inner(), &status);
    Ok(status)
}

#[tauri::command]
pub(crate) async fn stop_recording(
    state: State<'_, Arc<AppState>>,
) -> Result<RecordingStatus, String> {
    let status = run_blocking_status(
        state.inner(),
        "녹화 중지",
        "manual_stop_failed",
        recording::stop_recording,
    )
    .await?;
    sync_recording_overlay_toast(state.inner(), &status);
    Ok(status)
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
