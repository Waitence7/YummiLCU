import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { matchHistory } from '../api/commands';
import { ChampionPortrait } from '../components/matches/ChampionPortrait';
import { ReplayViewer } from '../components/matches/ReplayViewer';
import { matchDate, matchTime, queueLabel, resultLabel, type Match, type MatchHistory, type MatchPlayer } from '../state/matches';
import './match-history.css';

const value = (number: number | null | undefined) => number == null ? '—' : number.toLocaleString('ko-KR');
const kda = (player?: MatchPlayer) => `${value(player?.kills)} / ${value(player?.deaths)} / ${value(player?.assists)}`;

export function MatchHistoryTab({ connected, active }: { connected: boolean; active: boolean }) {
  const [history, setHistory] = useState<MatchHistory | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [view, setView] = useState<'result' | 'replay'>('result');
  const generation = useRef(0);
  const nextOffset = useRef(0);
  const listRef = useRef<HTMLDivElement>(null);
  const scrollTop = useRef(0);
  const selected = history?.matches.find((match) => match.id === selectedId);

  const refresh = useCallback(async (append = false) => {
    const request = ++generation.current;
    setBusy(true);
    setError(null);
    try {
      const offset = append ? nextOffset.current : 0;
      const result = await matchHistory(offset);
      if (request !== generation.current) return;
      nextOffset.current = offset + 20;
      setHistory((current) => {
        if (!append || !current || current.summoner !== result.summoner) return result;
        const ids = new Set(current.matches.map((match) => match.id));
        return { ...result, matches: [...current.matches, ...result.matches.filter((match) => !ids.has(match.id))] };
      });
      if (!append) { setSelectedId(null); scrollTop.current = 0; }
    } catch (caught) {
      if (request === generation.current) setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      if (request === generation.current) setBusy(false);
    }
  }, []);

  useEffect(() => {
    if (active && connected && !history && !error && !busy) void refresh();
  }, [active, connected, history, error, busy, refresh]);
  useEffect(() => () => { generation.current++; }, []);
  useEffect(() => { if (!selected && listRef.current) listRef.current.scrollTop = scrollTop.current; }, [selected]);

  const matches = useMemo(() => (history?.matches ?? []).filter((match) => {
    const me = match.players.find((player) => player.id === match.me);
    const queueMatches = filter === 'all' || (filter === 'custom' ? match.queueId === 0 : filter === 'ranked' ? [420, 440].includes(match.queueId) : match.queueId === 450);
    return queueMatches && `${me?.champion ?? ''} ${match.players.map((player) => player.name).join(' ')} ${match.id}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase());
  }), [history, filter, query]);
  const wins = matches.filter((match) => match.players.find((player) => player.id === match.me)?.win === true).length;
  const losses = matches.filter((match) => match.players.find((player) => player.id === match.me)?.win === false).length;

  function choose(match: Match) {
    scrollTop.current = listRef.current?.scrollTop ?? 0;
    setSelectedId(match.id);
    setView('result');
  }

  return <div className={`match-history${selected ? ' has-selection' : ''}`}>
    <div className="match-history-heading">
      <div><h1>전적</h1><span>{history?.summoner || '내 경기'}{history && <span className="match-record">{wins}승 {losses}패 <span>· {matches.length}경기</span></span>}</span></div>
      <button className="match-button" disabled={busy || !connected} onClick={() => void refresh()}>{busy ? '불러오는 중…' : '새로고침'}</button>
    </div>
    {!connected && <p className="match-notice">{history ? '롤 클라이언트 연결이 끊겼습니다. 불러온 전적을 표시합니다.' : '롤 클라이언트를 실행하면 최근 전적을 불러옵니다.'}</p>}
    {error && <p className="match-notice match-error" role="alert">{error}<button disabled={busy || !connected} onClick={() => void refresh()}>다시 시도</button></p>}
    <div className="match-workspace">
      <section className="match-list-pane" aria-label="전적 목록">
        <div className="match-list-filters">
          <input type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="챔피언, 소환사 검색" aria-label="전적 검색" />
          <select value={filter} onChange={(event) => setFilter(event.target.value)} aria-label="게임 모드"><option value="all">전체</option><option value="custom">내전</option><option value="ranked">랭크</option><option value="aram">총력전</option></select>
        </div>
        <div className="match-list-scroll" ref={listRef} aria-busy={busy}>
          {busy && !history ? <div className="match-loading" role="status">전적을 불러오는 중…{Array.from({ length: 5 }, (_, index) => <div className="match-skeleton" key={index} />)}</div> : matches.length ? <>
            <ol className="match-list">{matches.map((match) => {
              const me = match.players.find((player) => player.id === match.me);
              return <li key={match.id}><button className={`match-row ${me?.win === true ? 'win' : me?.win === false ? 'loss' : 'unknown'}${match.id === selectedId ? ' selected' : ''}`} onClick={() => choose(match)} aria-pressed={match.id === selectedId} aria-label={`${resultLabel(me?.win)} ${me?.champion ?? '경기'} ${queueLabel(match.queueId)} ${matchDate(match.createdAt)}`}>
                <span className="match-result"><strong>{resultLabel(me?.win)}</strong><span>{matchTime(match.duration)}</span></span>
                <ChampionPortrait id={me?.championId ?? 0} name={me?.champion ?? '?'} />
                <span className="match-champion"><strong>{me?.champion ?? '참가자 정보 없음'}</strong><span>{queueLabel(match.queueId)}</span></span>
                <span className="match-performance"><strong>{kda(me)}</strong><span>CS {value(me?.cs)}</span></span>
                <span className="match-when">{matchDate(match.createdAt)}</span><span className="match-row-arrow" aria-hidden="true">›</span>
              </button></li>;
            })}</ol>
            {history?.hasMore && <button className="match-load-more" disabled={busy || !connected} onClick={() => void refresh(true)}>{busy ? '불러오는 중…' : '이전 경기 더 보기'}</button>}
          </> : <div className="match-empty"><strong>{query || filter !== 'all' ? '검색 결과가 없습니다' : connected ? '최근 경기가 없습니다' : '전적을 기다리고 있습니다'}</strong><p>{query || filter !== 'all' ? '다른 검색어나 게임 모드를 선택하세요.' : connected ? '경기가 끝난 뒤 새로고침해 주세요.' : '롤 로그인 후 이 화면에서 확인할 수 있습니다.'}</p></div>}
        </div>
      </section>
      {selected && <section className="match-detail" aria-label="선택한 경기" key={selected.id}>
        <div className="match-detail-heading">
          <button className="match-back" onClick={() => setSelectedId(null)} aria-label="전적 목록으로">‹ <span>전적 목록</span></button>
          <span>{queueLabel(selected.queueId)} · {matchTime(selected.duration)}</span>
          <time>{matchDate(selected.createdAt)}</time>
        </div>
        <div className="match-detail-tabs" role="tablist" aria-label="경기 보기">
          <button id="match-result-tab" role="tab" aria-selected={view === 'result'} aria-controls="match-result-panel" tabIndex={view === 'result' ? 0 : -1} onClick={() => setView('result')} onKeyDown={(event) => { if (event.key === 'ArrowRight') { setView('replay'); document.getElementById('match-replay-tab')?.focus(); } }}>경기 결과</button>
          <button id="match-replay-tab" role="tab" aria-selected={view === 'replay'} aria-controls="match-replay-panel" tabIndex={view === 'replay' ? 0 : -1} onClick={() => setView('replay')} onKeyDown={(event) => { if (event.key === 'ArrowLeft') { setView('result'); document.getElementById('match-result-tab')?.focus(); } }}>리플레이</button>
          <span>#{selected.id}</span>
        </div>
        <div className={`match-detail-scroll${view === 'replay' ? ' is-replay' : ''}`}>
          {view === 'result' ? <div id="match-result-panel" role="tabpanel" aria-labelledby="match-result-tab"><MatchResult match={selected} onReplay={() => setView('replay')} /></div> : <div id="match-replay-panel" role="tabpanel" aria-labelledby="match-replay-tab">{active && <ReplayViewer match={selected} connected={connected} />}</div>}
        </div>
      </section>}
    </div>
  </div>;
}

function MatchResult({ match, onReplay }: { match: Match; onReplay(): void }) {
  const me = match.players.find((player) => player.id === match.me);
  const teams = [...new Set(match.players.map((player) => player.team))];
  return <>
    <div className="match-result-summary">
      {me && <><ChampionPortrait id={me.championId} name={me.champion} /><div><strong>{me.champion} <span className={me.win === true ? 'result-win' : me.win === false ? 'result-loss' : ''}>{resultLabel(me.win)}</span></strong><p>{kda(me)} <span>· CS {value(me.cs)}</span></p></div></>}
      <button className="match-button match-button-primary" onClick={onReplay}>리플레이 보기 <span aria-hidden="true">›</span></button>
    </div>
    {teams.map((team) => {
      const players = match.players.filter((player) => player.team === team);
      const maxDamage = Math.max(1, ...players.map((player) => player.damage ?? 0));
      return <div className="match-team" key={team}>
        <div className="match-team-heading"><strong>{team === 100 ? '블루 팀' : team === 200 ? '레드 팀' : `팀 ${team}`} <span className={players[0]?.win === true ? 'result-win' : players[0]?.win === false ? 'result-loss' : ''}>{resultLabel(players[0]?.win)}</span></strong><span>{players.every((player) => player.kills != null) ? players.reduce((sum, player) => sum + (player.kills ?? 0), 0) : '—'} 킬</span></div>
        <table><caption className="sr-only">{team} 팀 최종 기록</caption><thead><tr><th>소환사</th><th>K / D / A</th><th>CS</th><th>챔피언 피해량</th></tr></thead><tbody>{players.map((player) => <tr key={player.id} className={player.id === match.me ? 'is-me' : ''}>
          <td><div className="match-player-name"><ChampionPortrait id={player.championId} name={player.champion} small /><span><strong title={player.name}>{player.name}{player.id === match.me && <em>나</em>}</strong><span>{player.champion}</span></span></div></td>
          <td className="match-numeric">{kda(player)}</td><td className="match-numeric">{value(player.cs)}</td><td className="match-damage"><span>{value(player.damage)}</span><div><i style={{ width: `${(player.damage ?? 0) / maxDamage * 100}%` }} /></div></td>
        </tr>)}</tbody></table>
      </div>;
    })}
    {!match.players.length && <div className="match-empty">참가자 정보를 불러오지 못했습니다.</div>}
    <p className="match-result-note">경기 종료 시점의 기록{match.version ? ` · 패치 ${match.version}` : ''}</p>
  </>;
}
