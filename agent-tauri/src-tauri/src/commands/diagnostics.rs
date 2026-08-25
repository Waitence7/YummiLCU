use std::{fs, sync::Arc};

use tauri::{AppHandle, State};

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

#[tauri::command]
pub(crate) async fn report_unexpected_error(
    state: State<'_, Arc<AppState>>,
    code: String,
    summary: String,
) -> Result<(), String> {
    let code = match code.as_str() {
        "uncaught_error" => "uncaught_error",
        "unhandled_rejection" => "unhandled_rejection",
        _ => return Err("지원하지 않는 UI 오류 코드입니다.".into()),
    };
    state.report_unexpected_error("ui", code, summary).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn report_tray_effect_diagnostic(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    code: String,
    detail: String,
) -> Result<(), String> {
    let code = validate_tray_effect_diagnostic_code(&code)
        .ok_or_else(|| "지원하지 않는 트레이 효과 진단 코드입니다.".to_string())?;
    let detail = sanitize_tray_effect_detail(&detail);
    let message = if code == "ready" {
        format!("HTML-in-Canvas 활성화: {detail}")
    } else {
        format!("HTML-in-Canvas fallback ({code}): {detail}")
    };
    state
        .record_flight("html_canvas", format!("{code}: {detail}"))
        .await;
    state.log(&app, message).await;
    Ok(())
}

fn validate_tray_effect_diagnostic_code(code: &str) -> Option<&'static str> {
    match code {
        "surface_invalid" => Some("surface_invalid"),
        "webgl2_unavailable" => Some("webgl2_unavailable"),
        "api_unavailable" => Some("api_unavailable"),
        "shader_compile_failed" => Some("shader_compile_failed"),
        "program_link_failed" => Some("program_link_failed"),
        "mesh_failed" => Some("mesh_failed"),
        "texture_failed" => Some("texture_failed"),
        "snapshot_failed" => Some("snapshot_failed"),
        "shader_bindings_missing" => Some("shader_bindings_missing"),
        "ready" => Some("ready"),
        _ => None,
    }
}

fn sanitize_tray_effect_detail(detail: &str) -> String {
    detail
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(400)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{sanitize_tray_effect_detail, validate_tray_effect_diagnostic_code};

    #[test]
    fn tray_effect_diagnostics_accept_only_known_codes() {
        assert_eq!(validate_tray_effect_diagnostic_code("ready"), Some("ready"));
        assert_eq!(
            validate_tray_effect_diagnostic_code("snapshot_failed"),
            Some("snapshot_failed")
        );
        assert_eq!(validate_tray_effect_diagnostic_code("arbitrary"), None);
    }

    #[test]
    fn tray_effect_diagnostic_detail_is_single_line_and_bounded() {
        let detail = format!("shader\nfailed {}", "x".repeat(500));
        let sanitized = sanitize_tray_effect_detail(&detail);
        assert!(!sanitized.contains('\n'));
        assert!(sanitized.chars().count() <= 400);
    }
}
