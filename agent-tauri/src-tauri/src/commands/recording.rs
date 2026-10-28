use crate::recording::{self, RecordingStatus};

async fn run_blocking(
    operation: &'static str,
    task: impl FnOnce() -> Result<RecordingStatus, String> + Send + 'static,
) -> Result<RecordingStatus, String> {
    match tokio::task::spawn_blocking(task).await {
        Ok(result) => result,
        Err(error) => Err(format!("{operation} 작업 스레드 실패: {error}")),
    }
}

#[tauri::command]
pub(crate) async fn get_recording_status() -> Result<RecordingStatus, String> {
    run_blocking("녹화 상태 조회", recording::status).await
}

#[tauri::command]
pub(crate) async fn start_recording(source: Option<String>) -> Result<RecordingStatus, String> {
    run_blocking("녹화 시작", move || {
        recording::start_recording(source.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn stop_recording() -> Result<RecordingStatus, String> {
    run_blocking("녹화 중지", recording::stop_recording).await
}

#[tauri::command]
pub(crate) async fn start_clip_buffer(source: Option<String>) -> Result<RecordingStatus, String> {
    run_blocking("클립 버퍼 시작", move || {
        recording::start_clip_buffer(source.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn stop_clip_buffer() -> Result<RecordingStatus, String> {
    run_blocking("클립 버퍼 중지", recording::stop_clip_buffer).await
}

#[tauri::command]
pub(crate) async fn save_last_clip(seconds: u64) -> Result<RecordingStatus, String> {
    run_blocking("최근 클립 저장", move || {
        recording::save_last_clip(seconds)
    })
    .await
}
