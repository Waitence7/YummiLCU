use tauri::AppHandle;

use crate::tray;

#[tauri::command]
pub(crate) fn hide_main_window(app: AppHandle) {
    tray::hide_main_window(&app);
}
