// Development preview fixtures only. Imported exclusively by mock.ts.
import type { Match, MatchHistory, MatchReplay } from '../state/matches';

const roster = [
  [164, '카밀', 'Camille', '모서리'], [64, '리 신', 'LeeSin', '새벽정글'],
  [103, '아리', 'Ahri', 'Waitence'], [222, '징크스', 'Jinx', '마지막 한 발'],
  [412, '쓰레쉬', 'Thresh', '랜턴 타세요'], [58, '레넥톤', 'Renekton', '상단'],
  [76, '니달리', 'Nidalee', '숲길'], [61, '오리아나', 'Orianna', '공의 위치'],
  [81, '이즈리얼', 'Ezreal', '비전 이동'], [111, '노틸러스', 'Nautilus', '깊은 바다'],
] as const;

export function previewHistory(offset: number): MatchHistory {
  const matches: Match[] = Array.from({ length: offset === 0 ? 20 : 4 }, (_, index) => {
    const n = index + offset;
    const won = n % 3 !== 1;
    return {
      id: String(8393991955 - n), createdAt: Date.UTC(2026, 8, 29, 12, 34) - n * 5_200_000,
      duration: 1834 + n * 31, queueId: [0, 420, 0, 450, 440][n % 5], mapId: n % 5 === 3 ? 12 : 11,
      version: n === 1 ? '16.19.1' : '16.17.810.4348', me: 3,
      players: roster.map(([championId, champion, , name], i) => ({
        id: i + 1, championId, champion, name, team: i < 5 ? 100 : 200, win: i < 5 ? won : !won,
        kills: [4, 7, 9, 8, 1, 3, 6, 5, 7, 0][i], deaths: [3, 4, 2, 2, 4, 5, 6, 4, 7, 7][i], assists: [6, 9, 11, 5, 18, 4, 6, 8, 2, 12][i],
        cs: [204, 165, 218, 246, 31, 201, 167, 208, 230, 27][i], gold: 11000 + i * 320,
        damage: [21342, 17211, 29483, 31058, 8603, 24013, 19772, 26214, 30291, 9851][i], items: [],
      })),
    };
  });
  return { summoner: 'Waitence', matches, hasMore: offset === 0 };
}

export function previewReplay(gameId: string): MatchReplay {
  const n = 8393991955 - Number(gameId);
  if (n === 1) return { status: 'unsupported', version: '16.19.1' };
  if (n === 2) return { status: 'missing' };
  const durationMs = (1834 + n * 31) * 1000;
  return { status: 'ready', version: '16.17.810.4348', durationMs,
    participants: roster.map(([, , champion], i) => ({ participantIndex: i, champion, team: i < 5 ? 100 : 200 })),
    movements: [{ kind: 'movement', startMs: 0, endMs: durationMs, stepMs: 1000,
      players: roster.map((_, i) => ({ participantIndex: i,
        xy: Array.from({ length: Math.floor(durationMs / 1000) + 1 }, (_, second) => {
          const team = i < 5 ? 0 : 1;
          const progress = Math.min(1, second / 120);
          const base = team ? 14000 : 800;
          const x = 3300 + (i % 5) * 2000 + Math.sin(second / 85 + i) * 1200;
          const y = 2300 + (i % 5) * 2100 + Math.cos(second / 95 + i) * 1100;
          return [Math.round(base + (x - base) * progress), Math.round(base + (y - base) * progress)];
        }).flat(),
      })),
    }],
  };
}
