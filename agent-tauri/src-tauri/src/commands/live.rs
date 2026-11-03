use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{session, state::AppState};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveControlStatus {
    enabled: bool,
    profile: String,
    #[serde(default)]
    share_url: Option<String>,
    #[serde(default)]
    expires_at: Option<u64>,
}

#[derive(Serialize)]
struct LiveControlRequest<'a> {
    enabled: bool,
    profile: &'a str,
}

fn valid_profile(value: &str) -> bool {
    matches!(value, "data" | "balanced" | "high" | "ultra")
}

async fn request_live_control(
    state: &Arc<AppState>,
    enabled: Option<bool>,
    profile: &str,
) -> Result<LiveControlStatus, String> {
    if !valid_profile(profile) {
        return Err("지원하지 않는 라이브 화질입니다.".into());
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
            .json(&LiveControlRequest { enabled, profile }),
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

#[tauri::command]
pub(crate) async fn get_live_control_status(
    state: State<'_, Arc<AppState>>,
) -> Result<LiveControlStatus, String> {
    request_live_control(state.inner(), None, "balanced").await
}

#[tauri::command]
pub(crate) async fn set_live_control_enabled(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
    profile: String,
) -> Result<LiveControlStatus, String> {
    let profile = profile.trim().to_ascii_lowercase();
    request_live_control(state.inner(), Some(enabled), &profile).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_profiles_are_bounded() {
        assert!(valid_profile("balanced"));
        assert!(valid_profile("ultra"));
        assert!(!valid_profile("4k"));
    }
}
