import type { AgentState, Config, RecentMatch } from '../state/types';

/**
 * 브라우저(비 Tauri) 환경에서는 목 브리지로 대체해 UI를 확인할 수 있게 한다.
 * 프로덕션 빌드는 VITE_UI_PREVIEW=1 로 빌드한 프리뷰 번들에서만 목이 포함된다.
 */
export const useMockBridge =
  (import.meta.env.DEV || import.meta.env.VITE_UI_PREVIEW === '1') &&
  typeof window !== 'undefined' &&
  !('__TAURI_INTERNALS__' in window);

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (useMockBridge) {
    const { mockInvoke } = await import('./mock');
    return mockInvoke<T>(command, args);
  }
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

export const getAgentState = () => call<AgentState>('get_agent_state');
export const loadConfig = () => call<Config>('load_config');
export const saveConfig = (config: Config) => call<void>('save_config', { config });
export const startAgent = () => call<void>('start_agent');
export const stopAgent = () => call<void>('stop_agent');
export const relogin = () => call<void>('relogin');
export const submitOAuthCode = (code: string) => call<void>('submit_oauth_code', { code });
export const recentMatch = () => call<RecentMatch>('recent_match');

export const getDiagnosticBundle = () => call<string>('get_diagnostic_bundle');
export const exportDiagnosticBundle = () => call<string>('export_diagnostic_bundle');
