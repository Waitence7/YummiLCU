use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::config::Config;

use super::client::LcuClient;

#[derive(Clone, Debug)]
pub(crate) struct LockfileDiscovery {
    pub path: Option<PathBuf>,
    pub diagnostics: Vec<String>,
    pub legacy_fallback: bool,
}

pub(crate) fn lockfile_path(config: &Config) -> Option<PathBuf> {
    discover_lockfile(config).path
}

pub(crate) fn discover_lockfile(config: &Config) -> LockfileDiscovery {
    if let Some(raw) = &config.lockfile_path {
        let path = PathBuf::from(expand_environment(raw));
        if valid_lcu_lockfile(&path) {
            return LockfileDiscovery { path: Some(path), diagnostics: Vec::new(), legacy_fallback: false };
        }
    }

    let (process_scan_available, league_process_seen, process_candidates) =
        running_lcu_lockfile_candidates();
    let mut candidates = process_candidates;

    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        let metadata = PathBuf::from(program_data)
            .join("Riot Games")
            .join("Metadata")
            .join("league_of_legends.live")
            .join("league_of_legends.live.product_settings.yaml");
        if let Ok(contents) = fs::read_to_string(metadata) {
            if let Some(install_path) = product_install_path(&contents) {
                candidates.push(install_path.join("lockfile"));
                candidates.push(install_path.join("Game").join("lockfile"));
            }
        }
    }

    // Riot Client's own Config/lockfile is not an LCU lockfile. Keep it only
    // as a presence hint so we can explain the "Riot Client only" state without
    // ever trying to authenticate it as LeagueClientUx.
    let riot_client_lockfile = std::env::var_os("LOCALAPPDATA").map(|local_app_data| {
        PathBuf::from(local_app_data)
            .join("Riot Games")
            .join("Riot Client")
            .join("Config")
            .join("lockfile")
    });

    // If Windows process enumeration succeeded and no League process exists,
    // scanning every drive cannot find a live LCU and can be very slow for
    // disconnected/network drives. Keep the broad path scan only as a fallback
    // when process enumeration itself is unavailable, or when a League process
    // exists but its executable-derived path did not work.
    if !process_scan_available || league_process_seen {
        for drive in b'C'..=b'Z' {
            let drive = drive as char;
            candidates.extend([
                PathBuf::from(format!(r"{drive}:\Riot Games\League of Legends\lockfile")),
                PathBuf::from(format!(
                    r"{drive}:\Riot Games\League of Legends\Game\lockfile"
                )),
                PathBuf::from(format!(
                    r"{drive}:\Program Files\Riot Games\League of Legends\lockfile"
                )),
                PathBuf::from(format!(r"{drive}:\League of Legends\lockfile")),
            ]);
        }
    }

    let mut diagnostics = Vec::new();
    let allow_legacy_fallback = !process_scan_available || league_process_seen;
    let mut legacy_candidate = None;
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        match LcuClient::from_lockfile(&path) {
            Ok(_) => return LockfileDiscovery { path: Some(path), diagnostics, legacy_fallback: false },
            Err(error) => {
                diagnostics.push(format!("LCU lockfile 후보 검증 실패: {} ({error})", path.display()));
                // Preserve the pre-hardening behavior as a final fallback: if a
                // real lockfile file exists, return it and let the normal LCU
                // client path report/handle the validation failure explicitly.
                if allow_legacy_fallback
                    && legacy_candidate.is_none()
                    && LcuClient::from_lockfile_legacy(&path).is_ok()
                {
                    legacy_candidate = Some(path);
                }
            }
        }
    }

    if let Some(path) = legacy_candidate {
        diagnostics.push(format!("LCU lockfile 과거 방식 fallback 사용: {}", path.display()));
        return LockfileDiscovery { path: Some(path), diagnostics, legacy_fallback: true };
    }

    if riot_client_lockfile.as_ref().is_some_and(|path| path.is_file()) {
        diagnostics.push(
            "Riot Client는 감지됐지만 LeagueClientUx lockfile은 없음 — League of Legends 클라이언트 실행 여부 확인"
                .into(),
        );
    }

    LockfileDiscovery { path: None, diagnostics, legacy_fallback: false }
}


#[cfg(windows)]
fn running_lcu_lockfile_candidates() -> (bool, bool, Vec<PathBuf>) {
    use std::mem::size_of;
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::CloseHandle,
            System::{
                Diagnostics::ToolHelp::{
                    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                    TH32CS_SNAPPROCESS,
                },
                Threading::{
                    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                    PROCESS_QUERY_LIMITED_INFORMATION,
                },
            },
        },
    };

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return (false, false, Vec::new());
    };

    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut candidates = Vec::new();
    let mut league_process_seen = false;

    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_err() {
        let _ = unsafe { CloseHandle(snapshot) };
        return (false, false, candidates);
    }

    loop {
        let name_len = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        let process_name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);

        if process_name.eq_ignore_ascii_case("LeagueClientUx.exe")
            || process_name.eq_ignore_ascii_case("LeagueClient.exe")
        {
            league_process_seen = true;
            let process_id = entry.th32ProcessID;
            if let Ok(handle) =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
            {
                let mut buffer = vec![0_u16; 32_768];
                let mut length = buffer.len() as u32;
                let query = unsafe {
                    QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        PWSTR(buffer.as_mut_ptr()),
                        &mut length,
                    )
                };
                let _ = unsafe { CloseHandle(handle) };
                if query.is_ok() {
                    let executable =
                        PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize]));
                    if let Some(directory) = executable.parent() {
                        let lockfile = directory.join("lockfile");
                        if !candidates.contains(&lockfile) {
                            candidates.push(lockfile);
                        }
                    }
                }
            }
        }

        if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
            break;
        }
    }

    let _ = unsafe { CloseHandle(snapshot) };
    (true, league_process_seen, candidates)
}

#[cfg(not(windows))]
fn running_lcu_lockfile_candidates() -> (bool, bool, Vec<PathBuf>) {
    (false, false, Vec::new())
}

fn valid_lcu_lockfile(path: &Path) -> bool {
    path.is_file() && LcuClient::from_lockfile(path).is_ok()
}

fn product_install_path(contents: &str) -> Option<PathBuf> {
    contents.lines().find_map(|line| {
        let value = line
            .trim()
            .strip_prefix("product_install_full_path:")?
            .trim()
            .trim_matches(['\'', '"']);
        (!value.is_empty()).then(|| PathBuf::from(value))
    })
}

fn expand_environment(value: &str) -> String {
    let mut expanded = value.to_owned();
    for (key, replacement) in std::env::vars() {
        expanded = expanded.replace(&format!("%{key}%"), &replacement);
    }
    expanded
}

#[cfg(test)]
mod tests {
    use super::product_install_path;
    use std::path::PathBuf;

    #[cfg(not(windows))]
    #[test]
    fn process_discovery_falls_back_on_non_windows_tests() {
        let (available, seen, candidates) = super::running_lcu_lockfile_candidates();
        assert!(!available);
        assert!(!seen);
        assert!(candidates.is_empty());
    }

    #[test]
    fn reads_riot_product_install_path() {
        let metadata = r#"
product_install_root: "D:/Riot Games"
product_install_full_path: "D:/Riot Games/League of Legends"
"#;

        assert_eq!(
            product_install_path(metadata),
            Some(PathBuf::from("D:/Riot Games/League of Legends"))
        );
    }
}
