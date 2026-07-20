import type { AgentState, Config, RecentMatch } from '../state/types';

type AppActions = {
  start(config: Config): Promise<boolean>;
  stop(): Promise<void>;
  relogin(): Promise<void>;
  submitOAuth(code: string): Promise<void>;
  refreshRecent(): Promise<void>;
};

export type AppView = {
  render(state: AgentState, recent?: RecentMatch): void;
};

export function mountApp(root: HTMLElement, actions: AppActions): AppView {
  root.innerHTML = `<section class="shell">
    <header><div><h1>Yummi LCU Agent</h1><p id="status-text"></p></div><div class="account-area" id="account"></div></header>
    <div class="buttons"><button id="start">연결 시작</button><button id="stop">중지</button><button id="relogin">Discord 재로그인</button></div>
    <div class="status"><span id="relay-dot" class="dot"></span> Relay <span id="lcu-dot" class="dot"></span> LCU <span id="discord-id" class="discord"></span></div>
    <div class="recent"><b>최근 경기</b> <span id="recent-content"></span><button id="recent">새로고침</button></div>
    <div id="oauth-panel" class="oauth" hidden><b>브라우저에 표시된 6자리 코드를 입력하세요.</b><input id="oauth" maxlength="6"/><button id="submit-oauth">코드 확인</button></div>
    <div id="update-panel" class="status" role="status" hidden></div>
    <fieldset><legend>설정</legend><label><input data-config type="checkbox" id="dodge"/> 닷지 후 매칭 자동 재시작 방지</label><label><input data-config type="checkbox" id="status"/> 연결 시 기본 상메 적용</label><label><input data-config type="checkbox" id="accept"/> 매치 자동 수락</label><label><input data-config type="checkbox" id="startup"/> Windows 시작 시 자동 실행</label><label><input data-config type="checkbox" id="auto-update"/> 시작 시 자동 업데이트 설치 (권장)</label></fieldset>
    <details class="advanced"><summary>고급 설정</summary><label>Relay URL<input data-config id="relay"/></label><label>League lockfile 경로<input data-config id="lockfile" placeholder="자동 감지 (기본값)"/></label><small>일반적으로 변경할 필요가 없습니다.</small></details>
    <fieldset class="log"><legend>로그</legend><pre id="logs"></pre></fieldset>
    <footer>v<span id="app-version">—</span> · 다운로드 <span id="downloaded-at">—</span></footer>
  </section>`;

  const statusText = required<HTMLElement>(root, '#status-text');
  const account = required<HTMLElement>(root, '#account');
  const relayDot = required<HTMLElement>(root, '#relay-dot');
  const lcuDot = required<HTMLElement>(root, '#lcu-dot');
  const discordId = required<HTMLElement>(root, '#discord-id');
  const recentContent = required<HTMLElement>(root, '#recent-content');
  const oauthPanel = required<HTMLElement>(root, '#oauth-panel');
  const updatePanel = required<HTMLElement>(root, '#update-panel');
  const logs = required<HTMLElement>(root, '#logs');
  const appVersion = required<HTMLElement>(root, '#app-version');
  const downloadedAt = required<HTMLElement>(root, '#downloaded-at');
  let currentConfig: Config;
  let configDirty = false;

  root.addEventListener('input', (event) => {
    if ((event.target as HTMLElement).matches('[data-config]')) configDirty = true;
  });

  root.addEventListener('click', async (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('button');
    if (!button) return;
    switch (button.id) {
      case 'start':
        if (await actions.start(readConfig(root, currentConfig))) configDirty = false;
        break;
      case 'stop':
        await actions.stop();
        break;
      case 'relogin':
        await actions.relogin();
        break;
      case 'submit-oauth':
        await actions.submitOAuth(required<HTMLInputElement>(root, '#oauth').value);
        break;
      case 'recent':
        await actions.refreshRecent();
        break;
    }
  });

  return {
    render(state, recent) {
      currentConfig = state.config;
      statusText.textContent = state.status;
      renderAccount(account, state);
      relayDot.classList.toggle('on', state.relay);
      lcuDot.classList.toggle('on', state.lcu);
      discordId.textContent = `Discord: ${state.discord_id ?? '—'}`;
      recentContent.textContent = recent ? recentText(recent) : 'League Client 연결 후 확인할 수 있습니다.';
      oauthPanel.hidden = !state.oauth_pending;
      updatePanel.hidden = !state.update_message;
      updatePanel.textContent = state.update_message ?? '';
      logs.textContent = state.logs.join('\n');
      appVersion.textContent = state.app_version || '—';
      downloadedAt.textContent = state.downloaded_at
        ? new Date(state.downloaded_at * 1000).toLocaleString('ko-KR')
        : '—';
      if (!configDirty) writeConfig(root, state.config);
    },
  };
}

function renderAccount(element: HTMLElement, state: AgentState) {
  element.replaceChildren();
  const container = document.createElement('div');
  container.className = state.discord_id ? 'account' : 'account guest';
  if (state.discord_id) {
    const image = document.createElement('img');
    image.src =
      state.discord_avatar ||
      `https://cdn.discordapp.com/embed/avatars/${state.discord_id % 6}.png`;
    image.alt = '';
    container.append(image);
  } else {
    const marker = document.createElement('span');
    marker.textContent = '●';
    container.append(marker);
  }
  const text = document.createElement('div');
  const name = document.createElement('strong');
  name.textContent = state.discord_id
    ? state.discord_name || 'Discord 사용자'
    : 'Discord 미연결';
  const detail = document.createElement('small');
  detail.textContent = state.discord_id ? String(state.discord_id) : '연결 시작 필요';
  text.append(name, detail);
  container.append(text);
  element.append(container);
}

function recentText(recent: RecentMatch): string {
  return `${String(recent.champion)} · ${recent.win ? '승리' : '패배'} · ${recent.kills ?? 0}/${recent.deaths ?? 0}/${recent.assists ?? 0} · CS ${recent.cs ?? 0} · ${Number(recent.gold ?? 0).toLocaleString()}G · ${Math.floor((recent.duration ?? 0) / 60)}분`;
}

function writeConfig(root: HTMLElement, config: Config) {
  required<HTMLInputElement>(root, '#relay').value = config.RelayPublicBaseUrl;
  required<HTMLInputElement>(root, '#lockfile').value = config.LockfilePath ?? '';
  required<HTMLInputElement>(root, '#dodge').checked = config.PreventQueueAfterDodge;
  required<HTMLInputElement>(root, '#status').checked = config.ApplyDefaultStatusOnConnect;
  required<HTMLInputElement>(root, '#accept').checked = config.AutoAcceptMatch;
  required<HTMLInputElement>(root, '#startup').checked = config.RunAtWindowsStartup;
  required<HTMLInputElement>(root, '#auto-update').checked = config.AutoUpdateEnabled;
}

function readConfig(root: HTMLElement, config: Config): Config {
  return {
    ...config,
    RelayPublicBaseUrl: required<HTMLInputElement>(root, '#relay').value,
    LockfilePath: required<HTMLInputElement>(root, '#lockfile').value || undefined,
    PreventQueueAfterDodge: required<HTMLInputElement>(root, '#dodge').checked,
    ApplyDefaultStatusOnConnect: required<HTMLInputElement>(root, '#status').checked,
    AutoAcceptMatch: required<HTMLInputElement>(root, '#accept').checked,
    RunAtWindowsStartup: required<HTMLInputElement>(root, '#startup').checked,
    AutoUpdateEnabled: required<HTMLInputElement>(root, '#auto-update').checked,
  };
}

function required<T extends Element>(root: ParentNode, selector: string): T {
  const element = root.querySelector<T>(selector);
  if (!element) throw new Error(`Missing UI element: ${selector}`);
  return element;
}
