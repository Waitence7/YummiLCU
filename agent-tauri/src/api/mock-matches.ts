// Development preview fixtures only. Imported exclusively by mock.ts.
import type { Match, MatchFight, MatchHistory, MatchReplay } from '../state/matches';

const roster = [
  [164, '카밀', 'Camille', '모서리'], [64, '리 신', 'LeeSin', '새벽정글'],
  [103, '아리', 'Ahri', 'Waitence'], [222, '징크스', 'Jinx', '마지막 한 발'],
  [412, '쓰레쉬', 'Thresh', '랜턴 타세요'], [58, '레넥톤', 'Renekton', '상단'],
  [76, '니달리', 'Nidalee', '숲길'], [61, '오리아나', 'Orianna', '공의 위치'],
  [81, '이즈리얼', 'Ezreal', '비전 이동'], [111, '노틸러스', 'Nautilus', '깊은 바다'],
] as const;

const builds = [
  [3078, 6333, 3053, 3110, 3047, 3364],
  [6692, 3071, 6333, 3156, 3047, 3364],
  [6655, 3089, 3165, 3135, 3020, 3363],
  [3031, 3094, 3072, 3036, 3006, 3363],
  [3190, 3107, 3870, 3050, 3117, 3364],
  [3071, 3053, 6333, 3065, 3047, 3364],
  [3100, 3089, 3135, 3157, 3020, 3364],
  [6657, 3089, 3157, 3135, 3020, 3363],
  [3078, 3004, 6694, 3071, 3158, 3363],
  [3190, 3109, 3870, 3050, 3117, 3364],
] as const;

function previewFights(n: number): MatchFight[] {
  const shift = n * 1700;
  return [
    {
      id: `fight-${n}-1`, startMs: 482_000 + shift, endMs: 503_000 + shift, location: '드래곤 둥지',
      winnerTeam: 100, blueKills: 2, redKills: 1, objective: '드래곤 확보', goldSwing: 920,
      summary: '강가 시야를 먼저 잡은 블루 팀이 진입 각을 만들고 드래곤까지 연결했습니다.',
      participants: [
        { playerId: 2, kills: 1, deaths: 0, assists: 1, damage: 2400 },
        { playerId: 3, kills: 0, deaths: 0, assists: 2, damage: 3100 },
        { playerId: 4, kills: 1, deaths: 0, assists: 1, damage: 2850 },
        { playerId: 7, kills: 1, deaths: 1, assists: 0, damage: 2700 },
        { playerId: 9, kills: 0, deaths: 1, assists: 1, damage: 2500 },
      ],
      events: [
        { atMs: 485_000 + shift, label: '교전 시작', detail: '강가 입구에서 첫 교전', team: 100 },
        { atMs: 491_000 + shift, label: '첫 킬', detail: '블루 팀이 선취', team: 100 },
        { atMs: 499_000 + shift, label: '드래곤 전환', detail: '상대 정글러 이탈 후 오브젝트 시작', team: 100 },
      ],
    },
    {
      id: `fight-${n}-2`, startMs: 861_000 + shift, endMs: 888_000 + shift, location: '미드 강가',
      winnerTeam: 200, blueKills: 1, redKills: 3, objective: null, goldSwing: -1430,
      summary: '블루 팀이 먼저 들어갔지만 후방 합류가 늦어 레드 팀이 역으로 포위했습니다.',
      participants: [
        { playerId: 1, kills: 0, deaths: 1, assists: 1, damage: 1800 },
        { playerId: 3, kills: 1, deaths: 1, assists: 0, damage: 3600 },
        { playerId: 4, kills: 0, deaths: 1, assists: 1, damage: 4100 },
        { playerId: 8, kills: 1, deaths: 0, assists: 2, damage: 4200 },
        { playerId: 9, kills: 2, deaths: 1, assists: 1, damage: 5300 },
      ],
      events: [
        { atMs: 864_000 + shift, label: '진입', detail: '블루 팀이 미드 강가로 선진입', team: 100 },
        { atMs: 872_000 + shift, label: '역포위', detail: '레드 팀 측면 합류', team: 200 },
        { atMs: 884_000 + shift, label: '교전 종료', detail: '레드 팀 3:1 교환', team: 200 },
      ],
    },
    {
      id: `fight-${n}-3`, startMs: 1_294_000 + shift, endMs: 1_326_000 + shift, location: '바론 앞',
      winnerTeam: 100, blueKills: 5, redKills: 1, objective: '바론 확보', goldSwing: 3470,
      summary: '쓰레쉬의 선진입 이후 아리가 상대 딜러를 묶었고, 징크스가 연속 처치로 한타를 마무리했습니다.',
      participants: [
        { playerId: 2, kills: 0, deaths: 0, assists: 4, damage: 2500 },
        { playerId: 3, kills: 1, deaths: 0, assists: 3, damage: 4700 },
        { playerId: 4, kills: 3, deaths: 0, assists: 1, damage: 6900 },
        { playerId: 5, kills: 1, deaths: 1, assists: 3, damage: 2100 },
        { playerId: 8, kills: 1, deaths: 1, assists: 0, damage: 3800 },
      ],
      events: [
        { atMs: 1_297_000 + shift, label: '교전 시작', detail: '바론 강가 시야 싸움', team: 100 },
        { atMs: 1_302_000 + shift, label: '핵심 CC', detail: '아리가 상대 원딜을 묶음', team: 100 },
        { atMs: 1_309_000 + shift, label: '연속 처치', detail: '징크스가 리셋으로 3킬', team: 100 },
        { atMs: 1_326_000 + shift, label: '바론 전환', detail: '레드 팀 정글러 사망 후 바론 시작', team: 100 },
      ],
    },
    {
      id: `fight-${n}-4`, startMs: 1_603_000 + shift, endMs: 1_625_000 + shift, location: '레드 팀 미드 2차',
      winnerTeam: 100, blueKills: 3, redKills: 0, objective: '미드 2차 포탑', goldSwing: 2210,
      summary: '바론 버프를 활용한 압박 중 레드 팀의 수비 진형이 갈라지면서 짧게 끝난 교전입니다.',
      participants: [
        { playerId: 1, kills: 1, deaths: 0, assists: 2, damage: 3300 },
        { playerId: 3, kills: 1, deaths: 0, assists: 2, damage: 3900 },
        { playerId: 4, kills: 1, deaths: 0, assists: 1, damage: 4600 },
      ],
      events: [
        { atMs: 1_606_000 + shift, label: '포탑 압박', detail: '바론 버프 웨이브 진입', team: 100 },
        { atMs: 1_612_000 + shift, label: '수비 붕괴', detail: '레드 팀 전열 분리', team: 100 },
        { atMs: 1_624_000 + shift, label: '포탑 파괴', detail: '교전 승리 후 미드 2차 제거', team: 100 },
      ],
    },
  ];
}

export function previewHistory(offset: number): MatchHistory {
  const matches: Match[] = Array.from({ length: offset === 0 ? 20 : 4 }, (_, index) => {
    const n = index + offset;
    const won = n % 3 !== 1;
    return {
      id: String(8393991955 - n), createdAt: Date.UTC(2026, 8, 29, 12, 34) - n * 5_200_000,
      duration: 1834 + n * 31, queueId: [0, 420, 0, 450, 440][n % 5], mapId: n % 5 === 3 ? 12 : 11,
      version: n === 1 ? '16.19.1' : '16.17.810.4348', me: 3,
      fights: n % 5 === 3 ? [] : previewFights(n),
      players: roster.map(([championId, champion, , name], i) => ({
        id: i + 1, championId, champion, name: `${name}#KR${i + 1}`, tagLine: `KR${i + 1}`,
        team: i < 5 ? 100 : 200, win: i < 5 ? won : !won, level: 16 + (i % 3),
        kills: [4, 7, 9, 8, 1, 3, 6, 5, 7, 0][i], deaths: [3, 4, 2, 2, 4, 5, 6, 4, 7, 7][i], assists: [6, 9, 11, 5, 18, 4, 6, 8, 2, 12][i],
        cs: [204, 165, 218, 246, 31, 201, 167, 208, 230, 27][i], gold: 11000 + i * 320,
        damage: [21342, 17211, 29483, 31058, 8603, 24013, 19772, 26214, 30291, 9851][i],
        vision: 18 + i * 3, lane: ['TOP', 'JUNGLE', 'MIDDLE', 'BOTTOM', 'BOTTOM'][i % 5],
        role: i % 5 === 4 ? 'DUO_SUPPORT' : i % 5 === 3 ? 'DUO_CARRY' : 'SOLO',
        summonerSpells: [4, i === 1 || i === 6 ? 11 : i % 2 ? 14 : 12], items: [...builds[i]],
      })),
    };
  });
  return { summoner: 'Waitence', matches, hasMore: offset === 0, source: 'live', savedAt: Date.now() };
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
