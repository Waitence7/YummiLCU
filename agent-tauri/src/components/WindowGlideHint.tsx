import { useEffect, useState } from 'react';

import * as api from '../api/commands';
import type { Config } from '../state/types';
import { WindowGlideSlider } from './WindowGlideSlider';

export function WindowGlideHint() {
  const [strength, setStrength] = useState<number | null>(1);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let disposed = false;
    void api
      .loadConfig()
      .then((config: Config) => {
        if (!disposed) setStrength(config.WindowGlideStrength);
      })
      .finally(() => {
        if (!disposed) setLoaded(true);
      });
    return () => {
      disposed = true;
    };
  }, []);

  const saveStrength = async (next: number | null) => {
    setStrength(next);
    await api.setWindowGlideStrength(next);
  };

  return (
    <div className="motion-hint-surface flex h-full flex-col overflow-hidden text-slate-100">
      <div className="flex items-start gap-3 px-4 pt-4">
        <div className="grid size-9 shrink-0 place-items-center rounded-xl bg-indigo-500/15 text-lg text-indigo-200">
          ↗
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-2">
            <h1 className="text-[13px] font-semibold">창 미끄러짐이 켜져 있어요</h1>
            <button
              type="button"
              className="grid size-7 shrink-0 place-items-center rounded-lg text-slate-400 transition hover:bg-white/10 hover:text-white"
              aria-label="닫기"
              onClick={() => void api.closeWindowGlideHint()}
            >
              ×
            </button>
          </div>
          <p className="mt-1 text-[10.5px] leading-relaxed text-slate-300">
            제목 표시줄을 빠르게 던지면 속도에 맞춰 관성으로 이동하고, 빠를수록 잔상 효과가
            강해집니다.
          </p>
        </div>
      </div>

      <div className="mt-3 px-4">
        {loaded ? (
          <WindowGlideSlider value={strength} onCommit={saveStrength} compact />
        ) : (
          <div className="h-[67px] animate-pulse rounded-xl border border-white/10 bg-white/5" />
        )}
        <p className="mt-2 text-[9px] leading-relaxed text-slate-400">
          ∞는 이동 중 마찰만 없앱니다. 화면 가장자리 충돌과 다시 잡기·최소화·트레이 이동은
          그대로 동작합니다.
        </p>
      </div>
    </div>
  );
}
