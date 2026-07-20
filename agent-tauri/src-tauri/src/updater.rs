use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
    time::Duration,
};

use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tokio::time::sleep;
use url::Url;
use uuid::Uuid;

use crate::{
    error::{AgentError, AgentResult},
    state::AppState,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateTarget {
    version: String,
    url: Option<String>,
    patch_url: Option<String>,
    patch_from: Option<String>,
    sha256: Option<String>,
    patch_sha256: Option<String>,
    executable: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateManifest {
    tauri: Option<UpdateTarget>,
}

const MAX_UPDATE_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_UPDATE_ARCHIVE_BYTES: usize = 256 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4_096;

impl UpdateManifest {
    fn select_tauri(self) -> Option<UpdateTarget> {
        self.tauri
    }
}

fn version_tuple(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.trim();
    if value.is_empty() || value.len() > 32 {
        return None;
    }
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() > 3 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts.get(1).map_or(Some(0), |part| part.parse().ok())?,
        parts.get(2).map_or(Some(0), |part| part.parse().ok())?,
    ))
}

fn is_newer(remote: &str, current: &str) -> bool {
    match (version_tuple(remote), version_tuple(current)) {
        (Some(remote), Some(current)) => remote > current,
        _ => false,
    }
}

fn verify_hash(bytes: &[u8], expected: &str) -> bool {
    let expected = expected.trim();
    let expected = expected.strip_prefix("0x").unwrap_or(expected);
    if expected.len() != 64 {
        return false;
    }
    let mut hash = Sha256::new();
    hash.update(bytes);
    format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected)
}

fn validate_hash(bytes: &[u8], expected: &str) -> AgentResult<()> {
    if verify_hash(bytes, expected) {
        Ok(())
    } else {
        Err(AgentError::Update("자동 업데이트 SHA-256 검증 실패".into()))
    }
}

fn safe_extract<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    output: &Path,
) -> AgentResult<()> {
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(AgentError::Update("업데이트 ZIP 파일 수 제한 초과".into()));
    }
    let mut extracted_bytes = 0_u64;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| AgentError::Update(error.to_string()))?;
        let Some(name) = file.enclosed_name() else {
            continue;
        };
        extracted_bytes = extracted_bytes
            .checked_add(file.size())
            .filter(|size| *size <= MAX_EXTRACTED_BYTES)
            .ok_or_else(|| AgentError::Update("업데이트 ZIP 압축 해제 크기 제한 초과".into()))?;
        let destination = output.join(name);
        if file.is_dir() {
            fs::create_dir_all(&destination)?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output_file = fs::File::create(destination)?;
            std::io::copy(&mut file, &mut output_file)?;
        }
    }
    Ok(())
}

async fn download_limited(client: &Client, url: Url, max_bytes: usize) -> AgentResult<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| AgentError::Update("업데이트 다운로드 연결 실패".into()))?;
    if !response.status().is_success() {
        return Err(AgentError::Update(format!(
            "업데이트 다운로드 실패 (HTTP {})",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(AgentError::Update(
            "업데이트 다운로드 크기 제한 초과".into(),
        ));
    }
    let mut bytes = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| AgentError::Update("업데이트 다운로드 실패".into()))?;
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(AgentError::Update(
                "업데이트 다운로드 크기 제한 초과".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn apply_update(
    manifest: UpdateManifest,
    app: &AppHandle,
    state: &AppState,
) -> AgentResult<bool> {
    let Some(target_manifest) = manifest.select_tauri() else {
        return Ok(false);
    };
    let current_version = env!("CARGO_PKG_VERSION");
    if !is_newer(&target_manifest.version, current_version) {
        return Ok(false);
    }

    let use_patch = target_manifest.patch_from.as_deref() == Some(current_version)
        && target_manifest.patch_url.is_some();
    let url = if use_patch {
        target_manifest.patch_url
    } else {
        target_manifest.url
    };
    let hash = if use_patch {
        target_manifest.patch_sha256
    } else {
        target_manifest.sha256
    };
    let (Some(url), Some(hash)) = (url, hash) else {
        return Ok(false);
    };
    let parsed = url::Url::parse(&url)
        .map_err(|error| AgentError::Update(format!("업데이트 URL 오류: {error}")))?;
    if parsed.scheme() != "https"
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(AgentError::Update(
            "업데이트 다운로드는 인증 정보가 없는 HTTPS URL만 허용됩니다.".into(),
        ));
    }

    state
        .set_update_message(
            app,
            Some(format!(
                "새 버전 {}을 다운로드하고 있습니다. 앱은 설치 후 자동으로 다시 시작됩니다.",
                target_manifest.version
            )),
        )
        .await;
    let client = Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| AgentError::Update("업데이트 HTTP client 생성 실패".into()))?;
    let bytes = download_limited(&client, parsed, MAX_UPDATE_ARCHIVE_BYTES).await?;
    validate_hash(&bytes, &hash)?;

    let work_root = std::env::temp_dir().join("yummi-lcu-update");
    fs::create_dir_all(&work_root)?;
    let work = work_root.join(Uuid::new_v4().to_string());
    fs::create_dir(&work)?;
    let extract = work.join("extract");
    fs::create_dir_all(&extract)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| AgentError::Update(format!("업데이트 ZIP 오류: {error}")))?;
    safe_extract(&mut archive, &extract)?;

    let executable_name = target_manifest
        .executable
        .as_deref()
        .unwrap_or("yummi-lcu-tauri.exe");
    if !executable_name.eq_ignore_ascii_case("yummi-lcu-tauri.exe") {
        return Err(AgentError::Update(
            "Tauri 대상 실행 파일이 올바르지 않습니다.".into(),
        ));
    }
    let source = std::iter::once(extract.join(executable_name))
        .chain(
            fs::read_dir(&extract)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    entry
                        .ok()
                        .map(|entry| entry.path())
                        .filter(|path| path.is_dir())
                        .map(|path| path.join(executable_name))
                }),
        )
        .find(|path| path.exists())
        .ok_or_else(|| AgentError::Update("업데이트 ZIP에 Tauri 실행 파일이 없습니다.".into()))?;
    let target = std::env::current_exe()
        .map_err(|error| AgentError::Update(format!("현재 실행 파일 확인 실패: {error}")))?;
    if !target
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(executable_name))
    {
        return Err(AgentError::Update(
            "현재 설치는 Tauri 배포물이 아닙니다.".into(),
        ));
    }
    let target_dir = target
        .parent()
        .ok_or_else(|| AgentError::Update("앱 폴더를 찾지 못했습니다.".into()))?;
    let script = work.join("apply-update.cmd");
    let source_dir = source
        .parent()
        .ok_or_else(|| AgentError::Update("압축 해제 폴더를 찾지 못했습니다.".into()))?;
    let backup = target.with_extension("exe.bak");
    let script_text = format!(
        "@echo off\r\n\
         timeout /t 2 /nobreak >nul\r\n\
         copy /Y \"{}\" \"{}\" >nul\r\n\
         if errorlevel 1 exit /b 1\r\n\
         robocopy \"{}\" \"{}\" /E /XF agent.json >nul\r\n\
         if errorlevel 8 (\r\n\
           copy /Y \"{}\" \"{}\" >nul\r\n\
           exit /b 1\r\n\
         )\r\n\
         del /Q \"{}\" >nul 2>&1\r\n\
         start \"\" \"{}\"\r\n\
         del \"%~f0\"\r\n",
        target.display(),
        backup.display(),
        source_dir.display(),
        target_dir.display(),
        backup.display(),
        target.display(),
        backup.display(),
        target.display(),
    );
    fs::write(&script, script_text)?;

    state
        .set_update_message(
            app,
            Some("업데이트를 설치하고 있습니다. 잠시 후 앱이 다시 시작됩니다."),
        )
        .await;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        std::process::Command::new("cmd.exe")
            .args(["/C", script.to_string_lossy().as_ref()])
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|error| AgentError::Update(format!("업데이트 실행 실패: {error}")))?;
    }
    Ok(true)
}

pub(crate) async fn auto_update_on_startup(app: AppHandle, state: Arc<AppState>) {
    let config = state.config.read().await.clone();
    if !config.check_updates_on_startup || !config.auto_update_enabled {
        return;
    }
    let Some(url) = config.update_manifest_url else {
        return;
    };
    sleep(Duration::from_secs(2)).await;
    let Ok(parsed) = Url::parse(&url) else {
        return;
    };
    if parsed.scheme() != "https"
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return;
    }
    let Ok(client) = Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(30))
        .build()
    else {
        return;
    };
    let Ok(bytes) = download_limited(&client, parsed, MAX_UPDATE_MANIFEST_BYTES).await else {
        return;
    };
    let Ok(manifest) = serde_json::from_slice::<UpdateManifest>(&bytes) else {
        return;
    };
    match apply_update(manifest, &app, &state).await {
        Ok(true) => {
            state.log(&app, "새 버전을 설치하고 재시작합니다.").await;
            sleep(Duration::from_secs(1)).await;
            app.exit(0);
        }
        Ok(false) => {}
        Err(error) => {
            state
                .log(&app, format!("자동 업데이트 실패: {error}"))
                .await;
            state
                .set_update_message(
                    &app,
                    Some(format!(
                        "자동 업데이트에 실패했습니다. 앱을 계속 사용할 수 있습니다: {error}"
                    )),
                )
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_manifest_keeps_public_schema() {
        let manifest: UpdateManifest = serde_json::from_str(
            r#"{
              "tauri": {
                "version": "0.6.8",
                "url": "https://example.test/agent.zip",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "executable": "yummi-lcu-tauri.exe"
              }
            }"#,
        )
        .unwrap();
        let target = manifest.select_tauri().unwrap();
        assert_eq!(target.version, "0.6.8");
        assert_eq!(target.executable.as_deref(), Some("yummi-lcu-tauri.exe"));
    }

    #[test]
    fn sha256_mismatch_stops_update_validation() {
        let wrong_hash = "00".repeat(32);
        assert!(validate_hash(b"yummi", &wrong_hash).is_err());
    }

    #[test]
    fn update_version_rejects_extra_or_path_like_segments() {
        assert_eq!(version_tuple("0.6.8"), Some((0, 6, 8)));
        assert!(version_tuple(r"0.6.8.\..\outside").is_none());
        assert!(version_tuple("0.6.8-beta").is_none());
    }
}
