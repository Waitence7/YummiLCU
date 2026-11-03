use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    session,
    state::{AppState, GameBarToast, GAMEBAR_TOAST_STICKY_EXPIRES_AT_MS},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveControlStatus {
    enabled: bool,
    profile: String,
    #[serde(default = "default_live_fps")]
    fps: u32,
    #[serde(default)]
    share_url: Option<String>,
    #[serde(default)]
    expires_at: Option<u64>,
}

#[derive(Serialize)]
struct LiveControlRequest<'a> {
    enabled: bool,
    profile: &'a str,
    fps: u32,
}

fn default_live_fps() -> u32 {
    30
}

fn valid_profile(value: &str) -> bool {
    matches!(value, "data" | "balanced" | "high" | "ultra")
}

fn valid_fps(value: u32) -> bool {
    matches!(value, 15 | 30 | 60)
}

fn live_profile_label(value: &str) -> &str {
    match value {
        "data" => "540p",
        "balanced" => "720p",
        "high" => "1080p",
        "ultra" => "1440p",
        _ => value,
    }
}

fn sync_live_overlay_toast(state: &Arc<AppState>, status: &LiveControlStatus) {
    const TOAST_ID: &str = "recording.live.active";
    if !status.enabled {
        state.remove_gamebar_toast(TOAST_ID);
        return;
    }
    state.push_gamebar_toast(GameBarToast {
        id: TOAST_ID.into(),
        source: "recording.live".into(),
        title: "실시간 공유 중".into(),
        body: format!("{} · {}fps", live_profile_label(&status.profile), status.fps),
        tone: "success".into(),
        timestamp_ms: 0,
        expires_at_ms: GAMEBAR_TOAST_STICKY_EXPIRES_AT_MS,
    });
}

async fn request_live_control(
    state: &Arc<AppState>,
    enabled: Option<bool>,
    profile: &str,
    fps: u32,
) -> Result<LiveControlStatus, String> {
    if !valid_profile(profile) {
        return Err("지원하지 않는 라이브 화질입니다.".into());
    }
    if !valid_fps(fps) {
        return Err("지원하지 않는 라이브 FPS입니다.".into());
    }
    let config = state.config.read().await.clone();
    let saved = session::load(&config).ok_or_else(|| "Yummi Relay 로그인 세션이 없습니다.".to_string())?;
    let url = config.live_control_url(&saved.session_id).map_err(|error| error.to_string())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("라이브 제어 HTTP 초기화 실패: {error}"))?;
    let request = match enabled {
        Some(enabled) => client
            .post(url)
            .header("x-yummi-ws-token", &saved.ws_token)
            .json(&LiveControlRequest { enabled, profile, fps }),
        None => client.get(url).header("x-yummi-ws-token", &saved.ws_token),
    };
    let response = request
        .send()
        .await
        .map_err(|error| format!("라이브 제어 요청 실패: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        return Err(format!("라이브 제어 실패 ({status}): {}", detail.chars().take(180).collect::<String>()));
    }
    response
        .json::<LiveControlStatus>()
        .await
        .map_err(|error| format!("라이브 제어 응답 해석 실패: {error}"))
}

pub(crate) async fn refresh_live_control_status(
    state: &Arc<AppState>,
) -> Result<LiveControlStatus, String> {
    let status = request_live_control(state, None, "balanced", default_live_fps()).await?;
    sync_live_overlay_toast(state, &status);
    Ok(status)
}

#[tauri::command]
pub(crate) async fn get_live_control_status(
    state: State<'_, Arc<AppState>>,
) -> Result<LiveControlStatus, String> {
    refresh_live_control_status(state.inner()).await
}

#[tauri::command]
pub(crate) async fn set_live_control_enabled(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
    profile: String,
    fps: u32,
) -> Result<LiveControlStatus, String> {
    let profile = profile.trim().to_ascii_lowercase();
    let status = request_live_control(state.inner(), Some(enabled), &profile, fps).await?;
    sync_live_overlay_toast(state.inner(), &status);
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_profiles_are_bounded() {
        assert!(valid_profile("balanced"));
        assert!(valid_profile("ultra"));
        assert!(!valid_profile("4k"));
        assert!(valid_fps(15));
        assert!(valid_fps(30));
        assert!(valid_fps(60));
        assert!(!valid_fps(45));
    }
}
