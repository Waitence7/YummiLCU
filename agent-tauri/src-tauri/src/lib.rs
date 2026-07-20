mod app;
mod commands;
mod config;
mod error;
mod lcu;
mod platform;
mod relay;
mod session;
mod state;
mod updater;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), tauri::Error> {
    app::run()
}
