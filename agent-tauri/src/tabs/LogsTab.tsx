import { useEffect, useRef } from 'react';

export function LogsTab({ logs }: { logs: string[] }) {
  const scroller = useRef<HTMLPreElement>(null);

  useEffect(() => {
    const element = scroller.current;
    if (element) element.scrollTop = element.scrollHeight;
  }, [logs]);

  return (
    <div className="flex h-full flex-col rounded-xl border border-white/8 bg-zinc-950/60">
      <div className="flex items-center justify-between border-b border-white/5 px-3 py-2">
        <span className="text-[12px] font-semibold text-zinc-400">로그</span>
        <span className="text-[11px] text-zinc-600">{logs.length}줄 · 최근 300줄 유지</span>
      </div>
      <pre
        ref={scroller}
        className="min-h-0 flex-1 select-text overflow-auto p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-zinc-400"
      >
        {logs.length > 0 ? logs.join('\n') : '아직 로그가 없습니다.'}
      </pre>
    </div>
  );
}
