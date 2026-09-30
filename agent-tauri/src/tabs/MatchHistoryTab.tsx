import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { matchFights, matchHistory } from '../api/commands';
import { ChampionPortrait } from '../components/matches/ChampionPortrait';
import { ItemBuild, SpellPair } from '../components/matches/MatchAssets';
import { ReplayViewer } from '../components/matches/ReplayViewer';
import {
  matchDate,
  matchTime,
  queueLabel,
  resultLabel,
  type Match,
  type MatchFight,
  type MatchFightAnalysis,
  type MatchHistory,
  type MatchPlayer,
} from '../state/matches';
import './match-history.css';

type DetailView = 'overview' | 'fights' | 'scoreboard' | 'replay';

const DETAIL_TABS: { id: DetailView; label: string }[] = [
  { id: 'overview', label: '개요' },
  { id: 'fights', label: '한타' },
  { id: 'scoreboard', label: '스코어보드' },
  { id: 'replay', label: '리플레이' },
];

const value = (number: number | null | undefined) => number == null ? '—' : number.toLocaleString('ko-KR');
const compact = (number: number | null | undefined) => {
  if (number == null) return '—';
  if (Math.abs(number) >= 1000) return `${(number / 1000).toFixed(1)}k`;
  return number.toLocaleString('ko-KR');
};
const kda = (player?: MatchPlayer) => `${value(player?.kills)} / ${value(player?.deaths)} / ${value(player?.assists)}`;
const ratio = (player?: MatchPlayer) => {
  if (player?.kills == null || player.deaths == null || player.assists == null) return '—';
  return `${((player.kills + player.assists) / Math.max(1, player.deaths)).toFixed(2)} KDA`;
};
const teamName = (team: number) => team === 100 ? '블루' : team === 200 ? '레드' : `팀 ${team}`;
const teamClass = (team: number | null | undefined) => team === 100 ? 'blue' : team === 200 ? 'red' : 'neutral';

function teamKills(match: Match, team: number) {
  const players = match.players.filter((player) => player.team === team);
  return players.every((player) => player.kills != null)
    ? players.reduce((sum, player) => sum + (player.kills ?? 0), 0)
    : null;
}

function fightOutcome(match: Match, fight: MatchFight) {
  const me = match.players.find((player) => player.id === match.me);
  if (!fight.winnerTeam || !me) return '교환';
  return fight.winnerTeam === me.team ? '한타 승리' : '한타 패배';
}

export function MatchHistoryTab({ connected, active }: { connected: boolean; active: boolean }) {
  const [history, setHistory] = useState<MatchHistory | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [view, setView] = useState<DetailView>('overview');
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
      if (!append) {
        setSelectedId(null);
        scrollTop.current = 0;
      }
    } catch (caught) {
      if (request === generation.current) setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      if (request === generation.current) setBusy(false);
    }
  }, []);

  const wasConnected = useRef(connected);
  useEffect(() => {
    const becameConnected = connected && !wasConnected.current;
    wasConnected.current = connected;
    if (active && !busy && !error && (!history || becameConnected)) void refresh();
  }, [active, connected, history, error, busy, refresh]);
  useEffect(() => () => { generation.current++; }, []);
  useEffect(() => {
    if (!selected && listRef.current) listRef.current.scrollTop = scrollTop.current;
  }, [selected]);

  const matches = useMemo(() => (history?.matches ?? []).filter((match) => {
    const me = match.players.find((player) => player.id === match.me);
    const queueMatches = filter === 'all'
      || (filter === 'custom' ? match.queueId === 0
        : filter === 'ranked' ? [420, 440].includes(match.queueId)
          : match.queueId === 450);
    return queueMatches && `${me?.champion ?? ''} ${match.players.map((player) => player.name).join(' ')} ${match.id}`
      .toLocaleLowerCase().includes(query.trim().toLocaleLowerCase());
  }), [history, filter, query]);

  const wins = matches.filter((match) => match.players.find((player) => player.id === match.me)?.win === true).length;
  const losses = matches.filter((match) => match.players.find((player) => player.id === match.me)?.win === false).length;

  function choose(match: Match) {
    scrollTop.current = listRef.current?.scrollTop ?? 0;
    setSelectedId(match.id);
    setView('overview');
  }

  const rememberFights = useCallback((gameId: string, analysis: MatchFightAnalysis) => {
    setHistory((current) => current ? {
      ...current,
      matches: current.matches.map((match) => match.id === gameId ? {
        ...match,
        fights: analysis.fights,
        fightsAnalyzedAt: analysis.analyzedAt,
        fightAnalysisMethod: analysis.method,
      } : match),
    } : current);
  }, []);

  return <div className={`match-history${selected ? ' has-selection' : ''}`}>
    <div className="match-history-heading">
      <div className="match-heading-copy">
        <span className="match-eyebrow">MATCH HISTORY</span>
        <div className="match-heading-line">
          <h1>전적</h1>
          <span>{history?.summoner || '내 경기'}</span>
          {history && <span className="match-record">{wins}승 {losses}패 <span>· {matches.length}경기{history.source === 'cache' ? ' · 저장됨' : ''}</span></span>}
        </div>
      </div>
      <button className="match-button" disabled={busy} onClick={() => void refresh()}>
        {busy ? '불러오는 중…' : connected ? '새로고침' : '저장된 전적 새로고침'}
      </button>
    </div>

    {!connected && <p className="match-notice">
      {history?.source === 'cache'
        ? '롤 클라이언트가 꺼져 있어 저장된 전적을 표시합니다. 다시 연결되면 최신 전적으로 갱신합니다.'
        : '롤 클라이언트를 실행하면 최근 전적을 불러와 저장합니다.'}
    </p>}
    {error && <p className="match-notice match-error" role="alert">
      {error}<button disabled={busy || !connected} onClick={() => void refresh()}>다시 시도</button>
    </p>}

    <div className="match-workspace">
      <section className="match-list-pane" aria-label="전적 목록">
        <div className="match-list-filters">
          <input type="search" value={query} onChange={(event) => setQuery(event.target.value)}
            placeholder="챔피언, 소환사 검색" aria-label="전적 검색" />
          <select value={filter} onChange={(event) => setFilter(event.target.value)} aria-label="게임 모드">
            <option value="all">전체</option>
            <option value="custom">내전</option>
            <option value="ranked">랭크</option>
            <option value="aram">총력전</option>
          </select>
        </div>

        <div className="match-list-scroll" ref={listRef} aria-busy={busy}>
          {busy && !history
            ? <div className="match-loading" role="status">전적을 불러오는 중…
              {Array.from({ length: 5 }, (_, index) => <div className="match-skeleton" key={index} />)}
            </div>
            : matches.length
              ? <>
                <ol className="match-list">{matches.map((match) => {
                  const me = match.players.find((player) => player.id === match.me);
                  const hasFightAnalysis = match.fights !== undefined;
                  const fights = match.fights?.length ?? 0;
                  return <li key={match.id}>
                    <button
                      className={`match-row ${me?.win === true ? 'win' : me?.win === false ? 'loss' : 'unknown'}${match.id === selectedId ? ' selected' : ''}`}
                      onClick={() => choose(match)}
                      aria-pressed={match.id === selectedId}
                      aria-label={`${resultLabel(me?.win)} ${me?.champion ?? '경기'} ${queueLabel(match.queueId)} ${matchDate(match.createdAt)}`}
                    >
                      <span className="match-result-rail" aria-hidden="true" />
                      <span className="match-result">
                        <strong>{resultLabel(me?.win)}</strong>
                        <span>{matchTime(match.duration)}</span>
                      </span>
                      <ChampionPortrait id={me?.championId ?? 0} name={me?.champion ?? '?'} />
                      <span className="match-card-main">
                        <span className="match-card-title">
                          <strong>{me?.champion ?? '참가자 정보 없음'}</strong>
                          <span>{queueLabel(match.queueId)}</span>
                        </span>
                        <span className="match-card-loadout">
                          <SpellPair spells={me?.summonerSpells ?? []} version={match.version} compact />
                          <ItemBuild items={me?.items ?? []} version={match.version} compact />
                        </span>
                      </span>
                      <span className="match-performance">
                        <strong>{kda(me)}</strong>
                        <span>{ratio(me)} · CS {value(me?.cs)}</span>
                      </span>
                      <span className="match-card-analysis">
                        <strong>{hasFightAnalysis ? `한타 ${fights}회` : 'ROFL 분석'}</strong>
                        <span>{hasFightAnalysis ? '분석 완료' : '상세에서 분석'}</span>
                      </span>
                      <span className="match-when">{matchDate(match.createdAt)}</span>
                      <span className="match-row-arrow" aria-hidden="true">›</span>
                    </button>
                  </li>;
                })}</ol>
                {history?.hasMore && <button className="match-load-more" disabled={busy} onClick={() => void refresh(true)}>
                  {busy ? '불러오는 중…' : '이전 경기 더 보기'}
                </button>}
              </>
              : <div className="match-empty">
                <strong>{query || filter !== 'all' ? '검색 결과가 없습니다' : connected ? '최근 경기가 없습니다' : '전적을 기다리고 있습니다'}</strong>
                <p>{query || filter !== 'all'
                  ? '다른 검색어나 게임 모드를 선택하세요.'
                  : connected ? '경기가 끝난 뒤 새로고침해 주세요.' : '롤 로그인 후 이 화면에서 확인할 수 있습니다.'}</p>
              </div>}
        </div>
      </section>

      {selected && <section className="match-detail" aria-label="선택한 경기" key={selected.id}>
        <div className="match-detail-heading">
          <button className="match-back" onClick={() => setSelectedId(null)} aria-label="전적 목록으로">‹ <span>전적 목록</span></button>
          <span>{queueLabel(selected.queueId)} · {matchTime(selected.duration)}</span>
          <time>{matchDate(selected.createdAt)}</time>
        </div>

        <MatchHero match={selected} />

        <div className="match-detail-tabs" role="tablist" aria-label="경기 보기">
          {DETAIL_TABS.map((tab) => <button
            key={tab.id}
            id={`match-${tab.id}-tab`}
            role="tab"
            aria-selected={view === tab.id}
            aria-controls={`match-${tab.id}-panel`}
            tabIndex={view === tab.id ? 0 : -1}
            onClick={() => setView(tab.id)}
            onKeyDown={(event) => {
              const current = DETAIL_TABS.findIndex((item) => item.id === view);
              const direction = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
              if (!direction) return;
              event.preventDefault();
              const next = DETAIL_TABS[(current + direction + DETAIL_TABS.length) % DETAIL_TABS.length];
              setView(next.id);
              document.getElementById(`match-${next.id}-tab`)?.focus();
            }}
          >{tab.label}{tab.id === 'fights' && selected.fights !== undefined && <em>{selected.fights.length}</em>}</button>)}
          <span>#{selected.id}</span>
        </div>

        <div className={`match-detail-scroll${view === 'replay' ? ' is-replay' : ''}`}>
          {view === 'overview' && <div id="match-overview-panel" role="tabpanel" aria-labelledby="match-overview-tab">
            <MatchOverview match={selected} onFights={() => setView('fights')} />
          </div>}
          {view === 'fights' && <div id="match-fights-panel" role="tabpanel" aria-labelledby="match-fights-tab">
            <FightExplorer match={selected} onLoaded={rememberFights} />
          </div>}
          {view === 'scoreboard' && <div id="match-scoreboard-panel" role="tabpanel" aria-labelledby="match-scoreboard-tab">
            <MatchScoreboard match={selected} />
          </div>}
          {view === 'replay' && <div id="match-replay-panel" role="tabpanel" aria-labelledby="match-replay-tab">
            {active && <ReplayViewer match={selected} connected={connected} />}
          </div>}
        </div>
      </section>}
    </div>
  </div>;
}

function MatchHero({ match }: { match: Match }) {
  const me = match.players.find((player) => player.id === match.me);
  const blueKills = teamKills(match, 100);
  const redKills = teamKills(match, 200);
  return <section className={`match-hero ${me?.win === true ? 'win' : me?.win === false ? 'loss' : 'unknown'}`}>
    <div className="match-hero-grid" aria-hidden="true" />
    <div className="match-hero-player">
      <ChampionPortrait id={me?.championId ?? 0} name={me?.champion ?? '?'} />
      <div>
        <span className="match-hero-result">{resultLabel(me?.win)}</span>
        <h2>{me?.champion ?? '경기 결과'}</h2>
        <p>{queueLabel(match.queueId)} · 패치 {match.version || '—'}</p>
      </div>
    </div>
    <div className="match-hero-score" aria-label="팀 킬 스코어">
      <span className="blue">BLUE</span>
      <strong>{value(blueKills)} <i>:</i> {value(redKills)}</strong>
      <span className="red">RED</span>
    </div>
    <div className="match-hero-stats">
      <span><small>K / D / A</small><strong>{kda(me)}</strong><em>{ratio(me)}</em></span>
      <span><small>챔피언 피해</small><strong>{compact(me?.damage)}</strong><em>{value(me?.damage)}</em></span>
      <span><small>CS · 골드</small><strong>{value(me?.cs)} CS</strong><em>{compact(me?.gold)} G</em></span>
    </div>
    {me && <div className="match-hero-loadout">
      <SpellPair spells={me.summonerSpells} version={match.version} />
      <ItemBuild items={me.items} version={match.version} />
    </div>}
  </section>;
}

function MatchOverview({ match, onFights }: { match: Match; onFights(): void }) {
  const me = match.players.find((player) => player.id === match.me);
  const fightsAnalyzed = match.fights !== undefined;
  const fights = match.fights ?? [];
  const wonFights = fights.filter((fight) => fight.winnerTeam && fight.winnerTeam === me?.team).length;
  const lostFights = fights.filter((fight) => fight.winnerTeam && fight.winnerTeam !== me?.team).length;
  const biggest = fights.reduce<MatchFight | null>((best, fight) => {
    if (!best) return fight;
    return Math.abs(fight.goldSwing ?? 0) > Math.abs(best.goldSwing ?? 0) ? fight : best;
  }, null);

  return <div className="match-overview">
    <section className="match-overview-metrics" aria-label="경기 핵심 지표">
      <Metric label="KDA" value={ratio(me)} detail={kda(me)} />
      <Metric label="피해량" value={compact(me?.damage)} detail="챔피언 대상" />
      <Metric label="CS" value={value(me?.cs)} detail={me?.lane || '라인 정보 없음'} />
      <Metric label="시야 점수" value={value(me?.vision)} detail={me?.role || '역할 정보 없음'} />
    </section>

    <section className="match-section match-fight-overview">
      <div className="match-section-heading">
        <div><span className="match-eyebrow">ROFL COMBAT</span><h3>한타 흐름</h3></div>
        {!!fights.length && <button className="match-text-button" onClick={onFights}>자세히 보기 <span>›</span></button>}
      </div>
      {fights.length ? <>
        <div className="fight-round-strip overview">
          {fights.map((fight, index) => <button key={fight.id} onClick={onFights} className={`fight-round ${teamClass(fight.winnerTeam)}`}>
            <span className="fight-round-number">한타 {index + 1}</span>
            <strong>{matchTime(fight.startMs / 1000)}</strong>
            <small>{fight.location || '위치 미상'}</small>
            <em>{fight.blueKills} : {fight.redKills}</em>
          </button>)}
        </div>
        <div className="match-fight-summary">
          <span><small>분석된 한타</small><strong>{fights.length}</strong></span>
          <span><small>한타 승패</small><strong>{wonFights}승 {lostFights}패</strong></span>
          <span><small>가장 큰 전환</small><strong>{biggest ? `${biggest.goldSwing != null && biggest.goldSwing > 0 ? '+' : ''}${compact(biggest?.goldSwing)} G` : '—'}</strong></span>
          <span><small>결정적 위치</small><strong>{biggest?.location || '—'}</strong></span>
        </div>
      </> : <div className="match-analysis-empty">
        <span className="match-analysis-mark">ROFL</span>
        <div>
          <strong>{fightsAnalyzed ? '의미 있는 한타가 감지되지 않았습니다' : '한타 분석 데이터가 아직 없습니다'}</strong>
          <p>{fightsAnalyzed ? '이 경기는 한타 기준을 만족한 대규모 교전이 없었습니다.' : '한타 탭을 열면 ROFL에서 전투 이벤트를 복원해 라운드처럼 정리합니다.'}</p>
        </div>
        {!fightsAnalyzed && <button className="match-button" onClick={onFights}>한타 분석</button>}
      </div>}
    </section>

    <section className="match-section">
      <div className="match-section-heading"><div><span className="match-eyebrow">TEAMS</span><h3>팀 구성</h3></div></div>
      <div className="match-team-preview-grid">
        <TeamPreview match={match} team={100} />
        <TeamPreview match={match} team={200} />
      </div>
    </section>
  </div>;
}

function Metric({ label, value: metricValue, detail }: { label: string; value: string; detail: string }) {
  return <div className="match-metric"><small>{label}</small><strong>{metricValue}</strong><span>{detail}</span></div>;
}

function TeamPreview({ match, team }: { match: Match; team: number }) {
  const players = match.players.filter((player) => player.team === team);
  return <div className={`match-team-preview ${teamClass(team)}`}>
    <div className="match-team-preview-heading"><strong>{teamName(team)} 팀</strong><span>{value(teamKills(match, team))} 킬</span></div>
    <ul>{players.map((player) => <li key={player.id} className={player.id === match.me ? 'is-me' : ''}>
      <ChampionPortrait id={player.championId} name={player.champion} small />
      <span className="match-team-preview-name"><strong>{player.name}</strong><small>{player.champion}</small></span>
      <span className="match-team-preview-kda">{kda(player)}</span>
    </li>)}</ul>
  </div>;
}

function MatchScoreboard({ match }: { match: Match }) {
  const teams = [...new Set(match.players.map((player) => player.team))];
  return <div className="match-scoreboard">
    {teams.map((team) => {
      const players = match.players.filter((player) => player.team === team);
      const maxDamage = Math.max(1, ...players.map((player) => player.damage ?? 0));
      return <section className={`match-team ${teamClass(team)}`} key={team}>
        <div className="match-team-heading">
          <div><strong>{teamName(team)} 팀</strong><span className={players[0]?.win === true ? 'result-win' : players[0]?.win === false ? 'result-loss' : ''}>{resultLabel(players[0]?.win)}</span></div>
          <span>{value(teamKills(match, team))} 킬</span>
        </div>
        <div className="match-scoreboard-scroll">
          <table>
            <caption className="sr-only">{teamName(team)} 팀 최종 기록</caption>
            <thead><tr><th>소환사</th><th>빌드</th><th>K / D / A</th><th>CS</th><th>골드</th><th>챔피언 피해량</th></tr></thead>
            <tbody>{players.map((player) => <tr key={player.id} className={player.id === match.me ? 'is-me' : ''}>
              <td><div className="match-player-name">
                <ChampionPortrait id={player.championId} name={player.champion} small />
                <SpellPair spells={player.summonerSpells} version={match.version} compact />
                <span><strong title={player.name}>{player.name}{player.id === match.me && <em>나</em>}</strong><span>{player.champion}{player.level != null ? ` · Lv.${player.level}` : ''}</span></span>
              </div></td>
              <td><ItemBuild items={player.items} version={match.version} compact /></td>
              <td className="match-numeric"><strong>{kda(player)}</strong><small>{ratio(player)}</small></td>
              <td className="match-numeric">{value(player.cs)}</td>
              <td className="match-numeric">{compact(player.gold)}</td>
              <td className="match-damage"><span>{value(player.damage)}</span><div><i style={{ width: `${(player.damage ?? 0) / maxDamage * 100}%` }} /></div></td>
            </tr>)}</tbody>
          </table>
        </div>
      </section>;
    })}
    {!match.players.length && <div className="match-empty">참가자 정보를 불러오지 못했습니다.</div>}
    <p className="match-result-note">경기 종료 시점의 기록{match.version ? ` · 패치 ${match.version}` : ''}</p>
  </div>;
}

function FightExplorer({ match, onLoaded }: { match: Match; onLoaded(gameId: string, analysis: MatchFightAnalysis): void }) {
  const [fights, setFights] = useState<MatchFight[] | undefined>(match.fights);
  const [analysisState, setAnalysisState] = useState<'loading' | 'ready' | 'error'>(match.fights === undefined ? 'loading' : 'ready');
  const [analysisError, setAnalysisError] = useState<string | null>(null);
  const generation = useRef(0);

  const load = useCallback(async () => {
    const request = ++generation.current;
    setAnalysisState('loading');
    setAnalysisError(null);
    try {
      const analysis = await matchFights(match.id);
      if (request !== generation.current) return;
      setFights(analysis.fights);
      setAnalysisState('ready');
      onLoaded(match.id, analysis);
    } catch (caught) {
      if (request !== generation.current) return;
      setAnalysisState('error');
      setAnalysisError(caught instanceof Error ? caught.message : String(caught));
    }
  }, [match.id, onLoaded]);

  useEffect(() => {
    generation.current++;
    setFights(match.fights);
    setAnalysisError(null);
    if (match.fights === undefined) void load();
    else setAnalysisState('ready');
    return () => { generation.current++; };
  }, [match.id, match.fights, load]);

  if (analysisState === 'loading') return <div className="match-empty match-fights-empty" role="status" aria-busy="true">
    <span className="match-analysis-mark">ROFL</span>
    <strong>한타를 분석하고 있습니다</strong>
    <p>저장된 리플레이를 확인하고, 필요한 경우 롤 클라이언트에서 내려받은 뒤 Yummi ROFL 분석기로 전투 이벤트를 복원합니다.</p>
  </div>;

  if (analysisState === 'error') return <div className="match-empty match-fights-empty" role="alert">
    <span className="match-analysis-mark">ROFL</span>
    <strong>한타 분석을 완료하지 못했습니다</strong>
    <p>{analysisError || '분석 중 오류가 발생했습니다.'}</p>
    <button className="match-button match-button-primary" onClick={() => void load()}>다시 분석</button>
  </div>;

  if (!fights?.length) return <div className="match-empty match-fights-empty">
    <span className="match-analysis-mark">ROFL</span>
    <strong>의미 있는 한타가 감지되지 않았습니다</strong>
    <p>분석은 완료됐지만 2대2 이상 참여 또는 충분한 챔피언 피해·사망 조건을 만족한 교전이 없었습니다.</p>
  </div>;

  return <FightExplorerContent match={{ ...match, fights }} />;
}

function FightExplorerContent({ match }: { match: Match }) {
  const fights = match.fights ?? [];
  const [selectedFightId, setSelectedFightId] = useState(fights[0]?.id ?? null);
  useEffect(() => setSelectedFightId(fights[0]?.id ?? null), [match.id, fights[0]?.id]);
  const fight = fights.find((item) => item.id === selectedFightId) ?? fights[0];

  if (!fight) return <div className="match-empty match-fights-empty">
    <span className="match-analysis-mark">ROFL</span>
    <strong>분석된 한타가 없습니다</strong>
    <p>이 경기의 ROFL에서 한타 감지가 완료되면, 각 한타가 라운드처럼 여기에 표시됩니다. 실제 분석 데이터가 없을 때는 추정값을 만들지 않습니다.</p>
  </div>;

  const index = fights.findIndex((item) => item.id === fight.id);
  const participants = fight.participants ?? [];
  const involved = participants
    .map((participant) => ({ participant, player: match.players.find((player) => player.id === participant.playerId) }))
    .filter((row): row is { participant: typeof participants[number]; player: MatchPlayer } => !!row.player);

  return <div className="fight-explorer">
    <section className="fight-rounds-panel">
      <div className="match-section-heading compact">
        <div><span className="match-eyebrow">TEAMFIGHTS</span><h3>한타 선택</h3></div>
        <span>{fights.length}개 감지</span>
      </div>
      <div className="fight-round-strip">
        {fights.map((item, fightIndex) => <button key={item.id}
          className={`fight-round ${teamClass(item.winnerTeam)}${item.id === fight.id ? ' selected' : ''}`}
          onClick={() => setSelectedFightId(item.id)}
          aria-pressed={item.id === fight.id}>
          <span className="fight-round-number">한타 {fightIndex + 1}</span>
          <strong>{matchTime(item.startMs / 1000)}</strong>
          <small>{item.location || '위치 미상'}</small>
          <em>{item.blueKills} : {item.redKills}</em>
        </button>)}
      </div>
    </section>

    <section className={`fight-detail-card ${teamClass(fight.winnerTeam)}`}>
      <div className="fight-detail-title">
        <div>
          <span className="match-eyebrow">TEAMFIGHT {String(index + 1).padStart(2, '0')}</span>
          <h3>{fightOutcome(match, fight)}</h3>
          <p>{matchTime(fight.startMs / 1000)} – {matchTime(fight.endMs / 1000)} · {fight.location || '위치 미상'}</p>
        </div>
        <div className="fight-detail-score">
          <span>BLUE</span><strong>{fight.blueKills}</strong><i>:</i><strong>{fight.redKills}</strong><span>RED</span>
        </div>
      </div>

      <div className="fight-detail-meta">
        <span><small>교전 시간</small><strong>{Math.max(1, Math.round((fight.endMs - fight.startMs) / 1000))}초</strong></span>
        <span><small>골드 변화</small><strong>{fight.goldSwing == null ? '—' : `${fight.goldSwing > 0 ? '+' : ''}${value(fight.goldSwing)} G`}</strong></span>
        <span><small>후속 결과</small><strong>{fight.objective || '오브젝트 없음'}</strong></span>
        <span><small>승리 팀</small><strong>{fight.winnerTeam ? `${teamName(fight.winnerTeam)} 팀` : '교환'}</strong></span>
      </div>

      {fight.summary && <p className="fight-summary">{fight.summary}</p>}

      <div className="fight-detail-columns">
        <div className="fight-participants">
          <h4>참여자</h4>
          <div className="fight-participant-list">
            {involved.map(({ participant, player }) => <div key={player.id} className={`fight-participant ${teamClass(player.team)}`}>
              <ChampionPortrait id={player.championId} name={player.champion} small />
              <span className="fight-participant-name"><strong>{player.name}</strong><small>{player.champion}</small></span>
              <span className="fight-participant-kda">{value(participant.kills)} / {value(participant.deaths)} / {value(participant.assists)}</span>
              <span className="fight-participant-damage">{compact(participant.damage)} 피해</span>
            </div>)}
            {!involved.length && <p className="fight-muted">참여자 상세 데이터가 없습니다.</p>}
          </div>
        </div>

        <div className="fight-events">
          <h4>교전 타임라인</h4>
          <ol>{(fight.events ?? []).map((event, eventIndex) => <li key={`${event.atMs}-${eventIndex}`} className={teamClass(event.team)}>
            <time>{matchTime(event.atMs / 1000)}</time>
            <span className="fight-event-dot" aria-hidden="true" />
            <div><strong>{event.label}</strong>{event.detail && <p>{event.detail}</p>}</div>
          </li>)}</ol>
          {!fight.events?.length && <p className="fight-muted">세부 이벤트 데이터가 없습니다.</p>}
        </div>
      </div>
    </section>
  </div>;
}
