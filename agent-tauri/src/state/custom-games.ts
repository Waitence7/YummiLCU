export type CustomGameRecord = {
  kdaFinal: boolean;
  gameId: string;
  mode: string;
  playedAt: string;
  champion: string | null;
  kills: number | null;
  deaths: number | null;
  assists: number | null;
  won: boolean | null;
  status: string;
  replayCollected: boolean;
  inviteCode: string | null;
};
