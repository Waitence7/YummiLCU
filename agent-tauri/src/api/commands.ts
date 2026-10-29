import type {
  AgentState,
  BetaReleaseInfo,
  Config,
  RecentMatch,
  RecordingStatus,
  RecordingCaptureSource,
  RecordingGameContext,
  RecordingSession,
  OverlayInstallStatus,
} from "../state/types";
import type {
  MatchFightAnalysis,
  MatchHistory,
  MatchReplay,
} from "../state/matches";

/**
 * 브라우저(비 Tauri) 환경에서는 목 브리지로 대체해 UI를 확인할 수 있게 한다.
 * 프로덕션 빌드는 VITE_UI_PREVIEW=1 로 빌드한 프리뷰 번들에서만 목이 포함된다.
 */
export const useMockBridge =
  (import.meta.env.DEV || import.meta.env.VITE_UI_PREVIEW === "1") &&
  typeof window !== "undefined" &&
  !("__TAURI_INTERNALS__" in window);

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (useMockBridge) {
    const { mockInvoke } = await import("./mock");
    return mockInvoke<T>(command, args);
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

export const getAgentState = () => call<AgentState>("get_agent_state");
export const loadConfig = () => call<Config>("load_config");
export const saveConfig = (config: Config) =>
  call<void>("save_config", { config });
export const startAgent = () => call<void>("start_agent");
export const stopAgent = () => call<void>("stop_agent");
export const logout = () => call<void>("logout");
export const recentMatch = () => call<RecentMatch>("recent_match");
export const matchHistory = (offset = 0, riotId?: string) =>
  call<MatchHistory>("match_history", { offset, riotId });
export const matchFights = (gameId: string) =>
  call<MatchFightAnalysis>("match_fights", { gameId });
export const matchReplay = (gameId: string) =>
  call<MatchReplay>("match_replay", { gameId });
export const matchReplayRemote = (gameId: string) =>
  call<MatchReplay>("match_replay_remote", { gameId });
export const matchReplayAnalysis = (gameId: string, jobId: string) =>
  call<MatchReplay>("match_replay_analysis", { gameId, jobId });
export const downloadMatchReplay = (gameId: string, gameVersion?: string) =>
  call<void>("download_match_replay", { gameId, gameVersion });
export const hideMainWindow = () => call<void>("hide_main_window");
export const completeTrayHide = () => call<void>("complete_tray_hide");
export const minimizeMainWindow = () => call<void>("minimize_main_window");
export const requestTrayHide = () => call<void>("request_tray_hide");
export const startMainWindowDrag = () =>
  useMockBridge ? Promise.resolve() : call<void>("start_main_window_drag");
export const windowUiHeartbeat = () => call<void>("window_ui_heartbeat");
export const showWindowGlideHint = () => call<void>("show_window_glide_hint");
export const closeWindowGlideHint = () => call<void>("close_window_glide_hint");
export const setWindowGlideStrength = (strength: number | null) =>
  call<void>("set_window_glide_strength", { strength });
export const syncMainWindowRotationMode = () =>
  useMockBridge
    ? Promise.resolve()
    : call<void>("sync_main_window_rotation_mode");
export const freezeMainWindowMotion = () =>
  useMockBridge ? Promise.resolve() : call<void>("freeze_main_window_motion");
export const stabilizeMainWindowRotation = () =>
  useMockBridge
    ? Promise.resolve()
    : call<void>("stabilize_main_window_rotation");
export const getBetaReleaseInfo = () =>
  call<BetaReleaseInfo>("get_beta_release_info");
export const openBetaDownload = () => call<void>("open_beta_download");
export const applyBetaUpdateNow = () => call<void>("apply_beta_update_now");
export const getOverlayInstallStatus = () =>
  call<OverlayInstallStatus>("get_overlay_install_status");
export const installOverlay = () =>
  call<OverlayInstallStatus>("install_overlay");

export const getDiagnosticBundle = () => call<string>("get_diagnostic_bundle");
export const exportDiagnosticBundle = () =>
  call<string>("export_diagnostic_bundle");
export const reportUnexpectedError = (
  code:
    | "uncaught_error"
    | "unhandled_rejection"
    | "window_command_failed"
    | "ui_operation_failed",
  summary: string,
) => call<void>("report_unexpected_error", { code, summary });

function reportOperationFailure(
  code: "window_command_failed" | "ui_operation_failed",
  operation: string,
  error: unknown,
): Promise<void> {
  const summary = `${operation}: ${String(error)}`;
  console.warn(
    `[${code === "window_command_failed" ? "window" : "ui"}]`,
    summary,
  );
  return reportUnexpectedError(code, summary).catch((reportError) => {
    console.warn("[diagnostic] 오류 보고에 실패했습니다.", reportError);
  });
}

export const reportWindowFailure = (operation: string, error: unknown) =>
  reportOperationFailure("window_command_failed", operation, error);
export const reportUiFailure = (operation: string, error: unknown) =>
  reportOperationFailure("ui_operation_failed", operation, error);
export const reportTrayEffectDiagnostic = (code: string, detail: string) =>
  call<void>("report_tray_effect_diagnostic", { code, detail });
export const reportRecordingDiagnostic = (code: string, detail: string) =>
  call<void>("report_recording_diagnostic", { code, detail });

export const getRecordingStatus = () => call<RecordingStatus>("get_recording_status");
export const listRecordingSessions = () => call<RecordingSession[]>("list_recording_sessions");
export const recordingGameContext = (gameId: string) =>
  call<RecordingGameContext>("recording_game_context", { gameId });
export const requestBorderlessCaptureAccess = () => call<RecordingStatus>("request_borderless_capture_access");
export const startRecording = (source: RecordingCaptureSource) =>
  call<RecordingStatus>("start_recording", { source });
export const stopRecording = () => call<RecordingStatus>("stop_recording");
export const startClipBuffer = (source: RecordingCaptureSource) =>
  call<RecordingStatus>("start_clip_buffer", { source });
export const stopClipBuffer = () => call<RecordingStatus>("stop_clip_buffer");
export const saveLastClip = (seconds: number) =>
  call<RecordingStatus>("save_last_clip", { seconds });
