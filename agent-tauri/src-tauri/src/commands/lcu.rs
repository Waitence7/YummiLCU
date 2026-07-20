use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::{
    lcu::{lockfile_path, LcuClient},
    state::AppState,
};

#[tauri::command]
pub(crate) async fn recent_match(state: State<'_, Arc<AppState>>) -> Result<Value, String> {
    let config = state.config.read().await.clone();
    let path = lockfile_path(&config).ok_or("League Client가 실행 중이 아닙니다.")?;
    let client = LcuClient::from_lockfile(&path).map_err(|error| error.to_string())?;
    client
        .recent_match()
        .await
        .map_err(|error| error.to_string())
}
