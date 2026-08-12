use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};

use crate::{relay::supervisor::RelaySupervisor, state::AppState};

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "yummi-agent-tray";
const OPEN_MENU_ID: &str = "open";
const QUIT_MENU_ID: &str = "quit";

pub(crate) fn setup(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN_MENU_ID, "열기", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT_MENU_ID, "종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = tauri::include_image!("icons/yummibot-desktop.png");
    let opening = Arc::new(AtomicBool::new(false));
    let exiting = Arc::new(AtomicBool::new(false));

    let menu_opening = opening.clone();
    let menu_exiting = exiting.clone();
    let tray_opening = opening;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Yummi LCU Agent")
        .on_menu_event(move |app, event| match event.id.as_ref() {
            OPEN_MENU_ID => request_main_window(app, menu_opening.clone()),
            QUIT_MENU_ID => request_exit(app, menu_exiting.clone()),
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
                request_main_window(tray.app_handle(), tray_opening.clone());
            }
        })
        .build(app)?;
    Ok(())
}

fn request_main_window(app: &AppHandle, opening: Arc<AtomicBool>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    if opening.swap(true, Ordering::AcqRel) {
        return;
    }

    // WebView2 can deadlock when a window is built inside a synchronous tray callback.
    let app = app.clone();
    std::thread::spawn(move || {
        let result = create_main_window(&app);
        opening.store(false, Ordering::Release);
        if let Err(error) = result {
            eprintln!("main window creation failed: {error}");
        }
    });
}

fn create_main_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
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
            .build()?
    };
    window.set_focus()?;
    Ok(())
}

fn request_exit(app: &AppHandle, exiting: Arc<AtomicBool>) {
    if exiting.swap(true, Ordering::AcqRel) {
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(state) = app.try_state::<Arc<AppState>>() {
            state.begin_shutdown();
            RelaySupervisor::stop(&app, state.inner()).await;
        }
        app.exit(0);
    });
}
