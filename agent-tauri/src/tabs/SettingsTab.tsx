import { useEffect, useState } from 'react';

import * as api from '../api/commands';
import { Button, Card, TextInput, Toggle } from '../components/ui';
import type { BetaReleaseInfo, Config, TrayHideEffect } from '../state/types';
import { playTrayHideEffect, TRAY_HIDE_EFFECT_OPTIONS } from '../trayEffects';

export function SettingsTab({
  config,
  currentReleaseLabel,
  currentBuildId,
  currentReleaseChannel,
  onPatchConfig,
}: {
  config: Config;
  currentReleaseLabel: string;
  currentBuildId: string;
  currentReleaseChannel: string;
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

      <TrayEffectCard config={config} onPatchConfig={onPatchConfig} />

      <BetaDownloadCard
        currentReleaseLabel={currentReleaseLabel}
        currentBuildId={currentBuildId}
        currentReleaseChannel={currentReleaseChannel}
      />

      <AdvancedCard config={config} onPatchConfig={onPatchConfig} />
    </div>
  );
}


function BetaDownloadCard({
  currentReleaseLabel,
  currentBuildId,
  currentReleaseChannel,
}: {
  currentReleaseLabel: string;
  currentBuildId: string;
  currentReleaseChannel: string;
}) {
  const [release, setRelease] = useState<BetaReleaseInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      setRelease(await api.getBetaReleaseInfo());
    } catch {
      setRelease(null);
      setError('최신 beta 정보를 불러오지 못했습니다.');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const download = async () => {
    setOpening(true);
    setError(null);
    try {
      await api.openBetaDownload();
    } catch {
      setError('beta 설치 파일을 열지 못했습니다.');
    } finally {
      setOpening(false);
    }
  };

  return (
    <Card
      title="베타 다운로드"
      action={
        <Button variant="ghost" disabled={loading} onClick={() => void refresh()}>
          새로고침
        </Button>
      }
    >
      <div className="space-y-2.5">
        <div className="grid grid-cols-2 gap-2 text-[11px]">
          <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2">
            <span className="block text-slate-500">현재 설치</span>
            <strong className="mt-0.5 block truncate text-slate-800">{currentReleaseLabel}</strong>
            <span className="text-[10px] text-slate-500">
              {currentReleaseChannel} · build {currentBuildId}
            </span>
          </div>
          <div className="rounded-lg border border-indigo-100 bg-indigo-50/60 px-3 py-2">
            <span className="block text-indigo-600">최신 beta</span>
            <strong className="mt-0.5 block truncate text-indigo-900">
              {loading ? '확인 중…' : release?.releaseLabel ?? '확인 실패'}
            </strong>
            <span className="text-[10px] text-indigo-700/80">
              {release ? `build ${release.buildId}` : '서명된 manifest 기준'}
            </span>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="primary" disabled={opening} onClick={() => void download()}>
            {opening ? '여는 중…' : '베타 설치 파일 다운로드'}
          </Button>
          <span className="text-[10px] leading-snug text-slate-500">
            업데이트 채널 설정은 바꾸지 않습니다. 설치 후 beta를 계속 받으려면 채널을 beta로 설정하세요.
          </span>
        </div>
        {error && <p className="text-[10px] text-rose-600">{error}</p>}
      </div>
    </Card>
  );
}

function TrayEffectCard({
  config,
  onPatchConfig,
}: {
  config: Config;
  onPatchConfig(patch: Partial<Config>): Promise<boolean>;
}) {
  const selected =
    TRAY_HIDE_EFFECT_OPTIONS.find((option) => option.value === config.TrayHideEffect) ??
    TRAY_HIDE_EFFECT_OPTIONS[0];

  return (
    <Card
      title="트레이 전환 효과"
      action={
        <Button
          variant="ghost"
          onClick={() =>
            void playTrayHideEffect(config.TrayHideEffect, undefined, {
              playbackRate: config.TrayEffectPlaybackRate,
            })
          }
        >
          미리보기
        </Button>
      }
    >
      <div className="space-y-2.5">
        <select
          className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-[12px] text-slate-800 focus:border-indigo-400/70 focus:outline-none"
          value={config.TrayHideEffect}
          onChange={(event) =>
            void onPatchConfig({ TrayHideEffect: event.target.value as TrayHideEffect })
          }
        >
          {TRAY_HIDE_EFFECT_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <div className="rounded-lg border border-indigo-100 bg-indigo-50/60 px-3 py-2">
          <p className="text-[11px] font-medium text-indigo-800">{selected.label}</p>
          <p className="mt-0.5 text-[11px] leading-snug text-indigo-700/80">
            {selected.description}
          </p>
        </div>
        <label className="block rounded-lg border border-slate-200 bg-slate-50 px-3 py-2">
          <span className="flex items-center justify-between text-[11px] font-medium text-slate-700">
            효과 속도
            <output className="rounded bg-white px-2 py-0.5 font-mono text-indigo-700 shadow-sm">
              {config.TrayEffectPlaybackRate.toFixed(1)}×
            </output>
          </span>
          <input
            className="mt-2 block w-full accent-indigo-600"
            type="range"
            min="0.1"
            max="4"
            step="0.1"
            value={config.TrayEffectPlaybackRate}
            onChange={(event) =>
              void onPatchConfig({ TrayEffectPlaybackRate: Number(event.target.value) })
            }
          />
          <span className="mt-1 flex justify-between text-[9px] text-slate-500">
            <span>0.1× 느리게 관찰</span>
            <span>1× 기본</span>
            <span>4× 빠르게</span>
          </span>
        </label>
        <p className="text-[10px] leading-relaxed text-slate-500">
          속도는 미리보기와 X 버튼으로 창을 트레이에 보낼 때 모두 적용됩니다. Windows의 동작
          줄이기 설정이 켜져 있으면 안전하게 짧은 페이드 효과로 전환됩니다.
        </p>
      </div>
    </Card>
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
