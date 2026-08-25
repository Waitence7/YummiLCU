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
const TRAY_ID: &str = "yummi-agent-tray";
const OPEN_MENU_ID: &str = "open";
const QUIT_MENU_ID: &str = "quit";
static OPENING_MAIN_WINDOW: AtomicBool = AtomicBool::new(false);
static EXITING: AtomicBool = AtomicBool::new(false);
static HIDE_REQUEST_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn setup(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN_MENU_ID, "열기", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_MENU_ID, "종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = tauri::include_image!("icons/yummibot-desktop.png");
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
        let _ = window.unminimize();
        let _ = window.set_skip_taskbar(false);
        let _ = window.show();
        let _ = window.set_focus();
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
            if let Some(state) = app.try_state::<Arc<AppState>>() {
                let state = state.inner().clone();
                let summary = error.to_string();
                tauri::async_runtime::spawn(async move {
                    state
                        .report_unexpected_error("ui", "window_creation_failed", summary)
                        .await;
                });
            }
        }
    });
}

pub(crate) fn remove(app: &AppHandle) {
    let _ = app.remove_tray_by_id(TRAY_ID);
}

pub(crate) fn hide_main_window(app: &AppHandle) {
    cancel_pending_hide();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.set_skip_taskbar(true);
        let _ = window.hide();
    }
}

pub(crate) fn request_animated_hide(app: &AppHandle) {
    let request_id = HIDE_REQUEST_ID.fetch_add(1, Ordering::AcqRel) + 1;
    let emitted = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .is_some_and(|window| window.emit("yummi://tray-hide-requested", ()).is_ok());
    if emitted {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
            destroy_main_window_if_pending(&app, request_id);
        });
    } else {
        destroy_main_window_if_pending(app, request_id);
    }
}

pub(crate) fn destroy_main_window(app: &AppHandle) {
    cancel_pending_hide();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.destroy();
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
        WebviewWindowBuilder::from_config(app, config)?.build()?
    } else {
        WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
            .title("Yummi LCU Agent")
            .inner_size(640.0, 620.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .build()?
    };
    window.set_skip_taskbar(false)?;
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    Ok(())
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
