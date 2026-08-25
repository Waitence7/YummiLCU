/**
 * 브라우저 개발용 목 브리지. Tauri 백엔드 없이 UI 상태 전환(시작 → OAuth → 연결)을
 * 시뮬레이션한다. 프로덕션 번들에는 포함되지 않는다 (DEV 전용 동적 import).
 */
import { initialState, type AgentState, type Config, type RecentMatch } from '../state/types';

const MAX_UI_LOGS = 2_000;

let state: AgentState = structuredClone(initialState);
const listeners = new Set<(state: AgentState) => void>();
const timers: ReturnType<typeof setTimeout>[] = [];

function emit() {
  const snapshot = structuredClone(state);
  for (const listener of listeners) listener(snapshot);
}

function log(message: string) {
  const time = new Date().toLocaleTimeString('ko-KR', { hour12: false });
  state.logs = [
    ...state.logs.slice(-(MAX_UI_LOGS - 1)),
    `[${time}] ${message}`,
  ];
}

function later(ms: number, run: () => void) {
  timers.push(setTimeout(run, ms));
}

export function mockListen(onState: (state: AgentState) => void): () => void {
  listeners.add(onState);
  onState(structuredClone(state));
  return () => listeners.delete(onState);
}

export async function mockInvoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  switch (command) {
    case 'get_agent_state':
      return structuredClone(state) as T;
    case 'load_config':
      return structuredClone(state.config) as T;
    case 'save_config': {
      state.config = args?.config as Config;
      log('설정 저장됨');
      emit();
      return undefined as T;
    }
    case 'start_agent': {
      state.status = 'Relay 연결 중…';
      log('에이전트 시작');
      emit();
      later(500, () => {
        state.relay = true;
        state.oauth_pending = true;
        state.status = '브라우저에서 Discord 로그인 후 코드를 입력하세요.';
        log('Relay 연결됨 — Discord OAuth 대기');
        emit();
      });
      return undefined as T;
    }
    case 'stop_agent': {
      state.relay = false;
      state.lcu = false;
      state.oauth_pending = false;
      state.status = '중지됨';
      log('에이전트 중지');
      emit();
      return undefined as T;
    }
    case 'relogin': {
      state.discord_id = null;
      state.discord_name = null;
      state.discord_avatar = null;
      state.oauth_pending = true;
      state.status = '브라우저에서 Discord 로그인 후 코드를 입력하세요.';
      log('세션 초기화 — 재로그인 필요');
      emit();
      return undefined as T;
    }
    case 'submit_oauth_code': {
      const code = String(args?.code ?? '');
      if (!/^\d{6}$/.test(code.trim())) throw new Error('6자리 숫자 코드를 입력하세요.');
      state.oauth_pending = false;
      state.discord_id = 123456789012345678;
      state.discord_name = 'Waitence';
      state.status = 'Discord 연결 완료 — LCU 확인 중…';
      log('Discord 연결 완료');
      emit();
      later(700, () => {
        state.lcu = true;
        state.status = '연결됨 — 내전 결과 자동 보고 활성';
        log('League Client 감지됨 (lockfile)');
        emit();
      });
      return undefined as T;
    }
    case 'hide_main_window':
    case 'complete_tray_hide':
    case 'minimize_main_window':
    case 'request_tray_hide': {
      return undefined as T;
    }
    case 'get_beta_release_info': {
      return {
        version: '0.6.15',
        releaseLabel: '0.6.15-beta.1',
        buildId: '20260825.32',
        commit: 'preview',
        installerUrl: 'https://yummi.duckdns.org/agent/releases/tauri/beta/latest-setup.exe',
      } as T;
    }
    case 'open_beta_download': {
      log('beta 설치 파일 다운로드 열기');
      emit();
      return undefined as T;
    }
    case 'get_diagnostic_bundle': {
      return [
        'Yummi LCU Agent Diagnostics',
        `app_version=${state.app_version ?? 'mock'}`,
        `relay_connected=${state.relay}`,
        `lcu_connected=${state.lcu}`,
        `discord_bound=${state.discord_id != null}`,
        '',
        '--- UI Logs ---',
        ...state.logs,
      ].join('\n') as T;
    }
    case 'export_diagnostic_bundle': {
      return 'Downloads/yummi-agent-diagnostics-preview.txt' as T;
    }
    case 'recent_match': {
      if (!state.lcu) throw new Error('League Client가 연결되지 않았습니다.');
      const recent: RecentMatch = {
        champion: '아리',
        champion_id: 103,
        win: true,
        kills: 9,
        deaths: 2,
        assists: 11,
        cs: 204,
        gold: 13250,
        items: [],
        duration: 1834,
        created_at: Math.floor(Date.now() / 1000) - 3600,
      };
      return recent as T;
    }
    default:
      throw new Error(`mock: 알 수 없는 명령 ${command}`);
  }
}
