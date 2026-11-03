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

const MAX_URL_BYTES: usize = 2_048;
const MAX_ROOM_BYTES: usize = 128;
const MAX_TOKEN_BYTES: usize = 8_192;
const POINTER_TOPIC: &str = "yummi.live.pointer.v1";
const PING_TOPIC: &str = "yummi.live.ping.v1";
const POINTER_MAX_BYTES: usize = 256;
const PING_MAX_BYTES: usize = 1_024;
const POINTER_VISIBLE_TTL_MS: u64 = 900;

#[derive(Clone, Debug)]
struct SfuConfig {
    app: AppHandle,
    url: String,
    token: String,
    codec: VideoCodec,
    stream: crate::recording::LiveStreamConfig,
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

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemotePingSnapshot {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) ping_type: String,
    pub(crate) sequence: u64,
    pub(crate) updated_at_ms: u64,
    pub(crate) viewer: String,
    pub(crate) viewer_discord_id: String,
    pub(crate) viewer_name: String,
    pub(crate) viewer_avatar_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WirePingSender {
    discord_id: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WirePing {
    v: u8,
    x: f64,
    y: f64,
    #[serde(rename = "type")]
    ping_type: String,
    #[serde(default)]
    sender: Option<WirePingSender>,
}

static REMOTE_POINTER: OnceLock<RwLock<RemotePointerSnapshot>> = OnceLock::new();
static REMOTE_PING: OnceLock<RwLock<RemotePingSnapshot>> = OnceLock::new();
static PING_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static LAST_PING_MS: AtomicU64 = AtomicU64::new(0);

static DEADLINE_MS: AtomicU64 = AtomicU64::new(0);
static STREAM_GENERATION: AtomicU64 = AtomicU64::new(1);
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

fn ping_slot() -> &'static RwLock<RemotePingSnapshot> {
    REMOTE_PING.get_or_init(|| RwLock::new(RemotePingSnapshot::default()))
}

pub(crate) fn remote_ping_snapshot() -> RemotePingSnapshot {
    ping_slot().read().map(|value| value.clone()).unwrap_or_default()
}

pub(crate) fn remote_pointer_snapshot() -> RemotePointerSnapshot {
    let mut snapshot = pointer_slot()
        .read()
        .map(|value| value.clone())
        .unwrap_or_default();
    if snapshot.visible && now_ms().saturating_sub(snapshot.updated_at_ms) > POINTER_VISIBLE_TTL_MS
    {
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


pub(crate) fn valid_ping_type(value: &str) -> bool {
    matches!(value, "retreat" | "push" | "on_my_way" | "all_in" | "assist_me" | "need_vision" | "enemy_missing" | "enemy_vision")
}

fn viewer_discord_id(identity: &str) -> Option<&str> {
    let raw = identity.strip_prefix("viewer-")?;
    let id = raw.split('-').next()?;
    (id.len() >= 6 && id.len() <= 25 && id.bytes().all(|byte| byte.is_ascii_digit()) && !id.starts_with('0')).then_some(id)
}

fn valid_discord_avatar_url(value: &str, discord_id: &str) -> bool {
    if value.len() > 512 { return false; }
    let Ok(url) = url::Url::parse(value) else { return false; };
    if url.scheme() != "https" || !matches!(url.host_str(), Some("cdn.discordapp.com" | "media.discordapp.net")) { return false; }
    let path = url.path();
    path.starts_with(&format!("/avatars/{discord_id}/")) || path.starts_with("/embed/avatars/")
}

fn clean_viewer_name(value: Option<String>) -> String {
    value.unwrap_or_default().trim().chars().filter(|ch| !ch.is_control()).take(64).collect()
}

fn parse_live_ping(payload: &[u8], topic: Option<&str>, participant_identity: Option<String>) -> Option<RemotePingSnapshot> {
    if topic != Some(PING_TOPIC) || payload.is_empty() || payload.len() > PING_MAX_BYTES { return None; }
    let viewer = participant_identity.filter(|value| value.starts_with("viewer-"))?;
    let trusted_viewer_id = viewer_discord_id(&viewer)?.to_owned();
    let ping = serde_json::from_slice::<WirePing>(payload).ok()?;
    if ping.v != 1 || !ping.x.is_finite() || !ping.y.is_finite() || !(0.0..=1.0).contains(&ping.x) || !(0.0..=1.0).contains(&ping.y) || !valid_ping_type(&ping.ping_type) { return None; }
    let (viewer_name, viewer_avatar_url) = ping.sender
        .filter(|sender| sender.discord_id == trusted_viewer_id)
        .map(|sender| {
            let avatar = sender.avatar_url.filter(|value| valid_discord_avatar_url(value, &trusted_viewer_id)).unwrap_or_default();
            (clean_viewer_name(sender.display_name), avatar)
        })
        .unwrap_or_default();
    Some(RemotePingSnapshot {
        x: ping.x,
        y: ping.y,
        ping_type: ping.ping_type,
        sequence: PING_SEQUENCE.fetch_add(1, Ordering::AcqRel).saturating_add(1),
        updated_at_ms: now_ms(),
        viewer,
        viewer_discord_id: trusted_viewer_id,
        viewer_name,
        viewer_avatar_url,
    })
}

fn handle_live_ping(app: &AppHandle, payload: &[u8], topic: Option<&str>, participant_identity: Option<String>) {
    let Some(snapshot) = parse_live_ping(payload, topic, participant_identity) else { return; };
    let now = snapshot.updated_at_ms;
    let previous = LAST_PING_MS.load(Ordering::Acquire);
    if previous > 0 && now.saturating_sub(previous) < 250 { return; }
    LAST_PING_MS.store(now, Ordering::Release);
    if let Ok(mut current) = ping_slot().write() { *current = snapshot.clone(); }
    let _ = app.emit("yummi://live-ping", snapshot);
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
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<u32>,
    bitrate: Option<u64>,
) -> Result<(), String> {
    if !enabled {
        DEADLINE_MS.store(0, Ordering::Release);
        STREAM_GENERATION.fetch_add(1, Ordering::AcqRel);
        crate::recording::set_live_stream_enabled(false, 1_000);
        let _ = crate::recording::stop_live_stream_capture();
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
    let defaults = crate::recording::LiveStreamConfig::default();
    let stream = crate::recording::LiveStreamConfig::new(
        width.unwrap_or(defaults.max_width),
        height.unwrap_or(defaults.max_height),
        fps.unwrap_or(defaults.fps),
        bitrate.unwrap_or(defaults.bitrate),
    )?;
    let ttl_ms = ttl_ms.clamp(1_000, 30_000);
    crate::league_ping_audio::warm_up();
    DEADLINE_MS.store(now_ms().saturating_add(ttl_ms), Ordering::Release);
    crate::recording::set_live_stream_enabled(true, ttl_ms);
    crate::recording::start_live_stream_capture(stream)?;

    let mut config = config_slot().lock().await;
    let stream_changed = config
        .as_ref()
        .is_some_and(|current| current.stream != stream);
    *config = Some(SfuConfig {
        app,
        url,
        token,
        codec,
        stream,
    });
    drop(config);
    if stream_changed {
        STREAM_GENERATION.fetch_add(1, Ordering::AcqRel);
    }

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
        let generation = STREAM_GENERATION.load(Ordering::Acquire);
        if let Err(error) = run_session(config, generation).await {
            eprintln!("LiveKit SFU publish session failed: {error}");
        }
        if lease_alive() {
            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    }
    crate::recording::set_live_stream_enabled(false, 1_000);
    let _ = crate::recording::stop_live_stream_capture();
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

async fn run_session(config: SfuConfig, generation: u64) -> Result<(), String> {
    let mut room_options = RoomOptions::default();
    room_options.auto_subscribe = false;
    room_options.dynacast = true;
    let (room, mut events) = Room::connect(&config.url, &config.token, room_options)
        .await
        .map_err(|error| format!("LiveKit room 연결 실패: {error}"))?;
    let mut frames = crate::recording::subscribe_live_stream();
    crate::recording::set_live_stream_enabled(true, 15_000);
    let first = wait_first_frame(&mut frames).await?;
    let first_width = first.width;
    let first_height = first.height;

    let source = NativeVideoSource::new(
        VideoResolution {
            width: first_width,
            height: first_height,
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
                    max_bitrate: config.stream.bitrate,
                    max_framerate: f64::from(config.stream.fps),
                }),
                video_codec: config.codec,
                simulcast: true,
                source: TrackSource::Screenshare,
                ..Default::default()
            },
        )
        .await
        .map_err(|error| format!("LiveKit H.264 track publish 실패: {error}"))?;

    let publish_frame = |frame: crate::recording::LiveStreamFrame| -> Result<bool, String> {
        let buffer = copy_frame_to_i420(&frame)?;
        let mut video = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
        video.timestamp_us = frame
            .captured_at_ms
            .saturating_mul(1_000)
            .min(i64::MAX as u64) as i64;
        // NativeVideoSource may reject/drop an individual frame while its encoder or
        // pacing queue is busy. That is a normal real-time backpressure signal, not
        // a room/session failure. Reconnecting here causes overlapping LiveKit
        // participants with the same identity and a DUPLICATE_IDENTITY loop.
        Ok(source.capture_frame(&video))
    };
    let _ = publish_frame(first)?;

    let mut tick = interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if !lease_alive() || STREAM_GENERATION.load(Ordering::Acquire) != generation {
                    break;
                }
                crate::recording::set_live_stream_enabled(true, 15_000);
            }
            event = events.recv() => {
                match event {
                    Some(RoomEvent::Disconnected { .. }) | None => break,
                    Some(RoomEvent::DataReceived { payload, topic, participant, .. }) => {
                        let participant_identity = participant.map(|value| value.identity().to_string());
                        handle_live_pointer(
                            &config.app,
                            payload.as_slice(),
                            topic.as_deref(),
                            participant_identity.clone(),
                        );
                        handle_live_ping(
                            &config.app,
                            payload.as_slice(),
                            topic.as_deref(),
                            participant_identity,
                        );
                    }
                    _ => {}
                }
            }
            received = frames.recv() => {
                if STREAM_GENERATION.load(Ordering::Acquire) != generation {
                    break;
                }
                let mut frame = match received {
                    Ok(frame) => frame,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                while let Ok(newer) = frames.try_recv() {
                    frame = newer;
                }
                if STREAM_GENERATION.load(Ordering::Acquire) != generation
                    || frame.width != first_width
                    || frame.height != first_height
                {
                    break;
                }
                let _ = publish_frame(frame)?;
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
    fn stream_quality_bounds_allow_1440p_and_reject_oversized_values() {
        assert!(crate::recording::LiveStreamConfig::new(2560, 1440, 30, 10_000_000).is_ok());
        assert!(crate::recording::LiveStreamConfig::new(3840, 2160, 60, 20_000_000).is_err());
        assert!(crate::recording::LiveStreamConfig::new(1920, 1080, 120, 10_000_000).is_err());
    }

    #[test]
    fn dropped_capture_frame_is_not_a_session_error_contract() {
        // capture_frame() returns bool because a native source may drop a frame under
        // backpressure. The publisher must keep the room alive in that case.
        let source = include_str!("live_sfu.rs");
        assert!(source.contains("Ok(source.capture_frame(&video))"));
        assert!(source.contains("simulcast: true"));
        assert!(source.contains("dynacast: true"));
    }

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
    fn ping_sender_profile_must_match_authenticated_viewer_identity() {
        let accepted = parse_live_ping(
            br#"{"v":1,"x":0.25,"y":0.75,"type":"retreat","sender":{"discordId":"777777","displayName":"tester","avatarUrl":"https://cdn.discordapp.com/avatars/777777/hash.png"}}"#,
            Some(PING_TOPIC),
            Some("viewer-777777-abc".into()),
        )
        .unwrap();
        assert_eq!(accepted.viewer_discord_id, "777777");
        assert_eq!(accepted.viewer_name, "tester");
        assert_eq!(accepted.viewer_avatar_url, "https://cdn.discordapp.com/avatars/777777/hash.png");

        let spoofed = parse_live_ping(
            br#"{"v":1,"x":0.25,"y":0.75,"type":"retreat","sender":{"discordId":"888888","displayName":"spoof","avatarUrl":"https://cdn.discordapp.com/avatars/888888/hash.png"}}"#,
            Some(PING_TOPIC),
            Some("viewer-777777-abc".into()),
        )
        .unwrap();
        assert_eq!(spoofed.viewer_discord_id, "777777");
        assert!(spoofed.viewer_name.is_empty());
        assert!(spoofed.viewer_avatar_url.is_empty());
    }

    #[test]
    fn ping_avatar_rejects_non_discord_hosts() {
        let ping = parse_live_ping(
            br#"{"v":1,"x":0.25,"y":0.75,"type":"push","sender":{"discordId":"777777","avatarUrl":"https://example.com/avatar.png"}}"#,
            Some(PING_TOPIC),
            Some("viewer-777777-abc".into()),
        )
        .unwrap();
        assert!(ping.viewer_avatar_url.is_empty());
    }

    #[test]
    fn pointer_packets_are_bounded_and_viewer_scoped() {
        let payload = br#"{"v":1,"x":0.25,"y":0.75,"visible":true}"#;
        let pointer =
            parse_live_pointer(payload, Some(POINTER_TOPIC), Some("viewer-42-abc".into())).unwrap();
        assert_eq!(pointer.x, 0.25);
        assert_eq!(pointer.y, 0.75);
        assert!(pointer.visible);
        assert!(parse_live_pointer(payload, Some("other"), Some("viewer-42-abc".into())).is_none());
        assert!(
            parse_live_pointer(payload, Some(POINTER_TOPIC), Some("agent-42".into())).is_none()
        );
        assert!(parse_live_pointer(
            br#"{"v":1,"x":1.1,"y":0.5,"visible":true}"#,
            Some(POINTER_TOPIC),
            Some("viewer-42-abc".into())
        )
        .is_none());
    }

    #[test]
    fn ping_packets_are_current_wheel_types_and_viewer_scoped() {
        let payload = br#"{"v":1,"x":0.12,"y":0.34,"type":"enemy_vision"}"#;
        let ping = parse_live_ping(payload, Some(PING_TOPIC), Some("viewer-777777-abc".into())).unwrap();
        assert_eq!(ping.ping_type, "enemy_vision");
        assert!(parse_live_ping(br#"{"v":1,"x":0.5,"y":0.5,"type":"bait"}"#, Some(PING_TOPIC), Some("viewer-777777-abc".into())).is_none());
        assert!(parse_live_ping(payload, Some(PING_TOPIC), Some("agent-42".into())).is_none());
    }

}
