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
    session::remove().map_err(|error| error.to_string())?;
    RelaySupervisor::restart(app, state.inner().clone())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn submit_oauth_code(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    code: String,
) -> Result<(), String> {
    let code = code.trim().to_string();
    if code.len() != 6 || !code.chars().all(|character| character.is_ascii_digit()) {
        return Err("6자리 숫자 코드를 입력하세요.".into());
    }
    state
        .relay
        .submit_oauth_code(code)
        .await
        .map_err(|error| error.to_string())?;
    state.log(&app, "Discord 연결 코드 전송됨").await;
    Ok(())
}
