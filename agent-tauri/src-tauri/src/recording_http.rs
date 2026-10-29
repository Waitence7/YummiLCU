#[cfg(windows)]
mod imp {
    use std::{
        fs::File,
        io::{Read, Seek, SeekFrom, Write},
        net::{TcpListener, TcpStream},
        path::Path,
        sync::Arc,
        thread,
        time::Duration,
    };

    use serde::Serialize;

    use crate::{
        recording::{self, RecordingSessionMetadata},
        state::AppState,
    };

    const ADDR: &str = "127.0.0.1:19516";
    const MAX_REQUEST_BYTES: usize = 16 * 1024;
    const CHUNK_BYTES: usize = 64 * 1024;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct PublicRecordingMetadata {
        schema: String,
        game_id: Option<String>,
        riot_id: Option<String>,
        source: String,
        automatic: bool,
        started_at_ms: u64,
        ended_at_ms: Option<u64>,
        game_clock_offset_ms: i64,
        rofl_file_name: Option<String>,
    }

    fn public_metadata(metadata: &RecordingSessionMetadata) -> PublicRecordingMetadata {
        PublicRecordingMetadata {
            schema: metadata.schema.clone(),
            game_id: metadata.game_id.clone(),
            riot_id: metadata.riot_id.clone(),
            source: metadata.source.clone(),
            automatic: metadata.automatic,
            started_at_ms: metadata.started_at_ms,
            ended_at_ms: metadata.ended_at_ms,
            game_clock_offset_ms: metadata.game_clock_offset_ms,
            rofl_file_name: metadata.rofl_path.as_deref().and_then(|value| {
                Path::new(value)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
            }),
        }
    }

    fn report(state: &Arc<AppState>, code: &'static str, detail: impl Into<String>) {
        let state = state.clone();
        let detail = detail.into();
        tauri::async_runtime::spawn(async move {
            state
                .record_flight("recording_http_error", format!("{code}: {detail}"))
                .await;
            state.report_diagnostic("recording", code, &detail).await;
        });
    }

    pub(super) fn start(state: Arc<AppState>) {
        let _ = thread::Builder::new()
            .name("yummi-recording-http".into())
            .spawn(move || server(state));
    }

    fn server(state: Arc<AppState>) {
        let listener = match TcpListener::bind(ADDR) {
            Ok(listener) => listener,
            Err(error) => {
                report(
                    &state,
                    "local_bridge_bind_failed",
                    format!("addr={ADDR} error={error}"),
                );
                return;
            }
        };
        for stream in listener.incoming().flatten() {
            let state = state.clone();
            let _ = thread::Builder::new()
                .name("yummi-recording-http-client".into())
                .spawn(move || handle(stream, state));
        }
    }

    fn allowed_origin(origin: Option<&str>) -> Option<&str> {
        let origin = origin?.trim();
        let allowed = origin == "https://yummi.duckdns.org"
            || origin == "tauri://localhost"
            || origin == "http://tauri.localhost"
            || origin == "https://tauri.localhost"
            || origin == "http://localhost:3000"
            || origin == "http://localhost:5173"
            || origin == "http://localhost:1420"
            || origin == "http://127.0.0.1:3000"
            || origin == "http://127.0.0.1:5173"
            || origin == "http://127.0.0.1:1420";
        allowed.then_some(origin)
    }

    fn common_headers(origin: Option<&str>) -> String {
        let mut headers = String::from(
            "Access-Control-Allow-Methods: GET, OPTIONS\r\nAccess-Control-Allow-Headers: Range, Content-Type\r\nAccess-Control-Expose-Headers: Content-Length, Content-Range, Accept-Ranges\r\nAccess-Control-Allow-Private-Network: true\r\nCache-Control: no-store\r\n",
        );
        if let Some(origin) = allowed_origin(origin) {
            headers.push_str(&format!(
                "Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\n"
            ));
        }
        headers
    }

    fn write_response(
        stream: &mut TcpStream,
        status: &str,
        content_type: &str,
        body: &[u8],
        origin: Option<&str>,
    ) {
        let headers = common_headers(origin);
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
    }

    fn handle(mut stream: TcpStream, state: Arc<AppState>) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
        let mut buffer = vec![0u8; MAX_REQUEST_BYTES];
        let Ok(size) = stream.read(&mut buffer) else {
            return;
        };
        if size == 0 {
            return;
        }
        let request = String::from_utf8_lossy(&buffer[..size]);
        let mut lines = request.split("\r\n");
        let first = lines.next().unwrap_or_default();
        let mut first_parts = first.split_whitespace();
        let method = first_parts.next().unwrap_or_default();
        let path = first_parts.next().unwrap_or("/");
        let mut origin = None;
        let mut range = None;
        for line in lines {
            if let Some(value) = line
                .strip_prefix("Origin:")
                .or_else(|| line.strip_prefix("origin:"))
            {
                origin = Some(value.trim());
            }
            if let Some(value) = line
                .strip_prefix("Range:")
                .or_else(|| line.strip_prefix("range:"))
            {
                range = Some(value.trim());
            }
        }
        if origin.is_some() && allowed_origin(origin).is_none() {
            write_response(
                &mut stream,
                "403 Forbidden",
                "application/json",
                br#"{"error":"origin not allowed"}"#,
                None,
            );
            return;
        }
        if method == "OPTIONS" {
            let headers = common_headers(origin);
            let _ = stream.write_all(format!("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n{headers}Connection: close\r\n\r\n").as_bytes());
            return;
        }
        if method != "GET" {
            write_response(
                &mut stream,
                "405 Method Not Allowed",
                "application/json",
                br#"{"error":"GET only"}"#,
                origin,
            );
            return;
        }
        if path == "/health" {
            write_response(
                &mut stream,
                "200 OK",
                "application/json",
                br#"{"ok":true}"#,
                origin,
            );
            return;
        }
        if path == "/v1/recordings" {
            match recording::list_sessions().and_then(|rows| {
                let public = rows.iter().map(public_metadata).collect::<Vec<_>>();
                serde_json::to_vec(&public).map_err(|error| error.to_string())
            }) {
                Ok(body) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    &body,
                    origin,
                ),
                Err(error) => {
                    report(&state, "local_bridge_list_failed", error.clone());
                    write_response(
                        &mut stream,
                        "500 Internal Server Error",
                        "text/plain; charset=utf-8",
                        error.as_bytes(),
                        origin,
                    );
                }
            }
            return;
        }
        let Some(rest) = path.strip_prefix("/v1/recordings/") else {
            write_response(
                &mut stream,
                "404 Not Found",
                "application/json",
                br#"{"error":"not found"}"#,
                origin,
            );
            return;
        };
        let (game_id, video) = rest
            .strip_suffix("/video")
            .map_or((rest, false), |value| (value, true));
        if game_id.is_empty()
            || !game_id
                .chars()
                .all(|value| value.is_ascii_alphanumeric() || value == '-' || value == '_')
        {
            write_response(
                &mut stream,
                "400 Bad Request",
                "application/json",
                br#"{"error":"invalid game id"}"#,
                origin,
            );
            return;
        }
        let metadata = match recording::session_for_game(game_id) {
            Ok(Some(metadata)) => metadata,
            Ok(None) => {
                write_response(
                    &mut stream,
                    "404 Not Found",
                    "application/json",
                    br#"{"error":"recording not found"}"#,
                    origin,
                );
                return;
            }
            Err(error) => {
                report(
                    &state,
                    "local_bridge_metadata_failed",
                    format!("game_id={game_id} error={error}"),
                );
                write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    "application/json",
                    br#"{"error":"recording metadata unavailable"}"#,
                    origin,
                );
                return;
            }
        };
        if !video {
            match serde_json::to_vec(&public_metadata(&metadata)) {
                Ok(body) => write_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    &body,
                    origin,
                ),
                Err(error) => {
                    report(
                        &state,
                        "local_bridge_metadata_failed",
                        format!("game_id={game_id} serialize_error={error}"),
                    );
                    write_response(
                        &mut stream,
                        "500 Internal Server Error",
                        "text/plain; charset=utf-8",
                        error.to_string().as_bytes(),
                        origin,
                    );
                }
            }
            return;
        }
        serve_video(
            &mut stream,
            &metadata.video_path,
            range,
            origin,
            &state,
            game_id,
        );
    }

    fn serve_video(
        stream: &mut TcpStream,
        path: &str,
        range: Option<&str>,
        origin: Option<&str>,
        state: &Arc<AppState>,
        game_id: &str,
    ) {
        let path = Path::new(path);
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) => {
                report(
                    state,
                    "video_open_failed",
                    format!("game_id={game_id} error={error}"),
                );
                write_response(
                    stream,
                    "404 Not Found",
                    "application/json",
                    br#"{"error":"video missing"}"#,
                    origin,
                );
                return;
            }
        };
        let info = match file.metadata() {
            Ok(info) => info,
            Err(error) => {
                report(
                    state,
                    "video_metadata_failed",
                    format!("game_id={game_id} error={error}"),
                );
                return;
            }
        };
        let total = info.len();
        if total == 0 {
            report(
                state,
                "video_metadata_failed",
                format!("game_id={game_id} empty_video"),
            );
            write_response(
                stream,
                "404 Not Found",
                "application/json",
                br#"{"error":"empty video"}"#,
                origin,
            );
            return;
        }
        let parsed = range
            .and_then(|value| value.strip_prefix("bytes="))
            .and_then(|value| {
                let (start, end) = value.split_once('-')?;
                let start = start.parse::<u64>().ok()?;
                let end = if end.is_empty() {
                    total - 1
                } else {
                    end.parse::<u64>().ok()?.min(total - 1)
                };
                (start <= end && start < total).then_some((start, end))
            });
        let (start, end, status) = parsed.map_or((0, total - 1, "200 OK"), |(start, end)| {
            (start, end, "206 Partial Content")
        });
        let length = end - start + 1;
        if let Err(error) = file.seek(SeekFrom::Start(start)) {
            report(
                state,
                "video_seek_io_failed",
                format!("game_id={game_id} start={start} error={error}"),
            );
            return;
        }
        let headers = common_headers(origin);
        let content_range = if status.starts_with("206") {
            format!("Content-Range: bytes {start}-{end}/{total}\r\n")
        } else {
            String::new()
        };
        let head = format!("HTTP/1.1 {status}\r\nContent-Type: video/mp4\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\n{content_range}{headers}Connection: close\r\n\r\n");
        if stream.write_all(head.as_bytes()).is_err() {
            return;
        }
        let mut remaining = length;
        let mut chunk = vec![0u8; CHUNK_BYTES];
        while remaining > 0 {
            let take = usize::try_from(remaining.min(CHUNK_BYTES as u64)).unwrap_or(CHUNK_BYTES);
            let read = match file.read(&mut chunk[..take]) {
                Ok(read) => read,
                Err(error) => {
                    report(
                        state,
                        "video_read_failed",
                        format!("game_id={game_id} error={error}"),
                    );
                    break;
                }
            };
            if read == 0 {
                break;
            }
            if stream.write_all(&chunk[..read]).is_err() {
                break;
            }
            remaining = remaining.saturating_sub(read as u64);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::state::AppState;
    use std::sync::Arc;

    pub(super) fn start(_state: Arc<AppState>) {}
}

pub(crate) fn start(state: std::sync::Arc<crate::state::AppState>) {
    imp::start(state);
}
