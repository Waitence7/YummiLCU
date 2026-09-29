use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_FLIGHT_RECORDS: usize = 2_048;
const MAX_BOOTSTRAP_LOG_BYTES: u64 = 64 * 1024;
const MAX_PERSISTENT_FLIGHT_LOG_BYTES: u64 = 1024 * 1024;
const PERSISTENT_FLIGHT_LOG_BACKUPS: usize = 2;
const MAX_PERSISTENT_FLIGHT_DETAIL_CHARS: usize = 2_048;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FlightRecord {
    pub(crate) at_ms: u64,
    pub(crate) category: &'static str,
    pub(crate) detail: String,
}

#[derive(Default)]
pub(crate) struct FlightRecorder {
    records: VecDeque<FlightRecord>,
}

impl FlightRecorder {
    pub(crate) fn record(&mut self, category: &'static str, detail: impl Into<String>) {
        let record = FlightRecord {
            at_ms: now_ms(),
            category,
            detail: detail.into(),
        };
        append_persistent_flight_record(&record);
        self.records.push_back(record);
        while self.records.len() > MAX_FLIGHT_RECORDS {
            self.records.pop_front();
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<FlightRecord> {
        self.records.iter().cloned().collect()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.records.len()
    }

    #[cfg(test)]
    fn first_detail(&self) -> Option<&str> {
        self.records.front().map(|record| record.detail.as_str())
    }
}

fn agent_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("YummiAgent")
}

fn persistent_flight_log_path() -> PathBuf {
    agent_data_dir().join("flight-recorder.log")
}

fn persistent_flight_backup_path(path: &std::path::Path, index: usize) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("flight-recorder.log");
    path.with_file_name(format!("{file_name}.{index}"))
}

fn rotate_persistent_flight_log(path: &std::path::Path, max_bytes: u64) {
    if !fs::metadata(path).is_ok_and(|metadata| metadata.len() >= max_bytes) {
        return;
    }

    for index in (1..=PERSISTENT_FLIGHT_LOG_BACKUPS).rev() {
        let source = if index == 1 {
            path.to_path_buf()
        } else {
            persistent_flight_backup_path(path, index - 1)
        };
        let destination = persistent_flight_backup_path(path, index);
        if destination.exists() {
            let _ = fs::remove_file(&destination);
        }
        if source.exists() {
            let _ = fs::rename(&source, &destination);
        }
    }
}

fn sanitize_persistent_flight_detail(value: &str) -> String {
    const KEYS: [&str; 15] = [
        "password",
        "token",
        "authorization",
        "cookie",
        "set-cookie",
        "secret",
        "api_key",
        "apikey",
        "private_key",
        "oauth_code",
        "oauthcode",
        "ws_token",
        "session_token",
        "remoting-auth-token",
        "access_token",
    ];

    let normalized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut output = normalized;
    for key in KEYS {
        output = redact_persistent_key_value(&output, key);
    }
    output
        .chars()
        .take(MAX_PERSISTENT_FLIGHT_DETAIL_CHARS)
        .collect()
}

fn redact_persistent_key_value(input: &str, key: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find(key) {
        let start = cursor + relative;
        out.push_str(&input[cursor..start]);
        out.push_str(&input[start..start + key.len()]);
        let mut value_start = start + key.len();
        while value_start < input.len() && input.as_bytes()[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if value_start < input.len() && matches!(input.as_bytes()[value_start], b'=' | b':') {
            out.push_str(&input[start + key.len()..=value_start]);
            value_start += 1;
            while value_start < input.len() && input.as_bytes()[value_start].is_ascii_whitespace() {
                out.push(' ');
                value_start += 1;
            }
            let quoted = input
                .as_bytes()
                .get(value_start)
                .copied()
                .filter(|byte| matches!(byte, b'"' | b'\''));
            if let Some(quote) = quoted {
                out.push(quote as char);
                value_start += 1;
                if let Some(end) = input[value_start..].find(quote as char) {
                    out.push_str("***");
                    out.push(quote as char);
                    cursor = value_start + end + 1;
                } else {
                    out.push_str("***");
                    cursor = input.len();
                }
            } else {
                let end = input[value_start..]
                    .find(|character: char| {
                        character.is_whitespace() || matches!(character, ',' | '}' | ']' | '&')
                    })
                    .map_or(input.len(), |offset| value_start + offset);
                out.push_str("***");
                cursor = end;
            }
        } else {
            cursor = start + key.len();
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn append_persistent_flight_record(record: &FlightRecord) {
    let path = persistent_flight_log_path();
    append_persistent_flight_record_to_path(&path, record, MAX_PERSISTENT_FLIGHT_LOG_BYTES);
}

fn append_persistent_flight_record_to_path(
    path: &std::path::Path,
    record: &FlightRecord,
    max_bytes: u64,
) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    rotate_persistent_flight_log(path, max_bytes);
    let detail = sanitize_persistent_flight_detail(&record.detail);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} [{}] {}", record.at_ms, record.category, detail);
    }
}

pub(crate) fn persistent_flight_log_snapshot() -> Option<String> {
    let path = persistent_flight_log_path();
    let bytes = fs::read(path).ok()?;
    if bytes.len() as u64 > MAX_PERSISTENT_FLIGHT_LOG_BYTES.saturating_add(16 * 1024) {
        return None;
    }
    String::from_utf8(bytes)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn bootstrap_log_path() -> PathBuf {
    agent_data_dir().join("bootstrap-errors.log")
}

pub(crate) fn write_bootstrap_error(summary: &str) {
    let path = bootstrap_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() >= MAX_BOOTSTRAP_LOG_BYTES) {
        let _ = fs::remove_file(&path);
    }
    let sanitized = summary
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(1_024)
        .collect::<String>();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {sanitized}", now_ms());
    }
}

pub(crate) fn bootstrap_log_snapshot() -> Option<String> {
    let bytes = fs::read(bootstrap_log_path()).ok()?;
    if bytes.len() as u64 > MAX_BOOTSTRAP_LOG_BYTES {
        return None;
    }
    String::from_utf8(bytes)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flight_recorder_is_bounded_and_fifo() {
        let mut recorder = FlightRecorder::default();
        for index in 0..(MAX_FLIGHT_RECORDS + 3) {
            recorder.records.push_back(FlightRecord {
                at_ms: index as u64,
                category: "test",
                detail: format!("event-{index}"),
            });
            while recorder.records.len() > MAX_FLIGHT_RECORDS {
                recorder.records.pop_front();
            }
        }
        assert_eq!(recorder.len(), MAX_FLIGHT_RECORDS);
        assert_eq!(recorder.first_detail(), Some("event-3"));
    }

    #[test]
    fn persistent_flight_detail_redacts_secrets_and_control_characters() {
        let sanitized =
            sanitize_persistent_flight_detail("token=abc123 password: \"hunter2\" hello\nworld");
        assert!(!sanitized.contains("abc123"));
        assert!(!sanitized.contains("hunter2"));
        assert!(!sanitized.contains('\n'));
        assert!(sanitized.contains("token=***"));
        assert!(sanitized.contains("password: \"***\""));
    }

    #[test]
    fn persistent_flight_log_rotates_and_keeps_recent_record() {
        let root = std::env::temp_dir().join(format!(
            "yummi-flight-log-test-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("flight-recorder.log");
        fs::write(&path, b"0123456789").unwrap();

        append_persistent_flight_record_to_path(
            &path,
            &FlightRecord {
                at_ms: 42,
                category: "test",
                detail: "after-rotation".into(),
            },
            8,
        );

        assert_eq!(
            fs::read_to_string(persistent_flight_backup_path(&path, 1)).unwrap(),
            "0123456789"
        );
        let current = fs::read_to_string(&path).unwrap();
        assert!(current.contains("42 [test] after-rotation"));
        let _ = fs::remove_dir_all(root);
    }
}
