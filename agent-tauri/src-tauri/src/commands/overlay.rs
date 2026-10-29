use std::sync::Arc;

use tauri::State;

use crate::{
    gamebar_bridge::{self, OverlayInstallStatus},
    state::AppState,
};

#[tauri::command]
pub(crate) async fn get_overlay_install_status(
    state: State<'_, Arc<AppState>>,
) -> Result<OverlayInstallStatus, String> {
    match tokio::task::spawn_blocking(gamebar_bridge::overlay_install_status).await {
        Ok(Ok(status)) => Ok(status),
        Ok(Err(error)) => {
            state
                .report_diagnostic("updater", "overlay_status_failed", &error)
                .await;
            Err(error)
        }
        Err(error) => {
            let detail = format!("overlay status worker failed: {error}");
            state
                .report_diagnostic("updater", "overlay_status_failed", &detail)
                .await;
            Err(detail)
        }
    }
}

#[tauri::command]
pub(crate) async fn install_overlay(
    state: State<'_, Arc<AppState>>,
) -> Result<OverlayInstallStatus, String> {
    state
        .record_flight("overlay_install", "user_requested")
        .await;
    match gamebar_bridge::install_overlay_latest().await {
        Ok(status) => {
            state
                .record_flight(
                    "overlay_install",
                    format!(
                        "completed version={}",
                        status.installed_version.as_deref().unwrap_or("unknown")
                    ),
                )
                .await;
            Ok(status)
        }
        Err(error) => {
            state
                .record_flight("overlay_install_error", &error)
                .await;
            state
                .report_diagnostic("updater", "overlay_install_failed", &error)
                .await;
            Err(error)
        }
    }
}

#[tauri::command]
pub(crate) async fn reset_overlay_tutorial(
    state: State<'_, Arc<AppState>>,
) -> Result<u64, String> {
    let generation = state.request_gamebar_tutorial_reset();
    state
        .record_flight(
            "overlay_tutorial",
            format!("developer_reset_requested generation={generation}"),
        )
        .await;
    Ok(generation)
}
