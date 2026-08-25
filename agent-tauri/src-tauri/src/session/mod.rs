mod dpapi;

use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    config::Config,
    error::{AgentError, AgentResult},
};

#[derive(Serialize, Deserialize)]
pub(crate) struct Session {
    pub(crate) session_id: String,
    pub(crate) ws_token: String,
    #[serde(default)]
    pub(crate) bound_discord_id: Option<u64>,
    saved_at_utc: u64,
    relay_base_url: String,
}

impl Drop for Session {
    fn drop(&mut self) {
        // The token is generated as ASCII; NUL replacement preserves String invariants.
        unsafe { self.ws_token.as_bytes_mut().fill(0) };
    }
}

const MAX_SESSION_FILE_BYTES: u64 = 64 * 1024;

fn path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("YummiAgent")
        .join("relay-session.json")
}

pub(crate) fn create(config: &Config) -> Session {
    Session {
        session_id: Uuid::new_v4().to_string(),
        ws_token: B64.encode(Uuid::new_v4().as_bytes()),
        bound_discord_id: None,
        saved_at_utc: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        relay_base_url: config
            .relay_public_base_url
            .trim_end_matches('/')
            .to_lowercase(),
    }
}

pub(crate) fn load(config: &Config) -> Option<Session> {
    let path = path();
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_SESSION_FILE_BYTES
    {
        return None;
    }
    let mut raw = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_SESSION_FILE_BYTES + 1)
        .read_to_end(&mut raw)
        .ok()?;
    if raw.len() as u64 > MAX_SESSION_FILE_BYTES {
        return None;
    }
    let raw = String::from_utf8(raw).ok()?;
    let wrapper: Value = serde_json::from_str(&raw).ok()?;
    let payload = wrapper.get("Payload")?.as_str()?;
    let mut plain = dpapi::unprotect(payload)?;
    let session = serde_json::from_slice::<Session>(&plain).ok();
    plain.fill(0);
    let session = session?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    let max_age = config.saved_session_max_age_days.saturating_mul(86_400);
    if !session_shape_valid(&session)
        || session.saved_at_utc > now.saturating_add(300)
        || session.relay_base_url
            != config
                .relay_public_base_url
                .trim_end_matches('/')
                .to_lowercase()
        || now.saturating_sub(session.saved_at_utc) > max_age
    {
        return None;
    }
    Some(session)
}

pub(crate) fn save(session: &Session) -> AgentResult<()> {
    if !session_shape_valid(session) {
        return Err(AgentError::Session(
            "세션 데이터가 올바르지 않습니다.".into(),
        ));
    }
    let path = path();
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(AgentError::Session(
            "세션 저장 경로가 올바르지 않습니다.".into(),
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut plain = serde_json::to_vec(session)?;
    let encrypted = dpapi::protect(&plain);
    plain.fill(0);
    let encrypted =
        encrypted.ok_or_else(|| AgentError::Session("DPAPI 세션 암호화 실패".into()))?;
    let serialized = serde_json::to_vec(&json!({"V": 3, "Payload": encrypted}))?;
    let parent = path
        .parent()
        .ok_or_else(|| AgentError::Session("세션 저장 경로 오류".into()))?;
    let temporary = parent.join(format!(".relay-session-{}.tmp", Uuid::new_v4()));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::options()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&serialized)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| AgentError::Session("세션 저장 실패".into()))?;
    Ok(())
}

fn session_shape_valid(session: &Session) -> bool {
    Uuid::parse_str(&session.session_id).is_ok()
        && (16..=128).contains(&session.ws_token.len())
        && session
            .ws_token
            .chars()
            .all(|character| character.is_ascii_graphic())
        && session
            .bound_discord_id
            .is_none_or(|discord_id| discord_id > 0)
        && !session.relay_base_url.is_empty()
        && session.relay_base_url.len() <= 2_048
}

pub(crate) fn pin_discord_id(session: &mut Session, discord_id: u64) -> AgentResult<()> {
    if discord_id == 0 {
        return Err(AgentError::Session(
            "Discord 계정 바인딩 값이 올바르지 않습니다.".into(),
        ));
    }
    if let Some(expected) = session.bound_discord_id {
        if expected != discord_id {
            return Err(AgentError::Session(
                "저장된 Discord 계정과 Relay 바인딩 계정이 일치하지 않습니다.".into(),
            ));
        }
        return Ok(());
    }
    session.bound_discord_id = Some(discord_id);
    save(session)
}

pub(crate) fn remove() -> AgentResult<()> {
    match fs::remove_file(path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AgentError::Session(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_json_field_names_remain_compatible() {
        let session = Session {
            session_id: Uuid::new_v4().to_string(),
            ws_token: "0123456789abcdef".into(),
            bound_discord_id: Some(42),
            saved_at_utc: 1,
            relay_base_url: "https://relay.example".into(),
        };
        let value = serde_json::to_value(session).unwrap();
        assert!(value.get("session_id").is_some());
        assert!(value.get("ws_token").is_some());
        assert_eq!(
            value.get("bound_discord_id").and_then(Value::as_u64),
            Some(42)
        );
        assert!(value.get("saved_at_utc").is_some());
        assert!(value.get("relay_base_url").is_some());
    }

    #[test]
    fn discord_binding_pin_rejects_account_switch() {
        let config = Config::default();
        let mut session = create(&config);
        session.bound_discord_id = Some(42);
        assert!(pin_discord_id(&mut session, 42).is_ok());
        assert!(pin_discord_id(&mut session, 43).is_err());
    }

    #[test]
    fn malformed_session_fields_are_rejected() {
        let session = Session {
            session_id: "not-a-uuid".into(),
            ws_token: "short".into(),
            bound_discord_id: None,
            saved_at_utc: 1,
            relay_base_url: "https://relay.example".into(),
        };
        assert!(!session_shape_valid(&session));
        assert!(dpapi::unprotect("not-valid-base64").is_none());
    }
}
