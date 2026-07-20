use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::{
    config::Config, platform::sync_windows_startup, relay::supervisor::RelaySupervisor,
    state::AppState,
};

#[tauri::command]
pub(crate) async fn load_config(state: State<'_, Arc<AppState>>) -> Result<Config, String> {
    Ok(state.config.read().await.clone())
}

#[tauri::command]
pub(crate) async fn save_config(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    mut config: Config,
) -> Result<(), String> {
    config.normalize();
    sync_windows_startup(config.run_at_windows_startup).map_err(|error| error.to_string())?;
    config.save().map_err(|error| error.to_string())?;

    let relay_url_changed =
        state.config.read().await.relay_public_base_url != config.relay_public_base_url;
    let relay_was_running = state.relay.is_running().await;
    state.update_config(config).await;
    state.emit(&app).await;

    if relay_url_changed && relay_was_running {
        RelaySupervisor::restart(app, state.inner().clone())
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
