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
    // Keep the agent alive after login; the UI is a tray-only background app.
    config.run_at_windows_startup = true;
    config.normalize();
    if let Err(error) = config.validate() {
        let summary = error.to_string();
        state
            .record_flight("config_error", format!("validation_failed: {summary}"))
            .await;
        state.log(&app, format!("설정 검증 실패: {summary}")).await;
        return Err(summary);
    }
    if let Err(error) = sync_windows_startup(config.run_at_windows_startup) {
        let summary = error.to_string();
        state
            .record_flight(
                "windows_startup",
                format!("registration_failed_during_save: {summary}"),
            )
            .await;
        state
            .log(
                &app,
                format!("설정 저장 중 Windows 자동 시작 등록 실패: {summary}"),
            )
            .await;
        state
            .report_unexpected_error(
                "config",
                "windows_startup_registration_failed",
                &summary,
            )
            .await;
        return Err(summary);
    }
    if let Err(error) = config.save() {
        let summary = error.to_string();
        state
            .record_flight("config_error", format!("save_failed: {summary}"))
            .await;
        state.log(&app, format!("설정 파일 저장 실패: {summary}")).await;
        state
            .report_unexpected_error("config", "save_failed", &summary)
            .await;
        return Err(summary);
    }

    state.record_flight("config", "saved").await;
    let relay_url_changed =
        state.config.read().await.relay_public_base_url != config.relay_public_base_url;
    let relay_was_running = state.relay.is_running().await;
    state.update_config(config).await;
    state.emit(&app).await;

    if relay_url_changed && relay_was_running {
        if let Err(error) = RelaySupervisor::restart(app.clone(), state.inner().clone()).await {
            let summary = error.to_string();
            state
                .record_flight("relay_error", format!("restart_after_config_failed: {summary}"))
                .await;
            state
                .log(&app, format!("설정 변경 후 Relay 재시작 실패: {summary}"))
                .await;
            state
                .report_unexpected_error("relay", "restart_after_config_failed", &summary)
                .await;
            return Err(summary);
        }
        state
            .record_flight("relay", "restarted_after_config_change")
            .await;
    }
    Ok(())
}
