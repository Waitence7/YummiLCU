use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, OnceLock,
};
use std::time::{Duration, Instant};

// Every toggle updates the same persisted pin/click-through settings. Coalesce
// rapid requests so an old UIA traversal cannot clog the queue for minutes.
static CONTROL_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(0);

fn control_apply_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

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
fn launch_windows_game_bar() -> Result<&'static str, String> {
    use windows::{
        core::w,
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };

    // Game Bar is a packaged Windows application. Starting its GameBar.exe
    // directly bypasses package activation and is not reliable across updates.
    // Use Microsoft's Game Bar launch URI instead of synthesizing Win+G.
    // The explicit launch command is preferred; the plain URI is a fallback
    // for older package handlers. ShellExecuteW reports dispatch errors.
    for (uri, name) in [
        (w!("ms-gamebar://launch"), "ms-gamebar://launch"),
        (w!("ms-gamebar:"), "ms-gamebar:"),
    ] {
        let result = unsafe { ShellExecuteW(None, w!("open"), uri, None, None, SW_SHOWNORMAL) };
        if (result.0 as isize) > 32 {
            return Ok(name);
        }
    }
    Err("Xbox Game Bar 실행 요청 실패: ms-gamebar 프로토콜이 등록되어 있는지 확인하세요.".into())
}

#[cfg(not(windows))]
fn launch_windows_game_bar() -> Result<&'static str, String> {
    Err("Xbox Game Bar 열기는 Windows에서만 지원됩니다.".into())
}

#[tauri::command]
pub(crate) async fn open_windows_game_bar(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state
        .record_flight("overlay", "game_bar_open_requested")
        .await;
    let result = launch_windows_game_bar();
    match &result {
        Ok(uri) => {
            state
                .record_flight("overlay", format!("game_bar_protocol_dispatched uri={uri}"))
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
    result.map(|_| ())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OverlayControlsStatus {
    supported: bool,
    game_bar_found: bool,
    yummi_selected: bool,
    pin: Option<bool>,
    click_through: Option<bool>,
    pin_source: Option<String>,
    click_through_source: Option<String>,
    widget_report_age_secs: Option<u64>,
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
        if probe.is_none() {
            Some("현재 설정은 저장되어 있습니다. Game Bar 실제 상태는 ‘상태 새로고침’에서 확인할 수 있습니다.".into())
        } else if !game_bar_found {
            Some("Game Bar 상태를 UI 자동화에서 찾지 못했습니다. 위젯을 연 뒤 다시 확인해 주세요.".into())
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
        pin_source: pin.map(|_| "uia".to_string()),
        click_through_source: click_through.map(|_| "uia".to_string()),
        widget_report_age_secs: None,
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

#[cfg(windows)]
fn read_widget_last_report() -> Option<(Value, u64)> {
    use std::time::SystemTime;

    // The UWP widget writes its own authoritative host SDK state into
    // LocalState/hud-status.json. The desktop UI can read this report even
    // when the Game Bar titlebar is hidden or UI Automation cannot attach.
    // Treat it as last-reported state, never as a verified live UIA result.
    let package_root = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Packages");
    let mut latest: Option<(SystemTime, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(package_root).ok()?.flatten().take(512) {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("YummiGameBarWidget_") {
            continue;
        }
        let path = entry.path().join("LocalState").join("hud-status.json");
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > 64 * 1024 {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if latest
            .as_ref()
            .is_none_or(|(previous, _)| modified > *previous)
        {
            latest = Some((modified, path));
        }
    }
    let (modified, path) = latest?;
    let age_secs = modified.elapsed().ok()?.as_secs();
    // Do not represent a report from a previous day as the current state.
    if age_secs > 6 * 60 * 60 {
        return None;
    }
    let report: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    report.get("pinned")?.as_bool()?;
    report.get("clickThroughEnabled")?.as_bool()?;
    Some((report, age_secs))
}

#[cfg(not(windows))]
fn read_widget_last_report() -> Option<(Value, u64)> {
    None
}

fn enrich_with_widget_report(mut status: OverlayControlsStatus) -> OverlayControlsStatus {
    if status.pin.is_some() && status.click_through.is_some() {
        return status;
    }
    let Some((report, age)) = read_widget_last_report() else {
        return status;
    };
    if status.pin.is_none() {
        status.pin = report.get("pinned").and_then(Value::as_bool);
        if status.pin.is_some() {
            status.pin_source = Some("widget_last_report".into());
        }
    }
    if status.click_through.is_none() {
        status.click_through = report.get("clickThroughEnabled").and_then(Value::as_bool);
        if status.click_through.is_some() {
            status.click_through_source = Some("widget_last_report".into());
        }
    }
    status.widget_report_age_secs = Some(age);
    status.message = Some(format!(
        "Game Bar 제목줄에서 상태를 읽을 수 없어 위젯의 마지막 보고({age}초 전)를 표시합니다. 실시간 상태가 아닐 수 있습니다."
    ));
    status
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
    force_live: Option<bool>,
) -> Result<OverlayControlsStatus, String> {
    let config = state.config.read().await.clone();
    // A full desktop-wide UIA traversal can take a long time (or fail if Game
    // Bar isn't focused). Never gate the initial toggle UI on it.
    let probe = if force_live.unwrap_or(false) {
        probe_controls(None, None, false).await
    } else {
        Ok(None)
    };
    Ok(enrich_with_widget_report(match probe {
        Ok(data) => controls_from_probe(&config, data.as_ref(), None),
        Err(error) => controls_from_probe(
            &config,
            None,
            Some(format!("Game Bar 상태 확인 실패: {error}")),
        ),
    }))
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
    let request_generation = CONTROL_REQUEST_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    drop(_guard);

    // The settings write is the user-visible operation and must complete
    // promptly. Game Bar UIA can enumerate thousands of desktop elements, so
    // apply it *asynchronously* instead of holding the Tauri command open and
    // leaving the toggle visually disabled for the entire probe.
    let apply_state = state.inner().clone();
    let apply_control = control.clone();
    tauri::async_runtime::spawn(async move {
        let requested_at = Instant::now();
        // Rapid ON/OFF/ON changes need only one traversal with the latest
        // persisted preferences. Do not hold the UIA lock during the debounce.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _apply_guard = control_apply_lock().lock().await;
        if request_generation != CONTROL_REQUEST_GENERATION.load(Ordering::SeqCst) {
            apply_state
                .record_flight(
                    "overlay",
                    format!("control_superseded control={apply_control} generation={request_generation}"),
                )
                .await;
            return;
        }
        let queue_wait_ms = requested_at.elapsed().as_millis();
        let probe_started = Instant::now();
        // The native bridge loads the latest preferences while holding its
        // UIA mutex, so an older running probe cannot replay a stale state.
        let mut result = probe_controls(None, None, true).await;
        if result
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .and_then(|data| data.get("selectionInvoked"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            tokio::time::sleep(Duration::from_millis(350)).await;
            // A newer toggle may arrive while Game Bar switches widgets.
            // Let its queued job perform the refreshed lookup instead.
            if request_generation != CONTROL_REQUEST_GENERATION.load(Ordering::SeqCst) {
                apply_state.record_flight(
                    "overlay",
                    format!("control_superseded_after_select control={apply_control} generation={request_generation}"),
                ).await;
                return;
            }
            result = probe_controls(None, None, true).await;
        }
        let probe_ms = probe_started.elapsed().as_millis();
        match result {
            Ok(Some(data)) => {
                let latest = Config::load();
                // A single traversal applies both preferences. Report both
                // outcomes even if the most recent request changed only one.
                for (name, field, expected) in [
                    (
                        "pin",
                        "pin",
                        Some(latest.overlay_pin_preference.unwrap_or(true)),
                    ),
                    (
                        "clickThrough",
                        "clickThrough",
                        Some(latest.overlay_click_through_preference.unwrap_or(true)),
                    ),
                ] {
                    let actual = ui_toggle_state(&data, field);
                    let toggled = data
                        .get(field)
                        .and_then(|value| value.get("toggled"))
                        .and_then(Value::as_bool);
                    let event = if actual.is_some() && actual == expected {
                        "control_applied"
                    } else {
                        "control_pending"
                    };
                    apply_state.record_flight("overlay", format!(
                        "{event} control={name} trigger={apply_control} requested={expected:?} actual={actual:?} toggleInvoked={toggled:?} selected={} queueWaitMs={queue_wait_ms} probeMs={probe_ms} totalMs={}",
                        data.get("yummiSelected").and_then(Value::as_bool).unwrap_or(false),
                        requested_at.elapsed().as_millis(),
                    )).await;
                }
            }
            Ok(None) => {}
            Err(error) => {
                apply_state
                    .report_diagnostic(
                        "overlay",
                        "control_apply_failed",
                        format!("control={apply_control} queueWaitMs={queue_wait_ms} probeMs={probe_ms} error={error}"),
                    )
                    .await;
            }
        }
    });

    let mut result = controls_from_probe(&config, None, None);
    result = enrich_with_widget_report(result);
    result.message = Some("설정 저장 완료 · Game Bar에 적용을 요청했습니다. 실제 반영 여부는 상태 새로고침으로 확인하세요.".into());
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
