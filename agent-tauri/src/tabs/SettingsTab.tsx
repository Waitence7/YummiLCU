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
        <div className="divide-y divide-slate-100">
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
  const [updateChannel, setUpdateChannel] = useState(config.UpdateChannel);
  const [dirty, setDirty] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (dirty) return;
    setRelayUrl(config.RelayPublicBaseUrl);
    setLockfile(config.LockfilePath ?? '');
    setUpdateChannel(config.UpdateChannel);
  }, [config, dirty]);

  const save = async () => {
    const ok = await onPatchConfig({
      RelayPublicBaseUrl: relayUrl,
      LockfilePath: lockfile || undefined,
      UpdateChannel: updateChannel,
    });
    if (ok) {
      setDirty(false);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    }
  };

  return (
    <details className="group rounded-xl border border-slate-200 bg-white">
      <summary className="cursor-pointer list-none px-4 py-3 text-[13px] font-semibold text-slate-600 select-none group-open:text-slate-900">
        고급 설정
        <span className="float-right text-slate-400 transition-transform group-open:rotate-180">▾</span>
      </summary>
      <div className="space-y-3 px-4 pb-4">
        <div className="rounded-lg border border-slate-200 bg-slate-50 px-3">
          <p className="pt-2 text-[12px] font-semibold text-slate-700">Windows 시작</p>
          <Toggle
            checked={config.RunAtWindowsStartup}
            onChange={() => undefined}
            disabled
            label="Windows 로그인 시 백그라운드 자동 실행"
            description="로그인 후 창과 작업표시줄 없이 트레이에서 자동 실행되며, LCU 연결과 Relay 동작을 유지합니다."
          />
        </div>
        <div className="rounded-lg border border-slate-200 bg-slate-50 px-3">
          <p className="pt-2 text-[12px] font-semibold text-slate-700">시작 시 업데이트</p>
          <div className="divide-y divide-slate-200">
            <Toggle
              checked={config.CheckUpdatesOnStartup}
              onChange={(next) => void onPatchConfig({ CheckUpdatesOnStartup: next })}
              label="시작 시 업데이트 확인"
              description="앱을 시작할 때 새 버전이 있는지 확인합니다."
            />
            <Toggle
              checked={config.AutoUpdateEnabled}
              onChange={(next) => void onPatchConfig({ AutoUpdateEnabled: next })}
              label="시작 시 업데이트 자동 설치"
              description="확인된 업데이트를 시작 과정에서 자동으로 설치합니다."
            />
          </div>
        </div>
        <label className="block">
          <span className="mb-1 block text-[12px] text-slate-600">Relay URL</span>
          <TextInput
            value={relayUrl}
            onChange={(event) => {
              setRelayUrl(event.target.value);
              setDirty(true);
            }}
          />
        </label>
        <label className="block">
          <span className="mb-1 block text-[12px] text-slate-600">League lockfile 경로</span>
          <TextInput
            value={lockfile}
            placeholder="자동 감지 (기본값)"
            onChange={(event) => {
              setLockfile(event.target.value);
              setDirty(true);
            }}
          />
        </label>
        <label className="block">
          <span className="mb-1 block text-[12px] text-slate-600">업데이트 채널</span>
          <select
            className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-[12px] text-slate-800 focus:border-indigo-400/70 focus:outline-none"
            value={updateChannel}
            onChange={(event) => {
              setUpdateChannel(event.target.value as Config['UpdateChannel']);
              setDirty(true);
            }}
          >
            <option value="stable">stable</option>
            <option value="beta">beta</option>
            <option value="dev">dev</option>
          </select>
        </label>
        <div className="flex items-center gap-2">
          <Button variant="primary" disabled={!dirty} onClick={() => void save()}>
            저장
          </Button>
          {saved && <span className="text-[11px] text-emerald-600">저장됨</span>}
          <span className="ml-auto text-[11px] text-slate-500">일반적으로 변경할 필요가 없습니다.</span>
        </div>
      </div>
    </details>
  );
}
