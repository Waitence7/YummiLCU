use std::{sync::{Arc, OnceLock}, time::Duration};

use bytes::Bytes;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{sync::Mutex, time::interval};
use webrtc::{
    api::APIBuilder,
    data_channel::{data_channel_state::RTCDataChannelState, RTCDataChannel},
    ice_transport::{ice_credential_type::RTCIceCredentialType, ice_server::RTCIceServer},
    peer_connection::{
        configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
        RTCPeerConnection,
    },
};

const CHANNEL_LABEL: &str = "yummi-live-v1";
const MAX_SDP_BYTES: usize = 96 * 1024;
const MAX_ICE_SERVERS: usize = 8;
const MAX_ICE_URLS: usize = 8;
const FRAME_CHUNK_BYTES: usize = 14 * 1024;
const FRAME_HEADER_BYTES: usize = 32;
const MAX_BUFFERED_BYTES: usize = 512 * 1024;

#[derive(Debug, Deserialize)]
struct WebRtcOfferPayload {
    sdp: String,
    #[serde(default)]
    ice_servers: Vec<IceServerPayload>,
}

#[derive(Debug, Deserialize)]
struct IceServerPayload {
    #[serde(default)]
    urls: Vec<String>,
    #[serde(default)]
    username: String,
    #[serde(default)]
    credential: String,
}

fn active_peer() -> &'static Mutex<Option<Arc<RTCPeerConnection>>> {
    static ACTIVE: OnceLock<Mutex<Option<Arc<RTCPeerConnection>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(None))
}

fn valid_ice_url(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 512
        && matches!(
            value.split(':').next().unwrap_or_default().to_ascii_lowercase().as_str(),
            "stun" | "stuns" | "turn" | "turns"
        )
        && value.is_ascii()
}

fn parse_offer(payload: &Value) -> Result<(RTCSessionDescription, Vec<RTCIceServer>), String> {
    let parsed: WebRtcOfferPayload = serde_json::from_value(payload.clone())
        .map_err(|_| "WebRTC offer 형식이 올바르지 않습니다.".to_string())?;
    if parsed.sdp.is_empty() || parsed.sdp.len() > MAX_SDP_BYTES || !parsed.sdp.starts_with("v=0") {
        return Err("WebRTC SDP가 올바르지 않습니다.".into());
    }
    if parsed.ice_servers.len() > MAX_ICE_SERVERS {
        return Err("WebRTC ICE 서버가 너무 많습니다.".into());
    }
    let mut servers = Vec::with_capacity(parsed.ice_servers.len());
    for server in parsed.ice_servers {
        if server.urls.is_empty() || server.urls.len() > MAX_ICE_URLS {
            return Err("WebRTC ICE URL 수가 올바르지 않습니다.".into());
        }
        if server.username.len() > 256 || server.credential.len() > 512 {
            return Err("WebRTC ICE 인증 정보가 너무 깁니다.".into());
        }
        let urls = server
            .urls
            .into_iter()
            .map(|url| url.trim().to_owned())
            .collect::<Vec<_>>();
        if urls.iter().any(|url| !valid_ice_url(url)) {
            return Err("허용되지 않은 WebRTC ICE URL입니다.".into());
        }
        let has_credentials = !server.username.is_empty() || !server.credential.is_empty();
        servers.push(RTCIceServer {
            urls,
            username: server.username,
            credential: server.credential,
            credential_type: if has_credentials {
                RTCIceCredentialType::Password
            } else {
                RTCIceCredentialType::Unspecified
            },
        });
    }
    RTCSessionDescription::offer(parsed.sdp)
        .map(|offer| (offer, servers))
        .map_err(|error| format!("WebRTC SDP 파싱 실패: {error}"))
}

fn frame_chunk(
    frame: &crate::recording::LiveStreamFrame,
    chunk_index: usize,
    chunk_count: usize,
    payload: &[u8],
) -> Bytes {
    let mut message = Vec::with_capacity(FRAME_HEADER_BYTES + payload.len());
    message.extend_from_slice(b"YMJ1");
    message.extend_from_slice(&frame.sequence.to_be_bytes());
    message.extend_from_slice(&frame.captured_at_ms.to_be_bytes());
    message.extend_from_slice(&(frame.width.min(u16::MAX as u32) as u16).to_be_bytes());
    message.extend_from_slice(&(frame.height.min(u16::MAX as u32) as u16).to_be_bytes());
    message.extend_from_slice(&(chunk_index.min(u16::MAX as usize) as u16).to_be_bytes());
    message.extend_from_slice(&(chunk_count.min(u16::MAX as usize) as u16).to_be_bytes());
    message.extend_from_slice(&(frame.jpeg.len().min(u32::MAX as usize) as u32).to_be_bytes());
    message.extend_from_slice(payload);
    Bytes::from(message)
}

async fn stream_frames(channel: Arc<RTCDataChannel>) {
    let mut frames = crate::recording::subscribe_live_stream();
    let mut keepalive = interval(Duration::from_secs(1));
    crate::recording::set_live_stream_enabled(true, 5_000);

    loop {
        if channel.ready_state() != RTCDataChannelState::Open {
            break;
        }
        tokio::select! {
            _ = keepalive.tick() => {
                crate::recording::set_live_stream_enabled(true, 5_000);
            }
            received = frames.recv() => {
                let mut frame = match received {
                    Ok(frame) => frame,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                while let Ok(newer) = frames.try_recv() {
                    frame = newer;
                }
                if channel.buffered_amount().await > MAX_BUFFERED_BYTES {
                    continue;
                }
                let chunk_count = frame.jpeg.len().div_ceil(FRAME_CHUNK_BYTES);
                if chunk_count == 0 || chunk_count > u16::MAX as usize {
                    continue;
                }
                let mut failed = false;
                for (index, payload) in frame.jpeg.chunks(FRAME_CHUNK_BYTES).enumerate() {
                    if channel.send(&frame_chunk(&frame, index, chunk_count, payload)).await.is_err() {
                        failed = true;
                        break;
                    }
                }
                if failed {
                    break;
                }
                crate::recording::set_live_stream_enabled(true, 5_000);
            }
        }
    }
}

pub(crate) async fn answer_offer(payload: &Value) -> Result<Value, String> {
    let (offer, ice_servers) = parse_offer(payload)?;

    let previous = active_peer().lock().await.take();
    if let Some(previous) = previous {
        let _ = previous.close().await;
    }

    let api = APIBuilder::new().build();
    let peer = Arc::new(
        api.new_peer_connection(RTCConfiguration {
            ice_servers,
            ..Default::default()
        })
        .await
        .map_err(|error| format!("WebRTC PeerConnection 생성 실패: {error}"))?,
    );

    peer.on_data_channel(Box::new(move |channel: Arc<RTCDataChannel>| {
        Box::pin(async move {
            if channel.label() != CHANNEL_LABEL {
                let _ = channel.close().await;
                return;
            }
            let sender_channel = Arc::clone(&channel);
            channel.on_open(Box::new(move || {
                let sender_channel = Arc::clone(&sender_channel);
                Box::pin(async move {
                    tokio::spawn(stream_frames(sender_channel));
                })
            }));
        })
    }));

    peer.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
        Box::pin(async move {
            if matches!(
                state,
                RTCPeerConnectionState::Failed
                    | RTCPeerConnectionState::Closed
                    | RTCPeerConnectionState::Disconnected
            ) {
                // The recording-side lease expires naturally if no open sender refreshes it.
            }
        })
    }));

    peer.set_remote_description(offer)
        .await
        .map_err(|error| format!("WebRTC offer 적용 실패: {error}"))?;
    let answer = peer
        .create_answer(None)
        .await
        .map_err(|error| format!("WebRTC answer 생성 실패: {error}"))?;
    let mut gathering_complete = peer.gathering_complete_promise().await;
    peer.set_local_description(answer)
        .await
        .map_err(|error| format!("WebRTC local description 설정 실패: {error}"))?;
    tokio::time::timeout(Duration::from_secs(8), gathering_complete.recv())
        .await
        .map_err(|_| "WebRTC ICE candidate 수집 시간 초과".to_string())?;
    let answer = peer
        .local_description()
        .await
        .ok_or_else(|| "WebRTC answer를 가져오지 못했습니다.".to_string())?;

    *active_peer().lock().await = Some(Arc::clone(&peer));
    Ok(json!({
        "type": "answer",
        "sdp": answer.sdp,
        "transport": "webrtc-datachannel-jpeg-v1",
        "width": 1280,
        "height": 720,
        "fps": 15
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ice_url_scheme_is_bounded() {
        assert!(valid_ice_url("stun:stun.l.google.com:19302"));
        assert!(valid_ice_url("turn:turn.example.com:3478?transport=udp"));
        assert!(!valid_ice_url("https://example.com"));
        assert!(!valid_ice_url("file:///etc/passwd"));
    }

    #[test]
    fn chunk_header_has_expected_size_and_magic() {
        let frame = crate::recording::LiveStreamFrame {
            sequence: 7,
            captured_at_ms: 9,
            width: 1280,
            height: 720,
            jpeg: vec![1, 2, 3],
        };
        let chunk = frame_chunk(&frame, 0, 1, &frame.jpeg);
        assert_eq!(&chunk[..4], b"YMJ1");
        assert_eq!(chunk.len(), FRAME_HEADER_BYTES + 3);
    }
}
