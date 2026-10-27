#[cfg(windows)]
use std::{
    io::Write,
    net::TcpListener,
    sync::{Mutex, OnceLock},
};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(windows)]
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tokio::time::sleep;

use crate::{
    lcu::{discover_lockfile_nonblocking, LcuClient},
    state::{AppState, GameBarSnapshot},
    updater::verify_manifest_payload_signature,
};

const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);
const FRIEND_REFRESH_EVERY_TICKS: u8 = 3;
#[cfg(windows)]
const OVERLAY_UPDATE_INTERVAL: Duration = Duration::from_secs(5 * 60);
#[cfg(windows)]
const GAMEBAR_VERSION_URL: &str = "https://raw.githubusercontent.com/Waitence7/YummiLCU/main/agent-tauri/gamebar-widget/version.json";
#[cfg(windows)]
const MAX_GAMEBAR_MANIFEST_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const MAX_GAMEBAR_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
#[cfg(windows)]
const GAMEBAR_LOOPBACK_ADDR: &str = "127.0.0.1:19514";

#[cfg(windows)]
#[derive(Clone, Debug, Default)]
struct ExternalClickTestState {
    active: bool,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    alt_x: f64,
    alt_y: f64,
    alt_width: f64,
    alt_height: f64,
    geometry_updated_at_ms: u64,
    sequence: u64,
    click_count: u64,
    event: String,
    event_x: i32,
    event_y: i32,
    cursor_x: i32,
    cursor_y: i32,
    monitor_x: i32,
    monitor_y: i32,
    matched_geometry: String,
    event_at_ms: u64,
    poll_count: u64,
    poll_updated_at_ms: u64,
    cursor_source: String,
    physical_x: i32,
    physical_y: i32,
    logical_x: i32,
    logical_y: i32,
    cursor_info_x: i32,
    cursor_info_y: i32,
    cursor_info_valid: bool,
}

#[cfg(windows)]
static EXTERNAL_CLICK_TEST: OnceLock<Mutex<ExternalClickTestState>> = OnceLock::new();

#[cfg(windows)]
fn external_click_test_state() -> &'static Mutex<ExternalClickTestState> {
    EXTERNAL_CLICK_TEST.get_or_init(|| Mutex::new(ExternalClickTestState::default()))
}

pub(crate) fn start(app: AppHandle, state: Arc<AppState>) {
    let watcher_state = state.clone();
    tauri::async_runtime::spawn(async move {
        watch_snapshot(watcher_state).await;
    });

    #[cfg(windows)]
    {
        let snapshot_state = state.clone();
        std::thread::Builder::new()
            .name("yummi-gamebar-pipe".into())
            .spawn(move || pipe_server(snapshot_state))
            .ok();

        let tcp_app = app.clone();
        let tcp_state = state.clone();
        std::thread::Builder::new()
            .name("yummi-gamebar-loopback".into())
            .spawn(move || loopback_server(tcp_app, tcp_state))
            .ok();

        let update_state = state.clone();
        tauri::async_runtime::spawn(async move {
            watch_overlay_updates(update_state).await;
        });

        std::thread::Builder::new()
            .name("yummi-gamebar-mouse-input".into())
            .spawn(external_mouse_input_thread)
            .ok();

        std::thread::Builder::new()
            .name("yummi-gamebar-command-pipe".into())
            .spawn(move || command_pipe_server(app, state))
            .ok();
    }

    #[cfg(not(windows))]
    {
        let _ = app;
        let _ = state;
    }
}

#[cfg(windows)]
async fn watch_overlay_updates(state: Arc<AppState>) {
    let mut shutdown = state.shutdown_receiver();

    // Check once immediately after Agent startup, then every five minutes.
    loop {
        if *shutdown.borrow() {
            break;
        }

        // Missing updater simply means the overlay has never been installed.
        // Other failures are persisted by check_overlay_update() so update problems
        // are diagnosable instead of silently disappearing.
        if let Err(error) = check_overlay_update().await {
            append_gamebar_update_log(&format!("오버레이 업데이트 확인 실패: {error}"));
        }

        tokio::select! {
            _ = sleep(OVERLAY_UPDATE_INTERVAL) => {},
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}

async fn watch_snapshot(state: Arc<AppState>) {
    let mut shutdown = state.shutdown_receiver();
    let mut friend_tick = 0u8;
    let mut cached_online_friends = Vec::new();
    loop {
        if *shutdown.borrow() {
            break;
        }

        let refresh_friends = friend_tick == 0;
        let snapshot = collect_snapshot(&state, refresh_friends, &cached_online_friends).await;
        if refresh_friends {
            cached_online_friends = snapshot.online_friends.clone();
        }
        friend_tick = (friend_tick + 1) % FRIEND_REFRESH_EVERY_TICKS;
        state.set_gamebar_snapshot(snapshot);

        tokio::select! {
            _ = sleep(SNAPSHOT_INTERVAL) => {},
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}

async fn collect_snapshot(
    state: &Arc<AppState>,
    refresh_friends: bool,
    cached_online_friends: &[String],
) -> GameBarSnapshot {
    let config = state.config.read().await.clone();
    let path = if let Some(path) = state.lcu_lockfile_path() {
        Some(path)
    } else {
        match discover_lockfile_nonblocking(&config).await {
            Ok(discovery) => {
                if let Some(path) = discovery.path.clone() {
                    state.set_lcu_lockfile_path(Some(path));
                }
                discovery.path
            }
            Err(_) => None,
        }
    };
    let Some(path) = path else {
        let previous = state.gamebar_snapshot();
        let agent_lcu = state.lcu_state().await;
        if agent_lcu.is_ready() && previous.connected {
            return GameBarSnapshot {
                updated_at_ms: now_ms(),
                ..previous
            };
        }
        return GameBarSnapshot {
            connected: agent_lcu.is_ready(),
            phase: if agent_lcu.is_ready() {
                "Connected".into()
            } else {
                "Offline".into()
            },
            updated_at_ms: now_ms(),
            ..GameBarSnapshot::default()
        };
    };

    let client =
        LcuClient::from_lockfile(&path).or_else(|_| LcuClient::from_lockfile_legacy(&path));
    let Ok(client) = client else {
        return GameBarSnapshot {
            phase: "LCU Error".into(),
            updated_at_ms: now_ms(),
            ..GameBarSnapshot::default()
        };
    };

    let phase = client
        .gameflow_phase()
        .await
        .unwrap_or_else(|_| "Unknown".into());
    let riot_id = client
        .current_summoner()
        .await
        .ok()
        .and_then(|value| riot_id_from_summoner(&value));
    let party_members = client
        .lobby()
        .await
        .ok()
        .map(|value| lobby_riot_ids(&value))
        .unwrap_or_default();
    let online_friends = if refresh_friends {
        client
            .friends()
            .await
            .ok()
            .map(|value| online_friend_riot_ids(&value))
            .unwrap_or_else(|| cached_online_friends.to_vec())
    } else {
        cached_online_friends.to_vec()
    };

    GameBarSnapshot {
        connected: true,
        riot_id,
        phase,
        party_members,
        online_friends,
        updated_at_ms: now_ms(),
    }
}

fn riot_id_from_summoner(value: &Value) -> Option<String> {
    let name = value
        .get("gameName")
        .or_else(|| value.get("riotIdGameName"))
        .or_else(|| value.get("displayName"))
        .and_then(Value::as_str)?
        .trim();
    if name.is_empty() {
        return None;
    }
    let tag = value
        .get("tagLine")
        .or_else(|| value.get("riotIdTagLine"))
        .or_else(|| value.get("riotIdTagline"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Some(match tag {
        Some(tag) if !name.contains('#') => format!("{name}#{tag}"),
        _ => name.to_owned(),
    })
}

fn lobby_riot_ids(value: &Value) -> Vec<String> {
    let Some(members) = value.get("members").and_then(Value::as_array) else {
        return Vec::new();
    };
    members
        .iter()
        .filter_map(|member| {
            let name = member
                .get("gameName")
                .or_else(|| member.get("riotIdGameName"))
                .or_else(|| member.get("summonerName"))
                .and_then(Value::as_str)?
                .trim();
            if name.is_empty() {
                return None;
            }
            let tag = member
                .get("tagLine")
                .or_else(|| member.get("riotIdTagLine"))
                .or_else(|| member.get("riotIdTagline"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            Some(match tag {
                Some(tag) if !name.contains('#') => format!("{name}#{tag}"),
                _ => name.to_owned(),
            })
        })
        .take(5)
        .collect()
}

fn online_friend_riot_ids(value: &Value) -> Vec<String> {
    let Some(friends) = value.as_array() else {
        return Vec::new();
    };
    let mut result = friends
        .iter()
        .filter(|friend| {
            friend
                .get("availability")
                .and_then(Value::as_str)
                .is_some_and(|availability| !matches!(availability, "offline" | "mobile"))
        })
        .filter_map(|friend| {
            let name = friend
                .get("gameName")
                .or_else(|| friend.get("riotIdGameName"))
                .or_else(|| friend.get("summonerName"))
                .and_then(Value::as_str)?
                .trim();
            if name.is_empty() {
                return None;
            }
            let tag = friend
                .get("gameTag")
                .or_else(|| friend.get("tagLine"))
                .or_else(|| friend.get("riotIdTagLine"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            Some(match tag {
                Some(tag) if !name.contains('#') => format!("{name}#{tag}"),
                _ => name.to_owned(),
            })
        })
        .collect::<Vec<_>>();
    result.sort_unstable();
    result.dedup();
    result
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(windows)]
fn loopback_server(app: AppHandle, state: Arc<AppState>) {
    use std::io::{BufRead, BufReader};

    // During an in-place update the previous Agent can take a moment to fully
    // release the loopback listener. Retry instead of silently disabling the
    // Game Bar bridge for the lifetime of the new process.
    let listener = {
        let mut bound = None;
        for attempt in 1..=20 {
            match TcpListener::bind(GAMEBAR_LOOPBACK_ADDR) {
                Ok(listener) => {
                    bound = Some(listener);
                    break;
                }
                Err(error) => {
                    eprintln!(
                        "[gamebar bridge] loopback bind failed attempt={attempt}/20 addr={GAMEBAR_LOOPBACK_ADDR}: {error}"
                    );
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
        let Some(listener) = bound else {
            eprintln!(
                "[gamebar bridge] loopback bind permanently failed addr={GAMEBAR_LOOPBACK_ADDR}"
            );
            return;
        };
        listener
    };

    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else { continue };
        let app = app.clone();
        let state = state.clone();
        std::thread::spawn(move || {
            // Preserve the legacy snapshot protocol: every connection receives
            // a snapshot immediately, without having to send a request first.
            let mut snapshot =
                serde_json::to_vec(&state.gamebar_snapshot()).unwrap_or_else(|_| b"{}".to_vec());
            snapshot.push(b'\n');
            if stream.write_all(&snapshot).is_err() || stream.flush().is_err() {
                return;
            }

            // Command-capable clients consume the snapshot greeting, then send
            // one newline-delimited command on the same localhost connection.
            // Snapshot-only clients close after the first line, yielding EOF.
            let Ok(reader_stream) = stream.try_clone() else {
                return;
            };
            let _ = reader_stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut reader = BufReader::new(reader_stream);
            let mut raw = String::new();
            let Ok(read) = reader.read_line(&mut raw) else {
                return;
            };
            if read == 0 {
                return;
            }
            let response = if raw.len() > 1024 {
                json_response(false, "명령을 읽지 못했습니다.")
            } else {
                handle_command(&app, state, raw.trim_end_matches(['\r', '\n']))
            };
            let mut payload = response.into_bytes();
            payload.push(b'\n');
            let _ = stream.write_all(&payload);
            let _ = stream.flush();
        });
    }
}

#[cfg(windows)]
fn named_pipe_connected(result: windows::core::Result<()>) -> bool {
    const ERROR_PIPE_CONNECTED_HRESULT: i32 = 0x80070217u32 as i32;
    match result {
        Ok(()) => true,
        Err(error) => error.code().0 == ERROR_PIPE_CONNECTED_HRESULT,
    }
}

#[cfg(windows)]
fn pipe_server(state: Arc<AppState>) {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{CloseHandle, LocalFree, HLOCAL, INVALID_HANDLE_VALUE},
            Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
            },
            Storage::FileSystem::{FlushFileBuffers, WriteFile, PIPE_ACCESS_OUTBOUND},
            System::Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
                PIPE_TYPE_BYTE, PIPE_WAIT,
            },
        },
    };

    const PIPE_NAME: &str = r"\\.\pipe\LOCAL\YummiLcuGameBarV1";
    // Read-only bridge data is intentionally exposed to local AppContainer clients.
    // The payload never contains Riot lockfile credentials, Discord tokens, or relay secrets.
    const SDDL: &str = "D:(A;;GA;;;WD)(A;;GA;;;AC)";
    const SDDL_REVISION_1: u32 = 1;

    let pipe_name = wide_null(PIPE_NAME);
    let sddl = wide_null(SDDL);

    loop {
        let mut descriptor = PSECURITY_DESCRIPTOR(std::ptr::null_mut());
        let descriptor_result = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        };
        if descriptor_result.is_err() || descriptor.0.is_null() {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }

        let mut security = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };

        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(pipe_name.as_ptr()),
                PIPE_ACCESS_OUTBOUND,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                4,
                16 * 1024,
                1024,
                0,
                Some(&mut security),
            )
        };
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        if pipe == INVALID_HANDLE_VALUE {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }

        let connected = named_pipe_connected(unsafe { ConnectNamedPipe(pipe, None) });
        if connected {
            let mut payload =
                serde_json::to_vec(&state.gamebar_snapshot()).unwrap_or_else(|_| b"{}".to_vec());
            payload.push(b'\n');
            let mut written = 0u32;
            unsafe {
                let _ = WriteFile(pipe, Some(&payload), Some(&mut written), None);
                let _ = FlushFileBuffers(pipe);
                let _ = DisconnectNamedPipe(pipe);
            }
        }
        unsafe {
            let _ = CloseHandle(pipe);
        }
    }
}

#[cfg(windows)]
fn monitor_origin_for_point(x: i32, y: i32) -> Option<(i32, i32)> {
    use std::mem::size_of;
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    };

    let handle = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    if handle.0.is_null() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(handle, &mut info) }.as_bool() {
        return None;
    }
    Some((info.rcMonitor.left, info.rcMonitor.top))
}

#[cfg(windows)]
fn external_mouse_input_thread() {
    use windows::Win32::{
        Foundation::POINT,
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
            WindowsAndMessaging::{GetCursorPos, GetPhysicalCursorPos},
        },
    };

    // Poll the native mouse state instead of depending on WH_MOUSE_LL. Some
    // Game Bar / elevation combinations allow the Agent to query global mouse
    // state but do not deliver low-level hook callbacks reliably.
    let mut was_down = false;
    loop {
        let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
        use windows::Win32::UI::WindowsAndMessaging::{GetCursorInfo, CURSORINFO};
        let mut physical = POINT::default();
        let mut logical = POINT::default();
        let mut cursor_info = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        let physical_ok = unsafe { GetPhysicalCursorPos(&mut physical) }.is_ok();
        let logical_ok = unsafe { GetCursorPos(&mut logical) }.is_ok();
        let cursor_info_ok = unsafe { GetCursorInfo(&mut cursor_info) }.is_ok();
        let (point, cursor_source) = if physical_ok {
            (physical, Some("physical"))
        } else if logical_ok {
            (logical, Some("logical"))
        } else if cursor_info_ok {
            (cursor_info.ptScreenPos, Some("cursor-info-fallback"))
        } else {
            (POINT::default(), None)
        };
        if let Some(cursor_source) = cursor_source {
            let now = now_ms();
            if let Ok(mut state) = external_click_test_state().lock() {
                // Prefer physical screen coordinates because Game Bar window
                // geometry is ultimately matched against native screen pixels.
                // Fall back to GetCursorPos for systems where physical cursor
                // coordinates are unavailable.
                state.poll_count = state.poll_count.wrapping_add(1);
                state.poll_updated_at_ms = now;
                state.cursor_source = cursor_source.into();
                state.cursor_x = point.x;
                state.cursor_y = point.y;
                state.physical_x = physical.x;
                state.physical_y = physical.y;
                state.logical_x = logical.x;
                state.logical_y = logical.y;
                state.cursor_info_x = cursor_info.ptScreenPos.x;
                state.cursor_info_y = cursor_info.ptScreenPos.y;
                state.cursor_info_valid = cursor_info_ok;
                let (monitor_x, monitor_y) =
                    monitor_origin_for_point(point.x, point.y).unwrap_or((0, 0));
                state.monitor_x = monitor_x;
                state.monitor_y = monitor_y;

                if down != was_down {
                    // Expire stale geometry so a crashed/closed widget can never
                    // leave a ghost hitbox active in the Agent.
                    let fresh = now.saturating_sub(state.geometry_updated_at_ms) <= 2_000;
                    let px = point.x as f64;
                    let py = point.y as f64;
                    let mx = monitor_x as f64;
                    let my = monitor_y as f64;
                    let inside = |rx: f64, ry: f64, rw: f64, rh: f64| {
                        fresh
                            && state.active
                            && px >= rx
                            && px < rx + rw
                            && py >= ry
                            && py < ry + rh
                    };
                    let inside_primary = inside(state.x, state.y, state.width, state.height);
                    let inside_alt =
                        inside(state.alt_x, state.alt_y, state.alt_width, state.alt_height);
                    let inside_primary_monitor =
                        inside(state.x + mx, state.y + my, state.width, state.height);
                    let inside_alt_monitor = inside(
                        state.alt_x + mx,
                        state.alt_y + my,
                        state.alt_width,
                        state.alt_height,
                    );
                    if inside_primary || inside_alt || inside_primary_monitor || inside_alt_monitor
                    {
                        state.sequence = state.sequence.wrapping_add(1);
                        if !down {
                            state.click_count = state.click_count.wrapping_add(1);
                        }
                        state.event = if down { "down" } else { "up" }.into();
                        state.event_x = point.x;
                        state.event_y = point.y;
                        state.matched_geometry = if inside_primary {
                            "scaled-bounds"
                        } else if inside_alt {
                            "raw-bounds"
                        } else if inside_primary_monitor {
                            "scaled-bounds+monitor"
                        } else {
                            "raw-bounds+monitor"
                        }
                        .into();
                        state.event_at_ms = now;
                    }
                }
            }
        }
        was_down = down;
        std::thread::sleep(Duration::from_millis(8));
    }
}

#[cfg(windows)]
fn sync_external_click_test(parsed: &Value) -> String {
    let active = parsed
        .get("active")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let number = |key: &str| parsed.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let x = number("x");
    let y = number("y");
    let width = number("width");
    let height = number("height");
    let alt_x = number("altX");
    let alt_y = number("altY");
    let alt_width = number("altWidth");
    let alt_height = number("altHeight");
    let gamebar_cursor_x = number("gamebarCursorX");
    let gamebar_cursor_y = number("gamebarCursorY");
    let gamebar_alt_cursor_x = number("gamebarAltCursorX");
    let gamebar_alt_cursor_y = number("gamebarAltCursorY");
    let gamebar_cursor_at_ms = parsed
        .get("gamebarCursorAtMs")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let valid_rect = |rx: f64, ry: f64, rw: f64, rh: f64| {
        rx.is_finite()
            && ry.is_finite()
            && rw.is_finite()
            && rh.is_finite()
            && rx.abs() <= 100_000.0
            && ry.abs() <= 100_000.0
            && (1.0..=2_000.0).contains(&rw)
            && (1.0..=2_000.0).contains(&rh)
    };
    let valid_geometry =
        valid_rect(x, y, width, height) && valid_rect(alt_x, alt_y, alt_width, alt_height);
    if active && !valid_geometry {
        return json_response(false, "외부 클릭 테스트 영역이 올바르지 않습니다.");
    }

    let now = now_ms();

    // Refresh the cursor synchronously for every Game Bar sync request. This
    // keeps diagnostics and hit testing fresh even if the background poller is
    // delayed, starved, or failed to start in a particular Windows/Game Bar
    // environment.
    let direct_cursor = {
        use windows::Win32::{
            Foundation::POINT,
            UI::WindowsAndMessaging::{
                GetCursorInfo, GetCursorPos, GetPhysicalCursorPos, CURSORINFO,
            },
        };
        let mut physical = POINT::default();
        let mut logical = POINT::default();
        let mut cursor_info = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        let physical_ok = unsafe { GetPhysicalCursorPos(&mut physical) }.is_ok();
        let logical_ok = unsafe { GetCursorPos(&mut logical) }.is_ok();
        let cursor_info_ok = unsafe { GetCursorInfo(&mut cursor_info) }.is_ok();
        if physical_ok {
            Some((
                physical.x,
                physical.y,
                "direct-physical",
                physical,
                logical,
                cursor_info.ptScreenPos,
                cursor_info_ok,
            ))
        } else if logical_ok {
            Some((
                logical.x,
                logical.y,
                "direct-logical",
                physical,
                logical,
                cursor_info.ptScreenPos,
                cursor_info_ok,
            ))
        } else if cursor_info_ok {
            Some((
                cursor_info.ptScreenPos.x,
                cursor_info.ptScreenPos.y,
                "direct-cursor-info-fallback",
                physical,
                logical,
                cursor_info.ptScreenPos,
                true,
            ))
        } else {
            None
        }
    };

    let gamebar_cursor = if gamebar_cursor_at_ms > 0
        && now.saturating_sub(gamebar_cursor_at_ms) <= 500
        && gamebar_cursor_x.is_finite()
        && gamebar_cursor_y.is_finite()
        && gamebar_alt_cursor_x.is_finite()
        && gamebar_alt_cursor_y.is_finite()
    {
        let native = direct_cursor.as_ref().map(|cursor| (cursor.0 as f64, cursor.1 as f64));
        let primary = (gamebar_cursor_x, gamebar_cursor_y);
        let alternate = (gamebar_alt_cursor_x, gamebar_alt_cursor_y);
        let chosen = if let Some((nx, ny)) = native {
            let primary_distance = (primary.0 - nx).powi(2) + (primary.1 - ny).powi(2);
            let alternate_distance = (alternate.0 - nx).powi(2) + (alternate.1 - ny).powi(2);
            if alternate_distance < primary_distance { alternate } else { primary }
        } else {
            primary
        };
        Some((chosen.0.round() as i32, chosen.1.round() as i32))
    } else {
        None
    };

    let mut state = match external_click_test_state().lock() {
        Ok(state) => state,
        Err(_) => return json_response(false, "외부 클릭 상태 잠금에 실패했습니다."),
    };
    if let Some((cursor_x, cursor_y, source, physical, logical, cursor_info, cursor_info_valid)) =
        direct_cursor
    {
        state.cursor_x = cursor_x;
        state.cursor_y = cursor_y;
        state.physical_x = physical.x;
        state.physical_y = physical.y;
        state.logical_x = logical.x;
        state.logical_y = logical.y;
        state.cursor_info_x = cursor_info.x;
        state.cursor_info_y = cursor_info.y;
        state.cursor_info_valid = cursor_info_valid;
        let (monitor_x, monitor_y) = monitor_origin_for_point(cursor_x, cursor_y).unwrap_or((0, 0));
        state.monitor_x = monitor_x;
        state.monitor_y = monitor_y;
        state.cursor_source = source.into();
    }
    if let Some((cursor_x, cursor_y)) = gamebar_cursor {
        state.cursor_x = cursor_x;
        state.cursor_y = cursor_y;
        let (monitor_x, monitor_y) = monitor_origin_for_point(cursor_x, cursor_y).unwrap_or((0, 0));
        state.monitor_x = monitor_x;
        state.monitor_y = monitor_y;
        state.cursor_source = "gamebar-pointer".into();
    }
    state.active = active;
    if active {
        state.x = x;
        state.y = y;
        state.width = width;
        state.height = height;
        state.alt_x = alt_x;
        state.alt_y = alt_y;
        state.alt_width = alt_width;
        state.alt_height = alt_height;
    }
    state.geometry_updated_at_ms = now;
    serde_json::json!({
        "ok": true,
        "active": state.active,
        "sequence": state.sequence,
        "clickCount": state.click_count,
        "event": state.event,
        "eventX": state.event_x,
        "eventY": state.event_y,
        "cursorX": state.cursor_x,
        "cursorY": state.cursor_y,
        "monitorX": state.monitor_x,
        "monitorY": state.monitor_y,
        "matchedGeometry": state.matched_geometry,
        "rectX": state.x,
        "rectY": state.y,
        "rectWidth": state.width,
        "rectHeight": state.height,
        "altRectX": state.alt_x,
        "altRectY": state.alt_y,
        "altRectWidth": state.alt_width,
        "altRectHeight": state.alt_height,
        "eventAtMs": state.event_at_ms,
        "pollCount": state.poll_count,
        "pollUpdatedAtMs": state.poll_updated_at_ms,
        "cursorSource": state.cursor_source,
        "physicalX": state.physical_x,
        "physicalY": state.physical_y,
        "logicalX": state.logical_x,
        "logicalY": state.logical_y,
        "cursorInfoX": state.cursor_info_x,
        "cursorInfoY": state.cursor_info_y,
        "cursorInfoValid": state.cursor_info_valid,
        "gamebarCursorAtMs": gamebar_cursor_at_ms,
        "gamebarCursorAgeMs": if gamebar_cursor_at_ms > 0 { now.saturating_sub(gamebar_cursor_at_ms) } else { 0 },
        "pollAgeMs": now.saturating_sub(state.poll_updated_at_ms),
        "agentPid": std::process::id(),
        "agentReleaseLabel": option_env!("YUMMI_AGENT_RELEASE_LABEL")
            .unwrap_or(env!("CARGO_PKG_VERSION")),
        "agentBuildId": option_env!("YUMMI_AGENT_BUILD_ID").unwrap_or("local"),
        "agentCommit": option_env!("YUMMI_AGENT_GIT_COMMIT").unwrap_or("unknown"),
    })
    .to_string()
}

#[cfg(windows)]
fn command_pipe_server(app: AppHandle, state: Arc<AppState>) {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{CloseHandle, LocalFree, HLOCAL, INVALID_HANDLE_VALUE},
            Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
            },
            Storage::FileSystem::{FlushFileBuffers, ReadFile, WriteFile, PIPE_ACCESS_DUPLEX},
            System::Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
                PIPE_TYPE_BYTE, PIPE_WAIT,
            },
        },
    };

    const PIPE_NAME: &str = r"\\.\pipe\LOCAL\YummiLcuGameBarCommandV1";
    const SDDL: &str = "D:(A;;GA;;;WD)(A;;GA;;;AC)";
    const SDDL_REVISION_1: u32 = 1;
    const MAX_REQUEST_BYTES: usize = 1024;

    let pipe_name = wide_null(PIPE_NAME);
    let sddl = wide_null(SDDL);

    loop {
        let mut descriptor = PSECURITY_DESCRIPTOR(std::ptr::null_mut());
        let descriptor_result = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        };
        if descriptor_result.is_err() || descriptor.0.is_null() {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }

        let mut security = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(pipe_name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                2,
                4096,
                4096,
                0,
                Some(&mut security),
            )
        };
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        if pipe == INVALID_HANDLE_VALUE {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }

        if named_pipe_connected(unsafe { ConnectNamedPipe(pipe, None) }) {
            let mut buffer = [0u8; MAX_REQUEST_BYTES];
            let mut read = 0u32;
            let read_result = unsafe { ReadFile(pipe, Some(&mut buffer), Some(&mut read), None) };
            let response = if read_result.is_ok() && read > 0 {
                let raw = String::from_utf8_lossy(&buffer[..read as usize]);
                handle_command(&app, state.clone(), raw.trim_end_matches(['\r', '\n']))
            } else {
                json_response(false, "명령을 읽지 못했습니다.")
            };

            let mut payload = response.into_bytes();
            payload.push(b'\n');
            let mut written = 0u32;
            unsafe {
                let _ = WriteFile(pipe, Some(&payload), Some(&mut written), None);
                let _ = FlushFileBuffers(pipe);
                let _ = DisconnectNamedPipe(pipe);
            }
        }
        unsafe {
            let _ = CloseHandle(pipe);
        }
    }
}

#[cfg(windows)]
fn handle_command(app: &AppHandle, state: Arc<AppState>, raw: &str) -> String {
    let parsed: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => return json_response(false, "잘못된 명령 형식입니다."),
    };
    let command = parsed
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if command == "check_overlay_update" {
        tauri::async_runtime::spawn(async move {
            if let Err(error) = check_overlay_update().await {
                append_gamebar_update_log(&format!("수동 오버레이 업데이트 확인 실패: {error}"));
            }
        });
        return json_response(true, "오버레이 업데이트 확인을 시작했습니다.");
    }
    if command == "open_url" {
        let Some(url) = parsed.get("url").and_then(Value::as_str) else {
            return json_response(false, "열 URL이 없습니다.");
        };
        if !(url == "https://www.youtube.com/"
            || url.starts_with("https://www.youtube.com/watch?v="))
        {
            return json_response(false, "허용되지 않은 URL입니다.");
        }
        return match crate::platform::open_login_url(app, url) {
            Ok(()) => json_response(true, "기본 브라우저를 열었습니다."),
            Err(error) => json_response(false, &format!("브라우저 열기 실패: {error}")),
        };
    }
    if command == "sync_external_click_test" {
        return sync_external_click_test(&parsed);
    }
    if command != "set_status_message" {
        return json_response(false, "허용되지 않은 명령입니다.");
    }
    let Some(value) = parsed.get("value").and_then(Value::as_str) else {
        return json_response(false, "상태 메시지가 없습니다.");
    };
    let value = match validate_status_message(value) {
        Ok(value) => value,
        Err(message) => return json_response(false, message),
    };

    let result = tauri::async_runtime::block_on(set_status_message(state, value.clone()));
    match result {
        Ok(()) => json_response(true, "상태 메시지를 변경했습니다."),
        Err(error) => json_response(false, &error),
    }
}

#[cfg(windows)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameBarUpdateTarget {
    version: String,
    release_tag: String,
    build_id: String,
    commit: String,
    url: String,
    sha256: String,
    size: u64,
}

#[cfg(windows)]
#[derive(Debug, Deserialize, Serialize, Default)]
struct GameBarUpdateState {
    build_id: String,
    sha256: String,
    release_tag: String,
}

fn parse_gamebar_release_version(tag: &str) -> Option<(u64, u64, u64)> {
    let raw = tag.strip_prefix("gamebar-v")?;
    let mut parts = raw.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn select_gamebar_manifest(releases: &Value) -> Option<(String, String)> {
    releases
        .as_array()?
        .iter()
        .filter_map(|release| {
            if release
                .get("draft")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return None;
            }
            let tag = release.get("tag_name")?.as_str()?;
            let version = parse_gamebar_release_version(tag)?;
            let manifest_url = release
                .get("assets")?
                .as_array()?
                .iter()
                .find_map(|asset| {
                    (asset.get("name")?.as_str()? == "gamebar-version.json")
                        .then(|| {
                            asset
                                .get("browser_download_url")?
                                .as_str()
                                .map(str::to_owned)
                        })
                        .flatten()
                })?;
            Some((version, tag.to_owned(), manifest_url))
        })
        .max_by_key(|(version, _, _)| *version)
        .map(|(_, tag, url)| (tag, url))
}

#[cfg(windows)]
fn append_gamebar_update_log(message: &str) {
    use std::{
        env,
        fs::{self, OpenOptions},
        io::Write,
        path::PathBuf,
    };
    let Some(local_app_data) = env::var_os("LOCALAPPDATA") else {
        return;
    };
    let updater_dir = PathBuf::from(local_app_data)
        .join("Yummi")
        .join("GameBarUpdater");
    if fs::create_dir_all(&updater_dir).is_err() {
        return;
    }
    let path = updater_dir.join("update.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "[{now}] {message}");
    }
}

#[cfg(windows)]
async fn check_overlay_update() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::{env, fs, path::PathBuf, process::Command};

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let local_app_data =
        env::var_os("LOCALAPPDATA").ok_or_else(|| "LOCALAPPDATA를 찾지 못했습니다.".to_owned())?;
    let updater_dir = PathBuf::from(local_app_data)
        .join("Yummi")
        .join("GameBarUpdater");
    let updater = updater_dir.join("Update-YummiGameBar.ps1");
    if !updater.is_file() {
        return Ok(());
    }
    append_gamebar_update_log("오버레이 업데이트 확인 시작");

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent("Yummi-LCU-Agent-GameBar-Updater")
        .build()
        .map_err(|e| format!("Game Bar 업데이트 클라이언트 생성 실패: {e}"))?;
    let latest: Value = client
        .get(GAMEBAR_VERSION_URL)
        .send()
        .await
        .map_err(|e| format!("Game Bar 최신 버전 조회 실패: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Game Bar 최신 버전 응답 실패: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Game Bar 최신 버전 JSON 실패: {e}"))?;
    let selected_version = latest
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| "Game Bar 최신 버전 정보가 없습니다.".to_owned())?;
    let selected_release_tag = format!("gamebar-v{selected_version}");
    let selected_semver = parse_gamebar_release_version(&selected_release_tag)
        .ok_or_else(|| "Game Bar 최신 버전 형식이 올바르지 않습니다.".to_owned())?;
    let manifest_url = format!(
        "https://github.com/Waitence7/YummiLCU/releases/download/{selected_release_tag}/gamebar-version.json"
    );
    append_gamebar_update_log(&format!(
        "최신 오버레이 릴리즈 감지: {selected_release_tag}"
    ));

    let response = client
        .get(&manifest_url)
        .send()
        .await
        .map_err(|e| format!("Game Bar manifest 다운로드 실패: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Game Bar manifest 응답 실패: {e}"))?;
    if response.content_length().unwrap_or(0) > MAX_GAMEBAR_MANIFEST_BYTES as u64 {
        return Err("Game Bar manifest가 너무 큽니다.".into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Game Bar manifest 읽기 실패: {e}"))?;
    if bytes.len() > MAX_GAMEBAR_MANIFEST_BYTES {
        return Err("Game Bar manifest가 너무 큽니다.".into());
    }
    let root: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("Game Bar manifest JSON 오류: {e}"))?;
    let mut payload = root
        .get("gamebar")
        .cloned()
        .ok_or_else(|| "Game Bar manifest에 gamebar 블록이 없습니다.".to_owned())?;
    let signature = payload
        .as_object_mut()
        .and_then(|o| o.remove("signature"))
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or_else(|| "Game Bar manifest 서명이 없습니다.".to_owned())?;
    verify_manifest_payload_signature(&payload, &signature).map_err(|e| e.to_string())?;
    let target: GameBarUpdateTarget =
        serde_json::from_value(payload).map_err(|e| format!("Game Bar manifest 형식 오류: {e}"))?;
    if target.release_tag != selected_release_tag || target.version != selected_version {
        return Err("Game Bar manifest 버전이 선택된 릴리즈와 일치하지 않습니다.".into());
    }

    if target.sha256.len() != 64 || !target.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Game Bar manifest SHA256 형식이 올바르지 않습니다.".into());
    }
    if target.size == 0 || target.size > MAX_GAMEBAR_ARCHIVE_BYTES as u64 {
        return Err("Game Bar ZIP 크기가 허용 범위를 벗어났습니다.".into());
    }
    let parsed =
        url::Url::parse(&target.url).map_err(|e| format!("Game Bar 다운로드 URL 오류: {e}"))?;
    let expected_release_path = format!(
        "/Waitence7/YummiLCU/releases/download/{}/",
        selected_release_tag
    );
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || !parsed.path().starts_with(&expected_release_path)
        || !parsed.path().ends_with(".zip")
    {
        return Err("Game Bar 다운로드 URL이 허용되지 않았습니다.".into());
    }

    let state_path = updater_dir.join("state.json");
    if let Ok(raw) = fs::read_to_string(&state_path) {
        if let Ok(state) = serde_json::from_str::<GameBarUpdateState>(&raw) {
            if let Some(installed_semver) = parse_gamebar_release_version(&state.release_tag) {
                if installed_semver > selected_semver {
                    append_gamebar_update_log(&format!(
                        "설치된 오버레이가 최신 채널보다 높아 유지: {}",
                        state.release_tag
                    ));
                    return Ok(());
                }
            }
            if state.build_id == target.build_id
                && state.sha256.eq_ignore_ascii_case(&target.sha256)
            {
                append_gamebar_update_log(&format!(
                    "이미 최신 오버레이: {} / build {}",
                    target.release_tag, target.build_id
                ));
                return Ok(());
            }
        }
    }

    let response = client
        .get(&target.url)
        .send()
        .await
        .map_err(|e| format!("Game Bar ZIP 다운로드 실패: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Game Bar ZIP 응답 실패: {e}"))?;
    if response.content_length().unwrap_or(target.size) != target.size {
        return Err("Game Bar ZIP 크기가 manifest와 다릅니다.".into());
    }
    let archive = response
        .bytes()
        .await
        .map_err(|e| format!("Game Bar ZIP 읽기 실패: {e}"))?;
    if archive.len() as u64 != target.size || archive.len() > MAX_GAMEBAR_ARCHIVE_BYTES {
        return Err("Game Bar ZIP 크기 검증 실패".into());
    }
    let actual = format!("{:x}", Sha256::digest(&archive));
    if !actual.eq_ignore_ascii_case(&target.sha256) {
        return Err("Game Bar ZIP SHA256 검증 실패".into());
    }

    fs::create_dir_all(&updater_dir)
        .map_err(|e| format!("Game Bar updater 폴더 생성 실패: {e}"))?;
    let zip_path = updater_dir.join("verified-update.zip");
    fs::write(&zip_path, &archive).map_err(|e| format!("검증된 Game Bar ZIP 저장 실패: {e}"))?;

    append_gamebar_update_log(&format!(
        "오버레이 업데이트 설치 시작: {} / build {}",
        target.release_tag, target.build_id
    ));
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&updater)
        .arg("-PackagePath")
        .arg(&zip_path)
        .arg("-ExpectedSha256")
        .arg(&target.sha256)
        .arg("-BuildId")
        .arg(&target.build_id)
        .arg("-ReleaseTag")
        .arg(&target.release_tag)
        .arg("-Quiet")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Game Bar 설치 실행 실패: {e}"))?;
    Ok(())
}

async fn set_status_message(state: Arc<AppState>, value: String) -> Result<(), String> {
    let config = state.config.read().await.clone();
    let path = discover_lockfile_nonblocking(&config)
        .await
        .map_err(|error| format!("LCU 탐색 실패: {error}"))?
        .path
        .ok_or_else(|| "League 클라이언트가 실행 중이 아닙니다.".to_owned())?;
    let client = LcuClient::from_lockfile(&path)
        .or_else(|_| LcuClient::from_lockfile_legacy(&path))
        .map_err(|error| format!("LCU 연결 실패: {error}"))?;
    client
        .set_status_message(&value)
        .await
        .map_err(|error| format!("상태 메시지 변경 실패: {error}"))
}

fn validate_status_message(raw: &str) -> Result<String, &'static str> {
    let value = raw.trim();
    if value.chars().count() > 128 {
        return Err("상태 메시지는 128자 이하로 입력해 주세요.");
    }
    if value
        .chars()
        .any(|character| character == '\0' || character == '\r' || character == '\n')
    {
        return Err("상태 메시지에는 줄바꿈을 사용할 수 없습니다.");
    }
    Ok(value.to_owned())
}

fn json_response(ok: bool, message: &str) -> String {
    serde_json::json!({ "ok": ok, "message": message }).to_string()
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        lobby_riot_ids, online_friend_riot_ids, parse_gamebar_release_version,
        riot_id_from_summoner, select_gamebar_manifest, validate_status_message,
    };
    use serde_json::json;

    #[test]
    fn selects_highest_gamebar_release_independent_of_api_order() {
        let releases = json!([
            {"tag_name":"v0.8.0-beta.3","draft":false,"assets":[{"name":"agent-version.json","browser_download_url":"https://example.invalid/agent"}]},
            {"tag_name":"gamebar-v0.1.9","draft":false,"assets":[{"name":"gamebar-version.json","browser_download_url":"https://example.invalid/0.1.9"}]},
            {"tag_name":"gamebar-v0.1.11","draft":false,"assets":[{"name":"gamebar-version.json","browser_download_url":"https://example.invalid/0.1.11"}]},
            {"tag_name":"gamebar-v0.1.10","draft":false,"assets":[{"name":"gamebar-version.json","browser_download_url":"https://example.invalid/0.1.10"}]}
        ]);
        let (tag, url) = select_gamebar_manifest(&releases).unwrap();
        assert_eq!(tag, "gamebar-v0.1.11");
        assert_eq!(url, "https://example.invalid/0.1.11");
        assert_eq!(parse_gamebar_release_version(&tag), Some((0, 1, 11)));
    }

    #[test]
    fn formats_current_riot_id() {
        assert_eq!(
            riot_id_from_summoner(&json!({"gameName":"Yummi","tagLine":"KR1"})).as_deref(),
            Some("Yummi#KR1")
        );
    }

    #[test]
    fn validates_status_message_command_input() {
        assert_eq!(validate_status_message("  hello  ").unwrap(), "hello");
        assert_eq!(validate_status_message("").unwrap(), "");
        assert!(validate_status_message("line\nbreak").is_err());
        assert!(validate_status_message(&"x".repeat(129)).is_err());
    }

    #[test]
    fn extracts_lobby_members() {
        assert_eq!(
            lobby_riot_ids(
                &json!({"members":[{"gameName":"A","tagLine":"KR1"},{"summonerName":"B#KR2"}]})
            ),
            vec!["A#KR1", "B#KR2"]
        );
    }
    #[test]
    fn extracts_only_online_friends() {
        assert_eq!(
            online_friend_riot_ids(&json!([
                {"gameName":"Online","gameTag":"KR1","availability":"online"},
                {"gameName":"Away","gameTag":"KR2","availability":"away"},
                {"gameName":"Offline","gameTag":"KR3","availability":"offline"},
                {"gameName":"Mobile","gameTag":"KR4","availability":"mobile"}
            ])),
            vec!["Away#KR2", "Online#KR1"]
        );
    }
}
