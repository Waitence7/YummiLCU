import { useCallback, useEffect, useRef, useState } from 'react';
import { downloadMatchReplay, matchReplay } from '../../api/commands';
import { matchTime, replayPosition, type Match, type MatchReplay } from '../../state/matches';
import mapImage from '../../assets/replay/summoners-rift.webp';

export function ReplayViewer({ match, connected }: { match: Match; connected: boolean }) {
  const [replay, setReplay] = useState<MatchReplay | null>(null);
  const [busy, setBusy] = useState<'reading' | 'downloading' | null>('reading');
  const [error, setError] = useState<string | null>(null);
  const [time, setTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const [selected, setSelected] = useState<number | null>(null);
  const generation = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const duration = replay?.durationMs ?? match.duration * 1000;
  const supportedMap = match.mapId === 11;
  const ready = replay?.status === 'ready' && supportedMap;

  const load = useCallback(async (download = false) => {
    const request = ++generation.current;
    clearTimeout(timer.current);
    setBusy(download ? 'downloading' : 'reading');
    setError(null);
    setPlaying(false);
    try {
      if (download) await downloadMatchReplay(match.id);
      const started = Date.now();
      const read = async (): Promise<void> => {
        if (request !== generation.current) return;
        try {
          const result = await matchReplay(match.id);
          if (request !== generation.current) return;
          if (download && result.status === 'missing' && Date.now() - started < 45_000) {
            timer.current = setTimeout(() => { void read(); }, 1500);
            return;
          }
          setReplay(result);
          setTime(0);
          if (download && result.status === 'missing') setError('아직 다운로드가 완료되지 않았습니다. 잠시 후 다시 확인하세요.');
          setBusy(null);
        } catch (caught) {
          if (request !== generation.current) return;
          // Riot can create the file before its envelope is fully written.
          if (download && Date.now() - started < 45_000) {
            timer.current = setTimeout(() => { void read(); }, 1500);
            return;
          }
          setError(caught instanceof Error ? caught.message : String(caught));
          setBusy(null);
        }
      };
      await read();
    } catch (caught) {
      if (request !== generation.current) return;
      setError(caught instanceof Error ? caught.message : String(caught));
      setBusy(null);
    }
  }, [match.id]);

  useEffect(() => {
    void load();
    return () => { generation.current++; clearTimeout(timer.current); };
  }, [load]);

  useEffect(() => {
    if (!playing || !ready) return;
    let previous = performance.now();
    const interval = setInterval(() => {
      const now = performance.now();
      const elapsed = Math.min(now - previous, 1000) * speed;
      previous = now;
      setTime((current) => Math.min(duration, current + elapsed));
    }, 100);
    return () => clearInterval(interval);
  }, [playing, ready, duration, speed]);

  useEffect(() => { if (time >= duration) setPlaying(false); }, [time, duration]);

  function seek(next: number) { setTime(Math.max(0, Math.min(duration, next))); }
  function togglePlay() { if (time >= duration) setTime(0); setPlaying((value) => !value); }

  if (busy || error || !ready) {
    const title = busy === 'downloading' ? '리플레이 다운로드 중' : busy ? '리플레이를 읽고 있습니다' : error ? '리플레이를 불러오지 못했습니다' : replay?.status === 'missing' ? '저장된 리플레이가 없습니다' : !supportedMap ? '이 맵은 아직 재생할 수 없습니다' : replay?.status === 'unsupported' ? '이 패치는 아직 재생할 수 없습니다' : '이동 데이터를 읽을 수 없습니다';
    const detail = busy ? '준비되면 이 화면에서 볼 수 있습니다.' : error ?? (replay?.status === 'missing' ? '롤 클라이언트에서 제공하는 리플레이를 내려받습니다.' : replay?.status === 'unsupported' ? `리플레이 버전 ${replay.version ?? '알 수 없음'} · 경기 결과는 확인할 수 있습니다.` : '경기 결과 탭에서 참가자와 최종 기록을 확인할 수 있습니다.');
    return <div className="match-empty replay-empty" role={error ? 'alert' : 'status'} aria-busy={!!busy}>
      <span className="replay-empty-symbol" aria-hidden="true">{busy ? '···' : '▷'}</span>
      <strong>{title}</strong><p>{detail}</p>
      {!busy && <div className="match-actions">
        {replay?.status === 'missing' && connected && <button className="match-button match-button-primary" onClick={() => void load(true)}>리플레이 다운로드</button>}
        <button className="match-button" onClick={() => void load()}>다시 확인</button>
      </div>}
      {!busy && replay?.status === 'missing' && !connected && <p>다운로드하려면 롤 클라이언트를 실행하세요.</p>}
    </div>;
  }

  return <section className="replay-viewer" aria-label="경기 리플레이" tabIndex={0}
    onKeyDown={(event) => {
      if ((event.target as HTMLElement).closest('button, input, select')) return;
      if (event.code === 'Space') { event.preventDefault(); togglePlay(); }
      if (event.code === 'ArrowLeft') { event.preventDefault(); seek(time - 10000); }
      if (event.code === 'ArrowRight') { event.preventDefault(); seek(time + 10000); }
    }}>
    <div className="replay-caption"><span>동선 리플레이</span><span>선수를 선택하면 위치를 강조합니다</span></div>
    <div className="replay-stage">
      <div className="replay-map" style={{ backgroundImage: `url(${mapImage})` }} aria-label={`${matchTime(time / 1000)} 선수 위치`}>
        {replay.participants?.map((participant) => {
          const point = replayPosition(replay.movements ?? [], participant.participantIndex, time);
          if (!point) return null;
          return <button key={participant.participantIndex} type="button"
            className={`replay-marker ${participant.team === 100 ? 'blue' : 'red'}${selected === participant.participantIndex ? ' selected' : ''}${selected !== null && selected !== participant.participantIndex ? ' muted' : ''}`}
            style={{ left: `${point.x}%`, top: `${point.y}%` }}
            aria-label={`${participant.champion ?? '선수'} 위치`} aria-pressed={selected === participant.participantIndex}
            title={participant.champion ?? `선수 ${participant.participantIndex + 1}`}
            onClick={() => setSelected(selected === participant.participantIndex ? null : participant.participantIndex)}>
            {participant.participantIndex + 1}
          </button>;
        })}
        <span className="replay-map-clock">{matchTime(time / 1000)}</span>
      </div>
      <div className="replay-roster" aria-label="선수 선택">
        {replay.participants?.map((participant) => {
          // ROFL participant indexes have their own ordering. Do not join them
          // to LCU participant IDs or pretend they carry live stats.
          return <button type="button" key={participant.participantIndex}
            className={`replay-player ${participant.team === 100 ? 'blue' : 'red'}${selected === participant.participantIndex ? ' selected' : ''}`}
            aria-pressed={selected === participant.participantIndex}
            onClick={() => setSelected(selected === participant.participantIndex ? null : participant.participantIndex)}>
            <span className="replay-player-number">{participant.participantIndex + 1}</span>
            <span>{participant.champion ?? `선수 ${participant.participantIndex + 1}`}</span>
          </button>;
        })}
      </div>
    </div>
    <div className="replay-controls">
      <input type="range" min={0} max={duration} step={1000} value={time} onChange={(event) => seek(Number(event.target.value))} aria-label="재생 위치" aria-valuetext={`${matchTime(time / 1000)} / ${matchTime(duration / 1000)}`} />
      <div className="replay-control-row">
        <div className="match-actions">
          <button type="button" className="match-button" onClick={() => seek(time - 10000)} aria-label="10초 이전">−10초</button>
          <button type="button" className="match-button match-button-primary replay-play" onClick={togglePlay} aria-label={playing ? '일시정지' : '재생'}>{playing ? 'Ⅱ' : '▶'}</button>
          <button type="button" className="match-button" onClick={() => seek(time + 10000)} aria-label="10초 이후">+10초</button>
        </div>
        <span className="replay-time">{matchTime(time / 1000)} <span>/ {matchTime(duration / 1000)}</span></span>
        <select aria-label="재생 속도" value={speed} onChange={(event) => setSpeed(Number(event.target.value))}>{[0.5, 1, 2, 4, 8].map((value) => <option key={value} value={value}>{value}×</option>)}</select>
      </div>
    </div>
  </section>;
}
