use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::{AppHandle, Manager, State};
#[cfg(windows)]
use tauri::{
    Emitter, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::state::{AppState, GameBarSnapshot};

pub(crate) const LABEL: &str = "fullscreen-hud";
static CHANGING: AtomicBool = AtomicBool::new(false);

pub(crate) fn restore(app: &AppHandle) {
    let enabled = app
        .try_state::<Arc<AppState>>()
        .and_then(|state| {
            state
                .config
                .try_read()
                .ok()
                .map(|config| config.fullscreen_hud_enabled)
        })
        .unwrap_or(false);
    if enabled {
        request(app, true);
    }
}

pub(crate) fn toggle(app: &AppHandle) {
    request(app, app.get_webview_window(LABEL).is_none());
}

pub(crate) fn request(app: &AppHandle, enabled: bool) {
    if CHANGING.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // WebView2 creation must not run inside a synchronous tray callback.
        let window_app = app.clone();
        let result =
            tauri::async_runtime::spawn_blocking(move || set_visible(&window_app, enabled)).await;
        let result = result
            .map_err(|error| error.to_string())
            .and_then(|result| result);
        if let Some(state) = app.try_state::<Arc<AppState>>() {
            match result {
                Ok(()) => {
                    let mut config = state.config.write().await;
                    if config.fullscreen_hud_enabled != enabled {
                        let mut next = config.clone();
                        next.fullscreen_hud_enabled = enabled;
                        match next.save() {
                            Ok(()) => *config = next,
                            Err(error) => {
                                drop(config);
                                state
                                    .report_unexpected_error(
                                        "hud",
                                        "preference_save_failed",
                                        error.to_string(),
                                    )
                                    .await;
                                CHANGING.store(false, Ordering::Release);
                                return;
                            }
                        }
                    }
                }
                Err(error) => {
                    state.log(&app, format!("전체화면 HUD 실패: {error}")).await;
                    state
                        .report_unexpected_error("hud", "window_failed", error)
                        .await;
                }
            }
        }
        CHANGING.store(false, Ordering::Release);
    });
}

#[cfg(windows)]
fn set_visible(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if !enabled {
        if let Some(window) = app.get_webview_window(LABEL) {
            window.destroy().map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        fit_to_monitor(&window)?;
        return Ok(());
    }
    // Keep the window hidden until geometry and OS-level input passthrough are
    // applied. No normal app/taskbar entry, activation, or input interception.
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Yummi HUD")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .resizable(false)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;
    let result = (|| {
        // Ignore the entire native window, including the rendered HUD panel.
        window
            .set_ignore_cursor_events(true)
            .map_err(|error| error.to_string())?;
        fit_to_monitor(&window)?;
        window.show().map_err(|error| error.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = window.destroy();
    }
    result
}

#[cfg(not(windows))]
fn set_visible(_app: &AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        Err("전체화면 HUD는 Windows에서 지원됩니다.".into())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
pub(crate) fn fit_to_monitor(window: &WebviewWindow) -> Result<(), String> {
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or(window
            .primary_monitor()
            .map_err(|error| error.to_string())?)
        .ok_or_else(|| "HUD 모니터를 찾지 못했습니다.".to_owned())?;
    // Monitor APIs return physical pixels. Never divide these by Windows DPI:
    // set_size/set_position receive the corresponding physical coordinate type.
    let position = *monitor.position();
    let size = *monitor.size();
    window
        .set_position(PhysicalPosition::new(position.x, position.y))
        .map_err(|error| error.to_string())?;
    window
        .set_size(PhysicalSize::new(size.width, size.height))
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn fullscreen_hud_snapshot(state: State<'_, Arc<AppState>>) -> GameBarSnapshot {
    state.gamebar_snapshot()
}

#[tauri::command]
pub(crate) fn fullscreen_hud_ready(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window(LABEL) {
        fit_to_monitor(&window)?;
        let monitor = window
            .current_monitor()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "HUD 모니터를 찾지 못했습니다.".to_owned())?;
        let position = window.outer_position().map_err(|error| error.to_string())?;
        let size = window.inner_size().map_err(|error| error.to_string())?;
        let native = window.hwnd().map_err(|error| error.to_string())?;
        let style = unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                windows::Win32::Foundation::HWND(native.0),
                windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE,
            )
        } as u32;
        let status = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "monitorX": monitor.position().x, "monitorY": monitor.position().y,
            "monitorWidth": monitor.size().width, "monitorHeight": monitor.size().height,
            "x": position.x, "y": position.y, "width": size.width, "height": size.height,
            "scaleFactor": monitor.scale_factor(),
            "coversMonitor": position == *monitor.position() && size == *monitor.size(),
            "mouseTransparent": style & 0x20 != 0,
            "layered": style & 0x80000 != 0,
            "noActivate": style & 0x08000000 != 0,
            "toolWindow": style & 0x80 != 0,
            "ready": true
        });
        let directory = dirs::data_local_dir()
            .ok_or_else(|| "HUD 상태 경로 없음".to_owned())?
            .join("YummiAgent");
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        // Only geometry/input flags; no account or LCU payload.
        std::fs::write(
            directory.join("fullscreen-hud-status.json"),
            status.to_string(),
        )
        .map_err(|error| error.to_string())?;
        if let Some(state) = app.try_state::<Arc<AppState>>() {
            window
                .emit("yummi://hud-snapshot", state.gamebar_snapshot())
                .map_err(|error| error.to_string())?;
        }
    }
    #[cfg(not(windows))]
    let _ = app;
    Ok(())
}
