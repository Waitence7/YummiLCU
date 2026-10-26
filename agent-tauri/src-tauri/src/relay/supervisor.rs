use std::{
    collections::VecDeque,
    fs,
    io::Write,
    panic::AssertUnwindSafe,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, OnceLock,
    },
    time::{Duration, Instant as StdInstant, SystemTime, UNIX_EPOCH},
};

use futures_util::{FutureExt, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::{
    sync::{broadcast, mpsc, watch, Mutex},
    task::JoinHandle,
    time::{interval, interval_at, sleep, timeout, Instant, MissedTickBehavior},
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{
    error::{AgentError, AgentResult},
    http_diagnostics::{safe_url, status_detail, transport_detail},
    lcu::{lockfile_path, lockfile_path_nonblocking, LcuClient, LcuEventPoller},
    platform::{launch_league_client, league_game_process_ids, open_login_url},
    session,
    state::{AgentEvent, AppState, DiscordJoinResolution, DiscordPresenceMatchContext},
};

use super::{
    command_auth,
    protocol::{
        Action, AgentDiagnosticReport, AgentEventMessage, AgentHelloMessage, AgentLogChunk,
        AuthMessage, CommandResult, DeviceProofMessage, IncomingMessage, OAuthCodeMessage,
        PongMessage, UnexpectedErrorReport, UpdateDiagnosticReport, MAX_RELAY_MESSAGE_BYTES,
    },
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);
static REPLAY_DISK_IO: crate::blocking_io::BlockingIo = crate::blocking_io::BlockingIo::new();
const REPLAY_DISK_TIMEOUT: Duration = Duration::from_secs(2);
static COMMAND_DISK_IO: crate::blocking_io::BlockingIo = crate::blocking_io::BlockingIo::new();
const COMMAND_DISK_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_LOCK_TIMEOUT: Duration = Duration::from_secs(2);

const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
const RELAY_SEND_TIMEOUT: Duration = Duration::from_secs(10);
// Presence match-context refreshes are advisory. They must never sit on the
// shared WebSocket writer long enough to delay the independent heartbeat.
const PRESENCE_CONTEXT_SEND_TIMEOUT: Duration = Duration::from_secs(1);
const SESSION_CONFLICT_CLOSE_CODE: u16 = 4009;
const SESSION_RESTORE_TIMEOUT: Duration = Duration::from_secs(5);
const LCU_WORKER_TICK_INTERVAL: Duration = Duration::from_secs(1);
const LCU_RECOVERY_POLL_INTERVAL: Duration = Duration::from_secs(8);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const MAX_DURABLE_REPLAY_EVENTS: usize = 4096;
const MAX_LIVE_PENDING_FILE_BYTES: u64 = 64 * 1024 * 1024;
const DURABLE_REPLAY_INTERVAL: Duration = Duration::from_secs(30);
const DIAGNOSTIC_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5 * 60);
const DIAGNOSTIC_LOG_SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
const DIAGNOSTIC_LOG_SYNC_TIMEOUT: Duration = Duration::from_secs(10);
const DIAGNOSTIC_LOG_CHUNK_BYTES: usize = 24 * 1024;
const MAX_REPLAY_UPLOAD_BYTES: u64 = 128 * 1024 * 1024;
const REPLAY_UPLOAD_ATTEMPTS: u8 = 8;
const REPLAY_LOCAL_DELETE_DELAY: Duration = Duration::from_secs(5 * 60);
const REPLAY_LOCAL_DELETE_MONITOR_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_SCHEDULER_WARN: Duration = Duration::from_secs(5);
const RELAY_PROGRESS_STALE_AFTER: Duration = Duration::from_secs(90);
const RELAY_HARD_REVIVE_CONNECTED_AFTER: Duration = Duration::from_secs(45);
const RELAY_HARD_REVIVE_CONNECTING_AFTER: Duration = Duration::from_secs(45);
const RELAY_HARD_REVIVE_RECONNECTING_AFTER: Duration = Duration::from_secs(120);
const RELAY_HARD_REVIVE_COOLDOWN: Duration = Duration::from_secs(60);
const RELAY_NATIVE_WATCHDOG_STALE_AFTER: Duration = Duration::from_secs(90);

static RELAY_PROGRESS_EPOCH: OnceLock<StdInstant> = OnceLock::new();
static RELAY_PROGRESS_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_NETWORK_PROGRESS_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_SELECT_LOOP_COUNT: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_SELECT_LOOP_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_BRANCH_CODE: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_BRANCH_START_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_BRANCH_DONE_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_SEND_STARTED_COUNT: AtomicU64 = AtomicU64::new(0);
static RELAY_SEND_LOCKED_COUNT: AtomicU64 = AtomicU64::new(0);
static RELAY_SEND_OK_COUNT: AtomicU64 = AtomicU64::new(0);
static RELAY_SEND_FAIL_COUNT: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_SEND_KIND: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_SEND_START_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_SEND_LOCKED_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_SEND_OK_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_SEND_FAIL_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_RECV_COUNT: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_RECV_KIND: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_RECV_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_HEARTBEAT_TICK_COUNT: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_HEARTBEAT_TICK_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_HEARTBEAT_GAP_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_PING_SENT_COUNT: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_PING_SENT_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_PONG_COUNT: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_PONG_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_PONG_RTT_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_HEARTBEAT_PHASE: AtomicU64 = AtomicU64::new(0);
static RELAY_HEARTBEAT_WATCHDOG_TICK_COUNT: AtomicU64 = AtomicU64::new(0);
static RELAY_LAST_HEARTBEAT_WATCHDOG_TICK_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_TRACE_THREAD: OnceLock<()> = OnceLock::new();
static RELAY_NATIVE_WATCHDOG_ARMED: AtomicBool = AtomicBool::new(false);
static RELAY_NATIVE_WATCHDOG_LAST_TICK_MS: AtomicU64 = AtomicU64::new(0);
static RELAY_NATIVE_WATCHDOG_RESTARTING: AtomicBool = AtomicBool::new(false);

const RELAY_TRACE_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const RELAY_TRACE_LOG_MAX_BYTES: u64 = 4 * 1024 * 1024;

fn relay_progress_now_ms() -> u64 {
    RELAY_PROGRESS_EPOCH
        .get_or_init(StdInstant::now)
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn touch_relay_progress() {
    RELAY_PROGRESS_MS.store(relay_progress_now_ms(), Ordering::Release);
}

fn relay_progress_age() -> Duration {
    let last = RELAY_PROGRESS_MS.load(Ordering::Acquire);
    Duration::from_millis(relay_progress_now_ms().saturating_sub(last))
}

fn relay_progress_is_stale(age: Duration) -> bool {
    age >= RELAY_PROGRESS_STALE_AFTER
}

fn relay_select_loop_age() -> Option<Duration> {
    let last_ms = RELAY_LAST_SELECT_LOOP_MS.load(Ordering::Acquire);
    if last_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(
            relay_progress_now_ms().saturating_sub(last_ms),
        ))
    }
}

fn reset_relay_select_progress() {
    let now = relay_progress_now_ms();
    RELAY_LAST_SELECT_LOOP_MS.store(now, Ordering::Release);
    RELAY_LAST_BRANCH_DONE_MS.store(now, Ordering::Release);
    RELAY_LAST_BRANCH_CODE.store(0, Ordering::Release);
    RELAY_LAST_BRANCH_START_MS.store(0, Ordering::Release);
}

fn trace_age_ms(last_ms: u64) -> String {
    if last_ms == 0 {
        "-1".into()
    } else {
        relay_progress_now_ms().saturating_sub(last_ms).to_string()
    }
}

fn touch_relay_network_progress() {
    RELAY_NETWORK_PROGRESS_MS.store(relay_progress_now_ms(), Ordering::Release);
}

fn relay_send_kind(failure: &'static str) -> u64 {
    match failure {
        "Relay heartbeat 전송 실패" => 1,
        "Relay pong 전송 실패" => 2,
        "Relay 이벤트 전송 실패" => 3,
        "운영 진단 전송 실패" => 4,
        "운영 상태 스냅샷 전송 실패" => 5,
        "업데이트 진단 전송 실패" => 6,
        "예상치 못한 오류 보고 전송 실패" => 7,
        "Discord 참가 요청 Relay 전송 실패" => 8,
        "Discord Presence 내전 조회 요청 전송 실패" => 9,
        "Relay 응답 전송 실패" => 10,
        "Relay 종료 프레임 전송 실패" => 11,
        "OAuth 코드 전송 실패" => 12,
        _ => 99,
    }
}

fn relay_send_kind_label(code: u64) -> &'static str {
    match code {
        1 => "heartbeat_ping",
        2 => "pong",
        3 => "agent_event",
        4 => "diagnostic",
        5 => "health_snapshot",
        6 => "update_diagnostic",
        7 => "unexpected_error",
        8 => "discord_join_request",
        9 => "discord_presence_context_request",
        10 => "command_response",
        11 => "close",
        12 => "oauth_code",
        99 => "other",
        _ => "none",
    }
}

fn relay_recv_kind_label(code: u64) -> &'static str {
    match code {
        1 => "text_ping",
        2 => "text",
        3 => "ws_ping",
        4 => "ws_pong",
        5 => "close",
        6 => "eof",
        7 => "other",
        8 => "error",
        _ => "none",
    }
}

fn relay_branch_label(code: u64) -> &'static str {
    match code {
        1 => "stop",
        2 => "heartbeat_failure",
        3 => "oauth",
        4 => "reader",
        5 => "durable_replay",
        6 => "health_snapshot",
        7 => "update_report",
        8 => "diagnostic_report",
        9 => "error_report",
        10 => "discord_join_request",
        11 => "discord_presence_request",
        12 => "watchdog",
        13 => "lcu_batch",
        14 => "diagnostic_log_sync",
        15 => "command_done",
        _ => "none",
    }
}

fn relay_heartbeat_phase_label(code: u64) -> &'static str {
    match code {
        1 => "tick",
        2 => "awaiting_check",
        3 => "send_ping",
        4 => "mark_awaiting",
        5 => "watchdog_tick",
        6 => "timeout_check",
        7 => "failed_exit",
        8 => "flight_record",
        _ => "idle",
    }
}

fn mark_relay_select_loop() {
    let now = relay_progress_now_ms();
    RELAY_LAST_BRANCH_DONE_MS.store(now, Ordering::Release);
    RELAY_LAST_SELECT_LOOP_MS.store(now, Ordering::Release);
    RELAY_SELECT_LOOP_COUNT.fetch_add(1, Ordering::AcqRel);
}

fn mark_relay_branch(code: u64) {
    RELAY_LAST_BRANCH_CODE.store(code, Ordering::Release);
    RELAY_LAST_BRANCH_START_MS.store(relay_progress_now_ms(), Ordering::Release);
}

fn mark_relay_receive(code: u64) {
    let now = relay_progress_now_ms();
    RELAY_RECV_COUNT.fetch_add(1, Ordering::AcqRel);
    RELAY_LAST_RECV_KIND.store(code, Ordering::Release);
    RELAY_LAST_RECV_MS.store(now, Ordering::Release);
    RELAY_NETWORK_PROGRESS_MS.store(now, Ordering::Release);
}

fn relay_trace_snapshot() -> String {
    let started = RELAY_SEND_STARTED_COUNT.load(Ordering::Acquire);
    let locked = RELAY_SEND_LOCKED_COUNT.load(Ordering::Acquire);
    let ok = RELAY_SEND_OK_COUNT.load(Ordering::Acquire);
    let fail = RELAY_SEND_FAIL_COUNT.load(Ordering::Acquire);
    format!(
        "progress_age_ms={} network_age_ms={} select_count={} select_age_ms={} branch={} branch_age_ms={} branch_done_age_ms={} send_started={} send_locked={} send_ok={} send_fail={} send_pending={} send_lock_pending={} last_send_kind={} last_send_start_age_ms={} last_send_lock_age_ms={} last_send_ok_age_ms={} last_send_fail_age_ms={} recv_count={} last_recv_kind={} last_recv_age_ms={} hb_ticks={} hb_tick_age_ms={} hb_gap_ms={} hb_phase={} hb_watchdog_ticks={} hb_watchdog_age_ms={} ping_sent={} ping_age_ms={} pong_count={} pong_age_ms={} last_pong_rtt_ms={} flight_disk_dropped={}",
        relay_progress_age().as_millis(),
        trace_age_ms(RELAY_NETWORK_PROGRESS_MS.load(Ordering::Acquire)),
        RELAY_SELECT_LOOP_COUNT.load(Ordering::Acquire),
        trace_age_ms(RELAY_LAST_SELECT_LOOP_MS.load(Ordering::Acquire)),
        relay_branch_label(RELAY_LAST_BRANCH_CODE.load(Ordering::Acquire)),
        trace_age_ms(RELAY_LAST_BRANCH_START_MS.load(Ordering::Acquire)),
        trace_age_ms(RELAY_LAST_BRANCH_DONE_MS.load(Ordering::Acquire)),
        started,
        locked,
        ok,
        fail,
        started.saturating_sub(ok.saturating_add(fail)),
        started.saturating_sub(locked.saturating_add(fail)),
        relay_send_kind_label(RELAY_LAST_SEND_KIND.load(Ordering::Acquire)),
        trace_age_ms(RELAY_LAST_SEND_START_MS.load(Ordering::Acquire)),
        trace_age_ms(RELAY_LAST_SEND_LOCKED_MS.load(Ordering::Acquire)),
        trace_age_ms(RELAY_LAST_SEND_OK_MS.load(Ordering::Acquire)),
        trace_age_ms(RELAY_LAST_SEND_FAIL_MS.load(Ordering::Acquire)),
        RELAY_RECV_COUNT.load(Ordering::Acquire),
        relay_recv_kind_label(RELAY_LAST_RECV_KIND.load(Ordering::Acquire)),
        trace_age_ms(RELAY_LAST_RECV_MS.load(Ordering::Acquire)),
        RELAY_HEARTBEAT_TICK_COUNT.load(Ordering::Acquire),
        trace_age_ms(RELAY_LAST_HEARTBEAT_TICK_MS.load(Ordering::Acquire)),
        RELAY_LAST_HEARTBEAT_GAP_MS.load(Ordering::Acquire),
        relay_heartbeat_phase_label(RELAY_HEARTBEAT_PHASE.load(Ordering::Acquire)),
        RELAY_HEARTBEAT_WATCHDOG_TICK_COUNT.load(Ordering::Acquire),
        trace_age_ms(RELAY_LAST_HEARTBEAT_WATCHDOG_TICK_MS.load(Ordering::Acquire)),
        RELAY_PING_SENT_COUNT.load(Ordering::Acquire),
        trace_age_ms(RELAY_LAST_PING_SENT_MS.load(Ordering::Acquire)),
        RELAY_PONG_COUNT.load(Ordering::Acquire),
        trace_age_ms(RELAY_LAST_PONG_MS.load(Ordering::Acquire)),
        RELAY_LAST_PONG_RTT_MS.load(Ordering::Acquire),
        crate::diagnostics::persistent_flight_dropped(),
    )
}

fn append_relay_runtime_trace(snapshot: &str) -> std::io::Result<()> {
    let root = dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("YummiAgent");
    fs::create_dir_all(&root)?;
    let path = root.join("relay-runtime-trace.log");
    if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) >= RELAY_TRACE_LOG_MAX_BYTES {
        let backup = root.join("relay-runtime-trace.log.1");
        let _ = fs::remove_file(&backup);
        let _ = fs::rename(&path, &backup);
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let wall_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    writeln!(file, "{wall_ms} {snapshot}")
}

fn native_watchdog_tick() {
    RELAY_NATIVE_WATCHDOG_LAST_TICK_MS.store(relay_progress_now_ms(), Ordering::Release);
}

#[cfg(windows)]
fn schedule_native_agent_restart() -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let executable =
        std::env::current_exe().map_err(|error| format!("current_exe failed: {error}"))?;
    let executable = executable.to_string_lossy();
    let command = format!(
        "ping 127.0.0.1 -n 3 >nul & start \"\" \"{}\" --background",
        executable.replace('"', "")
    );
    std::process::Command::new("cmd.exe")
        .args(["/D", "/C", &command])
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|error| format!("restart helper spawn failed: {error}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn schedule_native_agent_restart() -> Result<(), String> {
    Err("native watchdog relaunch is only supported on Windows".into())
}

#[derive(Default)]
struct NativeWatchdogObservation {
    last_sample_ms: Option<u64>,
    stale_since_ms: Option<u64>,
}

impl NativeWatchdogObservation {
    fn should_restart(&mut self, now_ms: u64, last_tick_ms: u64, armed: bool) -> bool {
        // A suspended PC (or a stalled native thread) must not be mistaken for
        // an executor freeze. Require fresh, consecutive observations first.
        let sample_gap = self
            .last_sample_ms
            .replace(now_ms)
            .map(|last| now_ms.saturating_sub(last));
        let max_gap_ms = RELAY_TRACE_SAMPLE_INTERVAL.as_millis() as u64 * 3;
        if !armed
            || last_tick_ms == 0
            || now_ms.saturating_sub(last_tick_ms)
                < RELAY_NATIVE_WATCHDOG_STALE_AFTER.as_millis() as u64
            || sample_gap.is_none_or(|gap| gap > max_gap_ms)
        {
            self.stale_since_ms = None;
            return false;
        }
        let stale_since = *self.stale_since_ms.get_or_insert(now_ms);
        now_ms.saturating_sub(stale_since) >= max_gap_ms
    }
}

fn maybe_native_watchdog_restart(observation: &mut NativeWatchdogObservation) {
    let last_tick = RELAY_NATIVE_WATCHDOG_LAST_TICK_MS.load(Ordering::Acquire);
    let now_ms = relay_progress_now_ms();
    if !observation.should_restart(
        now_ms,
        last_tick,
        RELAY_NATIVE_WATCHDOG_ARMED.load(Ordering::Acquire),
    ) {
        return;
    }
    let age_ms = now_ms.saturating_sub(last_tick);
    if RELAY_NATIVE_WATCHDOG_RESTARTING.swap(true, Ordering::AcqRel) {
        return;
    }

    let detail = format!(
        "native_runtime_watchdog_stale age_ms={} threshold_ms={} trace={}",
        age_ms,
        RELAY_NATIVE_WATCHDOG_STALE_AFTER.as_millis(),
        relay_trace_snapshot()
    );
    let _ = append_relay_runtime_trace(&detail);
    eprintln!("[yummi relay watchdog] {detail}");

    // Diagnostic disk I/O above can be slow; recheck recovery/stop before exit.
    if !RELAY_NATIVE_WATCHDOG_ARMED.load(Ordering::Acquire)
        || RELAY_NATIVE_WATCHDOG_LAST_TICK_MS.load(Ordering::Acquire) != last_tick
    {
        RELAY_NATIVE_WATCHDOG_RESTARTING.store(false, Ordering::Release);
        return;
    }
    match schedule_native_agent_restart() {
        Ok(()) => {
            // The helper waits before relaunching so the single-instance lock is
            // released by the time the replacement process starts.
            std::process::exit(78);
        }
        Err(error) => {
            eprintln!("[yummi relay watchdog] self-restart scheduling failed: {error}");
            RELAY_NATIVE_WATCHDOG_RESTARTING.store(false, Ordering::Release);
            native_watchdog_tick();
        }
    }
}

fn ensure_relay_trace_thread_started() {
    RELAY_TRACE_THREAD.get_or_init(|| {
        // Measure executor scheduling independently of recovery operations,
        // which may await a control lock, UI event, or disk operation.
        tokio::spawn(async {
            let mut ticks = interval(Duration::from_secs(10));
            ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                native_watchdog_tick();
            }
        });
        let _ = std::thread::Builder::new()
            .name("yummi-relay-trace".into())
            .spawn(|| {
                let mut observation = NativeWatchdogObservation::default();
                loop {
                    std::thread::sleep(RELAY_TRACE_SAMPLE_INTERVAL);
                    if let Err(error) = append_relay_runtime_trace(&relay_trace_snapshot()) {
                        eprintln!("[yummi relay trace] write failed: {error}");
                    }
                    maybe_native_watchdog_restart(&mut observation);
                }
            });
    });
}

struct AbortTaskOnDrop(JoinHandle<()>);

impl Drop for AbortTaskOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

type SharedRelayWriter<S> = Arc<Mutex<S>>;

#[derive(Clone, Serialize, Deserialize)]
struct SerializedAgentEvent {
    event_id: String,
    text: String,
    live_stream_id: Option<String>,
    live_seq: Option<u64>,
}

#[derive(Clone)]
struct DurableReplayBuffer {
    pending: VecDeque<SerializedAgentEvent>,
    live_pending_path: PathBuf,
    load_issue: Option<String>,
    needs_live_resync: bool,
    persistence_dirty: bool,
}

fn quarantine_live_pending(path: &std::path::Path) -> String {
    if !path.exists() {
        return "missing".into();
    }
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let quarantine = path.with_file_name(format!("live-delta-pending.corrupt-{suffix}.json"));
    match fs::rename(path, &quarantine) {
        Ok(()) => quarantine
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("quarantined")
            .to_owned(),
        Err(error) => format!("rename_failed:{error}"),
    }
}

impl DurableReplayBuffer {
    fn live_pending_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("YummiAgent")
            .join("live-delta-pending.json")
    }

    fn load() -> Self {
        Self::load_from_path(Self::live_pending_path())
    }

    fn load_from_path(path: PathBuf) -> Self {
        let mut load_issue = None;
        let mut needs_live_resync = false;

        if !path.exists() {
            if let Some(parent) = path.parent() {
                let recovered = fs::read_dir(parent)
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".live-delta-pending-")
                    })
                    .filter_map(|entry| {
                        let metadata = entry.metadata().ok()?;
                        if !metadata.is_file() || metadata.len() > MAX_LIVE_PENDING_FILE_BYTES {
                            return None;
                        }
                        Some((metadata.modified().ok()?, entry.path()))
                    })
                    .max_by_key(|(modified, _)| *modified)
                    .map(|(_, path)| path);
                if let Some(recovered) = recovered {
                    match fs::rename(&recovered, &path) {
                        Ok(()) => {
                            load_issue =
                                Some("recovered interrupted live-delta pending write".into());
                        }
                        Err(error) => {
                            load_issue = Some(format!("pending temp recovery failed: {error}"));
                            needs_live_resync = true;
                        }
                    }
                }
            }
        }

        let mut pending = VecDeque::new();
        if path.exists() {
            let load_result = (|| -> Result<Vec<SerializedAgentEvent>, String> {
                let metadata =
                    fs::symlink_metadata(&path).map_err(|error| format!("metadata: {error}"))?;
                if metadata.file_type().is_symlink() {
                    return Err("pending path is a symlink".into());
                }
                if !metadata.is_file() {
                    return Err("pending path is not a regular file".into());
                }
                if metadata.len() > MAX_LIVE_PENDING_FILE_BYTES {
                    return Err(format!("pending file too large: {} bytes", metadata.len()));
                }
                let raw = fs::read(&path).map_err(|error| format!("read: {error}"))?;
                serde_json::from_slice::<Vec<SerializedAgentEvent>>(&raw)
                    .map_err(|error| format!("json: {error}"))
            })();

            match load_result {
                Ok(events) => {
                    let mut invalid = 0usize;
                    for event in events.into_iter().take(MAX_DURABLE_REPLAY_EVENTS) {
                        if event.live_stream_id.is_none()
                            || event.live_seq.is_none()
                            || uuid::Uuid::parse_str(&event.event_id).is_err()
                            || event
                                .live_stream_id
                                .as_deref()
                                .is_none_or(|value| uuid::Uuid::parse_str(value).is_err())
                            || event.text.len() > MAX_RELAY_MESSAGE_BYTES
                        {
                            invalid = invalid.saturating_add(1);
                            continue;
                        }
                        pending.push_back(event);
                    }
                    if invalid > 0 {
                        let preserved = quarantine_live_pending(&path);
                        pending.clear();
                        needs_live_resync = true;
                        load_issue = Some(format!(
                            "invalid live-delta pending events: count={invalid}; preserved={preserved}"
                        ));
                    }
                }
                Err(error) => {
                    let preserved = quarantine_live_pending(&path);
                    needs_live_resync = true;
                    load_issue = Some(format!(
                        "live-delta pending load failed: {error}; preserved={preserved}"
                    ));
                }
            }
        }

        Self {
            pending,
            live_pending_path: path,
            load_issue,
            needs_live_resync,
            persistence_dirty: false,
        }
    }

    fn take_load_issue(&mut self) -> Option<String> {
        self.load_issue.take()
    }

    fn take_needs_live_resync(&mut self) -> bool {
        std::mem::take(&mut self.needs_live_resync)
    }

    async fn persist_live_pending(&mut self) -> Result<(), String> {
        self.persistence_dirty = true;
        let snapshot = self.clone();
        REPLAY_DISK_IO
            .run(REPLAY_DISK_TIMEOUT, move || snapshot.write_live_pending())
            .await??;
        self.persistence_dirty = false;
        Ok(())
    }

    fn write_live_pending(&self) -> Result<(), String> {
        if fs::symlink_metadata(&self.live_pending_path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err("live delta pending path is a symlink".into());
        }
        let live_events = self
            .pending
            .iter()
            .filter(|event| event.live_stream_id.is_some() && event.live_seq.is_some())
            .collect::<Vec<_>>();
        let serialized =
            serde_json::to_vec(&live_events).map_err(|error| format!("serialize: {error}"))?;
        if serialized.len() as u64 > MAX_LIVE_PENDING_FILE_BYTES {
            return Err("live delta pending file exceeds size limit".into());
        }
        let Some(parent) = self.live_pending_path.parent() else {
            return Err("live delta pending parent missing".into());
        };
        fs::create_dir_all(parent).map_err(|error| format!("mkdir: {error}"))?;
        let temporary = parent.join(format!(".live-delta-pending-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::File::options()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&serialized)?;
            file.sync_all()?;
            match fs::remove_file(&self.live_pending_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            fs::rename(&temporary, &self.live_pending_path)
        })();
        if let Err(error) = result {
            if self.live_pending_path.exists() {
                let _ = fs::remove_file(&temporary);
            }
            return Err(error.to_string());
        }
        Ok(())
    }

    async fn track(&mut self, event: SerializedAgentEvent) -> Result<bool, String> {
        if self
            .pending
            .iter()
            .any(|pending| pending.event_id == event.event_id)
        {
            return Ok(false);
        }
        let live_changed = event.live_stream_id.is_some() && event.live_seq.is_some();
        let dropped = if self.pending.len() >= MAX_DURABLE_REPLAY_EVENTS {
            self.pending.pop_front();
            true
        } else {
            false
        };
        self.pending.push_back(event);
        if live_changed || dropped {
            self.persist_live_pending().await?;
        }
        Ok(dropped)
    }

    async fn ack(&mut self, event_id: &str) -> Result<bool, String> {
        let live_changed = self.pending.iter().any(|event| {
            event.event_id == event_id && event.live_stream_id.is_some() && event.live_seq.is_some()
        });
        let before = self.pending.len();
        self.pending.retain(|event| event.event_id != event_id);
        let changed = self.pending.len() != before;
        if changed && live_changed {
            self.persist_live_pending().await?;
        }
        Ok(changed)
    }

    fn snapshot(&self) -> Vec<SerializedAgentEvent> {
        self.pending.iter().cloned().collect()
    }

    fn pending_summary(&self) -> String {
        let mut items = self
            .pending
            .iter()
            .take(4)
            .map(|event| {
                let summary = serde_json::from_str::<Value>(&event.text)
                    .ok()
                    .map(|message| {
                        event_summary(
                            message
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown"),
                            message.get("data").unwrap_or(&Value::Null),
                        )
                    })
                    .unwrap_or_else(|| "unknown event".into());
                format!(
                    "{} event_id={}",
                    summary.chars().take(160).collect::<String>(),
                    event.event_id
                )
            })
            .collect::<Vec<_>>();
        if self.pending.len() > items.len() {
            items.push(format!("and {} more", self.pending.len() - items.len()));
        }
        items.join("; ")
    }

    fn len(&self) -> usize {
        self.pending.len()
    }

    fn live_range(&self, stream_id: &str, from_seq: u64, to_seq: u64) -> Vec<SerializedAgentEvent> {
        self.pending
            .iter()
            .filter(|event| {
                event.live_stream_id.as_deref() == Some(stream_id)
                    && event
                        .live_seq
                        .is_some_and(|seq| seq >= from_seq && seq <= to_seq)
            })
            .cloned()
            .collect()
    }

    async fn drop_live_stream(&mut self, stream_id: &str) -> Result<usize, String> {
        let before = self.pending.len();
        self.pending
            .retain(|event| event.live_stream_id.as_deref() != Some(stream_id));
        let removed = before.saturating_sub(self.pending.len());
        if removed > 0 {
            self.persist_live_pending().await?;
        }
        Ok(removed)
    }
}

struct LcuSocketWatch {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Drop for LcuSocketWatch {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.task.abort();
    }
}

struct LcuPollBatch {
    events: Vec<(&'static str, Value)>,
    diagnostics: Vec<String>,
}

struct RoflPreflightDecision {
    game_id: String,
    needed: Option<bool>,
}

struct LcuPollWorker {
    stop: watch::Sender<bool>,
    trigger: mpsc::Sender<()>,
    live_game_polling: watch::Sender<bool>,
    live_game_delta_enabled: watch::Sender<bool>,
    live_game_resync: mpsc::Sender<()>,
    rofl_preflight: mpsc::Sender<RoflPreflightDecision>,
    task: JoinHandle<()>,
}

impl LcuPollWorker {
    fn start(state: Arc<AppState>) -> (Self, mpsc::Receiver<LcuPollBatch>) {
        let (batch_tx, batch_rx) = mpsc::channel(8);
        let (trigger_tx, mut trigger_rx) = mpsc::channel(8);
        let (live_game_polling_tx, mut live_game_polling_rx) = watch::channel(true);
        let (live_game_delta_enabled_tx, mut live_game_delta_enabled_rx) = watch::channel(false);
        let (live_game_resync_tx, mut live_game_resync_rx) = mpsc::channel(4);
        let (rofl_preflight_tx, mut rofl_preflight_rx) = mpsc::channel::<RoflPreflightDecision>(8);
        let (stop_tx, mut stop_rx) = watch::channel(false);

        let task = tokio::spawn(async move {
            let mut poller = LcuEventPoller::default();
            let mut poll_tick = interval(LCU_WORKER_TICK_INTERVAL);
            poll_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
            poll_tick.tick().await;
            let mut last_lcu_poll: Option<Instant> = None;

            loop {
                let poll_lcu = tokio::select! {
                    changed = stop_rx.changed() => {
                        if changed.is_err() || *stop_rx.borrow() {
                            return;
                        }
                        continue;
                    }
                    changed = live_game_polling_rx.changed() => {
                        if changed.is_err() {
                            return;
                        }
                        poller.set_live_game_polling(*live_game_polling_rx.borrow());
                        continue;
                    }
                    changed = live_game_delta_enabled_rx.changed() => {
                        if changed.is_err() {
                            return;
                        }
                        poller.set_live_game_delta_enabled(*live_game_delta_enabled_rx.borrow());
                        continue;
                    }
                    Some(()) = live_game_resync_rx.recv() => {
                        poller.force_live_game_resync();
                        true
                    }
                    Some(decision) = rofl_preflight_rx.recv() => {
                        match decision.needed {
                            Some(needed) => poller.set_rofl_upload_decision(&decision.game_id, needed),
                            None => poller.retry_rofl_upload_check(&decision.game_id),
                        }
                        true
                    }
                    _ = poll_tick.tick() => {
                        last_lcu_poll.is_none_or(|last| last.elapsed() >= LCU_RECOVERY_POLL_INTERVAL)
                    },
                    Some(()) = trigger_rx.recv() => true,
                };

                if poll_lcu {
                    last_lcu_poll = Some(Instant::now());
                }

                let config = state.config.read().await.clone();
                let poll_started = Instant::now();
                let events = poller.poll(&config, poll_lcu).await;
                let poll_elapsed = poll_started.elapsed();
                if poll_elapsed >= Duration::from_secs(5) {
                    state
                        .report_diagnostic(
                            "lcu",
                            "poll_slow",
                            format!(
                                "elapsed_ms={} poll_lcu={poll_lcu}",
                                poll_elapsed.as_millis()
                            ),
                        )
                        .await;
                }
                let diagnostics = poller.take_diagnostics();
                if events.is_empty() && diagnostics.is_empty() {
                    continue;
                }

                if batch_tx
                    .send(LcuPollBatch {
                        events,
                        diagnostics,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
            }
        });

        (
            Self {
                stop: stop_tx,
                trigger: trigger_tx,
                live_game_polling: live_game_polling_tx,
                live_game_delta_enabled: live_game_delta_enabled_tx,
                live_game_resync: live_game_resync_tx,
                rofl_preflight: rofl_preflight_tx,
                task,
            },
            batch_rx,
        )
    }
}

impl Drop for LcuPollWorker {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.task.abort();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RelayConnectionState {
    Stopped,
    Connecting,
    Authenticating,
    Connected,
    Reconnecting,
    Failed,
}

struct SupervisorControl {
    generation: u64,
    desired_running: bool,
    stop_tx: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<()>>,
    oauth_tx: Option<mpsc::Sender<String>>,
    connection_state: RelayConnectionState,
    connection_state_since_ms: u64,
    last_hard_revive_ms: u64,
}

pub(crate) struct RelaySupervisor {
    control: Mutex<SupervisorControl>,
}

impl RelaySupervisor {
    pub(crate) fn new() -> Self {
        Self {
            control: Mutex::new(SupervisorControl {
                generation: 0,
                desired_running: false,
                stop_tx: None,
                task: None,
                oauth_tx: None,
                connection_state: RelayConnectionState::Stopped,
                connection_state_since_ms: relay_progress_now_ms(),
                last_hard_revive_ms: 0,
            }),
        }
    }

    pub(crate) fn note_native_watchdog_tick() {
        native_watchdog_tick();
    }

    pub(crate) async fn is_running(&self) -> bool {
        let control = self.control.lock().await;
        control
            .task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    }

    pub(crate) async fn start_background(app: AppHandle, state: Arc<AppState>) -> AgentResult<()> {
        Self::start_impl(app, state, true, false).await.map(|_| ())
    }

    pub(crate) async fn start_user_requested(
        app: AppHandle,
        state: Arc<AppState>,
    ) -> AgentResult<()> {
        let started = Self::start_impl(app.clone(), state.clone(), true, true).await?;
        if !started && state.oauth_pending().await {
            Self::open_pending_login(&app, &state).await?;
        }
        Ok(())
    }

    pub(crate) async fn restart_if_desired(
        app: AppHandle,
        state: Arc<AppState>,
    ) -> AgentResult<bool> {
        Self::start_impl(app, state, false, false).await
    }

    async fn start_impl(
        app: AppHandle,
        state: Arc<AppState>,
        explicit_start: bool,
        user_initiated_login: bool,
    ) -> AgentResult<bool> {
        ensure_relay_trace_thread_started();
        let (generation, stop_rx) = {
            let mut control = state.relay.control.lock().await;
            let task_running = control
                .task
                .as_ref()
                .is_some_and(|task| !task.is_finished());
            if explicit_start {
                control.desired_running = true;
                touch_relay_progress();
                if task_running {
                    return Ok(false);
                }
            } else if !watchdog_restart_allowed(control.desired_running, task_running) {
                return Ok(false);
            }
            // Only an accepted start can arm process-level recovery. Periodic
            // restart_if_desired calls must not rearm a manually stopped Agent.
            RELAY_NATIVE_WATCHDOG_ARMED.store(true, Ordering::Release);
            native_watchdog_tick();
            control.task.take();
            control.generation = control.generation.wrapping_add(1);
            let generation = control.generation;
            let (stop_tx, stop_rx) = watch::channel(false);
            control.stop_tx = Some(stop_tx);
            control.oauth_tx = None;
            control.connection_state = RelayConnectionState::Connecting;
            control.connection_state_since_ms = relay_progress_now_ms();
            (generation, stop_rx)
        };

        state
            .publish(
                &app,
                AgentEvent::RelayStateChanged(RelayConnectionState::Connecting),
            )
            .await;
        state.log(&app, "Relay 연결 시작").await;

        let task_state = state.clone();
        let task_app = app.clone();
        let task = tokio::spawn(async move {
            let mut pending_panic_summary: Option<String> = None;
            loop {
                if *stop_rx.borrow() {
                    break;
                }

                let error_reports = task_state.unexpected_error_receiver();
                let update_reports = task_state.update_diagnostic_receiver();
                let diagnostic_reports = task_state.diagnostic_report_receiver();
                let discord_join_requests = task_state.discord_join_request_receiver();
                let discord_presence_context_requests =
                    task_state.discord_presence_context_request_receiver();

                if let Some(summary) = pending_panic_summary.take() {
                    task_state
                        .report_unexpected_error("relay", "task_panicked", &summary)
                        .await;
                    task_state
                        .report_diagnostic("relay", "supervisor_recovered", &summary)
                        .await;
                }

                let result = AssertUnwindSafe(run_supervisor(
                    task_app.clone(),
                    task_state.clone(),
                    generation,
                    stop_rx.clone(),
                    user_initiated_login,
                    error_reports,
                    update_reports,
                    diagnostic_reports,
                    discord_join_requests,
                    discord_presence_context_requests,
                ))
                .catch_unwind()
                .await;

                match result {
                    Ok(()) => break,
                    Err(payload) => {
                        let summary = payload
                            .downcast_ref::<&str>()
                            .map(|value| (*value).to_owned())
                            .or_else(|| payload.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "relay supervisor panicked".into());
                        task_state
                            .record_flight("relay_error", format!("task_panicked: {summary}"))
                            .await;
                        task_state
                            .log(
                                &task_app,
                                format!("Relay supervisor panic 감지 — 자동 복구: {summary}"),
                            )
                            .await;
                        pending_panic_summary = Some(summary);
                        let mut retry_stop_rx = stop_rx.clone();
                        if !wait_for_retry(&mut retry_stop_rx, Duration::from_secs(1)).await {
                            break;
                        }
                    }
                }
            }
            task_state
                .relay
                .finish(&task_app, &task_state, generation)
                .await;
        });

        let mut control = state.relay.control.lock().await;
        if control.generation == generation && control.desired_running {
            control.task = Some(task);
            Ok(true)
        } else {
            task.abort();
            Ok(false)
        }
    }

    pub(crate) async fn restart_if_stale(
        app: AppHandle,
        state: Arc<AppState>,
    ) -> AgentResult<bool> {
        // The independent heartbeat intentionally keeps network/progress counters
        // moving even when the main connection task is no longer being polled.
        // Therefore the watchdog must key off the main select loop specifically.
        let Some(age) = relay_select_loop_age() else {
            return Ok(false);
        };
        if !relay_progress_is_stale(age) {
            return Ok(false);
        }

        let stale_task = {
            let mut control = state.relay.control.lock().await;
            let task_running = control
                .task
                .as_ref()
                .is_some_and(|task| !task.is_finished());
            if !control.desired_running
                || !task_running
                || control.connection_state != RelayConnectionState::Connected
            {
                return Ok(false);
            }

            control.generation = control.generation.wrapping_add(1);
            if let Some(stop_tx) = control.stop_tx.take() {
                let _ = stop_tx.send(true);
            }
            control.oauth_tx = None;
            control.connection_state = RelayConnectionState::Failed;
            control.connection_state_since_ms = relay_progress_now_ms();
            control.task.take()
        };

        if let Some(task) = stale_task {
            task.abort();
        }
        touch_relay_progress();
        state
            .record_flight(
                "relay_watchdog",
                format!(
                    "stale_task_aborted age_ms={} trace={}",
                    age.as_millis(),
                    relay_trace_snapshot()
                ),
            )
            .await;
        state
            .log(
                &app,
                format!(
                    "Relay 진행 정지 감지({}ms) — supervisor 강제 재시작",
                    age.as_millis()
                ),
            )
            .await;

        Self::start_impl(app, state, false, false).await
    }

    pub(crate) async fn hard_revive_if_unhealthy(
        app: AppHandle,
        state: Arc<AppState>,
    ) -> AgentResult<bool> {
        let now_ms = relay_progress_now_ms();
        let network_last_ms = RELAY_NETWORK_PROGRESS_MS.load(Ordering::Acquire);
        let network_age = if network_last_ms == 0 {
            None
        } else {
            Some(Duration::from_millis(
                now_ms.saturating_sub(network_last_ms),
            ))
        };

        let (reason, old_task) = {
            let mut control = state.relay.control.lock().await;
            let task_running = control
                .task
                .as_ref()
                .is_some_and(|task| !task.is_finished());
            if !control.desired_running || !task_running {
                return Ok(false);
            }

            let state_age =
                Duration::from_millis(now_ms.saturating_sub(control.connection_state_since_ms));
            if control.last_hard_revive_ms != 0
                && Duration::from_millis(now_ms.saturating_sub(control.last_hard_revive_ms))
                    < RELAY_HARD_REVIVE_COOLDOWN
            {
                return Ok(false);
            }
            let reason = match control.connection_state {
                RelayConnectionState::Connected
                    if network_age.is_some_and(|age| age >= RELAY_HARD_REVIVE_CONNECTED_AFTER) =>
                {
                    format!(
                        "connected_network_stale state_age_ms={} network_age_ms={}",
                        state_age.as_millis(),
                        network_age.map(|age| age.as_millis()).unwrap_or(0),
                    )
                }
                RelayConnectionState::Connecting | RelayConnectionState::Authenticating
                    if state_age >= RELAY_HARD_REVIVE_CONNECTING_AFTER =>
                {
                    format!(
                        "connection_phase_stuck state={} state_age_ms={} network_age_ms={}",
                        relay_state_label(control.connection_state),
                        state_age.as_millis(),
                        network_age
                            .map(|age| age.as_millis().to_string())
                            .unwrap_or_else(|| "-1".into()),
                    )
                }
                RelayConnectionState::Failed | RelayConnectionState::Reconnecting
                    if state_age >= RELAY_HARD_REVIVE_RECONNECTING_AFTER =>
                {
                    format!(
                        "reconnect_phase_stuck state={} state_age_ms={} network_age_ms={}",
                        relay_state_label(control.connection_state),
                        state_age.as_millis(),
                        network_age
                            .map(|age| age.as_millis().to_string())
                            .unwrap_or_else(|| "-1".into()),
                    )
                }
                _ => return Ok(false),
            };

            control.generation = control.generation.wrapping_add(1);
            if let Some(stop_tx) = control.stop_tx.take() {
                let _ = stop_tx.send(true);
            }
            control.oauth_tx = None;
            control.connection_state = RelayConnectionState::Failed;
            control.connection_state_since_ms = now_ms;
            control.last_hard_revive_ms = now_ms;
            (reason, control.task.take())
        };

        if let Some(task) = old_task {
            task.abort();
        }
        touch_relay_progress();
        state
            .record_flight(
                "relay_hard_revive",
                format!("triggered reason={reason} trace={}", relay_trace_snapshot()),
            )
            .await;
        state
            .log(&app, format!("Relay 강제 부활 실행: {reason}"))
            .await;
        state
            .report_diagnostic("relay", "hard_revive", &reason)
            .await;

        Self::start_impl(app, state, false, false).await
    }

    pub(crate) async fn stop(app: &AppHandle, state: &Arc<AppState>) {
        let task = {
            let mut control = state.relay.control.lock().await;
            control.desired_running = false;
            RELAY_NATIVE_WATCHDOG_ARMED.store(false, Ordering::Release);
            touch_relay_progress();
            control.generation = control.generation.wrapping_add(1);
            if let Some(stop_tx) = control.stop_tx.take() {
                let _ = stop_tx.send(true);
            }
            control.oauth_tx = None;
            control.connection_state = RelayConnectionState::Stopped;
            control.connection_state_since_ms = relay_progress_now_ms();
            control.task.take()
        };

        if let Some(mut task) = task {
            if timeout(Duration::from_secs(2), &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
        }
        state
            .publish(
                app,
                AgentEvent::RelayStateChanged(RelayConnectionState::Stopped),
            )
            .await;
        state.mark_stopped(app).await;
    }

    pub(crate) async fn restart(app: AppHandle, state: Arc<AppState>) -> AgentResult<()> {
        Self::stop(&app, &state).await;
        Self::start_background(app, state).await
    }

    async fn open_pending_login(app: &AppHandle, state: &Arc<AppState>) -> AgentResult<()> {
        let config = state.config.read().await.clone();
        let session = session::load(&config).ok_or_else(|| {
            AgentError::Relay("Discord 로그인 세션을 준비하지 못했습니다. 다시 시도하세요.".into())
        })?;
        open_login_url(app, &config.login_url(&session.session_id)?)?;
        state
            .set_oauth_pending(app, true, "브라우저에서 Discord 로그인 중…")
            .await;
        state
            .record_flight("relay", "oauth_browser_opened_by_user")
            .await;
        Ok(())
    }

    pub(crate) async fn submit_oauth_code(&self, mut code: String) -> AgentResult<()> {
        let sender = self.control.lock().await.oauth_tx.clone();
        let Some(sender) = sender else {
            unsafe { code.as_bytes_mut().fill(0) };
            return Err(AgentError::Relay("먼저 연결을 시작하세요.".into()));
        };
        match sender.send(code).await {
            Ok(()) => Ok(()),
            Err(mut error) => {
                unsafe { error.0.as_bytes_mut().fill(0) };
                Err(AgentError::Relay("Relay 연결이 없습니다.".into()))
            }
        }
    }

    async fn set_connection_state(
        &self,
        app: &AppHandle,
        state: &AppState,
        generation: u64,
        next: RelayConnectionState,
    ) {
        let changed = {
            let mut control = self.control.lock().await;
            if control.generation != generation || control.connection_state == next {
                false
            } else {
                control.connection_state = next;
                control.connection_state_since_ms = relay_progress_now_ms();
                true
            }
        };
        if changed {
            state
                .publish(app, AgentEvent::RelayStateChanged(next))
                .await;
            state
                .log(app, format!("Relay 상태 변경: {}", relay_state_label(next)))
                .await;
        }
    }

    async fn set_oauth_sender(&self, generation: u64, sender: Option<mpsc::Sender<String>>) {
        let mut control = self.control.lock().await;
        if control.generation == generation {
            control.oauth_tx = sender;
        }
    }

    async fn suspend_for_session_conflict(&self, generation: u64) -> bool {
        let mut control = self.control.lock().await;
        if control.generation != generation {
            return false;
        }
        // A deliberate server-side handover is not a transport failure. Keep
        // the app watchdog from restarting this displaced connection as well.
        control.desired_running = false;
        RELAY_NATIVE_WATCHDOG_ARMED.store(false, Ordering::Release);
        touch_relay_progress();
        control.oauth_tx = None;
        true
    }

    async fn finish(&self, app: &AppHandle, state: &AppState, generation: u64) {
        let should_publish = {
            let mut control = self.control.lock().await;
            if control.generation != generation {
                false
            } else {
                control.stop_tx = None;
                control.oauth_tx = None;
                control.connection_state = RelayConnectionState::Stopped;
                control.connection_state_since_ms = relay_progress_now_ms();
                true
            }
        };
        if should_publish {
            state
                .publish(
                    app,
                    AgentEvent::RelayStateChanged(RelayConnectionState::Stopped),
                )
                .await;
        }
    }
}

async fn run_supervisor(
    app: AppHandle,
    state: Arc<AppState>,
    generation: u64,
    mut stop_rx: watch::Receiver<bool>,
    mut user_initiated_login: bool,
    mut error_reports: broadcast::Receiver<UnexpectedErrorReport>,
    mut update_reports: broadcast::Receiver<UpdateDiagnosticReport>,
    mut diagnostic_reports: broadcast::Receiver<AgentDiagnosticReport>,
    mut discord_join_requests: broadcast::Receiver<u64>,
    mut discord_presence_context_requests: broadcast::Receiver<()>,
) {
    let config = state.config.read().await.clone();
    let saved_session = session::load(&config);
    state
        .record_flight(
            "session",
            if saved_session.is_some() {
                "saved_session_loaded"
            } else {
                "no_valid_saved_session"
            },
        )
        .await;
    let mut needs_login = saved_session.is_none();
    let mut session = saved_session.unwrap_or_else(|| session::create(&config));
    let mut attempt = 0_u32;
    // Never treat a timed-out load as an empty journal: retry without losing
    // persisted events, and keep manual stop responsive during disk trouble.
    let mut durable_replay = loop {
        let loaded = tokio::select! {
            _ = stop_rx.changed() => return,
            loaded = REPLAY_DISK_IO.run(REPLAY_DISK_TIMEOUT, DurableReplayBuffer::load) => loaded,
        };
        match loaded {
            Ok(replay) => break replay,
            Err(error) => {
                state
                    .record_flight("live_delta_persist_error", format!("load: {error}"))
                    .await;
                if !wait_for_retry(&mut stop_rx, Duration::from_secs(5)).await {
                    return;
                }
            }
        }
    };
    if let Some(issue) = durable_replay.take_load_issue() {
        state
            .report_diagnostic("live_delta", "pending_load_issue", issue)
            .await;
    }

    loop {
        if *stop_rx.borrow() {
            break;
        }
        touch_relay_progress();
        state
            .relay
            .set_connection_state(&app, &state, generation, RelayConnectionState::Connecting)
            .await;

        let connection_result = AssertUnwindSafe(connect_once(
            &app,
            &state,
            generation,
            &config,
            &mut session,
            &mut stop_rx,
            &mut needs_login,
            user_initiated_login,
            &mut durable_replay,
            &mut error_reports,
            &mut update_reports,
            &mut diagnostic_reports,
            &mut discord_join_requests,
            &mut discord_presence_context_requests,
        ))
        .catch_unwind()
        .await;
        // A browser launch is allowed only as the direct consequence of one
        // explicit user action, never on later automatic reconnect attempts.
        user_initiated_login = false;

        match connection_result {
            Ok(Ok(ConnectionEnd::Stopped)) => break,
            Ok(Ok(ConnectionEnd::ForceReconnect { reason })) => {
                attempt = 0;
                state
                    .record_flight(
                        "relay_force_reconnect",
                        format!("server_requested reason={reason}"),
                    )
                    .await;
                state
                    .report_diagnostic("relay", "force_reconnect", &reason)
                    .await;
                state
                    .log(&app, format!("Relay 서버 강제 재연결 요청: {reason}"))
                    .await;
            }
            Ok(Ok(ConnectionEnd::SessionConflict)) => {
                if state.relay.suspend_for_session_conflict(generation).await {
                    let status = "다른 Agent에서 연결 중입니다. 기존 Agent를 종료하거나 Discord 로그아웃 후 다시 로그인하세요.";
                    state.mark_stopped(&app).await;
                    state.set_oauth_pending(&app, false, status).await;
                    state
                        .log(&app, format!("{status} 자동 재접속을 중지했습니다."))
                        .await;
                }
                break;
            }
            Ok(Ok(ConnectionEnd::Closed { authenticated })) => {
                if authenticated {
                    attempt = 0;
                }
                state.log(&app, "Relay 연결 종료 — 재연결 대기").await;
                state
                    .report_diagnostic(
                        "relay",
                        "connection_closed",
                        format!("authenticated={authenticated}"),
                    )
                    .await;
            }
            Ok(Err(error)) => {
                state
                    .relay
                    .set_connection_state(&app, &state, generation, RelayConnectionState::Failed)
                    .await;
                let trace = relay_trace_snapshot();
                state
                    .record_flight("relay_error", format!("{error}; trace={trace}"))
                    .await;
                state
                    .report_diagnostic(
                        "relay",
                        "connection_error",
                        format!("{error}; trace={trace}"),
                    )
                    .await;
                state.log(&app, format!("Relay 오류: {error}")).await;
                let diagnostic_state = Arc::clone(&state);
                let diagnostic_config = config.clone();
                let trigger = error.to_string();
                tokio::spawn(async move {
                    collect_relay_network_diagnostic(diagnostic_state, diagnostic_config, trigger)
                        .await;
                });
            }
            Err(payload) => {
                let summary = payload
                    .downcast_ref::<&str>()
                    .map(|value| (*value).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "relay connection task panicked".into());
                state
                    .relay
                    .set_connection_state(&app, &state, generation, RelayConnectionState::Failed)
                    .await;
                state
                    .report_diagnostic("relay", "connection_panicked", &summary)
                    .await;
                state
                    .report_unexpected_error("relay", "task_panicked", &summary)
                    .await;
                state
                    .log(
                        &app,
                        format!("Relay 연결 처리 panic — 자동 재연결: {summary}"),
                    )
                    .await;
            }
        }
        state.relay.set_oauth_sender(generation, None).await;

        if !should_reconnect(*stop_rx.borrow()) {
            break;
        }
        state
            .relay
            .set_connection_state(&app, &state, generation, RelayConnectionState::Reconnecting)
            .await;
        attempt = attempt.saturating_add(1);
        if !wait_for_retry(&mut stop_rx, backoff_delay(attempt)).await {
            break;
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ConnectionEnd {
    Stopped,
    SessionConflict,
    ForceReconnect { reason: String },
    Closed { authenticated: bool },
}

impl ConnectionEnd {
    fn for_close(code: Option<u16>, authenticated: bool) -> Self {
        if code == Some(SESSION_CONFLICT_CLOSE_CODE) {
            Self::SessionConflict
        } else {
            Self::Closed { authenticated }
        }
    }
}

async fn lcu_identity_for_relay(
    config: &crate::config::Config,
) -> (Option<String>, Option<String>) {
    let discovery_config = config.clone();
    let client = COMMAND_DISK_IO
        .run(COMMAND_DISK_TIMEOUT, move || {
            let path = lockfile_path(&discovery_config)
                .ok_or_else(|| AgentError::Lcu("LCU 미연결".into()))?;
            LcuClient::from_lockfile(&path).or_else(|_| LcuClient::from_lockfile_legacy(&path))
        })
        .await;
    let Ok(Ok(client)) = client else {
        return (None, None);
    };
    let Ok(summoner) = client.current_summoner().await else {
        return (None, None);
    };
    let puuid = summoner
        .get("puuid")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .map(str::to_owned);
    let game_name = summoner
        .get("gameName")
        .or_else(|| summoner.get("riotIdGameName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let tag_line = summoner
        .get("tagLine")
        .or_else(|| summoner.get("riotIdTagLine"))
        .or_else(|| summoner.get("riotIdTagline"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let riot_id = match (game_name, tag_line) {
        (Some(game_name), Some(tag_line)) => Some(format!("{game_name}#{tag_line}")),
        _ => summoner
            .get("displayName")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| value.contains('#') && value.len() <= 128)
            .map(str::to_owned),
    };
    (puuid, riot_id)
}

async fn connect_once(
    app: &AppHandle,
    state: &Arc<AppState>,
    generation: u64,
    config: &crate::config::Config,
    session: &mut session::Session,
    stop_rx: &mut watch::Receiver<bool>,
    needs_login: &mut bool,
    user_initiated_login: bool,
    durable_replay: &mut DurableReplayBuffer,
    error_reports: &mut broadcast::Receiver<UnexpectedErrorReport>,
    update_reports: &mut broadcast::Receiver<UpdateDiagnosticReport>,
    diagnostic_reports: &mut broadcast::Receiver<AgentDiagnosticReport>,
    discord_join_requests: &mut broadcast::Receiver<u64>,
    discord_presence_context_requests: &mut broadcast::Receiver<()>,
) -> AgentResult<ConnectionEnd> {
    let url = config.ws_url(&session.session_id)?;
    let connection = tokio::select! {
        changed = stop_rx.changed() => {
            let _ = changed;
            return Ok(ConnectionEnd::Stopped);
        }
        result = timeout(
            CONNECT_TIMEOUT,
            connect_async_with_config(
                url,
                Some(
                    WebSocketConfig::default()
                        .max_message_size(Some(MAX_RELAY_MESSAGE_BYTES))
                        .max_frame_size(Some(MAX_RELAY_MESSAGE_BYTES)),
                ),
                false,
            ),
        ) => result,
    };
    let (mut websocket, _) = connection
        .map_err(|_| AgentError::Relay("Relay 연결 시간 초과".into()))?
        .map_err(|error| {
            AgentError::Relay(format!("Relay TLS/WebSocket 연결 실패: {error}").into())
        })?;
    let connection_id = Uuid::new_v4().to_string();
    state
        .record_flight(
            "relay_connection",
            format!(
                "opened connection_id={} generation={} session={}",
                connection_id, generation, session.session_id
            ),
        )
        .await;
    state
        .log(
            app,
            format!("Relay WebSocket 연결 성공 connection_id={connection_id}"),
        )
        .await;

    let device_identity = session::load_or_create_device_identity()?;
    let device_public_key = device_identity.public_key_b64();
    let device_fingerprint = device_identity.fingerprint();
    send_relay_message(
        &mut websocket,
        Message::Text(
            serde_json::to_string(&AuthMessage::new(
                &session.ws_token,
                session.refresh_token.as_deref(),
                &connection_id,
                device_identity.device_id(),
                &device_public_key,
            ))?
            .into(),
        ),
        "Relay 인증 메시지 전송 실패",
    )
    .await?;
    state
        .record_flight(
            "device_identity",
            format!("advertised fingerprint={}", &device_fingerprint[..16]),
        )
        .await;
    state.log(app, "Relay 인증 메시지 전송 완료").await;
    if *stop_rx.borrow() {
        let _ = websocket.close(None).await;
        return Ok(ConnectionEnd::Stopped);
    }
    let lcu_ready = state.lcu_state().await.is_ready();
    let (puuid, riot_id) = if lcu_ready && !session::auto_login_suppressed() {
        lcu_identity_for_relay(config).await
    } else {
        (None, None)
    };
    let hello = AgentHelloMessage::new(lcu_ready, session.bound_discord_id, puuid, riot_id);
    send_relay_message(
        &mut websocket,
        Message::Text(serde_json::to_string(&hello)?.into()),
        "Relay hello 전송 실패",
    )
    .await?;
    state
        .log(
            app,
            format!(
                "Agent hello 전송: lcu_ready={}",
                state.lcu_state().await.is_ready()
            ),
        )
        .await;
    state
        .relay
        .set_connection_state(app, state, generation, RelayConnectionState::Authenticating)
        .await;

    // Split read/write after authentication bootstrap so heartbeat transmission is
    // completely independent from LCU polling/event handling in the main select loop.
    let (writer_half, mut reader) = websocket.split();
    let writer = Arc::new(Mutex::new(writer_half));

    let (heartbeat_failure_tx, mut heartbeat_failure_rx) = mpsc::channel::<String>(1);
    let heartbeat_awaiting = Arc::new(Mutex::new(None::<Instant>));
    let heartbeat_writer = Arc::clone(&writer);
    let heartbeat_awaiting_task = Arc::clone(&heartbeat_awaiting);
    let heartbeat_state = Arc::clone(state);
    let heartbeat_app = app.clone();
    let heartbeat_connection_id = connection_id.clone();
    let heartbeat_task = AbortTaskOnDrop(tokio::spawn(async move {
        let mut heartbeat = interval(HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
        heartbeat.tick().await;
        let mut watchdog = interval(Duration::from_secs(1));
        watchdog.set_missed_tick_behavior(MissedTickBehavior::Skip);
        watchdog.tick().await;
        let mut last_heartbeat_tick = Instant::now();

        heartbeat_state
            .record_flight(
                "relay_heartbeat",
                format!("independent_task_started connection_id={heartbeat_connection_id}"),
            )
            .await;

        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    RELAY_HEARTBEAT_PHASE.store(1, Ordering::Release);
                    touch_relay_progress();
                    let now = Instant::now();
                    let gap = now.duration_since(last_heartbeat_tick);
                    last_heartbeat_tick = now;
                    RELAY_HEARTBEAT_TICK_COUNT.fetch_add(1, Ordering::AcqRel);
                    RELAY_LAST_HEARTBEAT_TICK_MS.store(relay_progress_now_ms(), Ordering::Release);
                    RELAY_LAST_HEARTBEAT_GAP_MS.store(
                        gap.as_millis().min(u128::from(u64::MAX)) as u64,
                        Ordering::Release,
                    );
                    if gap > HEARTBEAT_INTERVAL + HEARTBEAT_SCHEDULER_WARN {
                        let detail = format!(
                            "scheduler_delay connection_id={} gap_ms={} expected_ms={}",
                            heartbeat_connection_id,
                            gap.as_millis(),
                            HEARTBEAT_INTERVAL.as_millis(),
                        );
                        heartbeat_state.record_flight("relay_heartbeat", detail.clone()).await;
                        heartbeat_state
                            .log(&heartbeat_app, format!("Relay heartbeat 스케줄 지연: {detail}"))
                            .await;
                    }

                    RELAY_HEARTBEAT_PHASE.store(2, Ordering::Release);
                    if heartbeat_awaiting_task.lock().await.is_some() {
                        RELAY_HEARTBEAT_PHASE.store(0, Ordering::Release);
                        continue;
                    }

                    RELAY_HEARTBEAT_PHASE.store(3, Ordering::Release);
                    let send_started = Instant::now();
                    match send_shared_relay_message(
                        &heartbeat_writer,
                        Message::Text("ping".into()),
                        "Relay heartbeat 전송 실패",
                    ).await {
                        Ok(()) => {
                            RELAY_PING_SENT_COUNT.fetch_add(1, Ordering::AcqRel);
                            RELAY_LAST_PING_SENT_MS.store(relay_progress_now_ms(), Ordering::Release);
                            RELAY_HEARTBEAT_PHASE.store(4, Ordering::Release);
                            *heartbeat_awaiting_task.lock().await = Some(Instant::now());
                            RELAY_HEARTBEAT_PHASE.store(8, Ordering::Release);
                            heartbeat_state
                                .record_flight(
                                    "relay_heartbeat",
                                    format!(
                                        "ping_sent connection_id={} send_ms={} trace={}",
                                        heartbeat_connection_id,
                                        send_started.elapsed().as_millis(),
                                        relay_trace_snapshot(),
                                    ),
                                )
                                .await;
                            RELAY_HEARTBEAT_PHASE.store(0, Ordering::Release);
                        }
                        Err(error) => {
                            RELAY_HEARTBEAT_PHASE.store(7, Ordering::Release);
                            let detail = format!(
                                "send_failed connection_id={} error={error} trace={}",
                                heartbeat_connection_id,
                                relay_trace_snapshot()
                            );
                            heartbeat_state.record_flight("relay_heartbeat", detail.clone()).await;
                            heartbeat_state
                                .log(&heartbeat_app, format!("Relay heartbeat 전송 실패: {error}"))
                                .await;
                            let _ = heartbeat_failure_tx.try_send(detail);
                            break;
                        }
                    }
                }
                _ = watchdog.tick() => {
                    RELAY_HEARTBEAT_PHASE.store(5, Ordering::Release);
                    RELAY_HEARTBEAT_WATCHDOG_TICK_COUNT.fetch_add(1, Ordering::AcqRel);
                    RELAY_LAST_HEARTBEAT_WATCHDOG_TICK_MS
                        .store(relay_progress_now_ms(), Ordering::Release);
                    RELAY_HEARTBEAT_PHASE.store(6, Ordering::Release);
                    let sent_at = *heartbeat_awaiting_task.lock().await;
                    if heartbeat_timed_out(sent_at, Instant::now()) {
                        if let Some(sent_at) = sent_at {
                            let age = sent_at.elapsed();
                            let detail = format!(
                                "pong_timeout connection_id={} age_ms={} trace={}",
                                heartbeat_connection_id,
                                age.as_millis(),
                                relay_trace_snapshot()
                            );
                            heartbeat_state.record_flight("relay_heartbeat", detail.clone()).await;
                            heartbeat_state
                                .log(&heartbeat_app, format!("Relay heartbeat 응답 시간 초과: {detail}"))
                                .await;
                            let _ = heartbeat_failure_tx.try_send(detail);
                            break;
                        }
                    }
                    RELAY_HEARTBEAT_PHASE.store(0, Ordering::Release);
                }
            }
        }
    }));
    let _heartbeat_task = heartbeat_task;

    let (oauth_tx, mut oauth_rx) = mpsc::channel::<String>(1);
    state
        .relay
        .set_oauth_sender(generation, Some(oauth_tx))
        .await;
    state
        .set_oauth_pending(
            app,
            *needs_login,
            if *needs_login {
                if user_initiated_login {
                    "브라우저에서 Discord 로그인 중…"
                } else {
                    "Discord 로그인이 필요합니다 — Agent에서 로그인 버튼을 눌러주세요."
                }
            } else {
                "저장된 Discord 세션 복원 중…"
            },
        )
        .await;
    if oauth_browser_should_open(user_initiated_login, *needs_login) {
        open_login_url(app, &config.login_url(&session.session_id)?)?;
        state
            .record_flight("relay", "oauth_browser_opened_by_user")
            .await;
    }
    let auth_started = Instant::now();
    // `login_opened` also means that a missing-session prompt has already been
    // surfaced. Automatic/background starts wait silently for an explicit click.
    let mut login_opened = *needs_login;
    if let Err(error) = session::save(session) {
        let summary = error.to_string();
        state
            .record_flight("session_error", format!("pre_auth_save_failed: {summary}"))
            .await;
        state
            .log(app, format!("Relay 세션 저장 실패: {summary}"))
            .await;
        state
            .report_unexpected_error("session", "save_failed", &summary)
            .await;
    } else {
        state.record_flight("session", "saved_before_auth").await;
    }

    // Start a fresh main-loop progress epoch for this connection. Without this,
    // a stale timestamp from the previous connection could make a newly-bound
    // session look hung before it reaches its first select iteration.
    reset_relay_select_progress();

    let mut watchdog = interval(Duration::from_secs(1));
    watchdog.set_missed_tick_behavior(MissedTickBehavior::Skip);
    watchdog.tick().await;
    let mut durable_replay_tick = interval_at(
        Instant::now() + DURABLE_REPLAY_INTERVAL,
        DURABLE_REPLAY_INTERVAL,
    );
    durable_replay_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut diagnostic_heartbeat_tick = interval_at(
        Instant::now() + DIAGNOSTIC_HEARTBEAT_INTERVAL,
        DIAGNOSTIC_HEARTBEAT_INTERVAL,
    );
    diagnostic_heartbeat_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut diagnostic_log_tick = interval_at(
        Instant::now() + DIAGNOSTIC_LOG_SYNC_INTERVAL,
        DIAGNOSTIC_LOG_SYNC_INTERVAL,
    );
    diagnostic_log_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut live_game_announced = false;
    let mut session_bound = false;
    let mut session_new_device = false;
    let mut last_identity_hello_at: Option<Instant> = None;
    let mut durable_replay_enabled = false;
    let mut unexpected_error_reports_enabled = false;
    let mut update_diagnostics_enabled = false;
    let mut diagnostic_reports_enabled = false;
    let mut diagnostic_log_chunks_enabled = false;
    let mut diagnostic_log_initial_sync_pending = false;
    let mut diagnostic_log_sync_running = false;
    let mut diagnostic_log_task: Option<AbortTaskOnDrop> = None;
    let mut command_tasks = tokio::task::JoinSet::new();
    let (diagnostic_log_done_tx, mut diagnostic_log_done_rx) =
        mpsc::channel::<Result<(usize, usize), String>>(1);
    let (lcu_poll_worker, mut lcu_poll_rx) = LcuPollWorker::start(Arc::clone(state));
    let mut lcu_socket_watch: Option<LcuSocketWatch> = None;

    loop {
        touch_relay_progress();
        mark_relay_select_loop();
        tokio::select! {
            changed = stop_rx.changed() => {
                mark_relay_branch(1);
                let _ = changed;
                let _ = send_shared_relay_message(
                    &writer,
                    Message::Close(None),
                    "Relay 종료 프레임 전송 실패",
                ).await;
                state.relay.set_oauth_sender(generation, None).await;
                return Ok(ConnectionEnd::Stopped);
            }
            failure = heartbeat_failure_rx.recv() => {
                mark_relay_branch(2);
                let detail = failure.unwrap_or_else(|| "heartbeat_task_channel_closed".into());
                state.record_flight("relay_heartbeat", format!("task_failed {detail}")).await;
                state.relay.set_oauth_sender(generation, None).await;
                return Err(AgentError::Relay(format!("독립 Relay heartbeat 실패: {detail}").into()));
            }
            mut code = receive_oauth_code(&mut oauth_rx) => {
                mark_relay_branch(3);
                state.log(app, "Discord OAuth 코드 전송 시작").await;
                let message = serde_json::to_string(&OAuthCodeMessage::new(&code))?;
                let sent = send_shared_relay_message(
                    &writer,
                    Message::Text(message.into()),
                    "OAuth 코드 전송 실패",
                ).await;
                // The code is ASCII, so zeroing its bytes keeps the String valid before drop.
                unsafe { code.as_bytes_mut().fill(0) };
                sent?;
                state.log(app, "Discord OAuth 코드 전송 완료").await;
            }
            message = reader.next() => {
                mark_relay_branch(4);
                let recv_kind = match &message {
                    Some(Ok(Message::Text(text))) if text.as_str() == "ping" => 1,
                    Some(Ok(Message::Text(_))) => 2,
                    Some(Ok(Message::Ping(_))) => 3,
                    Some(Ok(Message::Pong(_))) => 4,
                    Some(Ok(Message::Close(_))) => 5,
                    None => 6,
                    Some(Ok(_)) => 7,
                    Some(Err(_)) => 8,
                };
                mark_relay_receive(recv_kind);
                match message {
                    Some(Ok(Message::Text(text))) if text.as_str() == "ping" => {
                        send_shared_relay_message(
                            &writer,
                            Message::Text(serde_json::to_string(&PongMessage::new())?.into()),
                            "Relay pong 전송 실패",
                        ).await?;
                    }
                    Some(Ok(Message::Text(text))) => {
                        let incoming = IncomingMessage::parse(text.as_str())
                            .map_err(|message| AgentError::Relay(message.into()))?;
                        match &incoming {
                            IncomingMessage::Pong => {
                                RELAY_PONG_COUNT.fetch_add(1, Ordering::AcqRel);
                                RELAY_LAST_PONG_MS.store(relay_progress_now_ms(), Ordering::Release);
                                touch_relay_network_progress();
                                let sent_at = heartbeat_awaiting.lock().await.take();
                                if let Some(sent_at) = sent_at {
                                    let rtt_ms = sent_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                                    RELAY_LAST_PONG_RTT_MS.store(rtt_ms, Ordering::Release);
                                    state
                                        .record_flight(
                                            "relay_heartbeat",
                                            format!(
                                                "pong connection_id={} rtt_ms={}",
                                                connection_id,
                                                rtt_ms,
                                            ),
                                        )
                                        .await;
                                } else {
                                    state
                                        .record_flight(
                                            "relay_heartbeat",
                                            format!("pong_without_pending_ping connection_id={connection_id}"),
                                        )
                                        .await;
                                }
                            }
                            IncomingMessage::ForceReconnect { reason } => {
                                let reason = reason
                                    .as_deref()
                                    .unwrap_or("server_requested")
                                    .chars()
                                    .take(160)
                                    .collect::<String>();
                                state
                                    .record_flight(
                                        "relay_force_reconnect",
                                        format!("received connection_id={connection_id} reason={reason}"),
                                    )
                                    .await;
                                state.relay.set_oauth_sender(generation, None).await;
                                return Ok(ConnectionEnd::ForceReconnect { reason });
                            }
                            IncomingMessage::EventAck { event_id } => {
                                match durable_replay.ack(event_id).await {
                                    Ok(true) => {
                                        state.record_flight("relay_ack", format!("event_id={event_id}")).await;
                                    }
                                    Ok(false) => {}
                                    Err(error) => {
                                        state.record_flight(
                                            "live_delta_persist_error",
                                            format!("ack event_id={event_id} error={error}"),
                                        ).await;
                                        state.log(app, format!("live delta pending 저장 실패: {error}")).await;
                                    }
                                }
                            }
                            IncomingMessage::LiveDeltaResend {
                                stream_id,
                                from_seq,
                                to_seq,
                            } => {
                                let replayed = replay_live_delta_range(
                                    &writer,
                                    durable_replay,
                                    stream_id,
                                    *from_seq,
                                    *to_seq,
                                ).await?;
                                let expected = to_seq.saturating_sub(*from_seq).saturating_add(1) as usize;
                                state
                                    .record_flight(
                                        "live_delta_resend",
                                        format!(
                                            "stream={} from={} to={} replayed={} expected={}",
                                            stream_id, from_seq, to_seq, replayed, expected
                                        ),
                                    )
                                    .await;
                                if replayed < expected {
                                    let removed = durable_replay.drop_live_stream(stream_id).await.unwrap_or(0);
                                    state
                                        .record_flight(
                                            "live_delta_resync",
                                            format!(
                                                "stream={} reason=missing_pending_range replayed={} expected={} dropped={}",
                                                stream_id, replayed, expected, removed
                                            ),
                                        )
                                        .await;
                                    let _ = lcu_poll_worker.live_game_resync.send(()).await;
                                }
                            }
                            IncomingMessage::LiveDeltaResync { stream_id } => {
                                let removed = durable_replay.drop_live_stream(stream_id).await.unwrap_or(0);
                                state
                                    .record_flight(
                                        "live_delta_resync",
                                        format!(
                                            "stream={} reason=relay_requested dropped={}",
                                            stream_id, removed
                                        ),
                                    )
                                    .await;
                                let _ = lcu_poll_worker.live_game_resync.send(()).await;
                            }
                            IncomingMessage::DeviceChallenge { challenge_id, nonce } => {
                                let signature = device_identity.sign_challenge(
                                    challenge_id,
                                    nonce,
                                    &session.session_id,
                                    &connection_id,
                                );
                                let proof = DeviceProofMessage::new(
                                    challenge_id,
                                    device_identity.device_id(),
                                    &device_public_key,
                                    signature,
                                );
                                send_shared_relay_message(
                                    &writer,
                                    Message::Text(serde_json::to_string(&proof)?.into()),
                                    "기기 인증 proof 전송 실패",
                                )
                                .await?;
                                state
                                    .record_flight(
                                        "device_identity",
                                        format!("proof_sent challenge={}", &challenge_id[..8]),
                                    )
                                    .await;
                            }
                            IncomingMessage::ServerHello { protocol_version, capabilities } => {
                                durable_replay_enabled = *protocol_version >= 1
                                    && capabilities.get("event_ack") == Some(&true)
                                    && capabilities.get("durable_event_replay") == Some(&true);
                                unexpected_error_reports_enabled = *protocol_version >= 1
                                    && capabilities.get("unexpected_error_reports") == Some(&true);
                                update_diagnostics_enabled = *protocol_version >= 1
                                    && capabilities.get("update_diagnostics") == Some(&true);
                                diagnostic_reports_enabled = *protocol_version >= 1
                                    && capabilities.get("diagnostic_reports") == Some(&true);
                                diagnostic_log_chunks_enabled = *protocol_version >= 1
                                    && capabilities.get("diagnostic_log_chunks") == Some(&true);
                                if diagnostic_log_chunks_enabled && session_bound {
                                    diagnostic_log_initial_sync_pending = true;
                                }
                                let live_delta_enabled = *protocol_version >= 1
                                    && capabilities.get("live_game_delta_v1") == Some(&true);
                                let _ = lcu_poll_worker
                                    .live_game_delta_enabled
                                    .send(live_delta_enabled);
                                state
                                    .record_flight(
                                        "protocol",
                                        format!(
                                            "server_protocol={} durable_replay={} live_delta={} error_reports={} diagnostics={} log_chunks={}",
                                            protocol_version,
                                            durable_replay_enabled,
                                            live_delta_enabled,
                                            unexpected_error_reports_enabled,
                                            diagnostic_reports_enabled,
                                            diagnostic_log_chunks_enabled,
                                        ),
                                    )
                                    .await;
                                if durable_replay_enabled && session_bound {
                                    let replayed = replay_durable_events(&writer, durable_replay).await?;
                                    if replayed > 0 {
                                        state.log(app, format!("미확인 durable 이벤트 {replayed}건 재전송")).await;
                                    }
                                }
                            }
                            IncomingMessage::SessionBound { discord_id, trust_level, refresh_token, .. } => {
                                let Some(discord_id) = *discord_id else {
                                    return Err(AgentError::Relay(
                                        "Relay session_bound에 Discord ID가 없습니다.".into(),
                                    ));
                                };
                                if let Err(error) = session::pin_discord_id(session, discord_id) {
                                    if let Err(remove_error) = session::remove() {
                                        state
                                            .record_flight(
                                                "session_error",
                                                format!("cleanup_after_pin_failure: {remove_error}"),
                                            )
                                            .await;
                                    }
                                    *needs_login = true;
                                    state
                                        .set_oauth_pending(
                                            app,
                                            true,
                                            "저장된 Discord 계정과 다른 계정이 감지되어 연결을 차단했습니다. 다시 로그인하세요.",
                                        )
                                        .await;
                                    state
                                        .record_flight("security", "discord_binding_mismatch")
                                        .await;
                                    state.log(app, format!("Discord 바인딩 pin 차단: {error}")).await;
                                    return Ok(ConnectionEnd::Stopped);
                                }
                                if let Some(refresh_token) = refresh_token.clone() {
                                    if let Err(error) = session::set_refresh_token(session, refresh_token) {
                                        state
                                            .record_flight(
                                                "session_error",
                                                format!("refresh_token_save_failed: {error}"),
                                            )
                                            .await;
                                        state
                                            .report_unexpected_error(
                                                "session",
                                                "refresh_token_save_failed",
                                                error.to_string(),
                                            )
                                            .await;
                                    } else {
                                        state.record_flight("session", "refresh_token_saved").await;
                                    }
                                }
                                *needs_login = false;
                                let is_new_device = trust_level.as_deref() == Some("new_device");
                                handle_incoming(
                                    app,
                                    state,
                                    &writer,
                                    incoming,
                                    false,
                                    is_new_device,
                                    &lcu_poll_worker.live_game_polling,
                                )
                                .await?;
                                session_bound = true;
                                session_new_device = is_new_device;
                                if diagnostic_log_chunks_enabled {
                                    diagnostic_log_initial_sync_pending = true;
                                }
                                if durable_replay_enabled {
                                    let replayed = replay_durable_events(&writer, durable_replay).await?;
                                    if replayed > 0 {
                                        state.log(app, format!("미확인 durable 이벤트 {replayed}건 재전송")).await;
                                    }
                                }
                                if durable_replay.take_needs_live_resync() {
                                    let _ = lcu_poll_worker.live_game_resync.send(()).await;
                                    state
                                        .report_diagnostic(
                                            "live_delta",
                                            "forced_resync_after_pending_load_failure",
                                            "discarded untrusted pending replay state and requested a full live snapshot",
                                        )
                                        .await;
                                }
                                state.log(app, "Relay 세션 인증 완료 — LCU 이벤트 감시 시작").await;
                                state.relay.set_oauth_sender(generation, None).await;
                                if lcu_socket_watch.is_none() {
                                    let (stop_tx, stop_rx) = watch::channel(false);
                                    let event_state = Arc::clone(&state);
                                    let event_config = config.clone();
                                    let event_tx = lcu_poll_worker.trigger.clone();
                                    let task = tokio::spawn(async move {
                                        LcuEventPoller::watch_socket(
                                            event_state,
                                            event_config,
                                            event_tx,
                                            stop_rx,
                                        )
                                        .await;
                                    });
                                    lcu_socket_watch = Some(LcuSocketWatch { stop: stop_tx, task });
                                }
                                state.relay.set_connection_state(
                                    app,
                                    state,
                                    generation,
                                    RelayConnectionState::Connected,
                                ).await;
                            }
                            IncomingMessage::Command { request_id, .. } => {
                                // Keep reader/pong processing live while a command waits on
                                // LCU. Bound concurrency instead of building an unbounded queue.
                                if !command_tasks.is_empty() {
                                    let result = CommandResult::failure(request_id.clone(), "다른 LCU 명령 처리 중");
                                    send_shared_relay_message(&writer, Message::Text(serde_json::to_string(&result)?.into()), "Relay 응답 전송 실패").await?;
                                } else {
                                    let command_app = app.clone();
                                    let command_state = Arc::clone(state);
                                    let command_writer = Arc::clone(&writer);
                                    let polling = lcu_poll_worker.live_game_polling.clone();
                                    command_tasks.spawn(async move {
                                        handle_incoming(
                                            &command_app,
                                            &command_state,
                                            &command_writer,
                                            incoming,
                                            session_bound,
                                            session_new_device,
                                            &polling,
                                        ).await
                                    });
                                }
                            }
                            _ => handle_incoming(
                                app,
                                state,
                                &writer,
                                incoming,
                                session_bound,
                                session_new_device,
                                &lcu_poll_worker.live_game_polling,
                            ).await?,
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        send_shared_relay_message(&writer, Message::Pong(payload), "Relay pong 전송 실패").await?;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        RELAY_PONG_COUNT.fetch_add(1, Ordering::AcqRel);
                        RELAY_LAST_PONG_MS.store(relay_progress_now_ms(), Ordering::Release);
                        touch_relay_network_progress();
                        let sent_at = heartbeat_awaiting.lock().await.take();
                        if let Some(sent_at) = sent_at {
                            state
                                .record_flight(
                                    "relay_heartbeat",
                                    format!("ws_pong rtt_ms={}", sent_at.elapsed().as_millis()),
                                )
                                .await;
                        } else {
                            state
                                .record_flight(
                                    "relay_heartbeat",
                                    format!("ws_pong_without_pending_ping connection_id={connection_id}"),
                                )
                                .await;
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let detail = frame
                            .as_ref()
                            .map(|frame| format!("code={:?} reason={}", frame.code, frame.reason))
                            .unwrap_or_else(|| "code=none reason=none".into());
                        state
                            .report_diagnostic(
                                "relay",
                                "websocket_close",
                                format!("{detail}; trace={}", relay_trace_snapshot()),
                            )
                            .await;
                        state.relay.set_oauth_sender(generation, None).await;
                        return Ok(ConnectionEnd::for_close(
                            frame.as_ref().map(|frame| u16::from(frame.code)),
                            session_bound,
                        ));
                    }
                    None => {
                        state
                            .report_diagnostic(
                                "relay",
                                "websocket_eof",
                                format!(
                                    "stream ended without close frame; trace={}",
                                    relay_trace_snapshot()
                                ),
                            )
                            .await;
                        state.relay.set_oauth_sender(generation, None).await;
                        return Ok(ConnectionEnd::Closed { authenticated: session_bound });
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        state
                            .report_diagnostic(
                                "relay",
                                "websocket_receive_failed",
                                format!("{error}; trace={}", relay_trace_snapshot()),
                            )
                            .await;
                        state.relay.set_oauth_sender(generation, None).await;
                        return Err(AgentError::Relay("Relay 메시지 수신 실패".into()));
                    }
                }
            }
            completed = command_tasks.join_next(), if !command_tasks.is_empty() => {
                mark_relay_branch(15);
                match completed {
                    Some(Ok(result)) => result?,
                    Some(Err(error)) => return Err(AgentError::Relay(format!("LCU command task failed: {error}").into())),
                    None => {}
                }
            }
            _ = durable_replay_tick.tick(), if session_bound && durable_replay_enabled => {
                mark_relay_branch(5);
                if durable_replay.persistence_dirty {
                    if let Err(error) = durable_replay.persist_live_pending().await {
                        state.record_flight("live_delta_persist_error", format!("retry: {error}")).await;
                    }
                }
                let replayed = replay_durable_events(&writer, durable_replay).await?;
                if replayed > 0 {
                    state.log(app, format!("ACK 대기 durable 이벤트 {replayed}건 재전송: {}", durable_replay.pending_summary())).await;
                }
            }
            _ = diagnostic_log_tick.tick(), if session_bound && diagnostic_log_chunks_enabled => {
                diagnostic_log_initial_sync_pending = true;
            }
            _ = async {}, if session_bound && diagnostic_log_chunks_enabled && diagnostic_log_initial_sync_pending && !diagnostic_log_sync_running => {
                mark_relay_branch(14);
                diagnostic_log_initial_sync_pending = false;
                diagnostic_log_sync_running = true;
                let sync_state = Arc::clone(state);
                let sync_writer = Arc::clone(&writer);
                let done_tx = diagnostic_log_done_tx.clone();
                diagnostic_log_task = Some(AbortTaskOnDrop(tokio::spawn(async move {
                    let result = AssertUnwindSafe(send_automatic_log_sources(&sync_state, &sync_writer))
                        .catch_unwind()
                        .await;
                    let result = match result {
                        Ok(Ok(value)) => Ok(value),
                        Ok(Err(error)) => Err(error.to_string()),
                        Err(_) => Err("automatic diagnostic log sync panicked".to_owned()),
                    };
                    let _ = done_tx.send(result).await;
                })));
            }
            result = diagnostic_log_done_rx.recv(), if diagnostic_log_sync_running => {
                mark_relay_branch(14);
                diagnostic_log_sync_running = false;
                drop(diagnostic_log_task.take());
                match result {
                    Some(Ok((sources, chunks))) => {
                        state
                            .record_flight(
                                "diagnostic_log_sync",
                                format!("sources={sources} chunks={chunks}"),
                            )
                            .await;
                    }
                    Some(Err(error)) => {
                        state
                            .record_flight(
                                "diagnostic_log_sync",
                                format!("failed error={error}"),
                            )
                            .await;
                    }
                    None => {
                        state
                            .record_flight("diagnostic_log_sync", "completion_channel_closed")
                            .await;
                    }
                }
            }
            _ = diagnostic_heartbeat_tick.tick(), if session_bound && diagnostic_reports_enabled => {
                mark_relay_branch(6);
                let report = AgentDiagnosticReport::new(
                    "lifecycle",
                    "health_snapshot",
                    format!(
                        "lcu_state={:?} durable_pending={} durable_replay={} live_delta={} trace={}",
                        state.lcu_state().await,
                        durable_replay.len(),
                        durable_replay_enabled,
                        *lcu_poll_worker.live_game_delta_enabled.borrow(),
                        relay_trace_snapshot(),
                    ),
                );
                let message = serde_json::to_string(&report)?;
                send_shared_relay_message(&writer, Message::Text(message.into()), "운영 상태 스냅샷 전송 실패").await?;
            }
            report = update_reports.recv(), if session_bound && update_diagnostics_enabled => {
                mark_relay_branch(7);
                match report {
                    Ok(report) => {
                        let message = serde_json::to_string(&report)?;
                        send_shared_relay_message(&writer, Message::Text(message.into()), "업데이트 진단 전송 실패").await?;
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        state.record_flight("updater", format!("diagnostic_queue_lagged skipped={skipped}")).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err(AgentError::Relay("업데이트 진단 큐가 종료되었습니다.".into()));
                    }
                }
            }
            report = diagnostic_reports.recv(), if session_bound && diagnostic_reports_enabled => {
                mark_relay_branch(8);
                match report {
                    Ok(report) => {
                        let message = serde_json::to_string(&report)?;
                        send_shared_relay_message(&writer, Message::Text(message.into()), "운영 진단 전송 실패").await?;
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        state.record_flight(
                            "server_diagnostic",
                            format!("queue_lagged skipped={skipped}"),
                        ).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err(AgentError::Relay("운영 진단 큐가 종료되었습니다.".into()));
                    }
                }
            }
            report = error_reports.recv(), if session_bound && unexpected_error_reports_enabled => {
                mark_relay_branch(9);
                match report {
                    Ok(report) => {
                        let message = serde_json::to_string(&report)?;
                        send_shared_relay_message(&writer, Message::Text(message.into()), "예상치 못한 오류 보고 전송 실패").await?;
                        state.record_flight("error_report", "unexpected_error_sent").await;
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        state.record_flight(
                            "error_report",
                            format!("queue_lagged skipped={skipped}"),
                        ).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err(AgentError::Relay("오류 보고 큐가 종료되었습니다.".into()));
                    }
                }
            }
            request = discord_join_requests.recv(), if session_bound => {
                mark_relay_branch(10);
                match request {
                    Ok(requester_discord_id) => {
                        let Some(event) = serialize_agent_event(
                            "discord_join_request",
                            json!({"requester_discord_id": requester_discord_id}),
                        )? else {
                            state
                                .record_flight(
                                    "discord_presence_join_request",
                                    "relay_payload_too_large",
                                )
                                .await;
                            continue;
                        };
                        send_shared_relay_message(
                            &writer,
                            Message::Text(event.text.into()),
                            "Discord 참가 요청 Relay 전송 실패",
                        ).await?;
                        state
                            .record_flight(
                                "discord_presence_join_request",
                                format!("lookup_sent requester_discord_id={requester_discord_id}"),
                            )
                            .await;
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        state
                            .record_flight(
                                "discord_presence_join_request",
                                format!("lookup_queue_lagged skipped={skipped}"),
                            )
                            .await;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err(AgentError::Relay("Discord 참가 요청 큐가 종료되었습니다.".into()));
                    }
                }
            }
            request = discord_presence_context_requests.recv(), if session_bound => {
                mark_relay_branch(11);
                match request {
                    Ok(()) => {
                        let Some(event) = serialize_agent_event(
                            "discord_presence_context_request",
                            json!({}),
                        )? else {
                            continue;
                        };
                        match try_send_presence_context_message(
                            &writer,
                            Message::Text(event.text.into()),
                        ).await? {
                            PresenceContextSendOutcome::Sent => {}
                            PresenceContextSendOutcome::WriterBusy => {
                                state
                                    .record_flight(
                                        "discord_presence_context",
                                        "refresh_send_skipped writer_busy",
                                    )
                                    .await;
                            }
                            PresenceContextSendOutcome::TimedOut => {
                                state
                                    .record_flight(
                                        "discord_presence_context",
                                        "refresh_send_skipped send_timeout",
                                    )
                                    .await;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        state
                            .record_flight(
                                "discord_presence_context",
                                format!("request_queue_lagged skipped={skipped}"),
                            )
                            .await;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err(AgentError::Relay("Discord Presence 조회 큐가 종료되었습니다.".into()));
                    }
                }
            }
            _ = watchdog.tick() => {
                mark_relay_branch(12);
                if !session_bound
                    && !session::auto_login_suppressed()
                    && state.lcu_state().await.is_ready()
                    && last_identity_hello_at
                        .is_none_or(|last| last.elapsed() >= Duration::from_secs(5))
                {
                    last_identity_hello_at = Some(Instant::now());
                    let (puuid, riot_id) = lcu_identity_for_relay(config).await;
                    if puuid.is_some() || riot_id.is_some() {
                        let hello = AgentHelloMessage::new(
                            true,
                            session.bound_discord_id,
                            puuid,
                            riot_id,
                        );
                        send_shared_relay_message(
                            &writer,
                            Message::Text(serde_json::to_string(&hello)?.into()),
                            "Relay identity hello 전송 실패",
                        ).await?;
                        state.record_flight("relay", "lcu_identity_hello_sent").await;
                    }
                }
                if should_start_login(session_bound, login_opened, auth_started.elapsed()) {
                    *needs_login = true;
                    login_opened = true;
                    let status = if user_initiated_login {
                        "저장된 세션이 만료되어 Discord 재인증 페이지를 열었습니다."
                    } else {
                        "저장된 세션이 만료되었습니다 — Agent에서 Discord 로그인을 눌러주세요."
                    };
                    state.set_oauth_pending(app, true, status).await;
                    if oauth_browser_should_open(user_initiated_login, true) {
                        open_login_url(app, &config.login_url(&session.session_id)?)?;
                        state.record_flight("relay", "oauth_browser_opened_by_user").await;
                    }
                }
            }
            Some(batch) = lcu_poll_rx.recv(), if session_bound => {
                mark_relay_branch(13);
                for (message_type, data) in batch.events {
                    if message_type == "match_rofl_candidate" {
                        if let Some(game_id) = replay_game_id(&data) {
                            let preflight_tx = lcu_poll_worker.rofl_preflight.clone();
                            let preflight_app = app.clone();
                            let preflight_state = Arc::clone(state);
                            let preflight_config = config.clone();
                            let preflight_session_id = session.session_id.clone();
                            let preflight_ws_token = session.ws_token.clone();
                            tokio::spawn(async move {
                                match check_replay_upload_needed(
                                    &preflight_config,
                                    &preflight_session_id,
                                    &preflight_ws_token,
                                    &game_id,
                                )
                                .await
                                {
                                    Ok(needed) => {
                                        preflight_state
                                            .record_flight(
                                                "rofl_preflight",
                                                format!("game_id={game_id} upload={needed}"),
                                            )
                                            .await;
                                        if needed {
                                            preflight_state
                                                .log(
                                                    &preflight_app,
                                                    format!(
                                                        "ROFL 서버 선확인 완료 — 다운로드 허용: game_id={game_id}"
                                                    ),
                                                )
                                                .await;
                                        }
                                        let _ = preflight_tx
                                            .send(RoflPreflightDecision {
                                                game_id,
                                                needed: Some(needed),
                                            })
                                            .await;
                                    }
                                    Err(error) => {
                                        preflight_state
                                            .record_flight(
                                                "rofl_preflight",
                                                format!("game_id={game_id} failed error={error}"),
                                            )
                                            .await;
                                        preflight_state
                                            .log(
                                                &preflight_app,
                                                format!(
                                                    "ROFL 서버 선확인 실패 — 다운로드 보류 후 재시도: game_id={game_id} error={error}"
                                                ),
                                            )
                                            .await;
                                        sleep(Duration::from_secs(30)).await;
                                        let _ = preflight_tx
                                            .send(RoflPreflightDecision {
                                                game_id,
                                                needed: None,
                                            })
                                            .await;
                                    }
                                }
                            });
                        } else {
                            state.record_flight("rofl_preflight", "invalid_candidate_event").await;
                        }
                        continue;
                    }
                    if message_type == "match_rofl_file_ready" {
                        if let Some(upload) = replay_upload_request(&data) {
                            let upload_app = app.clone();
                            let upload_state = Arc::clone(state);
                            let upload_config = config.clone();
                            let upload_session_id = session.session_id.clone();
                            let upload_ws_token = session.ws_token.clone();
                            tokio::spawn(async move {
                                match upload_replay_file(
                                    &upload_config,
                                    &upload_session_id,
                                    &upload_ws_token,
                                    &upload,
                                ).await {
                                    Ok(ReplayUploadOutcome::Uploaded) => {
                                        upload_state.record_flight("rofl_upload", format!("uploaded game_id={} bytes={} auto_cleanup={}", upload.game_id, upload.file_size, upload.delete_after_upload)).await;
                                        upload_state.log(&upload_app, format!("ROFL 원본 서버 업로드 완료: game_id={} bytes={}", upload.game_id, upload.file_size)).await;
                                        if upload.delete_after_upload {
                                            match delete_uploaded_auto_replay(&upload_config, &upload).await {
                                                Ok(true) => {
                                                    upload_state.record_flight("rofl_cleanup", format!("deleted_auto_download game_id={}", upload.game_id)).await;
                                                    upload_state.log(&upload_app, format!("Yummi 자동 다운로드 ROFL 로컬 정리 완료: game_id={}", upload.game_id)).await;
                                                }
                                                Ok(false) => {
                                                    upload_state.record_flight("rofl_cleanup", format!("preserved_or_already_missing game_id={}", upload.game_id)).await;
                                                }
                                                Err(error) => {
                                                    upload_state.record_flight("rofl_cleanup", format!("preserved game_id={} reason={}", upload.game_id, error)).await;
                                                    upload_state.log(&upload_app, format!("ROFL 로컬 파일 보존: game_id={} reason={}", upload.game_id, error)).await;
                                                }
                                            }
                                        }
                                    }
                                    Ok(ReplayUploadOutcome::NotNeeded) => {
                                        upload_state.record_flight("rofl_upload", format!("skipped_not_yummi_match game_id={}", upload.game_id)).await;
                                    }
                                    Err(error) => {
                                        upload_state.record_flight("rofl_upload", format!("failed game_id={} error={}", upload.game_id, error)).await;
                                        upload_state.log(&upload_app, format!("ROFL 원본 서버 업로드 실패: game_id={} error={}", upload.game_id, error)).await;
                                    }
                                }
                            });
                        } else {
                            state.record_flight("rofl_upload", "invalid_file_ready_event").await;
                        }
                        continue;
                    }
                    let event_log = event_summary(message_type, &data);
                    state.record_flight("lcu_event", event_log.clone()).await;
                    let live_participant_count =
                        (message_type == "live_game_update" && !live_game_announced).then(|| {
                            data.get("participants")
                                .and_then(Value::as_array)
                                .or_else(|| data.pointer("/state/participants").and_then(Value::as_array))
                                .map_or(0, Vec::len)
                        });
                    let Some(event) = serialize_agent_event(message_type, data)? else {
                        state.log(app, "LCU 이벤트가 Relay 크기 제한을 초과해 생략됨").await;
                        continue;
                    };
                    if durable_replay_enabled && is_durable_event(message_type, &event) {
                        match durable_replay.track(event.clone()).await {
                            Ok(true) => {
                                state.log(app, "durable replay buffer가 가득 차 가장 오래된 이벤트를 제거함").await;
                            }
                            Ok(false) => {}
                            Err(error) => {
                                state.record_flight(
                                    "live_delta_persist_error",
                                    format!("track error={error}"),
                                ).await;
                                state.log(app, format!("live delta pending 저장 실패: {error}")).await;
                            }
                        }
                    }
                    send_shared_relay_message(
                        &writer,
                        Message::Text(event.text.into()),
                        "Relay 이벤트 전송 실패",
                    ).await?;
                    state.log(app, format!("Relay 이벤트 전송: {event_log}")).await;
                    if let Some(count) = live_participant_count {
                        live_game_announced = true;
                        state.log(app, format!("실시간 관전 데이터 서버 전송 확인 ({count}명)")).await;
                    }
                }
                for diagnostic in batch.diagnostics {
                    state.record_flight("lcu_diagnostic", diagnostic.clone()).await;
                    state
                        .report_diagnostic("lcu", "poll", &diagnostic)
                        .await;
                    state.log(app, format!("LCU 진단: {diagnostic}")).await;
                }
            }
        }
    }
}

async fn send_automatic_log_sources<S>(
    state: &Arc<AppState>,
    writer: &SharedRelayWriter<S>,
) -> AgentResult<(usize, usize)>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let sources = timeout(DIAGNOSTIC_LOG_SYNC_TIMEOUT, state.automatic_log_sources())
        .await
        .map_err(|_| AgentError::Relay("자동 진단 로그 수집 시간 초과".into()))?;
    let mut source_count = 0usize;
    let mut chunk_total = 0usize;

    for (source, content) in sources {
        if content.is_empty() {
            continue;
        }
        let mut chunks = Vec::new();
        let mut start = 0usize;
        while start < content.len() {
            let mut end = (start + DIAGNOSTIC_LOG_CHUNK_BYTES).min(content.len());
            while end > start && !content.is_char_boundary(end) {
                end -= 1;
            }
            if end == start {
                return Err(AgentError::Relay("자동 진단 로그 UTF-8 분할 실패".into()));
            }
            chunks.push(content[start..end].to_owned());
            start = end;
        }
        let batch_id = Uuid::new_v4().to_string();
        let chunk_count = chunks.len() as u32;
        for (index, content) in chunks.into_iter().enumerate() {
            let report =
                AgentLogChunk::new(batch_id.clone(), source, index as u32, chunk_count, content);
            let message = serde_json::to_string(&report)?;
            if message.len() > MAX_RELAY_MESSAGE_BYTES {
                return Err(AgentError::Relay("자동 진단 로그 메시지 크기 초과".into()));
            }
            send_shared_relay_message(
                writer,
                Message::Text(message.into()),
                "자동 진단 로그 전송 실패",
            )
            .await?;
            chunk_total += 1;
        }
        source_count += 1;
    }

    Ok((source_count, chunk_total))
}

async fn receive_oauth_code(receiver: &mut mpsc::Receiver<String>) -> String {
    match receiver.recv().await {
        Some(code) => code,
        // SessionBound drops the sender. A closed receiver immediately returns None;
        // keep this select branch pending so it cannot spin the authenticated relay loop.
        None => std::future::pending().await,
    }
}

async fn handle_incoming<S>(
    app: &AppHandle,
    state: &Arc<AppState>,
    websocket: &SharedRelayWriter<S>,
    incoming: IncomingMessage,
    session_bound: bool,
    session_new_device: bool,
    live_game_polling: &watch::Sender<bool>,
) -> AgentResult<()>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    match incoming {
        IncomingMessage::Command {
            action,
            request_id,
            payload,
        } => {
            let parsed_action = Action::parse(&action);
            if !command_is_authorized(session_bound, session_new_device, parsed_action) {
                let message = if session_bound {
                    "신규 기기 자동 로그인 상태에서 허용되지 않은 LCU 명령입니다."
                } else {
                    "Relay 인증이 완료되지 않았습니다."
                };
                let result = CommandResult::failure(request_id, message);
                send_shared_relay_message(
                    websocket,
                    Message::Text(serde_json::to_string(&result)?.into()),
                    "Relay 응답 전송 실패",
                )
                .await?;
                state.log(app, "인증 전 Relay 명령 차단").await;
                return Ok(());
            }

            let payload = match command_auth::verify_command(
                &action,
                &payload,
                state.bound_discord_id().await,
            )
            .await
            {
                Ok(payload) => payload,
                Err(error) => {
                    let result = CommandResult::failure(request_id, "LCU 명령 인증 실패");
                    send_shared_relay_message(
                        websocket,
                        Message::Text(serde_json::to_string(&result)?.into()),
                        "Relay 응답 전송 실패",
                    )
                    .await?;
                    state
                        .log(app, format!("Relay LCU 명령 서명 차단: {error}"))
                        .await;
                    return Ok(());
                }
            };

            let action_label = parsed_action.map(Action::as_str).unwrap_or("unknown");
            let result = match parsed_action {
                None => CommandResult::failure(request_id, "unknown action"),
                Some(Action::LaunchClient) => {
                    match COMMAND_DISK_IO
                        .run(COMMAND_DISK_TIMEOUT, launch_league_client)
                        .await
                    {
                        Ok((ok, message)) => {
                            CommandResult::from_parts(request_id, ok, message, json!({}))
                        }
                        Err(error) => CommandResult::failure(
                            request_id,
                            format!("클라이언트 실행 결과 확인 불가: {error}"),
                        ),
                    }
                }
                Some(Action::Ping) => CommandResult::success(request_id, "pong", json!({})),
                Some(action) => {
                    let config = state.config.read().await.clone();
                    let _guard =
                        match timeout(COMMAND_LOCK_TIMEOUT, state.command_lock.lock()).await {
                            Ok(guard) => guard,
                            Err(_) => {
                                let result = CommandResult::failure(
                                    request_id,
                                    "LCU 명령 처리 중: 잠금 대기 시간 초과",
                                );
                                send_shared_relay_message(
                                    websocket,
                                    Message::Text(serde_json::to_string(&result)?.into()),
                                    "Relay 응답 전송 실패",
                                )
                                .await?;
                                state.record_flight("command", "command_lock_timeout").await;
                                return Ok(());
                            }
                        };
                    let discovery_config = config.clone();
                    let client = COMMAND_DISK_IO
                        .run(COMMAND_DISK_TIMEOUT, move || {
                            let path = lockfile_path(&discovery_config)
                                .ok_or_else(|| AgentError::Lcu("LCU 미연결".into()))?;
                            LcuClient::from_lockfile(&path)
                                .or_else(|_| LcuClient::from_lockfile_legacy(&path))
                        })
                        .await;
                    match client {
                        Ok(Ok(client)) => {
                            match client.execute_action(action, &payload, &config).await {
                                Ok(outcome) => CommandResult::from_parts(
                                    request_id,
                                    outcome.ok,
                                    outcome.message,
                                    outcome.data,
                                ),
                                Err(error) => CommandResult::failure(request_id, error.to_string()),
                            }
                        }
                        Ok(Err(error)) => CommandResult::failure(request_id, error.to_string()),
                        Err(error) => CommandResult::failure(
                            request_id,
                            format!("LCU 파일 접근 실패: {error}"),
                        ),
                    }
                }
            };
            let result_detail = result
                .message()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(1_200)
                .collect::<String>();
            state
                .record_flight(
                    "command",
                    format!(
                        "action={action_label} result={} detail={result_detail}",
                        if result.is_ok() { "ok" } else { "error" }
                    ),
                )
                .await;
            send_shared_relay_message(
                websocket,
                Message::Text(serde_json::to_string(&result)?.into()),
                "Relay 응답 전송 실패",
            )
            .await?;
            if !parsed_action.is_some_and(Action::is_background) {
                let result_label = if result.is_ok() { "성공" } else { "실패" };
                state
                    .log(
                        app,
                        format!("명령 완료: {action_label} ({result_label}) — {result_detail}"),
                    )
                    .await;
            }
        }
        IncomingMessage::SessionBound {
            discord_id,
            discord_name,
            username,
            discord_avatar,
            avatar_url,
            trust_level,
            refresh_token: _,
        } => {
            state
                .apply_session_bound(
                    app,
                    discord_id,
                    discord_name.or(username),
                    safe_avatar_url(discord_avatar.or(avatar_url)),
                    trust_level,
                )
                .await;
        }
        IncomingMessage::DiscordJoinRequestResolved {
            requester_discord_id,
            riot_id,
            status,
        } => {
            if session_bound {
                let published = state.publish_discord_join_resolution(DiscordJoinResolution {
                    requester_discord_id,
                    riot_id,
                    status: status.clone(),
                });
                state
                    .record_flight(
                        "discord_presence_join_request",
                        format!(
                            "lookup_result requester_discord_id={requester_discord_id} status={status} published={published}"
                        ),
                    )
                    .await;
            }
        }
        IncomingMessage::DiscordPresenceContext {
            active,
            status,
            invite_code,
            discord_guild_id,
        } => {
            if session_bound {
                let context = if active {
                    match (discord_guild_id, invite_code) {
                        (Some(discord_guild_id), Some(invite_code)) => {
                            Some(DiscordPresenceMatchContext {
                                discord_guild_id,
                                invite_code,
                                status: status.clone(),
                            })
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                state.set_discord_presence_match(context).await;
                state
                    .record_flight(
                        "discord_presence_context",
                        format!("active={active} status={status}"),
                    )
                    .await;
            }
        }
        IncomingMessage::LiveGamePolling { enabled } => {
            if session_bound {
                let _ = live_game_polling.send(enabled);
                state
                    .log(
                        app,
                        if enabled {
                            "Relay live game polling 활성화"
                        } else {
                            "Relay live game polling 비활성화 — 구독자 없음"
                        },
                    )
                    .await;
            }
        }
        IncomingMessage::Pong
        | IncomingMessage::ForceReconnect { .. }
        | IncomingMessage::ServerHello { .. }
        | IncomingMessage::DeviceChallenge { .. }
        | IncomingMessage::EventAck { .. }
        | IncomingMessage::LiveDeltaResend { .. }
        | IncomingMessage::LiveDeltaResync { .. }
        | IncomingMessage::Unknown => {}
    }
    Ok(())
}

fn command_is_authorized(
    session_bound: bool,
    session_new_device: bool,
    action: Option<Action>,
) -> bool {
    session_bound && (!session_new_device || action.is_some_and(Action::allowed_on_new_device))
}

fn safe_avatar_url(value: Option<String>) -> Option<String> {
    let value = value?;
    let url = url::Url::parse(&value).ok()?;
    (url.scheme() == "https"
        && url.host_str() == Some("cdn.discordapp.com")
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(value)
}

#[derive(Clone, Debug)]
struct ReplayUploadRequest {
    game_id: String,
    path: PathBuf,
    file_name: String,
    file_size: u64,
    file_modified_at_ms: Option<u64>,
    delete_after_upload: bool,
    baseline_league_process_ids: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplayUploadOutcome {
    Uploaded,
    NotNeeded,
}

fn replay_game_id(data: &Value) -> Option<String> {
    let game_id = data.get("gameId")?.as_str()?.trim().to_owned();
    if game_id.is_empty()
        || game_id.len() > 64
        || !game_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(game_id)
}

fn replay_upload_request(data: &Value) -> Option<ReplayUploadRequest> {
    let game_id = replay_game_id(data)?;
    let path = PathBuf::from(data.get("path")?.as_str()?);
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("rofl"))
    {
        return None;
    }
    let raw_file_name = data
        .get("fileName")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 160)
        .unwrap_or("replay.rofl");
    let mut file_name = raw_file_name
        .chars()
        .take(120)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if !file_name.to_ascii_lowercase().ends_with(".rofl") {
        file_name = format!("{}.rofl", game_id);
    }
    let file_size = data.get("fileSize")?.as_u64()?;
    if file_size == 0 || file_size > MAX_REPLAY_UPLOAD_BYTES {
        return None;
    }
    let file_modified_at_ms = data.get("fileModifiedAtMs").and_then(Value::as_u64);
    let path_matches_game_id = path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.contains(&game_id));
    let delete_after_upload = data
        .get("deleteAfterUpload")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && path_matches_game_id
        && file_modified_at_ms.is_some();
    Some(ReplayUploadRequest {
        game_id,
        path,
        file_name,
        file_size,
        file_modified_at_ms,
        delete_after_upload,
        baseline_league_process_ids: league_game_process_ids(),
    })
}

fn metadata_accessed_at_ms(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .accessed()
        .ok()
        .and_then(|accessed| accessed.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn metadata_modified_at_ms(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

async fn delete_uploaded_auto_replay(
    config: &crate::config::Config,
    upload: &ReplayUploadRequest,
) -> Result<bool, String> {
    if !upload.delete_after_upload {
        return Ok(false);
    }
    let Some(expected_modified_at_ms) = upload.file_modified_at_ms else {
        return Ok(false);
    };

    let baseline_process_ids = &upload.baseline_league_process_ids;
    let baseline_accessed_at_ms = tokio::fs::symlink_metadata(&upload.path)
        .await
        .ok()
        .and_then(|metadata| metadata_accessed_at_ms(&metadata));
    let monitoring_started = Instant::now();
    while monitoring_started.elapsed() < REPLAY_LOCAL_DELETE_DELAY {
        sleep(REPLAY_LOCAL_DELETE_MONITOR_INTERVAL).await;
        if let Ok(Some(lockfile)) = lockfile_path_nonblocking(config).await {
            if let Ok(client) = LcuClient::from_lockfile(&lockfile)
                .or_else(|_| LcuClient::from_lockfile_legacy(&lockfile))
            {
                if let Ok(Some(watch_phase)) = client.gameflow_watch_phase().await {
                    if !watch_phase.eq_ignore_ascii_case("none") && !watch_phase.trim().is_empty() {
                        return Err(format!(
                            "ROFL 삭제 생략: League Client 재생 상태 감지 (watch_phase={watch_phase})"
                        ));
                    }
                }
            }
        }

        let process_ids = league_game_process_ids();
        if process_ids
            .iter()
            .any(|process_id| !baseline_process_ids.contains(process_id))
        {
            return Err(format!(
                "ROFL 삭제 생략: 업로드 후 새 League of Legends 프로세스가 실행됨 (재생/새 게임 가능성, pid={})",
                process_ids
                    .iter()
                    .find(|process_id| !baseline_process_ids.contains(process_id))
                    .copied()
                    .unwrap_or_default()
            ));
        }
    }

    let metadata = match tokio::fs::symlink_metadata(&upload.path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("ROFL 삭제 전 파일 상태 확인 실패: {error}")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("ROFL 삭제 생략: 일반 파일이 아니거나 심볼릭 링크입니다.".into());
    }
    if metadata.len() != upload.file_size {
        return Err(format!(
            "ROFL 삭제 생략: 업로드 후 파일 크기가 변경됨 (before={} after={})",
            upload.file_size,
            metadata.len()
        ));
    }
    if let (Some(before_accessed_at_ms), Some(after_accessed_at_ms)) =
        (baseline_accessed_at_ms, metadata_accessed_at_ms(&metadata))
    {
        if after_accessed_at_ms > before_accessed_at_ms {
            return Err(format!(
                "ROFL 삭제 생략: 업로드 후 파일 접근 시각이 변경됨 (재생/사용 가능성, before_atime={} after_atime={})",
                before_accessed_at_ms, after_accessed_at_ms
            ));
        }
    }

    let Some(current_modified_at_ms) = metadata_modified_at_ms(&metadata) else {
        return Err("ROFL 삭제 생략: 수정 시각을 확인할 수 없습니다.".into());
    };
    if current_modified_at_ms != expected_modified_at_ms {
        return Err(format!(
            "ROFL 삭제 생략: 업로드 후 파일이 변경되거나 다시 다운로드됨 (before_mtime={} after_mtime={})",
            expected_modified_at_ms, current_modified_at_ms
        ));
    }
    let path_matches_game_id = upload
        .path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.contains(&upload.game_id));
    if !path_matches_game_id {
        return Err("ROFL 삭제 생략: 파일명이 gameId와 일치하지 않습니다.".into());
    }

    tokio::fs::remove_file(&upload.path)
        .await
        .map_err(|error| format!("ROFL 로컬 파일 삭제 실패: {error}"))?;
    Ok(true)
}

async fn check_replay_upload_needed(
    config: &crate::config::Config,
    session_id: &str,
    ws_token: &str,
    game_id: &str,
) -> AgentResult<bool> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| AgentError::Relay("ROFL 선확인 클라이언트 생성 실패".into()))?;
    let target_url = config.replay_upload_target_url(session_id, game_id)?;
    let target_url_label = reqwest::Url::parse(&target_url)
        .map(|url| safe_url(&url))
        .unwrap_or_else(|_| "invalid-url".into());
    let mut last_error = "ROFL 업로드 대상 확인 실패".to_owned();

    for attempt in 1..=REPLAY_UPLOAD_ATTEMPTS {
        match client
            .get(target_url.clone())
            .header("x-yummi-ws-token", ws_token)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    match response.json::<Value>().await {
                        Ok(body) => match body.get("upload").and_then(Value::as_bool) {
                            Some(true) => return Ok(true),
                            Some(false) => return Ok(false),
                            None => {
                                last_error = format!(
                                    "ROFL 업로드 대상 응답 형식 오류 (method=GET endpoint={target_url_label} {} missing_upload_boolean)",
                                    status_detail(status)
                                );
                            }
                        },
                        Err(error) => {
                            last_error = format!(
                                "ROFL 업로드 대상 응답 형식 오류 (method=GET endpoint={target_url_label} {} decode_reason={error})",
                                status_detail(status)
                            );
                        }
                    }
                } else {
                    last_error = format!(
                        "ROFL 업로드 대상 확인 실패 (method=GET endpoint={target_url_label} {})",
                        status_detail(status)
                    );
                }
            }
            Err(error) => {
                last_error = format!(
                    "ROFL 업로드 대상 확인 네트워크 실패 (method=GET endpoint={target_url_label} {})",
                    transport_detail(&error)
                );
            }
        }

        if attempt < REPLAY_UPLOAD_ATTEMPTS {
            sleep(Duration::from_secs(u64::from(attempt.min(6)) * 2)).await;
        }
    }

    Err(AgentError::Relay(last_error))
}

async fn upload_replay_file(
    config: &crate::config::Config,
    session_id: &str,
    ws_token: &str,
    upload: &ReplayUploadRequest,
) -> AgentResult<ReplayUploadOutcome> {
    let metadata = tokio::fs::symlink_metadata(&upload.path)
        .await
        .map_err(|_| AgentError::Relay("ROFL 원본 파일을 열 수 없습니다.".into()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != upload.file_size
    {
        return Err(AgentError::Relay(
            "ROFL 원본 파일 상태가 변경되었습니다.".into(),
        ));
    }
    if metadata.len() == 0 || metadata.len() > MAX_REPLAY_UPLOAD_BYTES {
        return Err(AgentError::Relay(
            "ROFL 원본 파일 크기가 허용 범위를 벗어났습니다.".into(),
        ));
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(5 * 60))
        .build()
        .map_err(|_| AgentError::Relay("ROFL 업로드 클라이언트 생성 실패".into()))?;
    let upload_url = config.replay_upload_url(session_id, &upload.game_id)?;
    let upload_url_label = reqwest::Url::parse(&upload_url)
        .map(|url| safe_url(&url))
        .unwrap_or_else(|_| "invalid-url".into());
    let mut last_error = "ROFL 원본 업로드 실패".to_owned();

    for attempt in 1..=REPLAY_UPLOAD_ATTEMPTS {
        let file = tokio::fs::File::open(&upload.path)
            .await
            .map_err(|_| AgentError::Relay("ROFL 원본 파일 열기 실패".into()))?;
        let stream = ReaderStream::new(file);
        match client
            .post(upload_url.clone())
            .header("x-yummi-ws-token", ws_token)
            .header("x-replay-file-name", &upload.file_name)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .header(reqwest::header::CONTENT_LENGTH, upload.file_size)
            .body(reqwest::Body::wrap_stream(stream))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                return Ok(ReplayUploadOutcome::Uploaded);
            }
            Ok(response) if response.status().as_u16() == 409 => {
                return Ok(ReplayUploadOutcome::NotNeeded);
            }
            Ok(response) => {
                last_error = format!(
                    "ROFL 원본 업로드 실패 (method=POST endpoint={upload_url_label} {})",
                    status_detail(response.status())
                );
            }
            Err(error) => {
                last_error = format!(
                    "ROFL 원본 업로드 네트워크 실패 (method=POST endpoint={upload_url_label} {})",
                    transport_detail(&error)
                );
            }
        }

        if attempt < REPLAY_UPLOAD_ATTEMPTS {
            sleep(Duration::from_secs(u64::from(attempt.min(6)) * 2)).await;
        }
    }

    Err(AgentError::Relay(last_error))
}

fn serialize_agent_event(
    message_type: &'static str,
    data: Value,
) -> AgentResult<Option<SerializedAgentEvent>> {
    let live_stream_id = (message_type == "live_game_update")
        .then(|| {
            data.get("stream_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .flatten();
    let live_seq = (message_type == "live_game_update")
        .then(|| data.get("seq").and_then(Value::as_u64))
        .flatten();
    let message = AgentEventMessage::new(message_type, data);
    let event_id = message.event_id().to_owned();
    let text = serde_json::to_string(&message)?;
    Ok(
        (text.len() <= MAX_RELAY_MESSAGE_BYTES).then_some(SerializedAgentEvent {
            event_id,
            text,
            live_stream_id,
            live_seq,
        }),
    )
}

fn is_durable_event(message_type: &str, event: &SerializedAgentEvent) -> bool {
    matches!(message_type, "match_eog" | "guild_match_eog" | "match_rofl")
        || (message_type == "live_game_update"
            && event.live_stream_id.is_some()
            && event.live_seq.is_some())
}

async fn replay_durable_events<S>(
    websocket: &SharedRelayWriter<S>,
    replay: &DurableReplayBuffer,
) -> AgentResult<usize>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let events = replay.snapshot();
    for event in &events {
        send_shared_relay_message(
            websocket,
            Message::Text(event.text.clone().into()),
            "Relay EOG 이벤트 재전송 실패",
        )
        .await?;
    }
    Ok(events.len())
}

async fn replay_live_delta_range<S>(
    websocket: &SharedRelayWriter<S>,
    replay: &DurableReplayBuffer,
    stream_id: &str,
    from_seq: u64,
    to_seq: u64,
) -> AgentResult<usize>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let events = replay.live_range(stream_id, from_seq, to_seq);
    let expected = to_seq.saturating_sub(from_seq).saturating_add(1) as usize;
    if events.len() != expected {
        return Ok(0);
    }
    for event in &events {
        send_shared_relay_message(
            websocket,
            Message::Text(event.text.clone().into()),
            "live delta 재전송 실패",
        )
        .await?;
    }
    Ok(events.len())
}

fn relay_state_label(state: RelayConnectionState) -> &'static str {
    match state {
        RelayConnectionState::Stopped => "중지됨",
        RelayConnectionState::Connecting => "연결 중",
        RelayConnectionState::Authenticating => "인증 중",
        RelayConnectionState::Connected => "연결됨",
        RelayConnectionState::Reconnecting => "재연결 대기",
        RelayConnectionState::Failed => "실패",
    }
}

fn event_summary(message_type: &str, data: &Value) -> String {
    match message_type {
        "live_game_update"
            if data.get("format").and_then(Value::as_str) == Some("live-delta-v1") =>
        {
            format!(
                "live_game_update delta stream={} seq={} base_seq={} kind={} ops={}",
                data.get("stream_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
                data.get("seq").and_then(Value::as_u64).unwrap_or(0),
                data.get("base_seq").and_then(Value::as_u64).unwrap_or(0),
                data.get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
                data.get("ops")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
            )
        }
        "live_game_update" => format!(
            "live_game_update game_id={} participants={} events={} active_player={}",
            data.pointer("/game/id")
                .map(Value::to_string)
                .unwrap_or_else(|| "unknown".into()),
            data.get("participants")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            data.get("events")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            if data.get("active_player").is_some_and(|v| !v.is_null()) {
                "yes"
            } else {
                "no"
            }
        ),
        "gameflow_update" => format!(
            "gameflow_update phase={}",
            data.pointer("/phase")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        "match_eog" | "guild_match_eog" => format!(
            "{message_type} participants={} none_reason={}",
            data.get("participants")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            data.get("eog_none_reason")
                .and_then(Value::as_str)
                .unwrap_or("none")
        ),
        "match_rofl" => format!(
            "match_rofl game_id={} kind={} participants={}",
            data.get("gameId")
                .map(Value::to_string)
                .unwrap_or_else(|| "unknown".into()),
            data.get("kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
            data.get("participants")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        ),
        "ready_check_update" => format!(
            "ready_check_update active={}",
            data.get("active").and_then(Value::as_bool).unwrap_or(false)
        ),
        "champ_select_update" => format!(
            "champ_select_update active={} current_action={}",
            data.get("active").and_then(Value::as_bool).unwrap_or(false),
            data.get("current_action").is_some_and(|v| !v.is_null())
        ),
        "party_lobby_update" => format!(
            "party_lobby_update members={}",
            data.get("members")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        ),
        "participant_status_update" => format!(
            "participant_status_update status={} phase={}",
            data.get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
            data.get("phase")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        other => other.to_owned(),
    }
}

fn watchdog_restart_allowed(desired_running: bool, task_running: bool) -> bool {
    desired_running && !task_running
}

fn should_reconnect(manual_stop: bool) -> bool {
    !manual_stop
}

fn heartbeat_timed_out(awaiting_since: Option<Instant>, now: Instant) -> bool {
    awaiting_since.is_some_and(|started| now.duration_since(started) >= HEARTBEAT_TIMEOUT)
}

async fn send_shared_relay_message<S>(
    websocket: &SharedRelayWriter<S>,
    message: Message,
    failure: &'static str,
) -> AgentResult<()>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let kind = relay_send_kind(failure);
    let started_at = relay_progress_now_ms();
    RELAY_SEND_STARTED_COUNT.fetch_add(1, Ordering::AcqRel);
    RELAY_LAST_SEND_KIND.store(kind, Ordering::Release);
    RELAY_LAST_SEND_START_MS.store(started_at, Ordering::Release);

    // One logical send gets one timeout budget. Previously lock acquisition
    // could consume 10s and the actual socket write another 10s, allowing a
    // single message to block the shared writer for ~20s.
    let deadline = Instant::now() + RELAY_SEND_TIMEOUT;
    let lock_started = Instant::now();
    let mut guard = match tokio::time::timeout_at(deadline, websocket.lock()).await {
        Ok(guard) => {
            RELAY_SEND_LOCKED_COUNT.fetch_add(1, Ordering::AcqRel);
            RELAY_LAST_SEND_LOCKED_MS.store(relay_progress_now_ms(), Ordering::Release);
            guard
        }
        Err(_) => {
            RELAY_SEND_FAIL_COUNT.fetch_add(1, Ordering::AcqRel);
            RELAY_LAST_SEND_FAIL_MS.store(relay_progress_now_ms(), Ordering::Release);
            return Err(AgentError::Relay(
                format!(
                    "{failure}: writer lock timeout after {}ms trace={}",
                    lock_started.elapsed().as_millis(),
                    relay_trace_snapshot(),
                )
                .into(),
            ));
        }
    };
    let lock_wait_ms = lock_started.elapsed().as_millis();
    let send_started = Instant::now();
    match tokio::time::timeout_at(deadline, guard.send(message)).await {
        Ok(Ok(())) => {
            RELAY_SEND_OK_COUNT.fetch_add(1, Ordering::AcqRel);
            RELAY_LAST_SEND_OK_MS.store(relay_progress_now_ms(), Ordering::Release);
            touch_relay_network_progress();
            Ok(())
        }
        Ok(Err(error)) => {
            RELAY_SEND_FAIL_COUNT.fetch_add(1, Ordering::AcqRel);
            RELAY_LAST_SEND_FAIL_MS.store(relay_progress_now_ms(), Ordering::Release);
            Err(AgentError::Relay(
                format!(
                    "{failure}: websocket send error={error} lock_wait_ms={lock_wait_ms} send_ms={} trace={}",
                    send_started.elapsed().as_millis(),
                    relay_trace_snapshot(),
                )
                .into(),
            ))
        }
        Err(_) => {
            RELAY_SEND_FAIL_COUNT.fetch_add(1, Ordering::AcqRel);
            RELAY_LAST_SEND_FAIL_MS.store(relay_progress_now_ms(), Ordering::Release);
            Err(AgentError::Relay(
                format!(
                    "{failure}: send timeout lock_wait_ms={lock_wait_ms} send_ms={} trace={}",
                    send_started.elapsed().as_millis(),
                    relay_trace_snapshot(),
                )
                .into(),
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PresenceContextSendOutcome {
    Sent,
    WriterBusy,
    TimedOut,
}

async fn try_send_presence_context_message<S>(
    websocket: &SharedRelayWriter<S>,
    message: Message,
) -> AgentResult<PresenceContextSendOutcome>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    // Never queue behind another writer user. Presence refreshes are repeated
    // periodically, so a busy writer is a safe reason to drop this refresh.
    let Ok(mut guard) = websocket.try_lock() else {
        return Ok(PresenceContextSendOutcome::WriterBusy);
    };

    let kind = relay_send_kind("Discord Presence 내전 조회 요청 전송 실패");
    RELAY_SEND_STARTED_COUNT.fetch_add(1, Ordering::AcqRel);
    RELAY_LAST_SEND_KIND.store(kind, Ordering::Release);
    RELAY_LAST_SEND_START_MS.store(relay_progress_now_ms(), Ordering::Release);
    RELAY_SEND_LOCKED_COUNT.fetch_add(1, Ordering::AcqRel);
    RELAY_LAST_SEND_LOCKED_MS.store(relay_progress_now_ms(), Ordering::Release);

    match timeout(PRESENCE_CONTEXT_SEND_TIMEOUT, guard.send(message)).await {
        Ok(Ok(())) => {
            RELAY_SEND_OK_COUNT.fetch_add(1, Ordering::AcqRel);
            RELAY_LAST_SEND_OK_MS.store(relay_progress_now_ms(), Ordering::Release);
            touch_relay_network_progress();
            Ok(PresenceContextSendOutcome::Sent)
        }
        Ok(Err(error)) => {
            RELAY_SEND_FAIL_COUNT.fetch_add(1, Ordering::AcqRel);
            RELAY_LAST_SEND_FAIL_MS.store(relay_progress_now_ms(), Ordering::Release);
            Err(AgentError::Relay(
                format!(
                    "Discord Presence 내전 조회 요청 전송 실패: websocket send error={error} trace={}",
                    relay_trace_snapshot(),
                )
                .into(),
            ))
        }
        Err(_) => {
            // A Presence refresh timeout is not a connection-fatal condition.
            // Heartbeat owns connection-health decisions and will reconnect if
            // the socket is genuinely wedged.
            RELAY_SEND_FAIL_COUNT.fetch_add(1, Ordering::AcqRel);
            RELAY_LAST_SEND_FAIL_MS.store(relay_progress_now_ms(), Ordering::Release);
            Ok(PresenceContextSendOutcome::TimedOut)
        }
    }
}

async fn send_relay_message<S>(
    websocket: &mut S,
    message: Message,
    failure: &'static str,
) -> AgentResult<()>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    match timeout(RELAY_SEND_TIMEOUT, websocket.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(AgentError::Relay(
            format!("{failure}: websocket send error={error}").into(),
        )),
        Err(_) => Err(AgentError::Relay(format!("{failure}: send timeout").into())),
    }
}

fn should_start_login(session_bound: bool, login_opened: bool, elapsed: Duration) -> bool {
    !session_bound && !login_opened && elapsed >= SESSION_RESTORE_TIMEOUT
}

fn oauth_browser_should_open(user_initiated_login: bool, login_required: bool) -> bool {
    user_initiated_login && login_required
}

fn backoff_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(5);
    let base = Duration::from_secs((1_u64 << exponent).min(MAX_BACKOFF.as_secs()));
    let jitter_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_millis()
        % 500;
    (base + Duration::from_millis(u64::from(jitter_ms))).min(MAX_BACKOFF)
}

async fn collect_relay_network_diagnostic(
    state: Arc<AppState>,
    config: crate::config::Config,
    trigger: String,
) {
    let base = config.relay_public_base_url.clone();
    let host = url::Url::parse(&base)
        .ok()
        .and_then(|u| u.host_str().map(ToOwned::to_owned));
    let dns = if let Some(host) = host {
        let host_for_lookup = host.clone();
        match tokio::task::spawn_blocking(move || {
            use std::net::ToSocketAddrs;
            (host_for_lookup.as_str(), 443)
                .to_socket_addrs()
                .map(|iter| iter.take(4).map(|a| a.ip().to_string()).collect::<Vec<_>>())
        })
        .await
        {
            Ok(Ok(addrs)) if !addrs.is_empty() => format!("ok:{}", addrs.join(",")),
            Ok(Ok(_)) => "empty".into(),
            Ok(Err(error)) => format!("error:{error}"),
            Err(error) => format!("join_error:{error}"),
        }
    } else {
        "invalid_host".into()
    };

    let http_started = StdInstant::now();
    let https = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => match client.get(&base).send().await {
            Ok(response) => format!(
                "ok:status={} elapsed_ms={}",
                response.status().as_u16(),
                http_started.elapsed().as_millis()
            ),
            Err(error) => format!(
                "error:{} elapsed_ms={}",
                crate::http_diagnostics::transport_detail(&error),
                http_started.elapsed().as_millis()
            ),
        },
        Err(error) => format!("client_error:{error}"),
    };
    let detail = format!(
        "trigger={} dns={} https={} trace={}",
        trigger,
        dns,
        https,
        relay_trace_snapshot()
    );
    state
        .record_flight("relay_network_diagnostic", detail.clone())
        .await;
    state
        .report_diagnostic("relay_network", "failure_bundle", detail)
        .await;
}

async fn wait_for_retry(stop_rx: &mut watch::Receiver<bool>, delay: Duration) -> bool {
    tokio::select! {
        _ = sleep(delay) => true,
        changed = stop_rx.changed() => {
            changed.is_ok() && !*stop_rx.borrow()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_watchdog_does_not_exit_on_resume_before_runtime_ticks() {
        let mut observer = NativeWatchdogObservation::default();
        assert!(!observer.should_restart(10_000, 9_000, true));
        // Native thread wakes before Tokio after ten minutes of suspension.
        assert!(!observer.should_restart(610_000, 9_000, true));
        assert!(!observer.should_restart(615_000, 9_000, true));
        assert!(!observer.should_restart(620_000, 619_000, true));
        assert!(!observer.should_restart(625_000, 624_000, true));
    }

    #[test]
    fn native_watchdog_restarts_a_continuously_stalled_runtime() {
        let mut observer = NativeWatchdogObservation::default();
        for now in (5_000..110_000).step_by(5_000) {
            assert!(
                !observer.should_restart(now, 1, true),
                "early restart at {now}"
            );
        }
        assert!(observer.should_restart(110_000, 1, true));
    }

    #[test]
    fn native_watchdog_cancels_restart_when_runtime_recovers() {
        let mut observer = NativeWatchdogObservation::default();
        for now in (5_000..110_000).step_by(5_000) {
            assert!(!observer.should_restart(now, 1, true));
        }
        assert!(!observer.should_restart(110_000, 109_000, true));
    }

    #[test]
    fn native_watchdog_cancels_confirmation_after_native_thread_stall() {
        let mut observer = NativeWatchdogObservation::default();
        for now in (5_000..110_000).step_by(5_000) {
            assert!(!observer.should_restart(now, 1, true));
        }
        assert!(!observer.should_restart(300_000, 1, true));
        for now in [305_000, 310_000, 315_000] {
            assert!(!observer.should_restart(now, 1, true));
        }
        assert!(observer.should_restart(320_000, 1, true));
    }

    #[test]
    fn native_watchdog_never_restarts_stopped_or_uninitialized_agent() {
        for (armed, tick) in [(false, 1), (true, 0)] {
            let mut observer = NativeWatchdogObservation::default();
            for now in (5_000..300_000).step_by(5_000) {
                assert!(!observer.should_restart(now, tick, armed));
            }
        }
    }

    fn test_replay_buffer() -> DurableReplayBuffer {
        DurableReplayBuffer {
            pending: VecDeque::new(),
            live_pending_path: std::env::temp_dir().join(format!(
                "yummi-live-pending-test-{}.json",
                uuid::Uuid::new_v4()
            )),
            load_issue: None,
            needs_live_resync: false,
            persistence_dirty: false,
        }
    }

    #[test]
    fn corrupt_live_pending_is_preserved_and_forces_resync() {
        let dir = std::env::temp_dir().join(format!(
            "yummi-live-pending-corrupt-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("live-delta-pending.json");
        fs::write(&path, b"{not-json").unwrap();

        let mut replay = DurableReplayBuffer::load_from_path(path.clone());

        assert!(replay.pending.is_empty());
        assert!(replay.take_needs_live_resync());
        let issue = replay.take_load_issue().unwrap();
        assert!(issue.contains("pending load failed"));
        assert!(!path.exists());
        assert!(fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("live-delta-pending.corrupt-")));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn closed_oauth_channel_stays_pending_across_select_iterations() {
        let (sender, mut receiver) = mpsc::channel::<String>(1);
        drop(sender);

        for _ in 0..3 {
            tokio::select! {
                biased;
                _ = receive_oauth_code(&mut receiver) => {
                    panic!("closed OAuth channel must not wake the relay loop");
                }
                _ = std::future::ready(()) => {}
            }
        }
    }

    #[tokio::test]
    async fn oauth_code_queued_before_sender_closes_is_delivered() {
        let (sender, mut receiver) = mpsc::channel::<String>(1);
        sender.send("test-code".into()).await.unwrap();
        drop(sender);

        assert_eq!(receive_oauth_code(&mut receiver).await, "test-code");
        assert!(futures_util::poll!(Box::pin(receive_oauth_code(&mut receiver))).is_pending());
    }

    #[tokio::test]
    async fn cancelled_oauth_receive_preserves_the_next_code() {
        let (sender, mut receiver) = mpsc::channel::<String>(1);
        assert!(futures_util::poll!(Box::pin(receive_oauth_code(&mut receiver))).is_pending());

        sender.send("test-code".into()).await.unwrap();
        assert_eq!(receive_oauth_code(&mut receiver).await, "test-code");
    }

    #[tokio::test]
    async fn closed_oauth_channel_allows_heartbeat_and_shutdown() {
        let (sender, mut receiver) = mpsc::channel::<String>(1);
        drop(sender);
        let mut heartbeat = interval(Duration::from_secs(15));

        tokio::select! {
            biased;
            _ = receive_oauth_code(&mut receiver) => panic!("unexpected OAuth wakeup"),
            _ = heartbeat.tick() => {}
        }

        let (stop_tx, mut stop_rx) = watch::channel(false);
        stop_tx.send(true).unwrap();
        tokio::select! {
            biased;
            _ = receive_oauth_code(&mut receiver) => panic!("unexpected OAuth wakeup"),
            changed = stop_rx.changed() => {
                changed.unwrap();
                assert!(*stop_rx.borrow());
            }
        }
    }

    #[tokio::test]
    async fn presence_context_refresh_drops_immediately_when_writer_is_busy() {
        let writer = Arc::new(Mutex::new(futures_util::sink::drain::<Message>()));
        let _guard = writer.lock().await;

        let result =
            try_send_presence_context_message(&writer, Message::Text("presence-refresh".into()))
                .await
                .unwrap();

        assert_eq!(result, PresenceContextSendOutcome::WriterBusy);
    }

    #[test]
    fn relay_progress_watchdog_uses_stale_threshold() {
        assert!(!relay_progress_is_stale(
            RELAY_PROGRESS_STALE_AFTER - Duration::from_millis(1)
        ));
        assert!(relay_progress_is_stale(RELAY_PROGRESS_STALE_AFTER));
        assert!(relay_progress_is_stale(
            RELAY_PROGRESS_STALE_AFTER + Duration::from_secs(30)
        ));
    }

    #[test]
    fn relay_watchdog_only_restarts_when_running_is_still_desired() {
        assert!(watchdog_restart_allowed(true, false));
        assert!(!watchdog_restart_allowed(true, true));
        assert!(!watchdog_restart_allowed(false, false));
        assert!(!watchdog_restart_allowed(false, true));
    }

    #[test]
    fn relay_close_reconnects_unless_manually_stopped() {
        assert!(should_reconnect(false));
        assert!(!should_reconnect(true));
    }

    #[test]
    fn session_conflict_close_is_terminal_even_before_binding() {
        for authenticated in [false, true] {
            assert_eq!(
                ConnectionEnd::for_close(Some(4009), authenticated),
                ConnectionEnd::SessionConflict,
            );
            for code in [None, Some(1000), Some(1001), Some(1008), Some(1011)] {
                assert_eq!(
                    ConnectionEnd::for_close(code, authenticated),
                    ConnectionEnd::Closed { authenticated },
                );
            }
        }
    }

    #[tokio::test]
    async fn session_conflict_disables_watchdog_until_explicit_start() {
        let supervisor = RelaySupervisor::new();
        {
            let mut control = supervisor.control.lock().await;
            control.generation = 2;
            control.desired_running = true;
        }
        // A late result from an older connection must not stop a newer one.
        assert!(!supervisor.suspend_for_session_conflict(1).await);
        assert!(supervisor.control.lock().await.desired_running);
        assert!(supervisor.suspend_for_session_conflict(2).await);
        let control = supervisor.control.lock().await;
        assert!(!watchdog_restart_allowed(control.desired_running, false));
    }

    #[test]
    fn heartbeat_timeout_uses_last_unanswered_ping() {
        let now = Instant::now();
        assert!(!heartbeat_timed_out(None, now));
        assert!(!heartbeat_timed_out(
            Some(now - HEARTBEAT_TIMEOUT + Duration::from_millis(1)),
            now
        ));
        assert!(heartbeat_timed_out(Some(now - HEARTBEAT_TIMEOUT), now));
    }

    #[test]
    fn oauth_browser_opens_only_for_explicit_user_action() {
        assert!(oauth_browser_should_open(true, true));
        assert!(!oauth_browser_should_open(false, true));
        assert!(!oauth_browser_should_open(true, false));
        assert!(!oauth_browser_should_open(false, false));
    }

    #[test]
    fn expired_saved_session_falls_back_to_login_once() {
        assert!(!should_start_login(false, false, Duration::from_secs(4)));
        assert!(should_start_login(false, false, SESSION_RESTORE_TIMEOUT));
        assert!(!should_start_login(false, true, Duration::from_secs(30)));
        assert!(!should_start_login(true, false, Duration::from_secs(30)));
    }

    #[test]
    fn reconnect_backoff_is_capped() {
        assert!(backoff_delay(1) <= Duration::from_millis(1_500));
        assert!(backoff_delay(20) <= MAX_BACKOFF);
    }

    #[test]
    fn commands_require_session_binding() {
        assert!(!command_is_authorized(false, false, Some(Action::Ping)));
        assert!(command_is_authorized(true, false, None));
        assert!(command_is_authorized(true, true, Some(Action::AcceptMatch)));
        assert!(!command_is_authorized(true, true, Some(Action::QueueStart)));
        assert!(!command_is_authorized(true, true, None));
    }

    #[test]
    fn relay_avatar_is_limited_to_discord_cdn() {
        assert!(safe_avatar_url(Some("file:///C:/secret.png".into())).is_none());
        assert!(safe_avatar_url(Some("http://cdn.discordapp.com/avatar.png".into())).is_none());
        assert!(safe_avatar_url(Some("https://cdn.example/avatar.png".into())).is_none());
        assert_eq!(
            safe_avatar_url(Some("https://cdn.discordapp.com/avatar.png".into())).as_deref(),
            Some("https://cdn.discordapp.com/avatar.png")
        );
    }

    #[tokio::test]
    async fn durable_replay_buffer_deduplicates_and_acks() {
        let mut replay = test_replay_buffer();
        let event = SerializedAgentEvent {
            event_id: "event-1".into(),
            text: "payload".into(),
            live_stream_id: None,
            live_seq: None,
        };
        assert!(!replay.track(event.clone()).await.unwrap());
        assert!(!replay.track(event).await.unwrap());
        assert_eq!(replay.snapshot().len(), 1);
        assert!(replay.ack("event-1").await.unwrap());
        assert!(replay.snapshot().is_empty());
    }

    #[tokio::test]
    async fn pending_replay_summary_identifies_event_without_logging_payload() {
        let mut replay = test_replay_buffer();
        let event = serialize_agent_event(
            "match_rofl",
            serde_json::json!({
                "gameId": 123,
                "kind": "summary",
                "participants": [],
                "token": "private-value",
            }),
        )
        .unwrap()
        .unwrap();
        replay.track(event).await.unwrap();

        let summary = replay.pending_summary();
        assert!(summary.contains("match_rofl game_id=123 kind=summary participants=0"));
        assert!(summary.contains("event_id="));
        assert!(!summary.contains("private-value"));
    }

    #[tokio::test]
    async fn durable_replay_selects_requested_live_sequence_range() {
        let mut replay = test_replay_buffer();
        for seq in 20..=22 {
            assert!(!replay
                .track(SerializedAgentEvent {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    text: format!("payload-{seq}"),
                    live_stream_id: Some("123e4567-e89b-42d3-a456-426614174000".into()),
                    live_seq: Some(seq),
                })
                .await
                .unwrap());
        }
        let events = replay.live_range("123e4567-e89b-42d3-a456-426614174000", 20, 21);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].live_seq, Some(20));
        assert_eq!(events[1].live_seq, Some(21));
        assert!(replay.live_pending_path.exists());
        let persisted: Vec<SerializedAgentEvent> =
            serde_json::from_slice(&fs::read(&replay.live_pending_path).unwrap()).unwrap();
        assert_eq!(persisted.len(), 3);

        // A failed ACK persistence must retain dirty state and be retryable.
        let original_path = replay.live_pending_path.clone();
        replay.live_pending_path = original_path.join("invalid-child.json");
        assert!(replay.ack(&events[0].event_id).await.is_err());
        assert!(replay.persistence_dirty);
        replay.live_pending_path = original_path;
        replay.persist_live_pending().await.unwrap();
        assert!(!replay.persistence_dirty);
        let persisted: Vec<SerializedAgentEvent> =
            serde_json::from_slice(&fs::read(&replay.live_pending_path).unwrap()).unwrap();
        assert_eq!(persisted.len(), 2);
        assert_eq!(
            replay
                .drop_live_stream("123e4567-e89b-42d3-a456-426614174000")
                .await
                .unwrap(),
            2
        );
        assert!(replay
            .live_range("123e4567-e89b-42d3-a456-426614174000", 20, 22)
            .is_empty());
        let _ = fs::remove_file(&replay.live_pending_path);
    }

    #[test]
    fn replay_cleanup_requires_explicit_owned_file_metadata() {
        let eligible = replay_upload_request(&json!({
            "gameId": "8393991955",
            "path": "C:/Users/test/Documents/League of Legends/Replays/KR-8393991955.rofl",
            "fileName": "KR-8393991955.rofl",
            "fileSize": 16_210_841,
            "fileModifiedAtMs": 1_790_000_000_000_u64,
            "deleteAfterUpload": true,
        }))
        .expect("valid replay upload");
        assert!(eligible.delete_after_upload);
        assert_eq!(eligible.file_modified_at_ms, Some(1_790_000_000_000));

        let preexisting = replay_upload_request(&json!({
            "gameId": "8393991955",
            "path": "C:/Users/test/Documents/League of Legends/Replays/KR-8393991955.rofl",
            "fileName": "KR-8393991955.rofl",
            "fileSize": 16_210_841,
            "fileModifiedAtMs": 1_790_000_000_000_u64,
            "deleteAfterUpload": false,
        }))
        .expect("valid replay upload");
        assert!(!preexisting.delete_after_upload);

        let unrelated_path = replay_upload_request(&json!({
            "gameId": "8393991955",
            "path": "C:/Users/test/Documents/League of Legends/Replays/manual-copy.rofl",
            "fileName": "manual-copy.rofl",
            "fileSize": 16_210_841,
            "fileModifiedAtMs": 1_790_000_000_000_u64,
            "deleteAfterUpload": true,
        }))
        .expect("valid replay upload");
        assert!(!unrelated_path.delete_after_upload);

        let missing_mtime = replay_upload_request(&json!({
            "gameId": "8393991955",
            "path": "C:/Users/test/Documents/League of Legends/Replays/KR-8393991955.rofl",
            "fileName": "KR-8393991955.rofl",
            "fileSize": 16_210_841,
            "deleteAfterUpload": true,
        }))
        .expect("valid replay upload");
        assert!(!missing_mtime.delete_after_upload);
    }

    #[test]
    fn oversized_lcu_event_is_not_forwarded_to_relay() {
        let data = json!({"value": "x".repeat(MAX_RELAY_MESSAGE_BYTES)});
        assert!(serialize_agent_event("gameflow_update", data)
            .unwrap()
            .is_none());
    }
}
