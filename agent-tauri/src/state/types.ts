
export type OverlayControlsStatus = {
  supported: boolean;
  gameBarFound: boolean;
  yummiSelected: boolean;
  pin: boolean | null;
  clickThrough: boolean | null;
  pinSource?: 'uia' | 'widget_last_report' | null;
  clickThroughSource?: 'uia' | 'widget_last_report' | null;
  widgetReportAgeSecs?: number | null;
  pinPreference: boolean | null;
  clickThroughPreference: boolean | null;
  selectionInvoked: boolean;
  message?: string | null;
};

export type OverlayInstallStatus = {
  supported: boolean;
  installed: boolean;
  installedVersion?: string | null;
  updaterReady: boolean;
};

export type BetaReleaseInfo = {
  version: string;
  releaseLabel: string;
  buildId: string;
  commit: string;
  installerUrl: string;
};

export type TrayHideEffect =
  | 'fold'
  | 'jelly'
  | 'pixels'
  | 'cat'
  | 'glass'
  | 'swirl'
  | 'suction'
  | 'page-curl'
  | 'book-return'
  | 'book-return-v2'
  | 'curtain'
  | 'shards'
  | 'fade'
  | 'none';

export type Config = {
  ConfigSchemaVersion: number;
  RelayPublicBaseUrl: string;
  AuthPollIntervalMs: number;
  LockfilePath?: string | null;
  PreventQueueAfterDodge: boolean;
  ApplyDefaultStatusOnConnect: boolean;
  AutoAcceptMatch: boolean;
  FollowLeagueClient: boolean;
  UpdateManifestUrl?: string | null;
  CheckUpdatesOnStartup: boolean;
  AutoUpdateEnabled: boolean;
  UpdateChannel: 'stable' | 'beta' | 'dev';
  SavedSessionMaxAgeDays: number;
  RunAtWindowsStartup: boolean;
  TrayHideEffect: TrayHideEffect;
  TrayEffectPlaybackRate: number;
  WindowGlideStrength: number | null;
  WindowFreeRotation: boolean;
  AutoRecordLeagueGames: boolean;
  OverlayPinPreference?: boolean | null;
  OverlayClickThroughPreference?: boolean | null;
  UiTestMode: boolean;
};

export type AgentState = {
  status: string;
  relay: boolean;
  lcu: boolean;
  discord_id?: string | null;
  discord_name?: string | null;
  discord_avatar?: string | null;
  trust_level?: 'unauthenticated' | 'new_device' | 'trusted' | string;
  logs: string[];
  oauth_pending: boolean;
  update_message?: string | null;
  app_version?: string | null;
  release_label?: string | null;
  release_channel?: 'stable' | 'beta' | 'dev' | string | null;
  build_id?: string | null;
  git_commit?: string | null;
  downloaded_at?: number | null;
  config: Config;
};


export type LiveQualityProfile = 'data' | 'balanced' | 'high' | 'ultra';
export type LiveFrameRate = 15 | 30 | 60;

export type LiveControlStatus = {
  enabled: boolean;
  profile: LiveQualityProfile;
  fps: LiveFrameRate;
  shareUrl?: string | null;
  expiresAt?: number | null;
};

export type RecordingCaptureSource = 'monitor' | 'leagueWindow';

export type RecordingStatus = {
  supported: boolean;
  recording: boolean;
  clipBuffer: boolean;
  recordingPath?: string | null;
  bufferedSeconds: number;
  lastClipPath?: string | null;
  lastError?: string | null;
  videoOnly: boolean;
  recordingSource: RecordingCaptureSource;
  clipSource: RecordingCaptureSource;
  borderlessCaptureRequested: boolean;
  borderlessCaptureAllowed: boolean;
  borderlessCaptureStatus: string;
  borderlessCaptureDetail?: string | null;
  autoGameRecording: boolean;
  autoGameId?: string | null;
  activeRecordingMetadataPath?: string | null;
};

export type RecordingSession = {
  schema: string;
  gameId?: string | null;
  riotId?: string | null;
  videoPath: string;
  source: RecordingCaptureSource | string;
  automatic: boolean;
  startedAtMs: number;
  endedAtMs?: number | null;
  gameClockOffsetMs: number;
  roflPath?: string | null;
};

export type RecordingPotg = {
  gameId: string;
  playerName: string;
  championName: string;
  startMs: number;
  endMs: number;
  kills: number;
  score: number;
  location?: string | null;
};

export type RecordingGameContext = {
  gameId: string;
  potg?: RecordingPotg | null;
};

export type RecentMatch = {
  champion: string | number;
  champion_id: number;
  win: boolean;
  kills?: number | null;
  deaths?: number | null;
  assists?: number | null;
  cs?: number | null;
  gold?: number | null;
  items: unknown[];
  duration?: number | null;
  created_at?: number | null;
};

export const initialState: AgentState = {
  status: '연결 시작 → Discord 로그인',
  relay: false,
  lcu: false,
  trust_level: 'unauthenticated',
  logs: [],
  oauth_pending: false,
  update_message: null,
  config: {
    ConfigSchemaVersion: 1,
    RelayPublicBaseUrl: 'https://yummi.duckdns.org',
    AuthPollIntervalMs: 1500,
    PreventQueueAfterDodge: true,
    ApplyDefaultStatusOnConnect: true,
    AutoAcceptMatch: false,
    FollowLeagueClient: true,
    UpdateManifestUrl: 'https://yummi.duckdns.org/agent/version.json',
    CheckUpdatesOnStartup: true,
    AutoUpdateEnabled: true,
    UpdateChannel: 'stable',
    SavedSessionMaxAgeDays: 0,
    RunAtWindowsStartup: true,
    TrayHideEffect: 'book-return-v2',
    TrayEffectPlaybackRate: 1,
    WindowGlideStrength: 1,
    WindowFreeRotation: false,
    AutoRecordLeagueGames: false,
    OverlayPinPreference: null,
    OverlayClickThroughPreference: null,
    UiTestMode: false,
  },
};
