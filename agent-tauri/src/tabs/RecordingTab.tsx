import { useCallback, useEffect, useState } from 'react';

import {
  getRecordingStatus,
  getLiveControlStatus,
  requestBorderlessCaptureAccess,
  saveLastClip,
  startClipBuffer,
  startRecording,
  setLiveControlEnabled,
  stopClipBuffer,
  stopRecording,
} from '../api/commands';
import type { LiveControlStatus, LiveQualityProfile, RecordingCaptureSource, RecordingStatus } from '../state/types';
import { RecordingReview } from '../components/recording/RecordingReview';

const EMPTY_LIVE: LiveControlStatus = { enabled: false, profile: 'balanced', shareUrl: null, expiresAt: null };
const LIVE_QUALITY_OPTIONS: Array<{ value: LiveQualityProfile; label: string }> = [
  { value: 'data', label: '절약 · 540p 15fps' },
  { value: 'balanced', label: '표준 · 720p 30fps' },
  { value: 'high', label: '고화질 · 1080p 30fps' },
  { value: 'ultra', label: '최고 · 1440p 30fps' },
];

const EMPTY: RecordingStatus = {
  supported: true,
  recording: false,
  clipBuffer: false,
  recordingPath: null,
  bufferedSeconds: 0,
  lastClipPath: null,
  lastError: null,
  videoOnly: true,
  recordingSource: 'monitor',
  clipSource: 'leagueWindow',
  borderlessCaptureRequested: false,
  borderlessCaptureAllowed: false,
  borderlessCaptureStatus: 'not_requested',
  autoGameRecording: false,
  autoGameId: null,
  activeRecordingMetadataPath: null,
};

function fileName(path?: string | null) {
  if (!path) return '—';
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

export function RecordingTab({ connected }: { connected: boolean }) {
  const [status, setStatus] = useState<RecordingStatus>(EMPTY);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [recordingSource, setRecordingSource] = useState<RecordingCaptureSource>('monitor');
  const [clipSource, setClipSource] = useState<RecordingCaptureSource>('leagueWindow');
  const [liveStatus, setLiveStatus] = useState<LiveControlStatus>(EMPTY_LIVE);
  const [liveProfile, setLiveProfile] = useState<LiveQualityProfile>('balanced');
  const [liveBusy, setLiveBusy] = useState(false);
  const [liveMessage, setLiveMessage] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await getRecordingStatus());
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    let stopped = false;
    const refreshLive = async () => {
      try {
        const next = await getLiveControlStatus();
        if (stopped) return;
        setLiveStatus(next);
        setLiveProfile(next.profile);
      } catch (cause) {
        if (!stopped) setLiveMessage(String(cause));
      }
    };
    void refreshLive();
    const timer = window.setInterval(() => void refreshLive(), 3_000);
    return () => { stopped = true; window.clearInterval(timer); };
  }, []);

  const toggleLive = useCallback(async () => {
    setLiveBusy(true);
    setLiveMessage(null);
    try {
      const next = await setLiveControlEnabled(!liveStatus.enabled, liveProfile);
      setLiveStatus(next);
      setLiveProfile(next.profile);
      setLiveMessage(next.enabled ? '라이브 공유가 켜졌습니다.' : '라이브 공유가 꺼졌습니다. 기존 링크도 즉시 차단됩니다.');
    } catch (cause) {
      setLiveMessage(String(cause));
    } finally {
      setLiveBusy(false);
    }
  }, [liveProfile, liveStatus.enabled]);

  const copyLiveLink = useCallback(async () => {
    if (!liveStatus.shareUrl) return;
    try {
      await navigator.clipboard.writeText(liveStatus.shareUrl);
      setLiveMessage('공유 링크를 복사했습니다.');
    } catch {
      setLiveMessage('링크를 자동 복사하지 못했습니다. 아래 링크를 직접 복사해 주세요.');
    }
  }, [liveStatus.shareUrl]);

  const run = useCallback(async (operation: () => Promise<RecordingStatus>) => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await operation());
    } catch (cause) {
      setError(String(cause));
      await refresh();
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  return (
    <div className="mx-auto flex w-full max-w-6xl flex-col gap-3">
      <section className={`rounded-xl border p-4 shadow-sm ${liveStatus.enabled ? 'border-emerald-200 bg-emerald-50' : 'border-slate-200 bg-white'}`}>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold text-slate-900">실시간 라이브 공유</h2>
            <p className="mt-1 max-w-2xl text-xs leading-5 text-slate-500">
              앱에서 켠 동안에만 공유 링크로 화면을 볼 수 있습니다. 라이브 캡처는 녹화와 분리되어 MP4 파일을 만들지 않습니다.
            </p>
          </div>
          <span className={`rounded-full px-2 py-1 text-[10px] font-semibold ${liveStatus.enabled ? 'bg-emerald-600 text-white' : 'bg-slate-100 text-slate-500'}`}>
            {liveStatus.enabled ? 'LIVE 허용' : '꺼짐'}
          </span>
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <select
            value={liveProfile}
            disabled={liveBusy || liveStatus.enabled}
            onChange={(event) => setLiveProfile(event.target.value as LiveQualityProfile)}
            className="rounded-lg border border-slate-300 bg-white px-2 py-2 text-xs text-slate-700 disabled:opacity-50"
          >
            {LIVE_QUALITY_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
          </select>
          <button
            type="button"
            disabled={liveBusy || !connected}
            onClick={() => void toggleLive()}
            className={`rounded-lg px-3 py-2 text-xs font-semibold text-white disabled:cursor-not-allowed disabled:opacity-40 ${liveStatus.enabled ? 'bg-red-600' : 'bg-emerald-600'}`}
          >
            {liveBusy ? '처리 중…' : liveStatus.enabled ? '라이브 끄기' : '라이브 켜기'}
          </button>
          {liveStatus.enabled && liveStatus.shareUrl ? (
            <button type="button" onClick={() => void copyLiveLink()} className="rounded-lg border border-emerald-300 bg-white px-3 py-2 text-xs font-semibold text-emerald-800">공유 링크 복사</button>
          ) : null}
        </div>
        {liveStatus.enabled && liveStatus.shareUrl ? (
          <div className="mt-3 break-all rounded-lg bg-white/80 px-3 py-2 text-[11px] text-slate-700">{liveStatus.shareUrl}</div>
        ) : null}
        <p className="mt-2 text-[10px] text-slate-500">로그인한 관전자만 좌클릭 홀드 커서와 G/Ctrl 핑을 보낼 수 있습니다. 익명 관전자는 보기만 가능합니다.</p>
        {liveMessage ? <p className="mt-2 text-[11px] text-slate-600">{liveMessage}</p> : null}
      </section>

      <RecordingReview connected={connected} />
      <section className="rounded-xl border border-violet-200 bg-violet-50 p-4 shadow-sm">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold text-violet-950">자동 게임 녹화</h2>
            <p className="mt-1 text-xs leading-5 text-violet-800/75">League가 InProgress가 되면 게임 창 녹화를 자동 시작하고 경기 종료를 확인하면 자동 저장합니다.</p>
          </div>
          <span className={`rounded-full px-2 py-1 text-[10px] font-semibold ${status.autoGameRecording ? 'bg-red-100 text-red-700' : 'bg-white text-violet-700'}`}>
            {status.autoGameRecording ? `REC · ${status.autoGameId ?? 'game'}` : '자동 감지 대기'}
          </span>
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <button type="button" disabled={busy || status.borderlessCaptureRequested} onClick={() => void run(requestBorderlessCaptureAccess)} className="rounded-lg border border-violet-300 bg-white px-3 py-2 text-xs font-semibold text-violet-800 disabled:cursor-not-allowed disabled:opacity-50">
            {status.borderlessCaptureAllowed ? '테두리 없는 캡처 허용됨' : status.borderlessCaptureRequested ? '캡처 권한 요청 완료' : '테두리 없는 캡처 권한 1회 요청'}
          </button>
          <span className="text-[10px] text-violet-700/70">상태: {status.borderlessCaptureStatus}</span>
        </div>
      </section>

      <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold text-slate-900">일반 녹화</h2>
            <p className="mt-1 text-xs leading-5 text-slate-500">
              League가 실행 중이면 해당 게임이 있는 모니터를, 아니면 주 모니터를 H.264 MP4로 녹화합니다.
              화면에 합성된 유미지지 오버레이도 함께 남기는 용도입니다.
            </p>
          </div>
          <span className={`shrink-0 rounded-full px-2 py-1 text-[10px] font-semibold ${status.recording ? 'bg-red-50 text-red-600' : 'bg-slate-100 text-slate-500'}`}>
            {status.recording ? 'REC' : '대기'}
          </span>
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-2">
          <select
            value={status.recording ? status.recordingSource : recordingSource}
            disabled={busy || status.recording}
            onChange={(event) => setRecordingSource(event.target.value as RecordingCaptureSource)}
            className="rounded-lg border border-slate-300 bg-white px-2 py-2 text-xs text-slate-700 disabled:opacity-50"
          >
            <option value="monitor">화면 전체 · 오버레이 포함</option>
            <option value="leagueWindow">게임 창만 · 오버레이 제외</option>
          </select>
          {!status.recording ? (
            <button
              type="button"
              disabled={busy || !status.supported}
              onClick={() => void run(() => startRecording(recordingSource))}
              className="rounded-lg bg-slate-900 px-3 py-2 text-xs font-semibold text-white disabled:cursor-not-allowed disabled:opacity-40"
            >
              녹화 시작
            </button>
          ) : (
            <button
              type="button"
              disabled={busy}
              onClick={() => void run(stopRecording)}
              className="rounded-lg bg-red-600 px-3 py-2 text-xs font-semibold text-white disabled:cursor-not-allowed disabled:opacity-40"
            >
              녹화 중지
            </button>
          )}
        </div>

        <div className="mt-3 rounded-lg bg-slate-50 px-3 py-2 text-[11px] text-slate-600">
          저장: <span className="font-medium text-slate-800">{fileName(status.recordingPath)}</span>
        </div>
      </section>

      <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold text-slate-900">최근 플레이 클립</h2>
            <p className="mt-1 text-xs leading-5 text-slate-500">
              League 게임 창만 별도로 캡처해 최근 약 75초를 압축 세그먼트로 유지합니다. 기본적으로 Game Bar 오버레이가 없는 게임 창 원본을 사용합니다.
            </p>
          </div>
          <span className={`shrink-0 rounded-full px-2 py-1 text-[10px] font-semibold ${status.clipBuffer ? 'bg-emerald-50 text-emerald-700' : 'bg-slate-100 text-slate-500'}`}>
            {status.clipBuffer ? `${status.bufferedSeconds}s` : '꺼짐'}
          </span>
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-2">
          <select
            value={status.clipBuffer ? status.clipSource : clipSource}
            disabled={busy || status.clipBuffer}
            onChange={(event) => setClipSource(event.target.value as RecordingCaptureSource)}
            className="rounded-lg border border-slate-300 bg-white px-2 py-2 text-xs text-slate-700 disabled:opacity-50"
          >
            <option value="leagueWindow">게임 창만 · 오버레이 제외</option>
            <option value="monitor">화면 전체 · 오버레이 포함</option>
          </select>
          {!status.clipBuffer ? (
            <button
              type="button"
              disabled={busy || !status.supported}
              onClick={() => void run(() => startClipBuffer(clipSource))}
              className="rounded-lg bg-indigo-600 px-3 py-2 text-xs font-semibold text-white disabled:cursor-not-allowed disabled:opacity-40"
            >
              클립 버퍼 시작
            </button>
          ) : (
            <button
              type="button"
              disabled={busy}
              onClick={() => void run(stopClipBuffer)}
              className="rounded-lg border border-slate-300 bg-white px-3 py-2 text-xs font-semibold text-slate-700 disabled:cursor-not-allowed disabled:opacity-40"
            >
              버퍼 중지
            </button>
          )}
          <button
            type="button"
            disabled={busy || !status.clipBuffer || status.bufferedSeconds < 1}
            onClick={() => void run(() => saveLastClip(30))}
            className="rounded-lg border border-indigo-200 bg-indigo-50 px-3 py-2 text-xs font-semibold text-indigo-700 disabled:cursor-not-allowed disabled:opacity-40"
          >
            최근 30초 저장
          </button>
          <button
            type="button"
            disabled={busy || !status.clipBuffer || status.bufferedSeconds < 1}
            onClick={() => void run(() => saveLastClip(60))}
            className="rounded-lg border border-indigo-200 bg-indigo-50 px-3 py-2 text-xs font-semibold text-indigo-700 disabled:cursor-not-allowed disabled:opacity-40"
          >
            최근 60초 저장
          </button>
        </div>

        <div className="mt-3 grid gap-2 text-[11px] text-slate-600 sm:grid-cols-2">
          <div className="rounded-lg bg-slate-50 px-3 py-2">버퍼: <b className="text-slate-800">{status.bufferedSeconds}초</b></div>
          <div className="rounded-lg bg-slate-50 px-3 py-2">최근 저장: <b className="text-slate-800">{fileName(status.lastClipPath)}</b></div>
        </div>
      </section>

      <section className="rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-[11px] leading-5 text-amber-900">
        <b>Windows 10 1903 이상 / Windows 11 지원.</b> Borderless 권한 API가 지원되지 않거나 앱 패키지 권한이 없으면 기존 WGC 테두리 모드로 자동 fallback합니다. 현재 영상 전용이며 오디오는 다음 단계입니다.
      </section>

      {(error || status.lastError) && (
        <section className="rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-xs text-red-700">
          {error ?? status.lastError}
        </section>
      )}
    </div>
  );
}
