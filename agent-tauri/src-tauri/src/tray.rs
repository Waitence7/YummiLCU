use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};

use crate::{relay::supervisor::RelaySupervisor, state::AppState};

const MAIN_WINDOW_LABEL: &str = "main";
#[cfg(any(windows, test))]
const HTML_CANVAS_BROWSER_ARGS: &str = "--enable-blink-features=CanvasDrawElement --enable-experimental-web-platform-features --enable-features=CanvasDrawElement";
const TRAY_ID: &str = "yummi-agent-tray";
const OPEN_MENU_ID: &str = "open";
const QUIT_MENU_ID: &str = "quit";
static OPENING_MAIN_WINDOW: AtomicBool = AtomicBool::new(false);
static EXITING: AtomicBool = AtomicBool::new(false);
static HIDE_REQUEST_ID: AtomicU64 = AtomicU64::new(0);

fn report_ui_error(app: &AppHandle, code: &'static str, error: impl ToString) {
    let summary = error.to_string();
    let app = app.clone();
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        let state = state.inner().clone();
        tauri::async_runtime::spawn(async move {
            state
                .record_flight("ui_error", format!("{code}: {summary}"))
                .await;
            state
                .log(&app, format!("UI/트레이 오류 ({code}): {summary}"))
                .await;
            state.report_unexpected_error("ui", code, summary).await;
        });
    }
}

pub(crate) fn setup(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN_MENU_ID, "열기", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_MENU_ID, "종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = tauri::include_image!("icons/yummibot-tray.png");
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Yummi LCU Agent")
        .on_menu_event(move |app, event| match event.id.as_ref() {
            OPEN_MENU_ID => request_main_window(app),
            QUIT_MENU_ID => request_exit(app),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                request_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub(crate) fn request_main_window(app: &AppHandle) {
    cancel_pending_hide();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Err(error) = window.unminimize() {
            report_ui_error(app, "window_unminimize_failed", error);
        }
        if let Err(error) = window.set_skip_taskbar(false) {
            report_ui_error(app, "window_taskbar_restore_failed", error);
        }
        if let Err(error) = window.show() {
            report_ui_error(app, "window_show_failed", error);
        }
        if let Err(error) = window.set_focus() {
            report_ui_error(app, "window_focus_failed", error);
        }
        return;
    }
    if OPENING_MAIN_WINDOW.swap(true, Ordering::AcqRel) {
        return;
    }

    // WebView2 can deadlock when a window is built inside a synchronous tray callback.
    let app = app.clone();
    std::thread::spawn(move || {
        let result = create_main_window(&app);
        OPENING_MAIN_WINDOW.store(false, Ordering::Release);
        if let Err(error) = result {
            eprintln!("main window creation failed: {error}");
            report_ui_error(&app, "window_creation_failed", error);
        }
    });
}

pub(crate) fn remove(app: &AppHandle) {
    let _ = app.remove_tray_by_id(TRAY_ID);
}

pub(crate) fn hide_main_window(app: &AppHandle) {
    cancel_pending_hide();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Err(error) = window.set_skip_taskbar(true) {
            report_ui_error(app, "window_taskbar_hide_failed", error);
        }
        if let Err(error) = window.hide() {
            report_ui_error(app, "window_hide_failed", error);
        }
    }
}

pub(crate) fn request_animated_hide(app: &AppHandle) {
    let request_id = HIDE_REQUEST_ID.fetch_add(1, Ordering::AcqRel) + 1;
    let playback_rate = app
        .try_state::<Arc<AppState>>()
        .and_then(|state| {
            state
                .config
                .try_read()
                .ok()
                .map(|config| config.tray_effect_playback_rate)
        })
        .unwrap_or(1.0);
    let watchdog_ms = tray_hide_watchdog_ms(playback_rate);
    let emitted = if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        match window.emit("yummi://tray-hide-requested", ()) {
            Ok(()) => true,
            Err(error) => {
                report_ui_error(app, "tray_hide_event_emit_failed", error);
                false
            }
        }
    } else {
        false
    };
    if emitted {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(watchdog_ms)).await;
            destroy_main_window_if_pending(&app, request_id);
        });
    } else {
        destroy_main_window_if_pending(app, request_id);
    }
}

fn tray_hide_watchdog_ms(playback_rate: f64) -> u64 {
    let rate = playback_rate.clamp(0.1, 4.0);
    // book-return is currently the longest effect at 1040 ms. Keep extra time
    // for snapshot acquisition, the post-effect pause and slower machines.
    ((1_200.0 / rate).ceil() as u64 + 400).clamp(1_500, 12_500)
}

pub(crate) fn destroy_main_window(app: &AppHandle) {
    cancel_pending_hide();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Err(error) = window.destroy() {
            report_ui_error(app, "window_destroy_failed", error);
        }
    }
}

fn destroy_main_window_if_pending(app: &AppHandle, request_id: u64) {
    if HIDE_REQUEST_ID.load(Ordering::Acquire) == request_id {
        destroy_main_window(app);
    }
}

fn cancel_pending_hide() {
    HIDE_REQUEST_ID.fetch_add(1, Ordering::AcqRel);
}

fn create_main_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window.set_skip_taskbar(false)?;
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
        return Ok(());
    }

    let window = if let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == MAIN_WINDOW_LABEL)
    {
        let builder = WebviewWindowBuilder::from_config(app, config)?;
        #[cfg(windows)]
        let builder = if html_canvas_experiment_enabled() {
            builder.additional_browser_args(HTML_CANVAS_BROWSER_ARGS)
        } else {
            builder
        };
        builder.build()?
    } else {
        let builder =
            WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
                .title("Yummi LCU Agent")
                .inner_size(640.0, 620.0)
                .resizable(false)
                .decorations(false)
                .transparent(true)
                .shadow(false);
        #[cfg(windows)]
        let builder = if html_canvas_experiment_enabled() {
            builder.additional_browser_args(HTML_CANVAS_BROWSER_ARGS)
        } else {
            builder
        };
        builder.build()?
    };
    window.set_skip_taskbar(false)?;
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    Ok(())
}

#[cfg(windows)]
fn html_canvas_experiment_enabled() -> bool {
    html_canvas_experiment_enabled_for_channel(
        option_env!("YUMMI_AGENT_RELEASE_CHANNEL").unwrap_or("stable"),
    )
}

#[cfg(any(windows, test))]
fn html_canvas_experiment_enabled_for_channel(channel: &str) -> bool {
    // HTML-in-Canvas is now part of the normal tray effect path. Enable the
    // Chromium feature for every supported release channel, including stable.
    // Runtime capability is still checked in the frontend; unsupported
    // WebView2 versions fall back to page-curl/fold without blocking hiding.
    matches!(channel.trim(), "stable" | "beta" | "dev")
}

pub(crate) fn request_exit(app: &AppHandle) {
    if EXITING.swap(true, Ordering::AcqRel) {
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        remove(&app);
        if let Some(state) = app.try_state::<Arc<AppState>>() {
            state.begin_shutdown();
            RelaySupervisor::stop(&app, state.inner()).await;
        }
        app.exit(0);
    });
}

#[cfg(test)]
mod tests {
    use super::{
        html_canvas_experiment_enabled_for_channel, tray_hide_watchdog_ms,
        HTML_CANVAS_BROWSER_ARGS,
    };

    #[test]
    fn html_canvas_browser_flag_is_enabled_for_supported_release_channels() {
        assert!(html_canvas_experiment_enabled_for_channel("stable"));
        assert!(html_canvas_experiment_enabled_for_channel("beta"));
        assert!(html_canvas_experiment_enabled_for_channel("dev"));
        assert!(!html_canvas_experiment_enabled_for_channel("nightly"));
    }

    #[test]
    fn html_canvas_browser_args_enable_blink_runtime_feature_directly() {
        assert!(HTML_CANVAS_BROWSER_ARGS.contains("--enable-blink-features=CanvasDrawElement"));
        assert!(HTML_CANVAS_BROWSER_ARGS.contains("--enable-experimental-web-platform-features"));
        assert!(HTML_CANVAS_BROWSER_ARGS.contains("--enable-features=CanvasDrawElement"));
    }

    #[test]
    fn tray_hide_watchdog_tracks_debug_playback_rate() {
        assert_eq!(tray_hide_watchdog_ms(1.0), 1_600);
        assert_eq!(tray_hide_watchdog_ms(4.0), 1_500);
        assert_eq!(tray_hide_watchdog_ms(0.1), 12_400);
    }
}
