use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::{
    relay::supervisor::RelaySupervisor,
    session,
    state::{AppState, UiState},
};

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
    state.record_flight("command", "manual_start").await;
    state
        .report_diagnostic("command", "manual_start", "user requested relay start")
        .await;
    start_agent_inner(app, state.inner().clone()).await
}

#[tauri::command]
pub(crate) async fn get_agent_state(state: State<'_, Arc<AppState>>) -> Result<UiState, String> {
    Ok(state.snapshot().await)
}

#[tauri::command]
pub(crate) async fn stop_agent(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.record_flight("command", "manual_stop").await;
    state
        .report_diagnostic("command", "manual_stop", "user requested relay stop")
        .await;
    RelaySupervisor::stop(&app, state.inner()).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn logout(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.record_flight("command", "discord_logout").await;
    state
        .report_diagnostic("command", "discord_logout", "user requested Discord logout")
        .await;
    RelaySupervisor::stop(&app, state.inner()).await;
    session::remove().map_err(|error| error.to_string())?;
    state.mark_logged_out(&app).await;
    state.log(&app, "Discord 로그아웃 완료").await;
    Ok(())
}
