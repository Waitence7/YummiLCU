import { invoke } from '@tauri-apps/api/core';
import './style.css';

type State = { status: string; relay: boolean; lcu: boolean; discord_id?: number; discord_name?: string; discord_avatar?: string; logs: string[]; oauth_pending: boolean; config: Config };
type Config = { RelayPublicBaseUrl: string; LockfilePath?: string; PreventQueueAfterDodge: boolean; ApplyDefaultStatusOnConnect: boolean; AutoAcceptMatch: boolean; FollowLeagueClient: boolean; RunAtWindowsStartup: boolean; UpdateManifestUrl?: string; CheckUpdatesOnStartup: boolean; AutoUpdateEnabled: boolean };
const initial: State = { status: '연결 시작 → Discord 로그인', relay: false, lcu: false, logs: [], oauth_pending: false, config: { RelayPublicBaseUrl: 'https://yummi.duckdns.org', PreventQueueAfterDodge: true, ApplyDefaultStatusOnConnect: true, AutoAcceptMatch: false, FollowLeagueClient: true, RunAtWindowsStartup: false, CheckUpdatesOnStartup: true, AutoUpdateEnabled: true } };
let state = initial;
const app = document.querySelector<HTMLElement>('#app')!;
const esc = (s: string) => s.replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#039;'}[c]!));
function render() {
  const c = state.config;
  const avatar = state.discord_avatar || (state.discord_id ? `https://cdn.discordapp.com/embed/avatars/${state.discord_id % 6}.png` : '');
  const account = state.discord_id ? `<div class="account"><img src="${esc(avatar)}"/><div><strong>${esc(state.discord_name || 'Discord 사용자')}</strong><small>${state.discord_id}</small></div></div>` : '<div class="account guest"><span>●</span><div><strong>Discord 미연결</strong><small>연결 시작 필요</small></div></div>';
  app.innerHTML = `<section class="shell"><header><div><h1>Yummi LCU Agent</h1><p>${esc(state.status)}</p></div><div class="account-area">${account}</div></header>
  <div class="buttons"><button id="start">연결 시작</button><button id="stop">중지</button><button id="relogin">Discord 재로그인</button></div>
  <div class="status"><span class="dot ${state.relay?'on':''}"></span> Relay <span class="dot ${state.lcu?'on':''}"></span> LCU <span class="discord">Discord: ${state.discord_id ?? '—'}</span></div>
  <label>Relay URL<input id="relay" value="${esc(c.RelayPublicBaseUrl)}"/></label>
  <label>lockfile<input id="lockfile" value="${esc(c.LockfilePath ?? '')}" placeholder="C:\\Riot Games\\League of Legends\\lockfile"/></label>
  ${state.oauth_pending ? '<div class="oauth"><b>브라우저에 표시된 6자리 코드를 입력하세요.</b><input id="oauth" maxlength="6"/><button id="submit-oauth">코드 확인</button></div>' : ''}
  <fieldset><legend>설정</legend><label><input type="checkbox" id="dodge" ${c.PreventQueueAfterDodge?'checked':''}/> 닷지 후 매칭 자동 재시작 방지</label><label><input type="checkbox" id="status" ${c.ApplyDefaultStatusOnConnect?'checked':''}/> 연결 시 기본 상메 적용</label><label><input type="checkbox" id="accept" ${c.AutoAcceptMatch?'checked':''}/> 매치 자동 수락</label><label><input type="checkbox" id="startup" ${c.RunAtWindowsStartup?'checked':''}/> Windows 시작 시 자동 실행</label></fieldset>
  <fieldset class="log"><legend>로그</legend><pre>${esc(state.logs.join('\n'))}</pre></fieldset></section>`;
  bind();
}
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | undefined> { try { return await invoke<T>(cmd, args); } catch (e) { addLog(String(e)); return undefined; } }
function addLog(s: string) { state.logs = [...state.logs.slice(-299), s]; render(); }
function configFromUi() { const c = state.config; c.RelayPublicBaseUrl = (document.querySelector<HTMLInputElement>('#relay')?.value ?? c.RelayPublicBaseUrl); c.LockfilePath = document.querySelector<HTMLInputElement>('#lockfile')?.value || undefined; c.PreventQueueAfterDodge = !!document.querySelector<HTMLInputElement>('#dodge')?.checked; c.ApplyDefaultStatusOnConnect = !!document.querySelector<HTMLInputElement>('#status')?.checked; c.AutoAcceptMatch = !!document.querySelector<HTMLInputElement>('#accept')?.checked; c.RunAtWindowsStartup = !!document.querySelector<HTMLInputElement>('#startup')?.checked; }
function bind() { document.querySelector('#start')?.addEventListener('click', async () => { configFromUi(); await call('save_config', { config: state.config }); await call('start_agent'); }); document.querySelector('#stop')?.addEventListener('click', () => call('stop_agent')); document.querySelector('#relogin')?.addEventListener('click', () => call('relogin')); document.querySelector('#submit-oauth')?.addEventListener('click', () => call('submit_oauth_code', { code: document.querySelector<HTMLInputElement>('#oauth')?.value ?? '' })); }
window.addEventListener('DOMContentLoaded', async () => { const loaded = await call<Config>('load_config'); if (loaded) state.config = loaded; render(); });
window.addEventListener('tauri://app-ready', () => undefined);
// Rust emits state updates; this listener is registered lazily to keep browser preview usable.
import('@tauri-apps/api/event').then(({ listen }) => listen<State>('agent-state', e => { state = e.payload; render(); }));
