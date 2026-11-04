use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use tauri::{AppHandle, State};

use crate::{
    commands::agent::logout_inner,
    config::Config,
    gamebar_bridge::{self, OverlayInstallStatus},
    state::AppState,
};

#[cfg(windows)]
fn launch_windows_game_bar() -> Result<(), String> {
    use windows::{
        core::{w, PCWSTR},
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    // Only the normal Game Bar URI is allowed here. Launching a specific
    // third-party widget is not guaranteed to work outside the Game Bar host.
    let handle = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("ms-gamebar:"),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    let result = handle.0 as isize;
    if result <= 32 {
        Err(format!(
            "Windows Game Bar URI 실행 실패: ShellExecuteW={result}"
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn launch_windows_game_bar() -> Result<(), String> {
    Err("Xbox Game Bar 열기는 Windows에서만 지원됩니다.".into())
}

#[tauri::command]
pub(crate) async fn open_windows_game_bar(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state
        .record_flight("overlay", "game_bar_open_requested")
        .await;
    let result = launch_windows_game_bar();
    match &result {
        Ok(()) => {
            state
                .record_flight("overlay", "game_bar_uri_dispatched")
                .await;
        }
        Err(error) => {
            state
                .record_flight("overlay_error", format!("game_bar_open_failed {error}"))
                .await;
            state
                .report_diagnostic("overlay", "game_bar_open_failed", error)
                .await;
        }
    }
    result
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OverlayControlsStatus {
    supported: bool,
    game_bar_found: bool,
    yummi_selected: bool,
    pin: Option<bool>,
    click_through: Option<bool>,
    pin_preference: Option<bool>,
    click_through_preference: Option<bool>,
    selection_invoked: bool,
    message: Option<String>,
}

fn ui_toggle_state(result: &Value, field: &str) -> Option<bool> {
    let toggle = result.get(field)?;
    (toggle.get("found")?.as_bool()? && toggle.get("error").is_none())
        .then(|| toggle.get("enabled").and_then(Value::as_bool))
        .flatten()
}

fn controls_from_probe(
    config: &Config,
    probe: Option<&Value>,
    fallback_message: Option<String>,
) -> OverlayControlsStatus {
    let selected = probe
        .and_then(|result| result.get("yummiSelected"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let pin = probe
        .filter(|_| selected)
        .and_then(|result| ui_toggle_state(result, "pin"));
    let click_through = probe
        .filter(|_| selected)
        .and_then(|result| ui_toggle_state(result, "clickThrough"));
    let game_bar_found = probe
        .and_then(|result| result.get("gameBarFound"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let selection_invoked = probe
        .and_then(|result| result.get("selectionInvoked"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = fallback_message.or_else(|| {
        if !game_bar_found {
            Some("Game Bar가 닫혀 있습니다. 선택한 상태는 다음에 열 때 적용됩니다.".into())
        } else if !selected {
            Some("Game Bar에서 유미지지 오버레이를 선택하면 제어가 적용됩니다.".into())
        } else if probe
            .and_then(|result| result.pointer("/pin/error"))
            .is_some()
            || probe
                .and_then(|result| result.pointer("/clickThrough/error"))
                .is_some()
        {
            Some("Game Bar 토글 상태 조회에 실패했습니다. Win+G에서 직접 확인해 주세요.".into())
        } else {
            None
        }
    });
    OverlayControlsStatus {
        supported: cfg!(windows),
        game_bar_found,
        yummi_selected: selected,
        pin,
        click_through,
        pin_preference: config.overlay_pin_preference,
        click_through_preference: config.overlay_click_through_preference,
        selection_invoked,
        message,
    }
}

async fn probe_controls(
    pin_target: Option<bool>,
    click_through_target: Option<bool>,
    auto_select: bool,
) -> Result<Option<Value>, String> {
    #[cfg(windows)]
    {
        let data = tokio::task::spawn_blocking(move || {
            gamebar_bridge::probe_overlay_controls(pin_target, click_through_target, auto_select)
        })
        .await
        .map_err(|error| format!("Game Bar 제어 작업 실패: {error}"))??;
        Ok(Some(data))
    }
    #[cfg(not(windows))]
    {
        let _ = (pin_target, click_through_target, auto_select);
        Ok(None)
    }
}

#[tauri::command]
pub(crate) async fn get_overlay_controls_status(
    state: State<'_, Arc<AppState>>,
) -> Result<OverlayControlsStatus, String> {
    let config = state.config.read().await.clone();
    let probe = probe_controls(None, None, false).await;
    Ok(match probe {
        Ok(data) => controls_from_probe(&config, data.as_ref(), None),
        Err(error) => controls_from_probe(
            &config,
            None,
            Some(format!("Game Bar 상태 확인 실패: {error}")),
        ),
    })
}

#[tauri::command]
pub(crate) async fn set_overlay_control(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    control: String,
    enabled: bool,
) -> Result<OverlayControlsStatus, String> {
    if !cfg!(windows) {
        return Err("Game Bar 제어는 Windows에서만 지원됩니다.".into());
    }
    if control != "pin" && control != "clickThrough" {
        return Err("지원하지 않는 Game Bar 제어 항목입니다.".into());
    }
    let _guard = state.command_lock.lock().await;
    let mut config = state.config.read().await.clone();
    match control.as_str() {
        "pin" => config.overlay_pin_preference = Some(enabled),
        "clickThrough" => config.overlay_click_through_preference = Some(enabled),
        _ => unreachable!(),
    }
    if let Err(error) = config.save() {
        let detail = format!("control={control} enabled={enabled} error={error}");
        state
            .report_diagnostic("overlay", "control_save_failed", &detail)
            .await;
        return Err(format!("오버레이 제어 설정 저장 실패: {error}"));
    }
    state.update_config(config.clone()).await;
    state.emit(&app).await;
    state
        .record_flight(
            "overlay",
            format!("control_requested control={control} enabled={enabled}"),
        )
        .await;
    drop(_guard);

    let pin_target = (control == "pin").then_some(enabled);
    let click_through_target = (control == "clickThrough").then_some(enabled);
    let mut probe = probe_controls(pin_target, click_through_target, true).await;
    if probe
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .and_then(|data| data.get("selectionInvoked"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        probe = probe_controls(pin_target, click_through_target, false).await;
    }

    let result = match probe {
        Ok(data) => controls_from_probe(&config, data.as_ref(), None),
        Err(error) => {
            state
                .report_diagnostic(
                    "overlay",
                    "control_apply_failed",
                    format!("control={control} error={error}"),
                )
                .await;
            controls_from_probe(
                &config,
                None,
                Some(format!(
                    "설정은 저장됐지만 즉시 적용에 실패했습니다: {error}"
                )),
            )
        }
    };
    let actual = if control == "pin" {
        result.pin
    } else {
        result.click_through
    };
    if actual != Some(enabled) {
        state
            .record_flight(
                "overlay",
                format!("control_pending control={control} enabled={enabled} current={actual:?}"),
            )
            .await;
    } else {
        state
            .record_flight(
                "overlay",
                format!("control_applied control={control} enabled={enabled}"),
            )
            .await;
    }
    Ok(result)
}

#[cfg(test)]
mod control_tests {
    use super::*;

    #[test]
    fn UI_status_distinguishes_unknown_from_disabled() {
        let config = Config {
            overlay_pin_preference: Some(false),
            overlay_click_through_preference: Some(true),
            ..Config::default()
        };
        let absent = controls_from_probe(&config, None, None);
        assert_eq!(absent.pin, None);
        assert_eq!(absent.click_through, None);
        assert_eq!(absent.pin_preference, Some(false));
        let actual = serde_json::json!({
            "gameBarFound":true, "yummiSelected":true,
            "pin":{"found":true,"enabled":false},
            "clickThrough":{"found":true,"enabled":true},
        });
        let present = controls_from_probe(&config, Some(&actual), None);
        assert_eq!(present.pin, Some(false));
        assert_eq!(present.click_through, Some(true));
        assert!(present.yummi_selected);
    }

    #[test]
    fn other_widget_controls_are_not_assumed_to_be_yummi() {
        let config = Config::default();
        let not_selected = serde_json::json!({
            "gameBarFound":true, "yummiSelected":false,
            "pin":{"found":true,"enabled":true},
            "clickThrough":{"found":true,"enabled":false}
        });
        let status = controls_from_probe(&config, Some(&not_selected), None);
        assert_eq!(status.pin, None);
        assert_eq!(status.click_through, None);
    }
}

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
            state.record_flight("overlay_install_error", &error).await;
            state
                .report_diagnostic("updater", "overlay_install_failed", &error)
                .await;
            Err(error)
        }
    }
}

#[tauri::command]
pub(crate) async fn reset_overlay_tutorial(state: State<'_, Arc<AppState>>) -> Result<u64, String> {
    let generation = state.request_gamebar_tutorial_reset();
    state
        .record_flight(
            "overlay_tutorial",
            format!("developer_reset_requested generation={generation}"),
        )
        .await;
    Ok(generation)
}

#[tauri::command]
pub(crate) async fn reset_development_onboarding(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<OverlayInstallStatus, String> {
    let channel = state.release_channel().await;
    if channel != "beta" && channel != "dev" {
        return Err("완전 온보딩 초기화는 beta/dev 채널에서만 사용할 수 있습니다.".into());
    }

    state
        .record_flight("overlay_onboarding", "developer_full_reset_requested")
        .await;
    logout_inner(&app, state.inner(), "developer_full_onboarding_reset").await?;

    match gamebar_bridge::reset_overlay_installation_for_development().await {
        Ok(status) => {
            let generation = state.request_gamebar_tutorial_reset();
            state
                .record_flight(
                    "overlay_onboarding",
                    format!("developer_full_reset_completed generation={generation}"),
                )
                .await;
            Ok(status)
        }
        Err(error) => {
            state
                .record_flight("overlay_onboarding_error", &error)
                .await;
            state
                .report_diagnostic("updater", "overlay_install_failed", &error)
                .await;
            Err(error)
        }
    }
}
