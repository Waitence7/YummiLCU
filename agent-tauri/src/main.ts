import './style.css';

import {
  loadConfig,
  recentMatch,
  relogin,
  saveConfig,
  startAgent,
  stopAgent,
  submitOAuthCode,
} from './api/commands';
import { listenToAgentState } from './events/agent-state';
import { createStore } from './state/store';
import { initialState, type RecentMatch } from './state/types';
import { mountApp } from './views/app';

const root = document.querySelector<HTMLElement>('#app');
if (!root) throw new Error('Missing #app');

const store = createStore(initialState);
let recent: RecentMatch | undefined;
let recentRequest = 0;

const addLog = (message: string) => {
  store.update((state) => ({
    ...state,
    logs: [...state.logs.slice(-299), message],
  }));
};

const execute = async (operation: () => Promise<unknown>): Promise<boolean> => {
  try {
    await operation();
    return true;
  } catch (error) {
    addLog(String(error));
    return false;
  }
};

const view = mountApp(root, {
  async start(config) {
    const saved = await execute(() => saveConfig(config));
    return saved && execute(startAgent);
  },
  async stop() {
    await execute(stopAgent);
  },
  async relogin() {
    await execute(relogin);
  },
  async submitOAuth(code) {
    await execute(() => submitOAuthCode(code));
  },
  async refreshRecent() {
    const request = ++recentRequest;
    try {
      const result = await recentMatch();
      if (request !== recentRequest) return;
      recent = result;
      view.render(store.get(), recent);
    } catch (error) {
      if (request === recentRequest) addLog(String(error));
    }
  },
});

store.subscribe((state) => view.render(state, recent));

void listenToAgentState((state) => store.set(state)).catch((error) => addLog(String(error)));

window.addEventListener('DOMContentLoaded', async () => {
  try {
    const config = await loadConfig();
    store.update((state) => ({ ...state, config }));
  } catch (error) {
    addLog(String(error));
  }
});

window.addEventListener('tauri://app-ready', () => undefined);
