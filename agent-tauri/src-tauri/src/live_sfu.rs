use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
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
use tokio::{sync::Mutex, task::JoinHandle, time::interval};

const STREAM_BITRATE: u64 = 3_000_000;
const STREAM_FPS: f64 = 15.0;
const MAX_URL_BYTES: usize = 2_048;
const MAX_ROOM_BYTES: usize = 128;
const MAX_TOKEN_BYTES: usize = 8_192;

#[derive(Clone, Debug)]
struct SfuConfig {
    url: String,
    token: String,
    codec: VideoCodec,
}

static DEADLINE_MS: AtomicU64 = AtomicU64::new(0);
static CONFIG: OnceLock<Mutex<Option<SfuConfig>>> = OnceLock::new();
static TASK: OnceLock<Mutex<Option<JoinHandle<()>>>> = OnceLock::new();

fn config_slot() -> &'static Mutex<Option<SfuConfig>> {
    CONFIG.get_or_init(|| Mutex::new(None))
}

fn task_slot() -> &'static Mutex<Option<JoinHandle<()>>> {
    TASK.get_or_init(|| Mutex::new(None))
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
    *config_slot().lock().await = Some(SfuConfig { url, token, codec });

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
}
