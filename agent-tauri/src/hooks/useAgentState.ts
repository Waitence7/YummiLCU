import { useCallback, useEffect, useRef, useState } from 'react';

import * as api from '../api/commands';
import { listenToAgentState } from '../events/agent-state';
import { initialState, type AgentState, type Config, type RecentMatch } from '../state/types';

export type RecentState = {
  match: RecentMatch | null;
  loading: boolean;
  error: string | null;
};

export type AgentActions = {
  start(): Promise<boolean>;
  stop(): Promise<boolean>;
  relogin(): Promise<boolean>;
  submitOAuth(code: string): Promise<boolean>;
  saveConfig(config: Config): Promise<boolean>;
  patchConfig(patch: Partial<Config>): Promise<boolean>;
  refreshRecent(): Promise<void>;
};

export function useAgentState(): {
  state: AgentState;
  recent: RecentState;
  actions: AgentActions;
} {
  const [state, setState] = useState<AgentState>(initialState);
  const [recent, setRecent] = useState<RecentState>({
    match: null,
    loading: false,
    error: null,
  });
  const recentRequest = useRef(0);
  const stateRef = useRef(state);
  stateRef.current = state;

  const addLog = useCallback((message: string) => {
    setState((current) => ({
      ...current,
      logs: [...current.logs.slice(-199), message],
    }));
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listenToAgentState((next) => {
      if (!cancelled) setState(next);
    })
      .then((dispose) => {
        if (cancelled) dispose();
        else unlisten = dispose;
      })
      .catch((error) => addLog(String(error)));
    api
      .getAgentState()
      .then((snapshot) => {
        if (!cancelled) setState(snapshot);
      })
      .catch(() => {
        api
          .loadConfig()
          .then((config) => {
            if (!cancelled) setState((current) => ({ ...current, config }));
          })
          .catch((error) => addLog(String(error)));
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [addLog]);

  const run = useCallback(
    async (operation: () => Promise<unknown>): Promise<boolean> => {
      try {
        await operation();
        return true;
      } catch (error) {
        addLog(String(error));
        return false;
      }
    },
    [addLog],
  );

  const saveConfig = useCallback(
    (config: Config) => run(() => api.saveConfig(config)),
    [run],
  );

  const refreshRecent = useCallback(async () => {
    const request = ++recentRequest.current;
    setRecent((current) => ({ ...current, loading: true }));
    try {
      const match = await api.recentMatch();
      if (request !== recentRequest.current) return;
      setRecent({ match, loading: false, error: null });
    } catch (error) {
      if (request !== recentRequest.current) return;
      setRecent((current) => ({
        ...current,
        loading: false,
        error: String(error),
      }));
    }
  }, []);

  const actions: AgentActions = {
    start: useCallback(() => run(api.startAgent), [run]),
    stop: useCallback(() => run(api.stopAgent), [run]),
    relogin: useCallback(() => run(api.relogin), [run]),
    submitOAuth: useCallback(
      (code: string) => run(() => api.submitOAuthCode(code)),
      [run],
    ),
    saveConfig,
    patchConfig: useCallback(
      (patch: Partial<Config>) => saveConfig({ ...stateRef.current.config, ...patch }),
      [saveConfig],
    ),
    refreshRecent,
  };

  return { state, recent, actions };
}
