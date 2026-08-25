use tauri::AppHandle;

use crate::{
    platform::open_beta_download_url,
    updater::{beta_release_info, BetaReleaseInfo},
};

#[tauri::command]
pub(crate) async fn get_beta_release_info() -> Result<BetaReleaseInfo, String> {
    beta_release_info().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn open_beta_download(app: AppHandle) -> Result<(), String> {
    open_beta_download_url(&app).map_err(|error| error.to_string())
}
