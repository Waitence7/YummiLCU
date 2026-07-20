use std::{sync::Arc, time::Duration};

use tauri::AppHandle;
use tokio::time::sleep;

use crate::{
    commands::agent::start_agent_inner,
    config::Config,
    lcu::{lockfile_path, LcuClient, LcuConnectionState},
    platform::sync_windows_startup,
    state::{AgentEvent, AppState},
    updater::auto_update_on_startup,
};

pub(crate) fn run() -> Result<(), tauri::Error> {
    let config = Config::load();
    let _ = sync_windows_startup(config.run_at_windows_startup);
    let state = Arc::new(AppState::new(config));
    let update_state = state.clone();
    let connect_state = state.clone();
    let lcu_state = state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            crate::commands::config::load_config,
            crate::commands::config::save_config,
            crate::commands::agent::start_agent,
            crate::commands::agent::stop_agent,
            crate::commands::agent::relogin,
            crate::commands::agent::submit_oauth_code,
            crate::commands::lcu::recent_match
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(auto_update_on_startup(handle.clone(), update_state));
            tauri::async_runtime::spawn(watch_lcu(handle.clone(), lcu_state));
            tauri::async_runtime::spawn(async move {
                let _ = start_agent_inner(handle, connect_state).await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
}

async fn watch_lcu(app: AppHandle, state: Arc<AppState>) {
    loop {
        let next = inspect_lcu(&app, &state).await;
        state.publish(&app, AgentEvent::LcuStateChanged(next)).await;
        sleep(Duration::from_secs(2)).await;
    }
}

async fn inspect_lcu(app: &AppHandle, state: &Arc<AppState>) -> LcuConnectionState {
    if !state.relay.is_running().await {
        return LcuConnectionState::ClientStopped;
    }
    let config = state.config.read().await.clone();
    let Some(path) = lockfile_path(&config) else {
        return LcuConnectionState::ClientStopped;
    };

    let current = state.lcu_state().await;
    if matches!(
        current,
        LcuConnectionState::ClientStopped | LcuConnectionState::Error
    ) {
        state
            .publish(
                app,
                AgentEvent::LcuStateChanged(LcuConnectionState::LockfileFound),
            )
            .await;
        state
            .publish(
                app,
                AgentEvent::LcuStateChanged(LcuConnectionState::Connecting),
            )
            .await;
    }

    let Ok(client) = LcuClient::from_lockfile(&path) else {
        return LcuConnectionState::Error;
    };
    if current != LcuConnectionState::LoggedIn {
        state
            .publish(
                app,
                AgentEvent::LcuStateChanged(LcuConnectionState::Connected),
            )
            .await;
    }
    match client.probe_logged_in().await {
        Ok(()) => LcuConnectionState::LoggedIn,
        Err(_) => LcuConnectionState::Error,
    }
}
