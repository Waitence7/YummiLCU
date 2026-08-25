use tauri::{AppHandle, Manager};

use crate::tray;

#[tauri::command]
pub(crate) fn hide_main_window(app: AppHandle) {
    tray::hide_main_window(&app);
}

#[tauri::command]
pub(crate) fn complete_tray_hide(app: AppHandle) {
    // Hide only after the visible surface has fully animated away, then destroy
    // the WebView shortly afterwards. The Agent process and tray stay alive.
    tray::hide_main_window(&app);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        tray::destroy_main_window(&app);
    });
}

#[tauri::command]
pub(crate) fn minimize_main_window(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "메인 창을 찾을 수 없습니다.".to_string())?;
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn request_tray_hide(app: AppHandle) {
    tray::request_animated_hide(&app);
}
