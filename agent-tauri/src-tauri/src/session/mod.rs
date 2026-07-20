mod dpapi;

use std::{
    fs,
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
    saved_at_utc: u64,
    relay_base_url: String,
}

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
    let raw = fs::read_to_string(path()).ok()?;
    let wrapper: Value = serde_json::from_str(&raw).ok()?;
    let payload = wrapper.get("Payload")?.as_str()?;
    let plain = dpapi::unprotect(payload)?;
    let session: Session = serde_json::from_str(&plain).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    if session.relay_base_url
        != config
            .relay_public_base_url
            .trim_end_matches('/')
            .to_lowercase()
        || now.saturating_sub(session.saved_at_utc) > config.saved_session_max_age_days * 86_400
    {
        return None;
    }
    Some(session)
}

pub(crate) fn save(session: &Session) -> AgentResult<()> {
    if let Some(parent) = path().parent() {
        fs::create_dir_all(parent)?;
    }
    let plain = serde_json::to_string(session)?;
    let encrypted = dpapi::protect(&plain)
        .ok_or_else(|| AgentError::Session("DPAPI 세션 암호화 실패".into()))?;
    fs::write(
        path(),
        serde_json::to_string(&json!({"V": 3, "Payload": encrypted}))?,
    )?;
    Ok(())
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
            session_id: "session".into(),
            ws_token: "token".into(),
            saved_at_utc: 1,
            relay_base_url: "https://relay.example".into(),
        };
        let value = serde_json::to_value(session).unwrap();
        assert!(value.get("session_id").is_some());
        assert!(value.get("ws_token").is_some());
        assert!(value.get("saved_at_utc").is_some());
        assert!(value.get("relay_base_url").is_some());
    }
}
