use std::{path::Path, time::Duration};

use futures_util::StreamExt;
use serde_json::Value;
use tokio_util::io::ReaderStream;

use crate::{
    config::Config,
    error::{AgentError, AgentResult},
    session::Session,
};

const MAX_REPLAY_UPLOAD_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ANALYSIS_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

fn client() -> AgentResult<reqwest::Client> {
    reqwest::Client::builder()
        // Custom session headers must never be forwarded to redirect targets.
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(5 * 60))
        .build()
        .map_err(|error| AgentError::Relay(format!("ROFL 분석 HTTP client 생성 실패: {error}")))
}

async fn response_json(response: reqwest::Response, label: &str) -> AgentResult<Value> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ANALYSIS_RESPONSE_BYTES as u64)
    {
        return Err(AgentError::Relay(format!("{label} 응답이 너무 큽니다.")));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|error| AgentError::Relay(format!("{label} 응답 읽기 실패: {error}")))?;
        if bytes.len().saturating_add(chunk.len()) > MAX_ANALYSIS_RESPONSE_BYTES {
            return Err(AgentError::Relay(format!("{label} 응답이 너무 큽니다.")));
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let detail = String::from_utf8_lossy(&bytes);
        return Err(AgentError::Relay(format!(
            "{label} 실패 (status={} detail={})",
            status.as_u16(),
            detail.chars().take(180).collect::<String>()
        )));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| AgentError::Relay(format!("{label} 응답 형식 오류: {error}")))
}

pub(crate) async fn start_remote_replay_analysis(
    config: &Config,
    session: &Session,
    game_id: &str,
    path: &Path,
) -> AgentResult<Value> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| AgentError::Relay(format!("ROFL 파일을 확인할 수 없습니다: {error}")))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_REPLAY_UPLOAD_BYTES
    {
        return Err(AgentError::Relay(
            "ROFL 파일 형식 또는 크기가 올바르지 않습니다.".into(),
        ));
    }

    let file = tokio::fs::File::open(path)
        .await
        .map_err(|error| AgentError::Relay(format!("ROFL 파일을 열 수 없습니다: {error}")))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("replay.rofl");
    let response = client()?
        .post(config.replay_analysis_url(&session.session_id, game_id)?)
        .header("x-yummi-ws-token", &session.ws_token)
        .header("x-replay-file-name", file_name)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header(reqwest::header::CONTENT_LENGTH, metadata.len())
        .body(reqwest::Body::wrap_stream(ReaderStream::new(file)))
        .send()
        .await
        .map_err(|error| AgentError::Relay(format!("ROFL 서버 분석 제출 연결 실패: {error}")))?;
    response_json(response, "ROFL 서버 분석 제출").await
}

pub(crate) async fn remote_replay_analysis_job(
    config: &Config,
    session: &Session,
    job_id: &str,
) -> AgentResult<Value> {
    let response = client()?
        .get(config.replay_analysis_job_url(&session.session_id, job_id)?)
        .header("x-yummi-ws-token", &session.ws_token)
        .send()
        .await
        .map_err(|error| {
            AgentError::Relay(format!("ROFL 서버 분석 상태 조회 연결 실패: {error}"))
        })?;
    response_json(response, "ROFL 서버 분석 상태 조회").await
}

pub(crate) async fn remote_replay_analysis_viewer(
    config: &Config,
    session: &Session,
    job_id: &str,
) -> AgentResult<Value> {
    let response = client()?
        .get(config.replay_analysis_viewer_url(&session.session_id, job_id)?)
        .header("x-yummi-ws-token", &session.ws_token)
        .send()
        .await
        .map_err(|error| {
            AgentError::Relay(format!("ROFL 서버 재생 데이터 조회 연결 실패: {error}"))
        })?;
    response_json(response, "ROFL 서버 재생 데이터 조회").await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[tokio::test]
    async fn authenticated_analysis_does_not_follow_redirects() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            let size = socket.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..size]).contains("x-yummi-ws-token"));
            socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let response = client()
            .unwrap()
            .get(format!("http://{address}/analysis"))
            .header("x-yummi-ws-token", "test-session-secret")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
        assert!(response_json(response, "test").await.is_err());
        server.join().unwrap();
    }
}
