import type { AgentState } from '../state/types';

export function Banners({ state }: { state: AgentState }) {
  return (
    <>
      {state.oauth_pending && <OAuthBanner />}
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

function OAuthBanner() {
  return (
    <div className="border-b border-amber-200 bg-amber-50 px-4 py-2.5">
      <p className="text-[12px] font-medium text-amber-800">Discord 로그인 대기 중</p>
      <p className="mt-0.5 text-[11px] text-amber-700">
        브라우저에서 Discord 로그인을 완료하면 자동으로 연결됩니다.
      </p>
    </div>
  );
}
