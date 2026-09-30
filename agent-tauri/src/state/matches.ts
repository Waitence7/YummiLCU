export type MatchPlayer = {
  id: number;
  name: string;
  championId: number;
  champion: string;
  team: number;
  win: boolean | null;
  kills: number | null;
  deaths: number | null;
  assists: number | null;
  cs: number | null;
  gold: number | null;
  damage: number | null;
  items: number[];
};

export type Match = {
  id: string;
  createdAt: number | null;
  duration: number;
  queueId: number;
  mapId: number;
  version: string;
  me: number | null;
  players: MatchPlayer[];
};

export type MatchHistory = {
  summoner: string;
  matches: Match[];
  hasMore: boolean;
};

export type ReplayParticipant = {
  participantIndex: number;
  champion: string | null;
  team: number | null;
};

export type ReplayMovement = {
  kind: 'movement';
  startMs: number;
  endMs: number;
  stepMs: number;
  players: { participantIndex: number; xy: number[] }[];
};

export type MatchReplay = {
  status: 'ready' | 'missing' | 'unsupported' | 'unavailable';
  version?: string;
  durationMs?: number;
  participants?: ReplayParticipant[];
  movements?: ReplayMovement[];
};

export function queueLabel(queue: number) {
  return ({ 0: '내전', 400: '일반', 420: '솔로 랭크', 430: '일반', 440: '자유 랭크', 450: '무작위 총력전', 480: '빠른 대전', 1700: '아레나' } as Record<number, string>)[queue] ?? `기타 · ${queue}`;
}

export function matchTime(seconds: number) {
  return `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;
}

export function matchDate(timestamp: number | null) {
  if (!timestamp) return '날짜 없음';
  return new Date(timestamp).toLocaleString('ko-KR', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false });
}

export function resultLabel(win: boolean | null | undefined) {
  return win === true ? '승리' : win === false ? '패배' : '결과 없음';
}

// Never interpolate across chunk boundaries or display a missing (0, 0) sample.
export function replayPosition(movements: ReplayMovement[], player: number, timeMs: number) {
  const chunk = movements.find((part) => timeMs >= part.startMs && timeMs <= part.endMs);
  const track = chunk?.players.find((row) => row.participantIndex === player);
  if (!chunk || !track || chunk.stepMs <= 0) return null;
  const index = Math.floor((timeMs - chunk.startMs) / chunk.stepMs) * 2;
  const x = track.xy[index];
  const y = track.xy[index + 1];
  if (!Number.isFinite(x) || !Number.isFinite(y) || (x === 0 && y === 0)) return null;
  return { x: Math.max(0, Math.min(100, x / 150)), y: Math.max(0, Math.min(100, 100 - y / 150)) };
}
