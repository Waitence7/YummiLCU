import { useEffect, useState } from 'react';

import * as api from '../api/commands';
import type { AgentState, OverlayInstallStatus } from '../state/types';
import { Button } from './ui';

export function Banners({
  state,
  onDiscordLogin,
}: {
  state: AgentState;
  onDiscordLogin(): void;
}) {
  return (
    <>
      {state.oauth_pending && <OAuthBanner onLogin={onDiscordLogin} />}
      <OverlaySetupBanner />
      {state.trust_level === 'new_device' && (
        <div
          role="status"
          className="border-b border-violet-200 bg-violet-50 px-4 py-2 text-[11px] text-violet-700"
        >
          <span className="font-medium">신규 기기 자동 로그인</span> · 메뉴 조회, 로비 생성, 파티 초대/강퇴, 매칭 수락 사용 가능 · 큐 시작 및 계정/보안 변경 제한
        </div>
      )}
      {state.update_message && (
        <div
          role="status"
          className="border-b border-indigo-200 bg-indigo-50 px-4 py-2 text-[12px] text-indigo-700"
        >
          {state.update_message}
        </div>
      )}
    </>
  );
}

function OverlaySetupBanner() {
  const [status, setStatus] = useState<OverlayInstallStatus | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void api.getOverlayInstallStatus()
      .then((value) => {
        if (!cancelled) setStatus(value);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  if (dismissed || !status?.supported || status.installed) return null;

  const install = async () => {
    setInstalling(true);
    setError(null);
    try {
      setStatus(await api.installOverlay());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      void api.reportUiFailure('overlay_install', cause);
    } finally {
      setInstalling(false);
    }
  };

  return (
    <div className="flex items-center gap-3 border-b border-indigo-200 bg-indigo-50 px-4 py-2.5">
      <div className="min-w-0 flex-1">
        <p className="text-[12px] font-medium text-indigo-900">유미지지 오버레이도 설치할까요?</p>
        <p className="mt-0.5 text-[11px] leading-snug text-indigo-700">
          Agent가 최신 오버레이를 검증해 자동 설치합니다. 최초 설치에서는 인증서와 Game Bar 연결 설정 때문에 Windows 관리자 승인(UAC)이 한 번 필요할 수 있습니다.
        </p>
        {error && <p className="mt-1 text-[10px] text-rose-600">설치 실패 · {error}</p>}
      </div>
      <div className="flex shrink-0 items-center gap-1.5">
        <Button variant="ghost" disabled={installing} onClick={() => setDismissed(true)}>
          나중에
        </Button>
        <Button variant="primary" disabled={installing} onClick={() => void install()}>
          {installing ? '설치 중…' : '오버레이 설치'}
        </Button>
      </div>
    </div>
  );
}

function OAuthBanner({ onLogin }: { onLogin(): void }) {
  return (
    <div className="flex items-center gap-3 border-b border-amber-200 bg-amber-50 px-4 py-2.5">
      <div className="min-w-0 flex-1">
        <p className="text-[12px] font-medium text-amber-800">Discord 로그인 필요</p>
        <p className="mt-0.5 text-[11px] text-amber-700">
          자동으로 브라우저를 열지 않습니다. 로그인할 때만 버튼을 눌러주세요.
        </p>
      </div>
      <Button variant="primary" onClick={onLogin}>Discord 로그인</Button>
    </div>
  );
}
