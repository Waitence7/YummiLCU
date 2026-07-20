export type Config = {
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
  SavedSessionMaxAgeDays: number;
  RunAtWindowsStartup: boolean;
  UiTestMode: boolean;
};

export type AgentState = {
  status: string;
  relay: boolean;
  lcu: boolean;
  discord_id?: number | null;
  discord_name?: string | null;
  discord_avatar?: string | null;
  logs: string[];
  oauth_pending: boolean;
  update_message?: string | null;
  app_version?: string | null;
  downloaded_at?: number | null;
  config: Config;
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
  logs: [],
  oauth_pending: false,
  update_message: null,
  config: {
    RelayPublicBaseUrl: 'https://yummi.duckdns.org',
    AuthPollIntervalMs: 1500,
    PreventQueueAfterDodge: true,
    ApplyDefaultStatusOnConnect: true,
    AutoAcceptMatch: false,
    FollowLeagueClient: true,
    UpdateManifestUrl: 'https://yummi.duckdns.org/agent/version.json',
    CheckUpdatesOnStartup: true,
    AutoUpdateEnabled: true,
    SavedSessionMaxAgeDays: 14,
    RunAtWindowsStartup: false,
    UiTestMode: false,
  },
};
