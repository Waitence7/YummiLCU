import './style.css';

import {
  getAgentState,
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

let unlistenAgentState: (() => void) | undefined;
const initialize = async () => {
  try {
    unlistenAgentState = await listenToAgentState((state) => store.set(state));
    const snapshot = await getAgentState();
    store.set(snapshot);
    if (snapshot.lcu) {
      const request = ++recentRequest;
      try {
        const result = await recentMatch();
        if (request === recentRequest) {
          recent = result;
          view.render(store.get(), recent);
        }
      } catch {
        // State restoration is best-effort while the League Client reconnects.
      }
    }
  } catch (error) {
    addLog(String(error));
  }
};

void initialize();

window.addEventListener('beforeunload', () => unlistenAgentState?.(), { once: true });

window.addEventListener('tauri://app-ready', () => undefined);
