use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use tauri::AppHandle;
use tokio::time::sleep;

use crate::{
    lcu::{discover_lockfile_nonblocking, LcuClient},
    state::{AppState, GameBarSnapshot},
};

const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);
const FRIEND_REFRESH_EVERY_TICKS: u8 = 3;
#[cfg(windows)]
const OVERLAY_UPDATE_INTERVAL: Duration = Duration::from_secs(5 * 60);

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

        let update_state = state.clone();
        tauri::async_runtime::spawn(async move {
            watch_overlay_updates(update_state).await;
        });

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
        // Keep this silent so Agent users who do not use Game Bar are unaffected.
        let _ = start_overlay_update_check();

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
    let path = match discover_lockfile_nonblocking(&config).await {
        Ok(discovery) => discovery.path,
        Err(_) => None,
    };
    let Some(path) = path else {
        return GameBarSnapshot {
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
            System::{
                Pipes::{
                    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
                    PIPE_TYPE_BYTE, PIPE_WAIT,
                },
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
            System::{
                Pipes::{
                    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
                    PIPE_TYPE_BYTE, PIPE_WAIT,
                },
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
fn handle_command(_app: &AppHandle, state: Arc<AppState>, raw: &str) -> String {
    let parsed: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => return json_response(false, "잘못된 명령 형식입니다."),
    };
    let command = parsed
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if command == "check_overlay_update" {
        return match start_overlay_update_check() {
            Ok(()) => json_response(true, "오버레이 업데이트 확인을 시작했습니다."),
            Err(error) => json_response(false, &error),
        };
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
fn start_overlay_update_check() -> Result<(), String> {
    use std::{env, path::PathBuf, process::Command};
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let local_app_data = env::var_os("LOCALAPPDATA")
        .ok_or_else(|| "LOCALAPPDATA를 찾지 못했습니다.".to_owned())?;
    let updater = PathBuf::from(local_app_data)
        .join("Yummi")
        .join("GameBarUpdater")
        .join("Update-YummiGameBar.ps1");
    if !updater.is_file() {
        return Err("오버레이 updater가 설치되어 있지 않습니다. 최신 오버레이를 한 번 다시 설치해 주세요.".to_owned());
    }

    Command::new("powershell.exe")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&updater)
        .arg("-Quiet")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("업데이트 확인 실행 실패: {error}"))?;
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
        lobby_riot_ids, online_friend_riot_ids, riot_id_from_summoner, validate_status_message,
    };
    use serde_json::json;

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
