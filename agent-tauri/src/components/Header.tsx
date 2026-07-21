import type { AgentState } from '../state/types';
import { Button, Dot } from './ui';

export function Header({
  state,
  onStart,
  onStop,
  onRelogin,
}: {
  state: AgentState;
  onStart(): void;
  onStop(): void;
  onRelogin(): void;
}) {
  const running = state.relay || state.lcu;
  return (
    <header className="border-b border-white/8 bg-zinc-950/80 px-4 pt-3 pb-2.5">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h1 className="text-[15px] font-bold tracking-tight text-zinc-100">
            Yummi LCU Agent
          </h1>
          <p className="mt-0.5 truncate text-[11px] text-zinc-500" title={state.status}>
            {state.status}
          </p>
        </div>
        <DiscordAccount state={state} />
      </div>
      <div className="mt-2.5 flex items-center gap-2">
        <Button variant="primary" onClick={onStart} disabled={running}>
          연결 시작
        </Button>
        <Button onClick={onStop} disabled={!running}>
          중지
        </Button>
        <Button variant="ghost" onClick={onRelogin}>
          Discord 재로그인
        </Button>
        <div className="ml-auto flex items-center gap-3 rounded-lg border border-white/8 bg-zinc-900/60 px-2.5 py-1.5 text-[11px] text-zinc-400">
          <span className="flex items-center gap-1.5">
            <Dot on={state.relay} /> Relay
          </span>
          <span className="flex items-center gap-1.5">
            <Dot on={state.lcu} /> LCU
          </span>
        </div>
      </div>
    </header>
  );
}

function DiscordAccount({ state }: { state: AgentState }) {
  const connected = state.discord_id != null;
  const avatar =
    state.discord_avatar ||
    (connected
      ? `https://cdn.discordapp.com/embed/avatars/${Number(state.discord_id) % 6}.png`
      : null);
  return (
    <div
      className={`flex shrink-0 items-center gap-2 rounded-lg border border-white/8 bg-zinc-900/60 px-2 py-1.5 ${
        connected ? '' : 'opacity-60'
      }`}
    >
      {avatar ? (
        <img src={avatar} alt="" className="size-7 rounded-full" />
      ) : (
        <span className="grid size-7 place-items-center rounded-full bg-zinc-800 text-[13px] text-zinc-500">
          ●
        </span>
      )}
      <div className="min-w-0 pr-1">
        <strong className="block max-w-36 truncate text-[12px] font-semibold text-zinc-200">
          {connected ? state.discord_name || 'Discord 사용자' : 'Discord 미연결'}
        </strong>
        <small className="block font-mono text-[10px] text-zinc-500">
          {connected ? String(state.discord_id) : '연결 시작 필요'}
        </small>
      </div>
    </div>
  );
}
