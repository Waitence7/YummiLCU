use std::{fs, sync::Arc};

use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub(crate) async fn get_diagnostic_bundle(
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    Ok(state.diagnostic_bundle().await)
}

#[tauri::command]
pub(crate) async fn export_diagnostic_bundle(
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let bundle = state.diagnostic_bundle().await;
    let directory = dirs::download_dir().ok_or("다운로드 폴더를 찾을 수 없습니다.")?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = directory.join(format!("yummi-agent-diagnostics-{timestamp}.txt"));
    fs::write(&path, bundle).map_err(|_| "진단 파일 저장에 실패했습니다.".to_string())?;
    Ok(path.to_string_lossy().into_owned())
}
