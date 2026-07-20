use crate::error::{AgentError, AgentResult};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

#[cfg(windows)]
pub(crate) fn sync_windows_startup(enabled: bool) -> AgentResult<()> {
    use std::os::windows::process::CommandExt;

    let executable = std::env::current_exe()?.to_string_lossy().to_string();
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
            &executable,
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

#[cfg(not(windows))]
pub(crate) fn sync_windows_startup(_: bool) -> AgentResult<()> {
    Ok(())
}

pub(crate) fn open_login_url(app: &AppHandle, url: &str) -> AgentResult<()> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| AgentError::Relay("Discord 로그인 페이지 열기 실패".into()))
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
