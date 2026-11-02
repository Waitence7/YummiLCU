use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock, RwLock,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use libwebrtc::{
    prelude::{I420Buffer, RtcVideoSource, VideoFrame, VideoResolution, VideoRotation},
    video_source::native::NativeVideoSource,
};
use livekit::{
    options::{TrackPublishOptions, VideoCodec, VideoEncoding},
    prelude::{LocalTrack, LocalVideoTrack, Room, RoomEvent, RoomOptions, TrackSource},
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::{sync::Mutex, task::JoinHandle, time::interval};

const STREAM_BITRATE: u64 = 3_000_000;
const STREAM_FPS: f64 = 15.0;
const MAX_URL_BYTES: usize = 2_048;
const MAX_ROOM_BYTES: usize = 128;
const MAX_TOKEN_BYTES: usize = 8_192;
const POINTER_TOPIC: &str = "yummi.live.pointer.v1";
const POINTER_MAX_BYTES: usize = 256;
const POINTER_VISIBLE_TTL_MS: u64 = 900;

#[derive(Clone, Debug)]
struct SfuConfig {
    app: AppHandle,
    url: String,
    token: String,
    codec: VideoCodec,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemotePointerSnapshot {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) visible: bool,
    pub(crate) updated_at_ms: u64,
    pub(crate) viewer: String,
}

impl Default for RemotePointerSnapshot {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            visible: false,
            updated_at_ms: 0,
            viewer: String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct WirePointer {
    v: u8,
    x: f64,
    y: f64,
    visible: bool,
}

static REMOTE_POINTER: OnceLock<RwLock<RemotePointerSnapshot>> = OnceLock::new();

static DEADLINE_MS: AtomicU64 = AtomicU64::new(0);
static CONFIG: OnceLock<Mutex<Option<SfuConfig>>> = OnceLock::new();
static TASK: OnceLock<Mutex<Option<JoinHandle<()>>>> = OnceLock::new();

fn config_slot() -> &'static Mutex<Option<SfuConfig>> {
    CONFIG.get_or_init(|| Mutex::new(None))
}

fn task_slot() -> &'static Mutex<Option<JoinHandle<()>>> {
    TASK.get_or_init(|| Mutex::new(None))
}

fn pointer_slot() -> &'static RwLock<RemotePointerSnapshot> {
    REMOTE_POINTER.get_or_init(|| RwLock::new(RemotePointerSnapshot::default()))
}

pub(crate) fn remote_pointer_snapshot() -> RemotePointerSnapshot {
    let mut snapshot = pointer_slot()
        .read()
        .map(|value| value.clone())
        .unwrap_or_default();
    if snapshot.visible && now_ms().saturating_sub(snapshot.updated_at_ms) > POINTER_VISIBLE_TTL_MS {
        snapshot.visible = false;
    }
    snapshot
}

fn parse_live_pointer(
    payload: &[u8],
    topic: Option<&str>,
    participant_identity: Option<String>,
) -> Option<RemotePointerSnapshot> {
    if topic != Some(POINTER_TOPIC) || payload.is_empty() || payload.len() > POINTER_MAX_BYTES {
        return None;
    }
    let viewer = participant_identity.filter(|value| value.starts_with("viewer-"))?;
    let pointer = serde_json::from_slice::<WirePointer>(payload).ok()?;
    if pointer.v != 1
        || !pointer.x.is_finite()
        || !pointer.y.is_finite()
        || !(0.0..=1.0).contains(&pointer.x)
        || !(0.0..=1.0).contains(&pointer.y)
    {
        return None;
    }
    Some(RemotePointerSnapshot {
        x: pointer.x,
        y: pointer.y,
        visible: pointer.visible,
        updated_at_ms: now_ms(),
        viewer,
    })
}

fn handle_live_pointer(
    app: &AppHandle,
    payload: &[u8],
    topic: Option<&str>,
    participant_identity: Option<String>,
) {
    let Some(snapshot) = parse_live_pointer(payload, topic, participant_identity) else {
        return;
    };
    if let Ok(mut current) = pointer_slot().write() {
        *current = snapshot.clone();
    }
    let _ = app.emit("yummi://live-pointer", snapshot);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn lease_alive() -> bool {
    let deadline = DEADLINE_MS.load(Ordering::Acquire);
    deadline > 0 && now_ms() <= deadline
}

fn parse_codec(value: Option<&str>) -> Result<VideoCodec, String> {
    match value.unwrap_or("h264") {
        "h264" => Ok(VideoCodec::H264),
        "av1" => Ok(VideoCodec::AV1),
        _ => Err("지원하지 않는 실시간 중계 코덱입니다.".into()),
    }
}

fn validate_url(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= MAX_URL_BYTES
        && value.is_ascii()
        && (value.starts_with("wss://")
            || cfg!(debug_assertions)
                && (value.starts_with("ws://127.0.0.1") || value.starts_with("ws://localhost")))
}

pub(crate) async fn control(
    app: AppHandle,
    enabled: bool,
    ttl_ms: u64,
    url: Option<String>,
    room: Option<String>,
    token: Option<String>,
    codec: Option<String>,
) -> Result<(), String> {
    if !enabled {
        DEADLINE_MS.store(0, Ordering::Release);
        crate::recording::set_live_stream_enabled(false, 1_000);
        return Ok(());
    }

    let url = url.ok_or_else(|| "LiveKit URL이 없습니다.".to_string())?;
    let room = room.ok_or_else(|| "LiveKit room이 없습니다.".to_string())?;
    let token = token.ok_or_else(|| "LiveKit token이 없습니다.".to_string())?;
    if !validate_url(&url)
        || room.is_empty()
        || room.len() > MAX_ROOM_BYTES
        || !room.is_ascii()
        || token.len() < 32
        || token.len() > MAX_TOKEN_BYTES
        || !token.is_ascii()
    {
        return Err("LiveKit SFU 설정이 올바르지 않습니다.".into());
    }
    let codec = parse_codec(codec.as_deref())?;
    let ttl_ms = ttl_ms.clamp(1_000, 30_000);
    DEADLINE_MS.store(now_ms().saturating_add(ttl_ms), Ordering::Release);
    crate::recording::set_live_stream_enabled(true, ttl_ms);
    *config_slot().lock().await = Some(SfuConfig { app, url, token, codec });

    let mut task = task_slot().lock().await;
    let needs_start = task.as_ref().is_none_or(JoinHandle::is_finished);
    if needs_start {
        *task = Some(tokio::spawn(publisher_loop()));
    }
    Ok(())
}

async fn publisher_loop() {
    while lease_alive() {
        let Some(config) = config_slot().lock().await.clone() else {
            break;
        };
        if let Err(error) = run_session(config).await {
            eprintln!("LiveKit SFU publish session failed: {error}");
        }
        if lease_alive() {
            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    }
    crate::recording::set_live_stream_enabled(false, 1_000);
}

fn copy_frame_to_i420(frame: &crate::recording::LiveStreamFrame) -> Result<I420Buffer, String> {
    let mut buffer = I420Buffer::new(frame.width, frame.height);
    let (dst_y, dst_u, dst_v) = buffer.data_mut();
    if dst_y.len() != frame.y.len() || dst_u.len() != frame.u.len() || dst_v.len() != frame.v.len()
    {
        return Err("LiveKit I420 프레임 크기가 일치하지 않습니다.".into());
    }
    dst_y.copy_from_slice(&frame.y);
    dst_u.copy_from_slice(&frame.u);
    dst_v.copy_from_slice(&frame.v);
    Ok(buffer)
}

async fn wait_first_frame(
    frames: &mut tokio::sync::broadcast::Receiver<crate::recording::LiveStreamFrame>,
) -> Result<crate::recording::LiveStreamFrame, String> {
    let mut tick = interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if !lease_alive() {
                    return Err("LiveKit 송출 lease가 만료되었습니다.".into());
                }
            }
            received = frames.recv() => {
                match received {
                    Ok(frame) => return Ok(frame),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return Err("실시간 캡처 채널이 종료되었습니다.".into()),
                }
            }
        }
    }
}

async fn run_session(config: SfuConfig) -> Result<(), String> {
    let (room, mut events) = Room::connect(&config.url, &config.token, RoomOptions::default())
        .await
        .map_err(|error| format!("LiveKit room 연결 실패: {error}"))?;
    let mut frames = crate::recording::subscribe_live_stream();
    crate::recording::set_live_stream_enabled(true, 15_000);
    let first = wait_first_frame(&mut frames).await?;

    let source = NativeVideoSource::new(
        VideoResolution {
            width: first.width,
            height: first.height,
        },
        false,
    );
    let track =
        LocalVideoTrack::create_video_track("yummi-screen", RtcVideoSource::Native(source.clone()));
    room.local_participant()
        .publish_track(
            LocalTrack::Video(track.clone()),
            TrackPublishOptions {
                video_encoding: Some(VideoEncoding {
                    max_bitrate: STREAM_BITRATE,
                    max_framerate: STREAM_FPS,
                }),
                video_codec: config.codec,
                simulcast: false,
                source: TrackSource::Screenshare,
                ..Default::default()
            },
        )
        .await
        .map_err(|error| format!("LiveKit H.264 track publish 실패: {error}"))?;

    let publish_frame = |frame: crate::recording::LiveStreamFrame| -> Result<(), String> {
        let buffer = copy_frame_to_i420(&frame)?;
        let mut video = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
        video.timestamp_us = frame
            .captured_at_ms
            .saturating_mul(1_000)
            .min(i64::MAX as u64) as i64;
        if !source.capture_frame(&video) {
            return Err("LiveKit video source가 프레임을 거부했습니다.".into());
        }
        Ok(())
    };
    publish_frame(first)?;

    let mut tick = interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if !lease_alive() {
                    break;
                }
                crate::recording::set_live_stream_enabled(true, 15_000);
            }
            event = events.recv() => {
                match event {
                    Some(RoomEvent::Disconnected { .. }) | None => break,
                    Some(RoomEvent::DataReceived { payload, topic, participant, .. }) => {
                        handle_live_pointer(
                            &config.app,
                            payload.as_slice(),
                            topic.as_deref(),
                            participant.map(|value| value.identity().to_string()),
                        );
                    }
                    _ => {}
                }
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
                publish_frame(frame)?;
            }
        }
    }

    let _ = room.local_participant().unpublish_track(&track.sid()).await;
    let _ = room.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_only_secure_livekit_origins_in_release_shape() {
        assert!(validate_url("wss://live.example.test"));
        assert!(!validate_url("https://live.example.test"));
        assert!(!validate_url("file:///tmp/live"));
    }

    #[test]
    fn codec_defaults_to_h264_and_supports_av1() {
        assert_eq!(parse_codec(None).unwrap(), VideoCodec::H264);
        assert_eq!(parse_codec(Some("av1")).unwrap(), VideoCodec::AV1);
        assert!(parse_codec(Some("vp8")).is_err());
    }

    #[test]
    fn pointer_packets_are_bounded_and_viewer_scoped() {
        let payload = br#"{"v":1,"x":0.25,"y":0.75,"visible":true}"#;
        let pointer = parse_live_pointer(payload, Some(POINTER_TOPIC), Some("viewer-42-abc".into())).unwrap();
        assert_eq!(pointer.x, 0.25);
        assert_eq!(pointer.y, 0.75);
        assert!(pointer.visible);
        assert!(parse_live_pointer(payload, Some("other"), Some("viewer-42-abc".into())).is_none());
        assert!(parse_live_pointer(payload, Some(POINTER_TOPIC), Some("agent-42".into())).is_none());
        assert!(parse_live_pointer(
            br#"{"v":1,"x":1.1,"y":0.5,"visible":true}"#,
            Some(POINTER_TOPIC),
            Some("viewer-42-abc".into())
        ).is_none());
    }
}
