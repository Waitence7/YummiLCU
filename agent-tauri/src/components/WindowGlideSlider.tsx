import { useEffect, useMemo, useState } from 'react';

import {
  formatGlideStrength,
  glideStrengthToSlider,
  sliderToGlideStrength,
} from '../windowMotion';

export function WindowGlideSlider({
  value,
  onCommit,
  compact = false,
}: {
  value: number | null;
  onCommit(value: number | null): Promise<unknown> | unknown;
  compact?: boolean;
}) {
  const [position, setPosition] = useState(() => glideStrengthToSlider(value));
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setPosition(glideStrengthToSlider(value));
  }, [value]);

  const displayedStrength = useMemo(() => sliderToGlideStrength(position), [position]);

  const commit = async () => {
    setSaving(true);
    try {
      await onCommit(displayedStrength);
    } finally {
      setSaving(false);
    }
  };

  return (
    <label
      className={
        compact
          ? 'block rounded-xl border border-white/10 bg-white/5 px-3 py-2.5'
          : 'block rounded-lg border border-slate-200 bg-slate-50 px-3 py-2'
      }
    >
      <span
        className={
          compact
            ? 'flex items-center justify-between text-[11px] font-medium text-slate-200'
            : 'flex items-center justify-between text-[11px] font-medium text-slate-700'
        }
      >
        미끄러짐
        <output
          className={
            compact
              ? 'rounded-md bg-white/10 px-2 py-0.5 font-mono text-indigo-200'
              : 'rounded bg-white px-2 py-0.5 font-mono text-indigo-700 shadow-sm'
          }
        >
          {formatGlideStrength(displayedStrength)}
          {saving ? ' · 저장 중' : ''}
        </output>
      </span>
      <input
        className="mt-2 block w-full accent-indigo-500"
        type="range"
        min="0"
        max="100"
        step="0.1"
        value={position}
        aria-label="창 미끄러짐 강도"
        onChange={(event) => setPosition(Number(event.target.value))}
        onPointerUp={() => void commit()}
        onKeyUp={() => void commit()}
        onBlur={() => void commit()}
      />
      <span
        className={
          compact
            ? 'mt-1 flex justify-between text-[9px] text-slate-400'
            : 'mt-1 flex justify-between text-[9px] text-slate-500'
        }
      >
        <span>0 · 관성 없음</span>
        <span>1× 기본</span>
        <span>∞ · 마찰 없음</span>
      </span>
    </label>
  );
}
