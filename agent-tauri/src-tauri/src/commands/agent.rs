use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::{relay::supervisor::RelaySupervisor, session, state::AppState};

pub(crate) async fn start_agent_inner(app: AppHandle, state: Arc<AppState>) -> Result<(), String> {
    RelaySupervisor::start(app, state)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn start_agent(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    start_agent_inner(app, state.inner().clone()).await
}

#[tauri::command]
pub(crate) async fn stop_agent(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    RelaySupervisor::stop(&app, state.inner()).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn relogin(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    RelaySupervisor::stop(&app, state.inner()).await;
    session::remove().map_err(|error| error.to_string())?;
    RelaySupervisor::start(app, state.inner().clone())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn submit_oauth_code(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    mut code: String,
) -> Result<(), String> {
    let mut normalized = code.trim().to_string();
    // NUL bytes are valid UTF-8, so the IPC-owned input remains a valid String until drop.
    unsafe { code.as_bytes_mut().fill(0) };
    if normalized.len() != 6
        || !normalized
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        unsafe { normalized.as_bytes_mut().fill(0) };
        return Err("6자리 숫자 코드를 입력하세요.".into());
    }
    state
        .relay
        .submit_oauth_code(normalized)
        .await
        .map_err(|error| error.to_string())?;
    state.log(&app, "Discord 연결 코드 전송됨").await;
    Ok(())
}
