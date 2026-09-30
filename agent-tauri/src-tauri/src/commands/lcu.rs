use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::{
    lcu::{lockfile_path, replay_for_viewer, LcuClient},
    state::AppState,
};

#[tauri::command]
pub(crate) async fn recent_match(state: State<'_, Arc<AppState>>) -> Result<Value, String> {
    let config = state.config.read().await.clone();
    let path = lockfile_path(&config).ok_or("League Client가 실행 중이 아닙니다.")?;
    let client = LcuClient::from_lockfile(&path)
        .or_else(|_| LcuClient::from_lockfile_legacy(&path))
        .map_err(|error| error.to_string())?;
    client
        .recent_match()
        .await
        .map_err(|error| error.to_string())
}

async fn history_client(state: &Arc<AppState>) -> Result<LcuClient, String> {
    let config = state.config.read().await.clone();
    let path = lockfile_path(&config).ok_or("롤 클라이언트를 실행한 뒤 다시 확인하세요.")?;
    LcuClient::from_lockfile(&path)
        .or_else(|_| LcuClient::from_lockfile_legacy(&path))
        .map_err(|error| error.to_string())
}

fn validate_game_id(game_id: &str) -> Result<(), String> {
    if game_id.is_empty() || game_id.len() > 20 || !game_id.bytes().all(|c| c.is_ascii_digit()) {
        return Err("올바른 경기 ID가 아닙니다.".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn match_history(
    state: State<'_, Arc<AppState>>,
    offset: u32,
) -> Result<Value, String> {
    history_client(state.inner())
        .await?
        .match_history(offset)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn match_replay(
    state: State<'_, Arc<AppState>>,
    game_id: String,
) -> Result<Value, String> {
    validate_game_id(&game_id)?;
    // Existing local files remain viewable if the League Client is closed.
    let replay_dir = match history_client(state.inner()).await {
        Ok(client) => client.replay_directory().await.ok().flatten(),
        Err(_) => None,
    };
    tokio::task::spawn_blocking(move || replay_for_viewer(&game_id, replay_dir.as_deref()))
        .await
        .map_err(|_| "리플레이를 읽는 중 오류가 발생했습니다.".to_owned())?
}

#[tauri::command]
pub(crate) async fn download_match_replay(
    state: State<'_, Arc<AppState>>,
    game_id: String,
) -> Result<(), String> {
    validate_game_id(&game_id)?;
    history_client(state.inner()).await?.download_history_replay(&game_id).await
        .map_err(|_| "리플레이를 다운로드하지 못했습니다. 롤 클라이언트에서 해당 경기의 다운로드 가능 여부를 확인하세요.".to_owned())?;
    Ok(())
}
