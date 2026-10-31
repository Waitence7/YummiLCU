use crate::error::{AgentError, AgentResult};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

#[cfg(windows)]
const LEGACY_PRODUCT_NAME: &str = "Yummi LCU Agent";
#[cfg(windows)]
const CURRENT_PRODUCT_NAME: &str = "유미지지 데스크톱";

#[cfg(windows)]
pub(crate) fn migrate_windows_branding() -> AgentResult<()> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::{fs, path::Path};

    fn migrate_shortcut(source: &Path, destination: &Path) -> std::io::Result<()> {
        if !source.is_file() {
            return Ok(());
        }
        if destination.is_file() {
            fs::remove_file(source)?;
            return Ok(());
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(source, destination)
    }

    if let Some(data_dir) = dirs::data_dir() {
        let programs = data_dir
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        let current = programs.join(format!("{CURRENT_PRODUCT_NAME}.lnk"));
        migrate_shortcut(
            &programs.join(format!("{LEGACY_PRODUCT_NAME}.lnk")),
            &current,
        )?;

        let legacy_folder = programs.join(LEGACY_PRODUCT_NAME);
        migrate_shortcut(
            &legacy_folder.join(format!("{LEGACY_PRODUCT_NAME}.lnk")),
            &current,
        )?;
        let _ = fs::remove_dir(&legacy_folder);
    }

    if let Some(desktop) = dirs::desktop_dir() {
        migrate_shortcut(
            &desktop.join(format!("{LEGACY_PRODUCT_NAME}.lnk")),
            &desktop.join(format!("{CURRENT_PRODUCT_NAME}.lnk")),
        )?;
    }

    // Zip-based auto-updates do not run NSIS, so older installs keep their
    // original uninstall key. Update only its visible DisplayName in place;
    // a later installer run performs the full one-way key migration.
    let legacy_uninstall_key =
        format!(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{LEGACY_PRODUCT_NAME}");
    let mut query = Command::new("reg.exe");
    query
        .creation_flags(0x08000000)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args(["query", &legacy_uninstall_key]);
    if query.status()?.success() {
        let mut update = Command::new("reg.exe");
        update
            .creation_flags(0x08000000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .args([
                "add",
                &legacy_uninstall_key,
                "/v",
                "DisplayName",
                "/t",
                "REG_SZ",
                "/d",
                CURRENT_PRODUCT_NAME,
                "/f",
            ]);
        if !update.status()?.success() {
            return Err(AgentError::Config(
                "Windows 앱 표시 이름 마이그레이션 실패".into(),
            ));
        }
    }

    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn migrate_windows_branding() -> AgentResult<()> {
    Ok(())
}

#[cfg(windows)]
pub(crate) fn sync_windows_startup(enabled: bool) -> AgentResult<()> {
    use std::os::windows::process::CommandExt;

    let executable = std::env::current_exe()?.to_string_lossy().to_string();
    // HKCU\...\Run is a command line, not an argv array. Quote the executable
    // so an install path containing spaces cannot be reparsed as another binary.
    let startup_command = windows_startup_command(&executable);
    let mut command = std::process::Command::new("reg.exe");
    command.creation_flags(0x08000000);
    if enabled {
        command.args([
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "YummiLcuAgent",
            "/t",
            "REG_SZ",
            "/d",
            &startup_command,
            "/f",
        ]);
    } else {
        command.args([
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "YummiLcuAgent",
            "/f",
        ]);
    }
    if !command.status()?.success() && enabled {
        return Err(AgentError::Config("Windows 시작 프로그램 등록 실패".into()));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn windows_startup_command(executable: &str) -> String {
    format!("\"{}\" --background", executable.replace('"', ""))
}

#[cfg(not(windows))]
pub(crate) fn sync_windows_startup(_: bool) -> AgentResult<()> {
    Ok(())
}

#[cfg(windows)]
pub(crate) fn league_game_process_ids() -> Vec<u32> {
    use std::mem::size_of;
    use windows::Win32::{
        Foundation::CloseHandle,
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut process_ids = Vec::new();

    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
        loop {
            let name_len = entry
                .szExeFile
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(entry.szExeFile.len());
            let process_name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);
            if process_name.eq_ignore_ascii_case("League of Legends.exe") {
                process_ids.push(entry.th32ProcessID);
            }
            if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }

    let _ = unsafe { CloseHandle(snapshot) };
    process_ids.sort_unstable();
    process_ids.dedup();
    process_ids
}

#[cfg(not(windows))]
pub(crate) fn league_game_process_ids() -> Vec<u32> {
    Vec::new()
}

pub(crate) fn open_login_url(app: &AppHandle, url: &str) -> AgentResult<()> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| AgentError::Relay(format!("Discord 로그인 페이지 열기 실패: {error}")))
}

pub(crate) fn open_beta_download_url(app: &AppHandle) -> AgentResult<()> {
    let url = crate::updater::beta_installer_url();
    app.opener().open_url(url, None::<&str>).map_err(|error| {
        AgentError::Update(format!("beta 설치 파일 다운로드 페이지 열기 실패: {error}"))
    })
}

#[cfg(windows)]
pub(crate) fn launch_league_client() -> (bool, String) {
    use std::{path::PathBuf, process::Command};

    const LAUNCH_ARGS: [&str; 2] = [
        "--launch-product=league_of_legends",
        "--launch-patchline=live",
    ];
    let mut candidates = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local_app_data)
                .join("Riot Games")
                .join("Riot Client")
                .join("RiotClientServices.exe"),
        );
    }
    for drive in ['C', 'D', 'E', 'F'] {
        candidates.push(PathBuf::from(format!(
            r"{drive}:\Riot Games\Riot Client\RiotClientServices.exe"
        )));
        candidates.push(PathBuf::from(format!(
            r"{drive}:\Program Files\Riot Games\Riot Client\RiotClientServices.exe"
        )));
    }
    if let Some(executable) = candidates.into_iter().find(|path| path.is_file()) {
        return match Command::new(executable).args(LAUNCH_ARGS).spawn() {
            Ok(_) => (true, "롤 클라이언트 실행 요청".into()),
            Err(error) => (false, format!("실행 실패: {error}")),
        };
    }

    match Command::new("cmd")
        .args([
            "/C",
            "start",
            "",
            "riotclient://launch product=league_of_legends patchline=live",
        ])
        .spawn()
    {
        Ok(_) => (true, "롤 클라이언트 실행 요청 (riotclient://)".into()),
        Err(error) => (false, format!("Riot Client를 찾을 수 없습니다. ({error})")),
    }
}

#[cfg(not(windows))]
pub(crate) fn launch_league_client() -> (bool, String) {
    (false, "Riot Client 실행은 Windows에서만 지원됩니다.".into())
}

#[cfg(test)]
mod tests {
    use super::windows_startup_command;

    #[test]
    fn windows_login_startup_is_quoted_and_backgrounded() {
        assert_eq!(
            windows_startup_command(r#"C:\Program Files\Yummi\yummi-lcu-tauri.exe"#),
            r#""C:\Program Files\Yummi\yummi-lcu-tauri.exe" --background"#
        );
    }
}
