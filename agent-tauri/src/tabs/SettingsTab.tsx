import { useEffect, useState } from 'react';

import { Button, Card, TextInput, Toggle } from '../components/ui';
import type { Config } from '../state/types';

export function SettingsTab({
  config,
  onPatchConfig,
}: {
  config: Config;
  onPatchConfig(patch: Partial<Config>): Promise<boolean>;
}) {
  return (
    <div className="space-y-3">
      <Card title="편의 기능">
        <div className="divide-y divide-white/5">
          <Toggle
            checked={config.PreventQueueAfterDodge}
            onChange={(next) => void onPatchConfig({ PreventQueueAfterDodge: next })}
            label="닷지 후 매칭 자동 재시작 방지"
            description="챔피언 선택에서 나간 뒤 매칭이 다시 자동으로 잡히는 것을 막습니다."
          />
          <Toggle
            checked={config.ApplyDefaultStatusOnConnect}
            onChange={(next) => void onPatchConfig({ ApplyDefaultStatusOnConnect: next })}
            label="연결 시 기본 상태 메시지 적용"
            description="에이전트 연결 시 롤 클라이언트 상태 메시지를 자동 설정합니다."
          />
          <Toggle
            checked={config.AutoAcceptMatch}
            onChange={(next) => void onPatchConfig({ AutoAcceptMatch: next })}
            label="매치 자동 수락"
            description="수락 창이 뜨면 자동으로 수락합니다."
          />
        </div>
      </Card>

      <Card title="앱">
        <div className="divide-y divide-white/5">
          <Toggle
            checked={config.RunAtWindowsStartup}
            onChange={(next) => void onPatchConfig({ RunAtWindowsStartup: next })}
            label="Windows 시작 시 자동 실행"
          />
          <Toggle
            checked={config.AutoUpdateEnabled}
            onChange={(next) => void onPatchConfig({ AutoUpdateEnabled: next })}
            label="시작 시 자동 업데이트 설치 (권장)"
          />
        </div>
      </Card>

      <AdvancedCard config={config} onPatchConfig={onPatchConfig} />
    </div>
  );
}

function AdvancedCard({
  config,
  onPatchConfig,
}: {
  config: Config;
  onPatchConfig(patch: Partial<Config>): Promise<boolean>;
}) {
  const [relayUrl, setRelayUrl] = useState(config.RelayPublicBaseUrl);
  const [lockfile, setLockfile] = useState(config.LockfilePath ?? '');
  const [dirty, setDirty] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (dirty) return;
    setRelayUrl(config.RelayPublicBaseUrl);
    setLockfile(config.LockfilePath ?? '');
  }, [config, dirty]);

  const save = async () => {
    const ok = await onPatchConfig({
      RelayPublicBaseUrl: relayUrl,
      LockfilePath: lockfile || undefined,
    });
    if (ok) {
      setDirty(false);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    }
  };

  return (
    <details className="group rounded-xl border border-white/8 bg-zinc-900/40">
      <summary className="cursor-pointer list-none px-4 py-3 text-[13px] font-semibold text-zinc-400 select-none group-open:text-zinc-200">
        고급 설정
        <span className="float-right text-zinc-600 transition-transform group-open:rotate-180">▾</span>
      </summary>
      <div className="space-y-3 px-4 pb-4">
        <label className="block">
          <span className="mb-1 block text-[12px] text-zinc-400">Relay URL</span>
          <TextInput
            value={relayUrl}
            onChange={(event) => {
              setRelayUrl(event.target.value);
              setDirty(true);
            }}
          />
        </label>
        <label className="block">
          <span className="mb-1 block text-[12px] text-zinc-400">League lockfile 경로</span>
          <TextInput
            value={lockfile}
            placeholder="자동 감지 (기본값)"
            onChange={(event) => {
              setLockfile(event.target.value);
              setDirty(true);
            }}
          />
        </label>
        <div className="flex items-center gap-2">
          <Button variant="primary" disabled={!dirty} onClick={() => void save()}>
            저장
          </Button>
          {saved && <span className="text-[11px] text-emerald-400">저장됨</span>}
          <span className="ml-auto text-[11px] text-zinc-600">일반적으로 변경할 필요가 없습니다.</span>
        </div>
      </div>
    </details>
  );
}
