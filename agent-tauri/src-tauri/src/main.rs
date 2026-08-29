#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if let Err(error) = yummi_lcu_tauri_lib::run() {
        yummi_lcu_tauri_lib::write_bootstrap_error(&format!("tauri_run_failed: {error}"));
    }
}
