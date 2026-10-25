import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

type Snapshot = {
  connected: boolean;
  riot_id: string | null;
  phase: string;
  party_members: string[];
};

export function FullscreenHud() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      unlisten = await listen<Snapshot>('yummi://hud-snapshot', ({ payload }) => {
        if (!disposed) setSnapshot(payload);
      });
      if (disposed) { unlisten(); return; }
      const initial = await invoke<Snapshot>('fullscreen_hud_snapshot');
      if (!disposed) setSnapshot(initial);
      await invoke('fullscreen_hud_ready');
    })().catch(() => { if (!disposed) setError(true); });
    return () => { disposed = true; unlisten?.(); };
  }, []);
  return (
    <div className="yummi-fullscreen-hud" aria-label="유미 전체화면 HUD">
      <aside className="yummi-fullscreen-hud-panel">
        <div className="yummi-fullscreen-hud-title">
          <span className={snapshot?.connected ? 'yummi-hud-dot connected' : 'yummi-hud-dot'} />
          <strong>유미 LCU</strong>
          <span>{snapshot?.connected ? snapshot.phase : 'League 대기 중'}</span>
        </div>
        <div>{snapshot?.riot_id || 'League 클라이언트를 기다리는 중'}</div>
        <div className="yummi-hud-muted">{snapshot?.party_members.join(' · ') || '파티 정보 없음'}</div>
        <div className="yummi-hud-muted">{error ? 'HUD 연결을 확인해 주세요' : '클릭·터치는 게임으로 통과합니다'}</div>
      </aside>
    </div>
  );
}
