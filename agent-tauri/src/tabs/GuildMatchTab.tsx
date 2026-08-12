import { Badge, Button, Card, Dot, Toggle } from '../components/ui';
import type { RecentState } from '../hooks/useAgentState';
import type { AgentState, RecentMatch } from '../state/types';

export function GuildMatchTab({
  state,
  recent,
  onRefreshRecent,
  onPatchConfig,
}: {
  state: AgentState;
  recent: RecentState;
  onRefreshRecent(): void;
  onPatchConfig(patch: { AutoAcceptMatch: boolean }): void;
}) {
  const linked = state.relay && state.lcu;
  return (
    <div className="space-y-3">
      <Card title="내전 연동 상태" action={<Badge tone={linked ? 'ok' : 'neutral'}>{linked ? '연동됨' : '대기 중'}</Badge>}>
        <ul className="space-y-2 text-[12px] text-slate-600">
          <ConnectionRow
            on={state.relay}
            label="Yummi 서버 (Relay)"
            detail={state.relay ? '봇과 실시간 연결됨' : '연결 시작을 눌러 Discord 계정을 연결하세요'}
          />
          <ConnectionRow
            on={state.lcu}
            label="League Client (LCU)"
            detail={state.lcu ? '롤 클라이언트 감지됨' : '롤 클라이언트 실행 시 자동으로 감지됩니다'}
          />
        </ul>
      </Card>

      <Card
        title="내전 결과 자동 보고"
        action={<Badge tone={linked ? 'ok' : 'warn'}>{linked ? '활성' : '연동 필요'}</Badge>}
      >
        <p className="text-[12px] leading-relaxed text-slate-600">
          내전(커스텀 게임)이 끝나면 경기 결과가 Yummi 봇으로 자동 전송되어 전적·티어 반영과
          디스코드 결과 공지가 이루어집니다. 별도 조작 없이 에이전트가 연결되어 있으면 동작합니다.
        </p>
      </Card>

      <Card
        title="최근 경기"
        action={
          <Button onClick={onRefreshRecent} disabled={recent.loading}>
            {recent.loading ? '불러오는 중…' : '새로고침'}
          </Button>
        }
      >
        <RecentMatchBody recent={recent} lcu={state.lcu} />
      </Card>

      <Card title="빠른 설정">
        <Toggle
          checked={state.config.AutoAcceptMatch}
          onChange={(next) => onPatchConfig({ AutoAcceptMatch: next })}
          label="매치 자동 수락"
          description="내전·일반 매칭에서 수락 창이 뜨면 자동으로 수락합니다."
        />
      </Card>
    </div>
  );
}

function ConnectionRow({ on, label, detail }: { on: boolean; label: string; detail: string }) {
  return (
    <li className="flex items-center gap-2.5">
      <Dot on={on} />
      <span className="w-40 shrink-0 font-medium text-slate-700">{label}</span>
      <span className="truncate text-slate-500">{detail}</span>
    </li>
  );
}

function RecentMatchBody({ recent, lcu }: { recent: RecentState; lcu: boolean }) {
  if (recent.match) return <RecentMatchCard match={recent.match} />;
  return (
    <p className="text-[12px] text-slate-500">
      {recent.error ?? (lcu ? '새로고침을 눌러 최근 경기를 확인하세요.' : 'League Client 연결 후 확인할 수 있습니다.')}
    </p>
  );
}

function RecentMatchCard({ match }: { match: RecentMatch }) {
  const kda = `${match.kills ?? 0} / ${match.deaths ?? 0} / ${match.assists ?? 0}`;
  return (
    <div className="flex items-center gap-4">
      <div
        className={`grid size-12 shrink-0 place-items-center rounded-lg border text-[11px] font-bold ${
          match.win
            ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
            : 'border-rose-200 bg-rose-50 text-rose-700'
        }`}
      >
        {match.win ? '승리' : '패배'}
      </div>
      <div className="min-w-0">
        <p className="text-[13px] font-semibold text-slate-800">{String(match.champion)}</p>
        <p className="mt-0.5 text-[11px] text-slate-500">
          KDA {kda} · CS {match.cs ?? 0} · {Number(match.gold ?? 0).toLocaleString()}G ·{' '}
          {Math.floor((match.duration ?? 0) / 60)}분
        </p>
      </div>
    </div>
  );
}
