use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::{
    platform::open_beta_download_url,
    state::AppState,
    updater::{beta_release_info, check_and_apply_update, BetaReleaseInfo},
};

#[tauri::command]
pub(crate) async fn get_beta_release_info() -> Result<BetaReleaseInfo, String> {
    beta_release_info().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn open_beta_download(app: AppHandle) -> Result<(), String> {
    open_beta_download_url(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn apply_beta_update_now(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let guard = state.command_lock.lock().await;
    let mut config = state.config.read().await.clone();
    config.update_channel = "beta".into();
    config.normalize();
    config.validate().map_err(|error| error.to_string())?;
    config.save().map_err(|error| error.to_string())?;

    state.update_config(config.clone()).await;
    state
        .record_flight("beta_update", "manual_apply_requested")
        .await;
    state.emit(&app).await;
    drop(guard);

    let manifest_url = config
        .update_manifest_url
        .clone()
        .ok_or_else(|| "beta manifest URL이 없습니다.".to_string())?;
    let app_for_update = app.clone();
    let state_for_update = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        check_and_apply_update(
            &manifest_url,
            &config,
            &app_for_update,
            state_for_update.as_ref(),
        )
        .await;
    });
    Ok(())
}
