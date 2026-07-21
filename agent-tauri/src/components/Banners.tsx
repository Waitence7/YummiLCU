import { useState } from 'react';

import type { AgentState } from '../state/types';
import { Button, TextInput } from './ui';

export function Banners({
  state,
  onSubmitOAuth,
}: {
  state: AgentState;
  onSubmitOAuth(code: string): Promise<boolean>;
}) {
  return (
    <>
      {state.oauth_pending && <OAuthBanner onSubmit={onSubmitOAuth} />}
      {state.update_message && (
        <div
          role="status"
          className="border-b border-indigo-400/20 bg-indigo-500/10 px-4 py-2 text-[12px] text-indigo-200"
        >
          {state.update_message}
        </div>
      )}
    </>
  );
}

function OAuthBanner({ onSubmit }: { onSubmit(code: string): Promise<boolean> }) {
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    try {
      if (await onSubmit(code)) setCode('');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="border-b border-amber-400/20 bg-amber-500/10 px-4 py-2.5">
      <p className="text-[12px] font-medium text-amber-200">
        브라우저에 표시된 6자리 코드를 입력하세요.
      </p>
      <div className="mt-1.5 flex items-center gap-2">
        <TextInput
          value={code}
          maxLength={6}
          inputMode="numeric"
          placeholder="000000"
          className="max-w-32 text-center font-mono tracking-[0.3em]"
          onChange={(event) => setCode(event.target.value.replace(/\D/g, ''))}
          onKeyDown={(event) => {
            if (event.key === 'Enter') void submit();
          }}
        />
        <Button variant="primary" disabled={code.length !== 6 || busy} onClick={() => void submit()}>
          코드 확인
        </Button>
      </div>
    </div>
  );
}
