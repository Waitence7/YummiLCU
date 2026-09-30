import { useState } from 'react';

export function ChampionPortrait({ id, name, small = false }: { id: number; name: string; small?: boolean }) {
  const [failed, setFailed] = useState<number | null>(null);
  return (
    <span className={`match-portrait${small ? ' match-portrait-small' : ''}`} title={name}>
      {id > 0 && failed !== id ? (
        <img src={`https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1/champion-icons/${id}.png`} alt="" loading="lazy" draggable={false} onError={() => setFailed(id)} />
      ) : <span>{name.slice(0, 1)}</span>}
    </span>
  );
}
