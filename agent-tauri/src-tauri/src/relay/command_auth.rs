use std::{
    collections::HashMap,
    sync::{Mutex as StdMutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::StreamExt;
use reqwest::{redirect::Policy, Client};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use url::Url;

use crate::error::{AgentError, AgentResult};

const AUTH_FIELD: &str = "__yummi_auth";
const COMMAND_KEY_URL: &str = "https://yummi.duckdns.org/api/public/lcu-command-key";
const COMMAND_SIGNATURE_CONTEXT: &[u8] = b"YUMMI-LCU-COMMAND-V1\n";
const MAX_KEY_RESPONSE_BYTES: usize = 8 * 1024;
const KEY_CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_CLOCK_SKEW_MS: u64 = 60_000;
const MAX_COMMAND_WINDOW_MS: u64 = 60_000;
const MAX_NONCES: usize = 4_096;

#[derive(Clone)]
struct CachedKey {
    key: VerifyingKey,
    key_id: String,
    fetched_at: Instant,
}

#[derive(Deserialize)]
struct CommandKeyResponse {
    algorithm: String,
    version: u8,
    key_id: String,
    public_key: String,
}

#[derive(Debug)]
struct CommandEnvelope {
    target_discord_id: String,
    issued_at_ms: u64,
    expires_at_ms: u64,
    nonce: String,
    signature: String,
    key_id: String,
}

fn key_cache() -> &'static RwLock<Option<CachedKey>> {
    static CACHE: OnceLock<RwLock<Option<CachedKey>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn nonce_cache() -> &'static StdMutex<HashMap<String, u64>> {
    static CACHE: OnceLock<StdMutex<HashMap<String, u64>>> = OnceLock::new();
    CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn canonical_json(value: &Value) -> AgentResult<String> {
    Ok(match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => {
            if value.as_i64().is_none() && value.as_u64().is_none() {
                return Err(AgentError::Relay(
                    "LCU 명령 payload에는 정수만 사용할 수 있습니다.".into(),
                ));
            }
            value.to_string()
        }
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
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let mut out = String::from("{");
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key)?);
                out.push(':');
                out.push_str(&canonical_json(&map[key])?);
            }
            out.push('}');
            out
        }
    })
}

fn parse_envelope(payload: &Value) -> AgentResult<(CommandEnvelope, Value)> {
    let object = payload
        .as_object()
        .ok_or_else(|| AgentError::Relay("LCU 명령 payload 형식 오류".into()))?;
    let auth = object
        .get(AUTH_FIELD)
        .and_then(Value::as_object)
        .ok_or_else(|| AgentError::Relay("서명되지 않은 LCU 명령을 차단했습니다.".into()))?;

    let version = auth.get("v").and_then(Value::as_u64).unwrap_or(0);
    if version != 1 {
        return Err(AgentError::Relay("지원하지 않는 LCU 명령 서명 버전".into()));
    }
    let target_discord_id = auth
        .get("target_discord_id")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= 20 && value.bytes().all(|b| b.is_ascii_digit())
        })
        .ok_or_else(|| AgentError::Relay("LCU 명령 대상 ID 형식 오류".into()))?
        .to_owned();
    let issued_at_ms = auth
        .get("issued_at_ms")
        .and_then(Value::as_u64)
        .ok_or_else(|| AgentError::Relay("LCU 명령 발급시각 없음".into()))?;
    let expires_at_ms = auth
        .get("expires_at_ms")
        .and_then(Value::as_u64)
        .ok_or_else(|| AgentError::Relay("LCU 명령 만료시각 없음".into()))?;
    let nonce = auth
        .get("nonce")
        .and_then(Value::as_str)
        .filter(|value| value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| AgentError::Relay("LCU 명령 nonce 형식 오류".into()))?
        .to_ascii_lowercase();
    let signature = auth
        .get("signature")
        .and_then(Value::as_str)
        .filter(|value| (80..=100).contains(&value.len()) && value.is_ascii())
        .ok_or_else(|| AgentError::Relay("LCU 명령 서명 형식 오류".into()))?
        .to_owned();
    let key_id = auth
        .get("key_id")
        .and_then(Value::as_str)
        .filter(|value| value.len() == 16 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| AgentError::Relay("LCU 명령 key id 형식 오류".into()))?
        .to_ascii_lowercase();

    let mut clean = object.clone();
    clean.remove(AUTH_FIELD);
    Ok((
        CommandEnvelope {
            target_discord_id,
            issued_at_ms,
            expires_at_ms,
            nonce,
            signature,
            key_id,
        },
        Value::Object(clean),
    ))
}

fn validate_time(envelope: &CommandEnvelope, now: u64) -> AgentResult<()> {
    if envelope.expires_at_ms < envelope.issued_at_ms
        || envelope.expires_at_ms.saturating_sub(envelope.issued_at_ms) > MAX_COMMAND_WINDOW_MS
        || envelope.issued_at_ms > now.saturating_add(MAX_CLOCK_SKEW_MS)
        || now > envelope.expires_at_ms.saturating_add(MAX_CLOCK_SKEW_MS)
    {
        return Err(AgentError::Relay(
            "만료되었거나 시각이 잘못된 LCU 명령".into(),
        ));
    }
    Ok(())
}

fn key_id(raw_key: &[u8; 32]) -> String {
    let mut hash = Sha256::new();
    hash.update(raw_key);
    format!("{:x}", hash.finalize())[..16].to_owned()
}

async fn fetch_key() -> AgentResult<CachedKey> {
    let url = Url::parse(COMMAND_KEY_URL)
        .map_err(|_| AgentError::Relay("LCU command key URL 오류".into()))?;
    if url.scheme() != "https"
        || url.host_str() != Some("yummi.duckdns.org")
        || url.path() != "/api/public/lcu-command-key"
    {
        return Err(AgentError::Relay("LCU command key URL 검증 실패".into()));
    }
    let client = Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| AgentError::Relay("LCU command key HTTP client 생성 실패".into()))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| AgentError::Relay("LCU command key 조회 실패".into()))?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_KEY_RESPONSE_BYTES as u64)
    {
        return Err(AgentError::Relay("LCU command key 응답 오류".into()));
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| AgentError::Relay("LCU command key 읽기 실패".into()))?;
        if bytes.len().saturating_add(chunk.len()) > MAX_KEY_RESPONSE_BYTES {
            return Err(AgentError::Relay("LCU command key 응답 크기 초과".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: CommandKeyResponse = serde_json::from_slice(&bytes)
        .map_err(|_| AgentError::Relay("LCU command key 응답 형식 오류".into()))?;
    if body.algorithm != "Ed25519" || body.version != 1 {
        return Err(AgentError::Relay("LCU command key 알고리즘 오류".into()));
    }
    let decoded = B64
        .decode(body.public_key.trim())
        .map_err(|_| AgentError::Relay("LCU command public key base64 오류".into()))?;
    let raw: [u8; 32] = decoded
        .try_into()
        .map_err(|_| AgentError::Relay("LCU command public key 길이 오류".into()))?;
    let calculated_id = key_id(&raw);
    if calculated_id != body.key_id.to_ascii_lowercase() {
        return Err(AgentError::Relay("LCU command key id 불일치".into()));
    }
    let key = VerifyingKey::from_bytes(&raw)
        .map_err(|_| AgentError::Relay("LCU command public key 오류".into()))?;
    Ok(CachedKey {
        key,
        key_id: calculated_id,
        fetched_at: Instant::now(),
    })
}

async fn command_key() -> AgentResult<CachedKey> {
    {
        let cache = key_cache().read().await;
        if let Some(cached) = cache
            .as_ref()
            .filter(|cached| cached.fetched_at.elapsed() < KEY_CACHE_TTL)
        {
            return Ok(cached.clone());
        }
    }
    let fetched = fetch_key().await?;
    *key_cache().write().await = Some(fetched.clone());
    Ok(fetched)
}

fn claim_nonce(nonce: &str, expires_at_ms: u64, now: u64) -> AgentResult<()> {
    let mut cache = nonce_cache()
        .lock()
        .map_err(|_| AgentError::Relay("LCU 명령 replay cache 오류".into()))?;
    cache.retain(|_, expires| expires.saturating_add(MAX_CLOCK_SKEW_MS) >= now);
    if cache.contains_key(nonce) {
        return Err(AgentError::Relay(
            "재사용된 LCU 명령을 차단했습니다.".into(),
        ));
    }
    if cache.len() >= MAX_NONCES {
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, expires)| **expires)
            .map(|(nonce, _)| nonce.clone())
        {
            cache.remove(&oldest);
        }
    }
    cache.insert(nonce.to_owned(), expires_at_ms);
    Ok(())
}

pub(crate) async fn verify_command(
    action: &str,
    payload: &Value,
    bound_discord_id: Option<u64>,
) -> AgentResult<Value> {
    let (envelope, clean_payload) = parse_envelope(payload)?;
    let bound = bound_discord_id
        .filter(|value| *value > 0)
        .ok_or_else(|| AgentError::Relay("Discord 세션 바인딩이 없습니다.".into()))?;
    if envelope.target_discord_id != bound.to_string() {
        return Err(AgentError::Relay("LCU 명령 대상 Discord ID 불일치".into()));
    }

    let now = now_ms();
    validate_time(&envelope, now)?;
    let cached = command_key().await?;
    if cached.key_id != envelope.key_id {
        return Err(AgentError::Relay("LCU 명령 서명 key id 불일치".into()));
    }

    let signature_bytes = B64
        .decode(envelope.signature.as_bytes())
        .map_err(|_| AgentError::Relay("LCU 명령 signature base64 오류".into()))?;
    let signature_bytes: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| AgentError::Relay("LCU 명령 signature 길이 오류".into()))?;
    let signature = Signature::from_bytes(&signature_bytes);
    let signed = json!({
        "action": action,
        "expires_at_ms": envelope.expires_at_ms,
        "issued_at_ms": envelope.issued_at_ms,
        "nonce": envelope.nonce,
        "payload": clean_payload,
        "target_discord_id": envelope.target_discord_id,
    });
    let mut message = COMMAND_SIGNATURE_CONTEXT.to_vec();
    message.extend_from_slice(canonical_json(&signed)?.as_bytes());
    cached
        .key
        .verify(&message, &signature)
        .map_err(|_| AgentError::Relay("LCU 명령 서명 검증 실패".into()))?;
    claim_nonce(&envelope.nonce, envelope.expires_at_ms, now)?;

    Ok(signed.get("payload").cloned().unwrap_or_else(|| json!({})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_command_json_sorts_keys_and_rejects_floats() {
        assert_eq!(
            canonical_json(&json!({"z": 1, "a": {"b": true, "a": "x"}})).unwrap(),
            r#"{"a":{"a":"x","b":true},"z":1}"#
        );
        assert!(canonical_json(&json!({"float": 1.5})).is_err());
    }

    #[test]
    fn auth_metadata_is_removed_before_lcu_execution() {
        let (auth, clean) = parse_envelope(&json!({
            "text": "hello",
            "__yummi_auth": {
                "v": 1,
                "target_discord_id": "42",
                "issued_at_ms": 1000,
                "expires_at_ms": 2000,
                "nonce": "0123456789abcdef0123456789abcdef",
                "signature": "A".repeat(88),
                "key_id": "0123456789abcdef"
            }
        }))
        .unwrap();
        assert_eq!(auth.target_discord_id, "42");
        assert_eq!(clean, json!({"text": "hello"}));
    }

    #[test]
    fn command_window_is_bounded() {
        assert!(validate_time(
            &CommandEnvelope {
                target_discord_id: "42".into(),
                issued_at_ms: 1_000,
                expires_at_ms: 31_000,
                nonce: "0".repeat(32),
                signature: "A".repeat(88),
                key_id: "0".repeat(16),
            },
            10_000,
        )
        .is_ok());
        assert!(validate_time(
            &CommandEnvelope {
                target_discord_id: "42".into(),
                issued_at_ms: 1_000,
                expires_at_ms: 100_000,
                nonce: "0".repeat(32),
                signature: "A".repeat(88),
                key_id: "0".repeat(16),
            },
            10_000,
        )
        .is_err());
    }
}
