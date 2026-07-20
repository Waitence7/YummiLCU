import { invoke } from '@tauri-apps/api/core';
import type { Config, RecentMatch } from '../state/types';

export const loadConfig = () => invoke<Config>('load_config');
export const saveConfig = (config: Config) => invoke<void>('save_config', { config });
export const startAgent = () => invoke<void>('start_agent');
export const stopAgent = () => invoke<void>('stop_agent');
export const relogin = () => invoke<void>('relogin');
export const submitOAuthCode = (code: string) =>
  invoke<void>('submit_oauth_code', { code });
export const recentMatch = () => invoke<RecentMatch>('recent_match');
