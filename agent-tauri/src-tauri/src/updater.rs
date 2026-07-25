use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    signature: Option<String>,
    channel: Option<String>,
    rollout_percent: Option<u8>,
    min_version: Option<String>,
    blocked_versions: Option<Vec<String>>,
    publisher_thumbprint: Option<String>,
    files: Option<Vec<UpdateFile>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateFile {
    path: String,
    sha256: String,
    size: u64,
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
const OFFICIAL_UPDATE_HOST: &str = "yummi.duckdns.org";

impl UpdateManifest {
    fn select_tauri(self) -> Option<UpdateTarget> {
        self.tauri
    }
}

fn canonical_json(value: &Value) -> AgentResult<String> {
    Ok(match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value)?,
        Value::Array(values) => {
            let mut out = String::from("[");
            for (index, item) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&canonical_json(item)?);
            }
            out.push(']');
            out
        }
        Value::Object(map) => {
            let sorted: BTreeMap<_, _> = map.iter().collect();
            let mut out = String::from("{");
            for (index, (key, item)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key)?);
                out.push(':');
                out.push_str(&canonical_json(item)?);
            }
            out.push('}');
            out
        }
    })
}

fn decode_base64_fixed<const N: usize>(label: &str, raw: &str) -> AgentResult<[u8; N]> {
    let decoded = BASE64_STANDARD
        .decode(raw.trim())
        .map_err(|_| AgentError::Update(format!("{label} base64 형식이 올바르지 않습니다.")))?;
    decoded
        .try_into()
        .map_err(|_| AgentError::Update(format!("{label} 길이가 올바르지 않습니다.")))
}

fn manifest_public_key() -> AgentResult<[u8; 32]> {
    let raw = option_env!("YUMMI_AGENT_MANIFEST_PUBLIC_KEY")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AgentError::Update("업데이트 manifest 공개키가 빌드에 설정되지 않았습니다.".into())
        })?;
    decode_base64_fixed("manifest public key", raw)
}

fn verify_tauri_manifest_signature(target: &Value, signature: &str) -> AgentResult<()> {
    let public_key = manifest_public_key()?;
    verify_tauri_manifest_signature_with_key(target, signature, &public_key)
}

fn verify_tauri_manifest_signature_with_key(
    target: &Value,
    signature: &str,
    public_key: &[u8; 32],
) -> AgentResult<()> {
    let signature_bytes = decode_base64_fixed::<64>("manifest signature", signature)?;
    let verifying_key = VerifyingKey::from_bytes(public_key)
        .map_err(|_| AgentError::Update("업데이트 manifest 공개키가 올바르지 않습니다.".into()))?;
    let signature = Signature::from_bytes(&signature_bytes);
    let payload = canonical_json(target)?;
    verifying_key
        .verify(payload.as_bytes(), &signature)
        .map_err(|_| AgentError::Update("업데이트 manifest 서명 검증 실패".into()))
}

fn parse_signed_manifest(bytes: &[u8]) -> AgentResult<UpdateManifest> {
    let value: Value = serde_json::from_slice(bytes)?;
    let Some(mut tauri_value) = value.get("tauri").cloned() else {
        return Ok(UpdateManifest { tauri: None });
    };
    let object = tauri_value.as_object_mut().ok_or_else(|| {
        AgentError::Update("업데이트 manifest tauri 블록이 올바르지 않습니다.".into())
    })?;
    let signature = object
        .remove("signature")
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .ok_or_else(|| AgentError::Update("업데이트 manifest 서명이 없습니다.".into()))?;
    verify_tauri_manifest_signature(&tauri_value, &signature)?;
    let mut target: UpdateTarget = serde_json::from_value(tauri_value)?;
    target.signature = Some(signature);
    Ok(UpdateManifest {
        tauri: Some(target),
    })
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

fn is_older(left: &str, right: &str) -> bool {
    match (version_tuple(left), version_tuple(right)) {
        (Some(left), Some(right)) => left < right,
        _ => false,
    }
}

fn update_channel_matches(target: &UpdateTarget, configured_channel: &str) -> bool {
    target.channel.as_deref().unwrap_or("stable") == configured_channel.trim()
}

fn rollout_bucket(seed: &str, version: &str) -> u8 {
    let mut hash = Sha256::new();
    hash.update(seed.as_bytes());
    hash.update(b":");
    hash.update(version.as_bytes());
    let digest = hash.finalize();
    digest[0] % 100
}

fn rollout_seed() -> String {
    std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "yummi-lcu-tauri".into())
}

fn should_apply_target(target: &UpdateTarget, current_version: &str, config_channel: &str) -> bool {
    if !is_newer(&target.version, current_version) {
        return false;
    }
    if !update_channel_matches(target, config_channel) {
        return false;
    }
    if target
        .min_version
        .as_deref()
        .is_some_and(|min_version| is_older(current_version, min_version))
    {
        return false;
    }
    let blocked_current = target
        .blocked_versions
        .as_ref()
        .is_some_and(|versions| versions.iter().any(|version| version == current_version));
    if blocked_current {
        return true;
    }
    let percent = target.rollout_percent.unwrap_or(100).min(100);
    percent > 0 && rollout_bucket(&rollout_seed(), &target.version) < percent
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

fn validate_update_download_url(url: &Url) -> AgentResult<()> {
    if url.scheme() != "https"
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.host_str() != Some(OFFICIAL_UPDATE_HOST)
        || !url.path().starts_with("/agent/")
    {
        return Err(AgentError::Update(
            "업데이트 다운로드는 공식 HTTPS /agent/ URL만 허용됩니다.".into(),
        ));
    }
    Ok(())
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

fn validate_manifest_file_path(path: &str) -> AgentResult<PathBuf> {
    let relative = Path::new(path);
    if relative.is_absolute()
        || path.contains('\0')
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(AgentError::Update(
            "업데이트 파일 목록 경로가 올바르지 않습니다.".into(),
        ));
    }
    Ok(relative.to_owned())
}

fn validate_manifest_files(root: &Path, files: Option<&[UpdateFile]>) -> AgentResult<()> {
    let Some(files) = files else {
        return Ok(());
    };
    if files.len() > MAX_ARCHIVE_ENTRIES {
        return Err(AgentError::Update("업데이트 파일 목록 제한 초과".into()));
    }
    for file in files {
        let relative = validate_manifest_file_path(&file.path)?;
        let path = root.join(relative);
        let metadata = fs::metadata(&path)
            .map_err(|_| AgentError::Update(format!("업데이트 파일이 없습니다: {}", file.path)))?;
        if !metadata.is_file() || metadata.len() != file.size {
            return Err(AgentError::Update(format!(
                "업데이트 파일 크기가 올바르지 않습니다: {}",
                file.path
            )));
        }
        let bytes = fs::read(&path)?;
        validate_hash(&bytes, &file.sha256)?;
    }
    Ok(())
}

fn normalize_thumbprint(raw: &str) -> Option<String> {
    let normalized = raw
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase();
    (normalized.len() == 40 || normalized.len() == 64).then_some(normalized)
}

fn expected_publisher_thumbprint(target: &UpdateTarget) -> Option<String> {
    option_env!("YUMMI_AGENT_WINDOWS_SIGNING_THUMBPRINT")
        .and_then(normalize_thumbprint)
        .or_else(|| {
            target
                .publisher_thumbprint
                .as_deref()
                .and_then(normalize_thumbprint)
        })
}

fn ps_single_quoted(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

fn windows_signature_check_cmd(source: &Path, expected_thumbprint: Option<&str>) -> String {
    let Some(expected_thumbprint) = expected_thumbprint else {
        return String::new();
    };
    let source = ps_single_quoted(source);
    format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -Command \"$sig = Get-AuthenticodeSignature -LiteralPath {}; if ($sig.Status -ne 'Valid') {{ exit 11 }}; if ($sig.SignerCertificate.Thumbprint.ToLowerInvariant() -ne '{}') {{ exit 12 }}\"\r\n\
         if errorlevel 1 goto restore\r\n",
        source, expected_thumbprint
    )
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
    config_channel: &str,
) -> AgentResult<bool> {
    let Some(target_manifest) = manifest.select_tauri() else {
        return Ok(false);
    };
    let current_version = env!("CARGO_PKG_VERSION");
    if !should_apply_target(&target_manifest, current_version, config_channel) {
        return Ok(false);
    }

    let use_patch = target_manifest.patch_from.as_deref() == Some(current_version)
        && target_manifest.patch_url.is_some();
    let url = if use_patch {
        target_manifest.patch_url.as_deref()
    } else {
        target_manifest.url.as_deref()
    };
    let hash = if use_patch {
        target_manifest.patch_sha256.as_deref()
    } else {
        target_manifest.sha256.as_deref()
    };
    let (Some(url), Some(hash)) = (url, hash) else {
        return Ok(false);
    };
    let parsed = url::Url::parse(&url)
        .map_err(|error| AgentError::Update(format!("업데이트 URL 오류: {error}")))?;
    validate_update_download_url(&parsed)?;

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
    validate_manifest_files(&extract, target_manifest.files.as_deref())?;

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
    let update_lock = target_dir.join(".yummi-update.lock");
    if update_lock.exists() {
        return Err(AgentError::Update("이미 업데이트가 진행 중입니다.".into()));
    }
    let script = work.join("apply-update.cmd");
    let source_dir = source
        .parent()
        .ok_or_else(|| AgentError::Update("압축 해제 폴더를 찾지 못했습니다.".into()))?;
    let backup_dir = work.join("backup");
    let signature_check = windows_signature_check_cmd(
        &source,
        expected_publisher_thumbprint(&target_manifest).as_deref(),
    );
    let script_text = format!(
        "@echo off\r\n\
         setlocal\r\n\
         timeout /t 2 /nobreak >nul\r\n\
         if exist \"{}\" exit /b 2\r\n\
         echo updating > \"{}\"\r\n\
         if errorlevel 1 exit /b 3\r\n\
         mkdir \"{}\" >nul 2>&1\r\n\
         robocopy \"{}\" \"{}\" /E /XF agent.json .yummi-update.lock >nul\r\n\
         if errorlevel 8 goto fail\r\n\
         {}\
         robocopy \"{}\" \"{}\" /E /XF agent.json >nul\r\n\
         if errorlevel 8 goto restore\r\n\
         if not exist \"{}\" goto restore\r\n\
         del /Q \"{}\" >nul 2>&1\r\n\
         start \"\" \"{}\"\r\n\
         rmdir /S /Q \"{}\" >nul 2>&1\r\n\
         del \"%~f0\"\r\n",
        update_lock.display(),
        update_lock.display(),
        backup_dir.display(),
        target_dir.display(),
        backup_dir.display(),
        signature_check,
        source_dir.display(),
        target_dir.display(),
        target.display(),
        update_lock.display(),
        target.display(),
        backup_dir.display(),
    );
    let script_text = format!(
        "{}\
         exit /b 0\r\n\
         :restore\r\n\
         robocopy \"{}\" \"{}\" /E /XF agent.json .yummi-update.lock >nul\r\n\
         if exist \"{}\" del /Q \"{}\" >nul 2>&1\r\n\
         start \"\" \"{}\"\r\n\
         exit /b 1\r\n\
         :fail\r\n\
         if exist \"{}\" del /Q \"{}\" >nul 2>&1\r\n\
         exit /b 1\r\n",
        script_text,
        backup_dir.display(),
        target_dir.display(),
        update_lock.display(),
        update_lock.display(),
        target.display(),
        update_lock.display(),
        update_lock.display(),
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
    let manifest = match parse_signed_manifest(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            state
                .log(&app, format!("자동 업데이트 manifest 검증 실패: {error}"))
                .await;
            return;
        }
    };
    match apply_update(manifest, &app, &state, &config.update_channel).await {
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
    use ed25519_dalek::{Signer, SigningKey};

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

    #[test]
    fn signed_tauri_manifest_payload_is_verified() {
        let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
        let verifying_key = signing_key.verifying_key();
        let target = serde_json::json!({
            "version": "0.6.9",
            "channel": "stable",
            "rolloutPercent": 100,
            "url": "https://yummi.duckdns.org/agent/releases/tauri/tauri-0.6.9.zip",
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "executable": "yummi-lcu-tauri.exe"
        });
        let payload = canonical_json(&target).unwrap();
        let signature = signing_key.sign(payload.as_bytes());
        let signature = BASE64_STANDARD.encode(signature.to_bytes());
        assert!(verify_tauri_manifest_signature_with_key(
            &target,
            &signature,
            verifying_key.as_bytes()
        )
        .is_ok());

        let mut tampered = target.clone();
        tampered["version"] = Value::String("0.7.0".into());
        assert!(verify_tauri_manifest_signature_with_key(
            &tampered,
            &signature,
            verifying_key.as_bytes()
        )
        .is_err());
    }

    #[test]
    fn update_policy_respects_channel_min_version_and_rollout() {
        let target = UpdateTarget {
            version: "0.7.0".into(),
            channel: Some("beta".into()),
            rollout_percent: Some(0),
            min_version: Some("0.6.0".into()),
            blocked_versions: None,
            url: None,
            patch_url: None,
            patch_from: None,
            sha256: None,
            patch_sha256: None,
            executable: None,
            signature: None,
            publisher_thumbprint: None,
            files: None,
        };
        assert!(!should_apply_target(&target, "0.6.8", "stable"));
        assert!(!should_apply_target(&target, "0.6.8", "beta"));

        let blocked = UpdateTarget {
            blocked_versions: Some(vec!["0.6.8".into()]),
            ..target
        };
        assert!(should_apply_target(&blocked, "0.6.8", "beta"));
    }

    #[test]
    fn update_downloads_are_limited_to_public_agent_host() {
        assert!(validate_update_download_url(
            &Url::parse("https://yummi.duckdns.org/agent/releases/tauri/tauri-0.6.9.zip").unwrap()
        )
        .is_ok());
        assert!(validate_update_download_url(
            &Url::parse("https://attacker.example/agent/releases/tauri.zip").unwrap()
        )
        .is_err());
        assert!(validate_update_download_url(
            &Url::parse("https://yummi.duckdns.org/other/tauri.zip").unwrap()
        )
        .is_err());
    }
}
