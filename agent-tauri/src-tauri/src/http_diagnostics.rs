use reqwest::{StatusCode, Url};
use serde_json::Value;

const MAX_REASON_CHARS: usize = 240;
const MAX_FIELD_CHARS: usize = 180;

pub(crate) fn status_detail(status: StatusCode) -> String {
    format!(
        "http_status={} reason={}",
        status.as_u16(),
        sanitize_text(
            status.canonical_reason().unwrap_or("Unknown"),
            MAX_FIELD_CHARS
        )
    )
}

pub(crate) fn transport_detail(error: &reqwest::Error) -> String {
    let kind = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_request() {
        "request"
    } else if error.is_body() {
        "body"
    } else if error.is_decode() {
        "decode"
    } else {
        "transport"
    };
    let mut reason = kind.to_owned();
    let mut source = std::error::Error::source(error);
    while let Some(error) = source {
        reason = error.to_string();
        source = error.source();
    }
    format!(
        "http_status=none transport={} reason={}",
        kind,
        sanitize_text(&reason, MAX_REASON_CHARS)
    )
}

pub(crate) fn response_detail(status: StatusCode, body: &[u8]) -> String {
    let mut detail = status_detail(status);
    if let Some(body_detail) = compact_error_body(body) {
        detail.push(' ');
        detail.push_str(&body_detail);
    } else if !body.is_empty() {
        detail.push_str(&format!(" response_body_bytes={}", body.len()));
    }
    detail
}

pub(crate) fn safe_endpoint(endpoint: &str) -> String {
    let path = endpoint.split('?').next().unwrap_or(endpoint);
    let mut out = String::with_capacity(path.len());
    for (index, segment) in path.split('/').enumerate() {
        if index > 0 {
            out.push('/');
        }
        if segment.len() > 32 {
            out.push_str(":id");
        } else {
            out.push_str(segment);
        }
    }
    out
}

pub(crate) fn safe_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or("unknown-host");
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    format!(
        "{}://{}{}{}",
        url.scheme(),
        host,
        port,
        safe_endpoint(url.path())
    )
}

fn compact_error_body(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let object = value.as_object()?;
    let mut fields = Vec::new();
    collect_object_fields(object, &mut fields);
    if let Some(nested) = object.get("error").and_then(Value::as_object) {
        collect_object_fields(nested, &mut fields);
    }
    (!fields.is_empty()).then(|| fields.join(" "))
}

fn collect_object_fields(object: &serde_json::Map<String, Value>, fields: &mut Vec<String>) {
    for key in [
        "errorCode",
        "httpStatus",
        "status",
        "code",
        "message",
        "reason",
        "detail",
        "error",
    ] {
        let Some(value) = object.get(key) else {
            continue;
        };
        let rendered = match value {
            Value::String(value) => sanitize_text(value, MAX_FIELD_CHARS),
            Value::Number(value) => value.to_string(),
            Value::Bool(value) => value.to_string(),
            _ => continue,
        };
        if !rendered.is_empty()
            && !fields
                .iter()
                .any(|field| field.starts_with(&format!("{key}=")))
        {
            fields.push(format!("{key}={rendered}"));
        }
    }
}

fn sanitize_text(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = normalized.to_ascii_lowercase();
    if [
        "authorization",
        "password",
        "set-cookie",
        "cookie",
        "access_token",
        "session_token",
        "ws_token",
        "private_key",
        "api_key",
        "apikey",
        "secret",
    ]
    .iter()
    .any(|key| lower.contains(key))
    {
        return "[redacted sensitive error detail]".into();
    }
    normalized.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_detail_contains_numeric_code_and_reason() {
        let detail = status_detail(StatusCode::CONFLICT);
        assert!(detail.contains("http_status=409"));
        assert!(detail.contains("reason=Conflict"));
    }

    #[test]
    fn response_detail_extracts_structured_error_fields() {
        let detail = response_detail(
            StatusCode::NOT_FOUND,
            br#"{"errorCode":"RPC_ERROR","httpStatus":404,"message":"party not ready"}"#,
        );
        assert!(detail.contains("http_status=404"));
        assert!(detail.contains("reason=Not Found"));
        assert!(detail.contains("errorCode=RPC_ERROR"));
        assert!(detail.contains("message=party not ready"));
    }

    #[test]
    fn sensitive_error_text_is_redacted() {
        assert_eq!(
            sanitize_text("authorization: Basic abc123", 200),
            "[redacted sensitive error detail]"
        );
        assert_eq!(
            sanitize_text("password=do-not-log", 200),
            "[redacted sensitive error detail]"
        );
    }

    #[test]
    fn safe_endpoint_hides_query_values_and_long_ids() {
        assert_eq!(
            safe_endpoint("/lol-summoner/v1/summoners?name=Secret%23KR1"),
            "/lol-summoner/v1/summoners"
        );
        assert_eq!(
            safe_endpoint(
                "/lol-match-history/v1/products/lol/123456789012345678901234567890123/matches"
            ),
            "/lol-match-history/v1/products/lol/:id/matches"
        );
    }
}
