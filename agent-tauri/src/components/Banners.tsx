import type { AgentState } from '../state/types';
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
