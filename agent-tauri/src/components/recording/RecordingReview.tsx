import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  listRecordingSessions,
  recordingGameContext,
  reportRecordingDiagnostic,
} from "../../api/commands";
import type {
  RecordingGameContext,
  RecordingPotg,
  RecordingSession,
} from "../../state/types";
import type { Match } from "../../state/matches";
import { ReplayViewer } from "../matches/ReplayViewer";

const RECORDING_BASE = "http://127.0.0.1:19516";

function timeLabel(seconds: number) {
  const safe = Math.max(0, Math.floor(seconds));
  return `${Math.floor(safe / 60)}:${String(safe % 60).padStart(2, "0")}`;
}

function dateLabel(timestamp: number) {
  return new Date(timestamp).toLocaleString("ko-KR", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function fileName(path?: string | null) {
  if (!path) return null;
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

function normalizedName(value?: string | null) {
  return (value ?? "")
    .split("#")[0]!
    .replace(/\s+/g, "")
    .toLocaleLowerCase("en-US");
}

function isMyPotg(session: RecordingSession, potg?: RecordingPotg | null) {
  if (!session.riotId || !potg?.playerName) return false;
  return normalizedName(session.riotId) === normalizedName(potg.playerName);
}

function syntheticMatch(session: RecordingSession): Match | null {
  if (!session.gameId) return null;
  const recordedMs = Math.max(
    1_000,
    (session.endedAtMs ?? session.startedAtMs + 30 * 60_000) - session.startedAtMs,
  );
  return {
    id: session.gameId,
    createdAt: session.startedAtMs,
    duration: Math.ceil((recordedMs + Math.max(0, session.gameClockOffsetMs)) / 1000),
    queueId: 0,
    mapId: 11,
    version: "",
    me: null,
    players: [],
  };
}

export function RecordingReview({ connected }: { connected: boolean }) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [sessions, setSessions] = useState<RecordingSession[]>([]);
  const [selectedGameId, setSelectedGameId] = useState<string | null>(null);
  const [context, setContext] = useState<RecordingGameContext | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [contextError, setContextError] = useState<string | null>(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);

  const refreshSessions = useCallback(async () => {
    try {
      const rows = (await listRecordingSessions()).filter(
        (row) => row.automatic && Boolean(row.gameId),
      );
      setSessions(rows);
      setListError(null);
      setSelectedGameId((current) => {
        if (current && rows.some((row) => row.gameId === current)) return current;
        return rows[0]?.gameId ?? null;
      });
    } catch (cause) {
      const detail = cause instanceof Error ? cause.message : String(cause);
      setListError(detail);
      void reportRecordingDiagnostic("recording_list_failed", `ui_invoke error=${detail}`).catch(() => undefined);
    }
  }, []);

  useEffect(() => {
    void refreshSessions();
    const timer = window.setInterval(() => void refreshSessions(), 4_000);
    return () => window.clearInterval(timer);
  }, [refreshSessions]);

  const selected = useMemo(
    () => sessions.find((row) => row.gameId === selectedGameId) ?? null,
    [selectedGameId, sessions],
  );

  useEffect(() => {
    setCurrentTime(0);
    setDuration(0);
    setContext(null);
    setContextError(null);
    if (!selected?.gameId) return;
    let cancelled = false;
    void recordingGameContext(selected.gameId)
      .then((value) => {
        if (!cancelled) setContext(value);
      })
      .catch((cause) => {
        if (!cancelled) {
          const detail = cause instanceof Error ? cause.message : String(cause);
          setContextError(detail);
          void reportRecordingDiagnostic(
            "recording_context_failed",
            `game_id=${selected.gameId} ui_invoke error=${detail}`,
          ).catch(() => undefined);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selected?.gameId]);

  const match = useMemo(() => (selected ? syntheticMatch(selected) : null), [selected]);
  const potg = context?.potg ?? null;
  const myPotg = selected && isMyPotg(selected, potg) ? potg : null;
  const offsetMs = selected?.gameClockOffsetMs ?? 0;
  const gameTimeMs = Math.max(0, Math.round(currentTime * 1000) + offsetMs);
  const timelineMs = Math.max(1, Math.round(duration * 1000));
  const marker = myPotg
    ? {
        left: Math.max(0, Math.min(100, ((myPotg.startMs - offsetMs) / timelineMs) * 100)),
        right: Math.max(0, Math.min(100, ((myPotg.endMs - offsetMs) / timelineMs) * 100)),
      }
    : null;
  const progress = duration > 0 ? Math.max(0, Math.min(100, (currentTime / duration) * 100)) : 0;

  const seekVideoGameTime = useCallback(
    (nextGameTimeMs: number) => {
      const video = videoRef.current;
      if (!video) return;
      const nextSeconds = Math.max(0, (nextGameTimeMs - offsetMs) / 1000);
      const maximum = Number.isFinite(video.duration) ? video.duration : nextSeconds;
      try {
        video.currentTime = Math.min(maximum, nextSeconds);
      } catch (cause) {
        const detail = cause instanceof Error ? cause.message : String(cause);
        void reportRecordingDiagnostic(
          "video_seek_failed",
          `game_id=${selectedGameId ?? "unknown"} target_ms=${Math.round(nextGameTimeMs)} error=${detail}`,
        ).catch(() => undefined);
      }
    },
    [offsetMs, selectedGameId],
  );

  const seekTimeline = (event: React.MouseEvent<HTMLButtonElement>) => {
    const video = videoRef.current;
    if (!video || !duration) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const next = Math.max(
      0,
      Math.min(duration, ((event.clientX - rect.left) / rect.width) * duration),
    );
    try {
      video.currentTime = next;
    } catch (cause) {
      const detail = cause instanceof Error ? cause.message : String(cause);
      void reportRecordingDiagnostic(
        "video_seek_failed",
        `game_id=${selectedGameId ?? "unknown"} target_ms=${Math.round(next * 1000)} error=${detail}`,
      ).catch(() => undefined);
    }
  };

  return (
    <section className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold text-slate-900">경기 다시보기</h2>
          <p className="mt-1 text-xs leading-5 text-slate-500">
            자동 녹화와 ROFL 분석을 같은 시간축으로 봅니다. 영상에서 이동하면 분석도 따라가고,
            ROFL 타임라인을 누르면 영상이 그 시점으로 이동합니다.
          </p>
        </div>
        <span className="rounded-full bg-slate-100 px-2 py-1 text-[10px] font-semibold text-slate-600">
          저장된 경기 {sessions.length}
        </span>
      </div>

      {listError ? (
        <div className="mt-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
          {listError}
        </div>
      ) : sessions.length === 0 ? (
        <div className="mt-3 rounded-lg border border-dashed border-slate-300 bg-slate-50 px-4 py-8 text-center text-xs text-slate-500">
          아직 자동 녹화가 저장된 경기가 없습니다. 다음 League 게임부터 여기에 표시됩니다.
        </div>
      ) : (
        <div className="mt-4 grid gap-4 xl:grid-cols-[220px_minmax(0,1fr)]">
          <aside className="max-h-[430px] space-y-2 overflow-y-auto pr-1">
            {sessions.map((row) => {
              const active = row.gameId === selectedGameId;
              return (
                <button
                  type="button"
                  key={`${row.gameId}-${row.startedAtMs}`}
                  onClick={() => setSelectedGameId(row.gameId ?? null)}
                  className={`w-full rounded-lg border px-3 py-2 text-left transition ${
                    active
                      ? "border-indigo-300 bg-indigo-50"
                      : "border-slate-200 bg-white hover:bg-slate-50"
                  }`}
                >
                  <div className="flex items-center justify-between gap-2">
                    <strong className="truncate text-xs text-slate-900">게임 {row.gameId}</strong>
                    <span className={`text-[9px] font-semibold ${row.endedAtMs ? "text-slate-400" : "text-red-600"}`}>
                      {row.endedAtMs ? "저장됨" : "REC"}
                    </span>
                  </div>
                  <div className="mt-1 text-[10px] text-slate-500">{dateLabel(row.startedAtMs)}</div>
                  {row.riotId ? <div className="mt-1 truncate text-[10px] text-slate-600">{row.riotId}</div> : null}
                  {row.roflPath ? <div className="mt-1 truncate text-[9px] text-emerald-600">ROFL · {fileName(row.roflPath)}</div> : null}
                </button>
              );
            })}
          </aside>

          {selected?.gameId && match ? (
            <div className="min-w-0">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <strong className="text-sm text-slate-900">게임 {selected.gameId}</strong>
                  <span className="ml-2 text-[10px] text-slate-500">
                    녹화 시작 게임시간 {timeLabel(offsetMs / 1000)}
                  </span>
                </div>
                <div className="flex items-center gap-2 text-[10px]">
                  {selected.roflPath ? (
                    <span className="rounded-full bg-emerald-50 px-2 py-1 font-semibold text-emerald-700">ROFL 연결됨</span>
                  ) : (
                    <span className="rounded-full bg-amber-50 px-2 py-1 font-semibold text-amber-700">ROFL 대기</span>
                  )}
                  {myPotg ? (
                    <span className="rounded-full bg-amber-100 px-2 py-1 font-semibold text-amber-800">내 POTG</span>
                  ) : null}
                </div>
              </div>

              {selected.endedAtMs ? (
                <>
                  <video
                    ref={videoRef}
                    key={selected.gameId}
                    className="mt-3 aspect-video w-full rounded-xl bg-black"
                    controls
                    preload="metadata"
                    crossOrigin="anonymous"
                    src={`${RECORDING_BASE}/v1/recordings/${encodeURIComponent(selected.gameId)}/video`}
                    onLoadedMetadata={(event) => {
                      const next = event.currentTarget.duration;
                      if (!Number.isFinite(next) || next <= 0) {
                        setDuration(0);
                        void reportRecordingDiagnostic(
                          "video_metadata_invalid",
                          `game_id=${selected.gameId} duration=${String(next)}`,
                        ).catch(() => undefined);
                        return;
                      }
                      setDuration(next);
                    }}
                    onError={(event) => {
                      const mediaError = event.currentTarget.error;
                      void reportRecordingDiagnostic(
                        "video_load_failed",
                        `game_id=${selected.gameId} media_error_code=${mediaError?.code ?? "unknown"}`,
                      ).catch(() => undefined);
                    }}
                    onTimeUpdate={(event) => setCurrentTime(event.currentTarget.currentTime)}
                  />

                  <div className="mt-2">
                    <button
                      type="button"
                      aria-label="녹화 타임라인 이동"
                      onClick={seekTimeline}
                      className="relative h-7 w-full overflow-hidden rounded-md border border-slate-200 bg-slate-100"
                    >
                      <span className="absolute inset-y-0 left-0 bg-indigo-200/60" style={{ width: `${progress}%` }} />
                      {marker && myPotg ? (
                        <span
                          className="absolute inset-y-0 border-x border-amber-500 bg-amber-300/70"
                          style={{ left: `${marker.left}%`, width: `${Math.max(0.7, marker.right - marker.left)}%` }}
                          title={`내 POTG ${timeLabel(myPotg.startMs / 1000)}–${timeLabel(myPotg.endMs / 1000)}`}
                        />
                      ) : null}
                      <span className="absolute inset-y-0 w-px bg-slate-900" style={{ left: `${progress}%` }} />
                    </button>
                    <div className="mt-1 flex items-center justify-between gap-2 text-[10px] text-slate-500">
                      <span>{timeLabel(currentTime)}</span>
                      {myPotg ? (
                        <button
                          type="button"
                          onClick={() => seekVideoGameTime(myPotg.startMs)}
                          className="font-semibold text-amber-700 hover:underline"
                        >
                          내 POTG {timeLabel(myPotg.startMs / 1000)}–{timeLabel(myPotg.endMs / 1000)} · {myPotg.kills}킬
                        </button>
                      ) : (
                        <span>현재 게임시간 {timeLabel(gameTimeMs / 1000)}</span>
                      )}
                      <span>{timeLabel(duration)}</span>
                    </div>
                  </div>
                </>
              ) : (
                <div className="mt-3 rounded-xl bg-red-50 px-4 py-8 text-center text-xs font-medium text-red-700">
                  현재 녹화 중입니다. 경기 종료 후 MP4가 완성되면 여기서 재생할 수 있습니다.
                </div>
              )}

              {contextError ? (
                <p className="mt-2 text-[10px] text-slate-400">POTG 정보: {contextError}</p>
              ) : null}

              <div className="mt-4 border-t border-slate-200 pt-4">
                <div className="mb-2 flex items-center justify-between gap-2">
                  <div>
                    <strong className="text-xs text-slate-900">ROFL 분석</strong>
                    <p className="mt-0.5 text-[10px] text-slate-500">기존 Agent ROFL 분석 API를 사용하며 실제 녹화 시간과 동기화됩니다.</p>
                  </div>
                  <span className="rounded bg-indigo-50 px-2 py-1 text-[9px] font-semibold text-indigo-700">SYNC {timeLabel(gameTimeMs / 1000)}</span>
                </div>
                <div className="overflow-hidden rounded-xl border border-slate-200 bg-slate-950">
                  <ReplayViewer
                    match={match}
                    connected={connected}
                    controlledTimeMs={gameTimeMs}
                    onSeek={seekVideoGameTime}
                    forceRemoteApi
                  />
                </div>
              </div>
            </div>
          ) : null}
        </div>
      )}
    </section>
  );
}
