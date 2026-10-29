use std::{sync::Arc, time::Duration};

use tauri::{AppHandle, State};

use crate::{
    config::Config,
    relay::supervisor::RelaySupervisor,
    session,
    state::{AppState, UiState},
};

pub(crate) async fn start_agent_inner(app: AppHandle, state: Arc<AppState>) -> Result<(), String> {
    RelaySupervisor::start_background(app, state)
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
    session::clear_auto_login_suppression().map_err(|error| error.to_string())?;
    RelaySupervisor::start_user_requested(app, state.inner().clone())
        .await
        .map_err(|error| error.to_string())
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

async fn revoke_persisted_refresh(config: &Config) -> Result<(), String> {
    let Some(saved) = session::load(config) else {
        return Ok(());
    };
    let Some(credential) = saved.refresh_token.as_deref() else {
        return Ok(());
    };
    let url = config
        .session_revoke_url()
        .map_err(|error| error.to_string())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| format!("revoke client: {error}"))?;
    let body = serde_json::json!({
        "session_id": saved.session_id,
        "refresh_token": credential,
    });
    let response = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("revoke request: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("revoke HTTP {}", response.status()));
    }
    Ok(())
}

pub(crate) async fn logout_inner(
    app: &AppHandle,
    state: &Arc<AppState>,
    reason: &'static str,
) -> Result<(), String> {
    state
        .record_flight("command", format!("discord_logout reason={reason}"))
        .await;
    state
        .report_diagnostic("command", "discord_logout", format!("reason={reason}"))
        .await;
    let config = state.config.read().await.clone();
    if let Err(error) = revoke_persisted_refresh(&config).await {
        state
            .record_flight("session_error", format!("refresh_revoke_failed: {error}"))
            .await;
        state
            .log(
                app,
                "서버 refresh 세션 폐기에 실패했지만 로컬 로그아웃은 계속합니다.",
            )
            .await;
    } else {
        state
            .record_flight("session", "refresh_revoked_on_logout")
            .await;
    }
    RelaySupervisor::stop(app, state).await;
    session::remove().map_err(|error| error.to_string())?;
    session::suppress_auto_login().map_err(|error| error.to_string())?;
    state.mark_logged_out(app).await;
    state.log(app, "Discord 로그아웃 완료").await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn logout(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    logout_inner(&app, state.inner(), "user_requested").await
}
