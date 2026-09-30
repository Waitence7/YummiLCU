import { useState } from 'react';

const SPELL_FILES: Record<number, string> = {
  1: 'SummonerBoost.png',
  3: 'SummonerExhaust.png',
  4: 'SummonerFlash.png',
  6: 'SummonerHaste.png',
  7: 'SummonerHeal.png',
  11: 'SummonerSmite.png',
  12: 'SummonerTeleport.png',
  13: 'SummonerMana.png',
  14: 'SummonerDot.png',
  21: 'SummonerBarrier.png',
  32: 'SummonerSnowball.png',
};

export function dataDragonVersion(version?: string | null) {
  const match = version?.match(/^(\d+)\.(\d+)/);
  return match ? `${match[1]}.${match[2]}.1` : '16.19.1';
}

function AssetImage({ src, alt, className }: { src: string; alt: string; className: string }) {
  const [failed, setFailed] = useState(false);
  return failed
    ? <span className={`${className} match-asset-fallback`} aria-label={alt}>?</span>
    : <img className={className} src={src} alt={alt} loading="lazy" draggable={false} onError={() => setFailed(true)} />;
}

export function ItemIcon({ id, version, small = false }: { id: number; version?: string; small?: boolean }) {
  if (!id) return <span className={`match-item-icon empty${small ? ' small' : ''}`} aria-hidden="true" />;
  const patch = dataDragonVersion(version);
  return <AssetImage
    className={`match-item-icon${small ? ' small' : ''}`}
    src={`https://ddragon.leagueoflegends.com/cdn/${patch}/img/item/${id}.png`}
    alt={`아이템 ${id}`}
  />;
}

export function ItemBuild({ items, version, compact = false }: { items: number[]; version?: string; compact?: boolean }) {
  const normalized = [...items.filter(Boolean).slice(0, 7)];
  while (normalized.length < (compact ? 4 : 7)) normalized.push(0);
  return <span className={`match-item-build${compact ? ' compact' : ''}`}>
    {normalized.map((item, index) => <ItemIcon key={`${item}-${index}`} id={item} version={version} small={compact} />)}
  </span>;
}

export function SpellIcon({ id, version, small = false }: { id: number | null | undefined; version?: string; small?: boolean }) {
  const file = id ? SPELL_FILES[id] : undefined;
  if (!file) return <span className={`match-spell-icon empty${small ? ' small' : ''}`} aria-hidden="true" />;
  return <AssetImage
    className={`match-spell-icon${small ? ' small' : ''}`}
    src={`https://ddragon.leagueoflegends.com/cdn/${dataDragonVersion(version)}/img/spell/${file}`}
    alt={`소환사 주문 ${id}`}
  />;
}

export function SpellPair({ spells, version, compact = false }: { spells: (number | null)[]; version?: string; compact?: boolean }) {
  return <span className={`match-spell-pair${compact ? ' compact' : ''}`}>
    <SpellIcon id={spells[0]} version={version} small={compact} />
    <SpellIcon id={spells[1]} version={version} small={compact} />
  </span>;
}
