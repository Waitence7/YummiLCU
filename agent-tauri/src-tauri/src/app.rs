use std::{future::Future, panic::AssertUnwindSafe, sync::Arc, time::Duration};

use futures_util::FutureExt;
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tokio::time::sleep;

use crate::{
    commands::agent::start_agent_inner,
    config::Config,
    discord_presence::watch_discord_presence,
    lcu::{lockfile_path, LcuClient, LcuConnectionState},
    platform::sync_windows_startup,
    state::{AgentEvent, AppState},
    tray,
    updater::auto_update_on_startup,
};

const INSTALLER_SHUTDOWN_ARG: &str = "--shutdown-for-install";
const BACKGROUND_START_ARG: &str = "--background";
const POST_INSTALL_LAUNCH_PREFIX: &str = "--post-install-launch=";
#[cfg(windows)]
const POST_INSTALL_PARENT_WAIT_MS: u32 = 15_000;

pub(crate) fn run() -> Result<(), tauri::Error> {
    let config = Config::load();
    let startup_sync_result = sync_windows_startup(config.run_at_windows_startup);
    let update_policy = (
        config.check_updates_on_startup,
        config.auto_update_enabled,
        config.update_channel.clone(),
    );
    let args = std::env::args().collect::<Vec<_>>();
    let shutdown_on_start = installer_shutdown_requested(&args);
    let startup_post_install_parent_pid = post_install_parent_pid(&args);
    let start_hidden = background_start_requested(&args);
    let state = Arc::new(AppState::new(config));
    let update_state = state.clone();
    let connect_state = state.clone();
    let lcu_state = state.clone();
    let presence_state = state.clone();
    let lifecycle_state = state.clone();

    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if installer_shutdown_requested(&args) {
                tray::request_exit(app);
                return;
            }
            if let Some(parent_pid) = post_install_parent_pid(&args) {
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    schedule_post_install_window(
                        app.clone(),
                        state.inner().clone(),
                        parent_pid,
                        "single_instance",
                    );
                    return;
                }
            }
            tray::request_main_window(app);
        }));
    }

    let app = builder
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            crate::commands::config::load_config,
            crate::commands::config::save_config,
            crate::commands::agent::start_agent,
            crate::commands::agent::stop_agent,
            crate::commands::agent::relogin,
            crate::commands::agent::submit_oauth_code,
            crate::commands::agent::get_agent_state,
            crate::commands::diagnostics::get_diagnostic_bundle,
            crate::commands::diagnostics::export_diagnostic_bundle,
            crate::commands::diagnostics::report_unexpected_error,
            crate::commands::diagnostics::report_tray_effect_diagnostic,
            crate::commands::lcu::recent_match,
            crate::commands::update::get_beta_release_info,
            crate::commands::update::open_beta_download,
            crate::commands::window::hide_main_window,
            crate::commands::window::complete_tray_hide,
            crate::commands::window::minimize_main_window,
            crate::commands::window::request_tray_hide
        ])
        .setup(move |app| {
            // The installer only launches this argument when it observed an
            // older Agent process. If that process exits during the race
            // between the check and launch, this process becomes primary and
            // must terminate without creating another tray icon.
            if shutdown_on_start {
                app.handle().exit(0);
                return Ok(());
            }
            let launch_mode = if startup_post_install_parent_pid.is_some() {
                "post_install"
            } else if start_hidden {
                "background"
            } else {
                "interactive"
            };
            let lifecycle_handle = app.handle().clone();
            let lifecycle_state = lifecycle_state.clone();
            tauri::async_runtime::spawn(async move {
                lifecycle_state
                    .record_flight("app_lifecycle", format!("started mode={launch_mode}"))
                    .await;
                lifecycle_state
                    .record_flight(
                        "update_policy",
                        format!(
                            "check_on_startup={} auto_install={} channel={}",
                            update_policy.0, update_policy.1, update_policy.2
                        ),
                    )
                    .await;
                match startup_sync_result {
                    Ok(()) => {
                        lifecycle_state
                            .record_flight("windows_startup", "registration_ok")
                            .await;
                        lifecycle_state
                            .log(&lifecycle_handle, "Windows 로그인 자동 시작 등록 확인")
                            .await;
                    }
                    Err(error) => {
                        let summary = error.to_string();
                        lifecycle_state
                            .record_flight(
                                "windows_startup",
                                format!("registration_failed: {summary}"),
                            )
                            .await;
                        lifecycle_state
                            .log(
                                &lifecycle_handle,
                                format!("Windows 로그인 자동 시작 등록 실패: {summary}"),
                            )
                            .await;
                        lifecycle_state
                            .report_unexpected_error(
                                "startup",
                                "windows_startup_registration_failed",
                                summary,
                            )
                            .await;
                    }
                }
            });

            tray::setup(app)?;
            // Tauri creates the configured main window before setup. Recreate it through
            // our builder so beta/dev builds can opt into HTML-in-Canvas WebView2 flags,
            // and background startup does not keep an unused WebView alive.
            tray::destroy_main_window(app.handle());
            if let Some(parent_pid) = startup_post_install_parent_pid {
                schedule_post_install_window(
                    app.handle().clone(),
                    update_state.clone(),
                    parent_pid,
                    "primary",
                );
            } else if !start_hidden {
                tray::request_main_window(app.handle());
            }
            let handle = app.handle().clone();
            spawn_monitored(
                update_state.clone(),
                "updater",
                "task_panicked",
                auto_update_on_startup(handle.clone(), update_state),
            );
            spawn_monitored(
                lcu_state.clone(),
                "lcu_watcher",
                "task_panicked",
                watch_lcu(handle.clone(), lcu_state),
            );
            spawn_monitored(
                presence_state.clone(),
                "discord_presence",
                "task_panicked",
                watch_discord_presence(handle.clone(), presence_state),
            );
            let start_report_state = connect_state.clone();
            spawn_monitored(connect_state, "relay", "task_panicked", async move {
                if let Err(error) = start_agent_inner(handle, start_report_state.clone()).await {
                    start_report_state
                        .report_unexpected_error("relay", "startup_failed", error)
                        .await;
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())?;

    app.run(|app, event| match event {
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "main" => {
            // Closing the window means minimize-to-tray, not stopping the relay.
            // The UI window is destroyed after its animation so the next tray open
            // always starts from a clean WebView state. Background workers stay alive.
            api.prevent_close();
            tray::request_animated_hide(app);
        }
        RunEvent::ExitRequested { code, api, .. } => {
            if should_keep_running(code) {
                api.prevent_exit();
            } else if let Some(state) = app.try_state::<Arc<AppState>>() {
                state.begin_shutdown();
            }
        }
        _ => {}
    });
    Ok(())
}

fn spawn_monitored<F>(state: Arc<AppState>, component: &'static str, code: &'static str, future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        if let Err(payload) = AssertUnwindSafe(future).catch_unwind().await {
            let summary = payload
                .downcast_ref::<&str>()
                .map(|value| (*value).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "background task panicked".into());
            state
                .report_unexpected_error(component, code, summary)
                .await;
        }
    });
}

fn should_keep_running(exit_code: Option<i32>) -> bool {
    exit_code.is_none()
}

fn installer_shutdown_requested(args: &[String]) -> bool {
    args.iter()
        .any(|argument| argument == INSTALLER_SHUTDOWN_ARG)
}

fn background_start_requested(args: &[String]) -> bool {
    args.iter().any(|argument| argument == BACKGROUND_START_ARG)
}

fn post_install_parent_pid(args: &[String]) -> Option<u32> {
    args.iter().find_map(|argument| {
        argument
            .strip_prefix(POST_INSTALL_LAUNCH_PREFIX)
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|pid| *pid > 0)
    })
}

fn schedule_post_install_window(
    app: AppHandle,
    state: Arc<AppState>,
    parent_pid: u32,
    source: &'static str,
) {
    spawn_monitored(state.clone(), "ui", "task_panicked", async move {
        state
            .record_flight(
                "post_install_launch",
                format!("received source={source} installer_pid={parent_pid}"),
            )
            .await;
        wait_for_installer_exit(parent_pid).await;
        state
            .record_flight("post_install_launch", "installer_exited")
            .await;
        sleep(Duration::from_millis(180)).await;
        for attempt in 1..=4 {
            tray::request_main_window(&app);
            sleep(Duration::from_millis(450)).await;
            if let Some(window) = app.get_webview_window("main") {
                let visible = window.is_visible().unwrap_or(false);
                state
                    .record_flight(
                        "post_install_launch",
                        format!("window_ready attempt={attempt} visible={visible}"),
                    )
                    .await;
                if visible {
                    return;
                }
            }
        }
        state
            .record_flight("post_install_launch", "window_not_visible_after_retries")
            .await;
        state
            .report_unexpected_error(
                "ui",
                "window_creation_failed",
                "post-install window was not visible after retries",
            )
            .await;
    });
}

async fn wait_for_installer_exit(process_id: u32) {
    #[cfg(windows)]
    {
        let _ = tokio::task::spawn_blocking(move || {
            use windows::Win32::{
                Foundation::CloseHandle,
                System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
            };

            let Ok(handle) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, process_id) })
            else {
                // The installer already exited between RunAsUser and Agent startup.
                return;
            };
            let _ = unsafe { WaitForSingleObject(handle, POST_INSTALL_PARENT_WAIT_MS) };
            let _ = unsafe { CloseHandle(handle) };
        })
        .await;
    }

    #[cfg(not(windows))]
    {
        let _ = process_id;
    }
}

async fn watch_lcu(app: AppHandle, state: Arc<AppState>) {
    let mut shutdown = state.shutdown_receiver();
    loop {
        if *shutdown.borrow() {
            break;
        }
        let next = inspect_lcu(&app, &state).await;
        state.publish(&app, AgentEvent::LcuStateChanged(next)).await;
        tokio::select! {
            _ = sleep(Duration::from_secs(4)) => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        background_start_requested, installer_shutdown_requested, post_install_parent_pid,
        should_keep_running,
    };

    #[test]
    fn user_window_close_keeps_background_services_running() {
        assert!(should_keep_running(None));
        assert!(!should_keep_running(Some(0)));
        assert!(!should_keep_running(Some(1)));
    }

    #[test]
    fn installer_shutdown_requires_the_exact_argument() {
        assert!(installer_shutdown_requested(&[
            "yummi-lcu-tauri.exe".into(),
            "--shutdown-for-install".into(),
        ]));
        assert!(!installer_shutdown_requested(&[
            "yummi-lcu-tauri.exe".into(),
            "--shutdown-for-installer".into(),
        ]));
    }

    #[test]
    fn post_install_launch_parses_only_valid_parent_pid() {
        assert_eq!(
            post_install_parent_pid(&[
                "yummi-lcu-tauri.exe".into(),
                "--post-install-launch=12345".into(),
            ]),
            Some(12345)
        );
        assert_eq!(
            post_install_parent_pid(&[
                "yummi-lcu-tauri.exe".into(),
                "--post-install-launch=0".into(),
            ]),
            None
        );
        assert_eq!(
            post_install_parent_pid(&[
                "yummi-lcu-tauri.exe".into(),
                "--post-install-launch=abc".into(),
            ]),
            None
        );
    }

    #[test]
    fn only_background_launches_start_hidden() {
        assert!(background_start_requested(&[
            "yummi-lcu-tauri.exe".into(),
            "--background".into(),
        ]));
        assert!(!background_start_requested(
            &["yummi-lcu-tauri.exe".into(),]
        ));
        assert!(!background_start_requested(&[
            "yummi-lcu-tauri.exe".into(),
            "--shutdown-for-install".into(),
        ]));
    }
}

async fn inspect_lcu(app: &AppHandle, state: &Arc<AppState>) -> LcuConnectionState {
    if !state.relay.is_running().await {
        return LcuConnectionState::ClientStopped;
    }
    let config = state.config.read().await.clone();
    let Some(path) = lockfile_path(&config) else {
        return match LcuClient::probe_live_game().await {
            Ok(()) => LcuConnectionState::LoggedIn,
            Err(_) => LcuConnectionState::ClientStopped,
        };
    };

    let current = state.lcu_state().await;
    if matches!(
        current,
        LcuConnectionState::ClientStopped | LcuConnectionState::Error
    ) {
        state
            .publish(
                app,
                AgentEvent::LcuStateChanged(LcuConnectionState::LockfileFound),
            )
            .await;
        state
            .publish(
                app,
                AgentEvent::LcuStateChanged(LcuConnectionState::Connecting),
            )
            .await;
    }

    let Ok(client) =
        LcuClient::from_lockfile(&path).or_else(|_| LcuClient::from_lockfile_legacy(&path))
    else {
        return LcuConnectionState::Error;
    };
    if current != LcuConnectionState::LoggedIn {
        state
            .publish(
                app,
                AgentEvent::LcuStateChanged(LcuConnectionState::Connected),
            )
            .await;
    }
    match client.probe_logged_in().await {
        Ok(()) => LcuConnectionState::LoggedIn,
        Err(_) => match LcuClient::probe_live_game().await {
            Ok(()) => LcuConnectionState::LoggedIn,
            Err(_) => LcuConnectionState::Error,
        },
    }
}
