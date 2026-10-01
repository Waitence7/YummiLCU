import { useCallback, useEffect, useRef, useState } from "react";
import {
  downloadMatchReplay,
  matchReplay,
  matchReplayAnalysis,
} from "../../api/commands";
import {
  matchTime,
  replayPosition,
  type Match,
  type MatchReplay,
  type ReplayEvent,
  type ReplayParticipant,
} from "../../state/matches";
import mapImage from "../../assets/replay/summoners-rift.webp";
import { ItemIcon, SpellPair } from "./MatchAssets";

type ReplayPlayerState = {
  level: number | null;
  skills: Record<string, number>;
  items: number[];
  health: number | null;
  maxHealth: number | null;
  allShield: number | null;
  physicalShield: number | null;
  magicalShield: number | null;
  resource: number | null;
  maxResource: number | null;
  experience: number | null;
  stats: Record<string, number>;
  lastDeathAt: number | null;
};

const SKILL_KEYS = ["Q", "W", "E", "R"];
const MAJOR_EVENT_KINDS = new Set([
  "levelUp",
  "upgradeSpell",
  "inventorySnapshot",
  "itemBuy",
  "itemRemove",
  "itemSet",
  "itemSetResearch",
  "championDeath",
]);
const HIDDEN_RECENT_EVENT_KINDS = new Set([
  "resourceUpdate",
  "combatStatUpdate",
]);

const COMBAT_STAT_LABELS: Array<[string, string]> = [
  ["attackDamage", "공격력"],
  ["abilityPower", "주문력"],
  ["armor", "방어력"],
  ["magicResistance", "마법 저항력"],
  ["abilityHaste", "스킬 가속"],
  ["attackRange", "사거리"],
  ["lifeSteal", "생명력 흡수"],
  ["percentOmnivampMod", "모든 피해 흡혈"],
];

function dataNumber(event: ReplayEvent, key: string) {
  const value = event.data?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function dataString(event: ReplayEvent, key: string) {
  const value = event.data?.[key];
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function newPlayerState(hasLevels: boolean): ReplayPlayerState {
  return {
    level: hasLevels ? 1 : null,
    skills: { Q: 0, W: 0, E: 0, R: 0 },
    items: Array(9).fill(0) as number[],
    health: null,
    maxHealth: null,
    allShield: null,
    physicalShield: null,
    magicalShield: null,
    resource: null,
    maxResource: null,
    experience: null,
    stats: {},
    lastDeathAt: null,
  };
}

function replayStatesAt(replay: MatchReplay, timeMs: number) {
  const states = new Map<number, ReplayPlayerState>();
  const ensure = (index: number) => {
    let state = states.get(index);
    if (!state) {
      state = newPlayerState(Boolean(replay.capabilities?.levels));
      states.set(index, state);
    }
    return state;
  };

  for (const participant of replay.participants ?? []) {
    ensure(participant.participantIndex);
  }

  for (const event of replay.events ?? []) {
    if (event.atMs > timeMs) break;
    const index = event.participantIndex;
    if (index == null) continue;
    const state = ensure(index);

    if (event.kind === "levelUp") {
      const level = dataNumber(event, "newLevel");
      if (level != null && level >= 1 && level <= 30) state.level = level;
      continue;
    }

    if (event.kind === "upgradeSpell") {
      const slotName = dataString(event, "spellSlotName");
      const slotNumber = dataNumber(event, "spellSlot");
      const key =
        slotName && SKILL_KEYS.includes(slotName)
          ? slotName
          : slotNumber != null &&
              slotNumber >= 0 &&
              slotNumber < SKILL_KEYS.length
            ? SKILL_KEYS[slotNumber]
            : null;
      const level = dataNumber(event, "newSpellLevel");
      if (key && level != null && level >= 0 && level <= 10) {
        state.skills[key] = level;
      }
      continue;
    }

    if (event.kind === "resourceUpdate") {
      for (const key of [
        "health",
        "maxHealth",
        "allShield",
        "physicalShield",
        "magicalShield",
        "resource",
        "maxResource",
        "experience",
      ] as const) {
        const value = dataNumber(event, key);
        if (value != null) state[key] = value;
      }
      continue;
    }

    if (event.kind === "combatStatUpdate") {
      for (const [key] of COMBAT_STAT_LABELS) {
        const value = dataNumber(event, key);
        if (value != null) state.stats[key] = value;
      }
      continue;
    }

    if (event.kind === "inventorySnapshot") {
      state.items.fill(0);
      for (const item of event.items ?? []) {
        state.items[item.slot] = item.itemId;
      }
      continue;
    }

    if (
      event.kind === "itemBuy" ||
      event.kind === "itemSet" ||
      event.kind === "itemSetResearch"
    ) {
      const itemId = dataNumber(event, "itemId");
      const slot = dataNumber(event, "slot");
      if (
        itemId != null &&
        itemId > 0 &&
        slot != null &&
        Number.isInteger(slot) &&
        slot >= 0 &&
        slot < state.items.length
      ) {
        state.items[slot] = itemId;
      }
      continue;
    }

    if (event.kind === "itemRemove") {
      const slot = dataNumber(event, "slot");
      if (
        slot != null &&
        Number.isInteger(slot) &&
        slot >= 0 &&
        slot < state.items.length
      ) {
        state.items[slot] = 0;
      }
      continue;
    }

    if (event.kind === "itemSwap") {
      const left = dataNumber(event, "leftSlot");
      const right = dataNumber(event, "rightSlot");
      if (
        left != null &&
        right != null &&
        Number.isInteger(left) &&
        Number.isInteger(right) &&
        left >= 0 &&
        right >= 0 &&
        left < state.items.length &&
        right < state.items.length
      ) {
        [state.items[left], state.items[right]] = [
          state.items[right],
          state.items[left],
        ];
      }
      continue;
    }

    if (event.kind === "championDeath") state.lastDeathAt = event.atMs;
  }
  return states;
}

function participantName(
  participant: ReplayParticipant | undefined,
  fallbackIndex?: number,
) {
  if (!participant) {
    return fallbackIndex == null ? "알 수 없음" : "선수 " + (fallbackIndex + 1);
  }
  return (
    participant.name ??
    participant.champion ??
    "선수 " + (participant.participantIndex + 1)
  );
}

function eventLabel(
  event: ReplayEvent,
  participants: Map<number, ReplayParticipant>,
) {
  const actor =
    event.participantIndex == null
      ? null
      : participantName(
          participants.get(event.participantIndex),
          event.participantIndex,
        );
  const target =
    event.targetParticipantIndex == null
      ? null
      : participantName(
          participants.get(event.targetParticipantIndex),
          event.targetParticipantIndex,
        );

  switch (event.kind) {
    case "levelUp":
      return (
        (actor ?? "선수") +
        " · Lv." +
        (dataNumber(event, "newLevel") ?? "?")
      );
    case "upgradeSpell":
      return (
        (actor ?? "선수") +
        " · " +
        (dataString(event, "spellSlotName") ?? "스킬") +
        " " +
        (dataNumber(event, "newSpellLevel") ?? "") +
        "레벨"
      );
    case "spellCast": {
      const key = dataString(event, "summonerSpellKey");
      return (
        (actor ?? "선수") +
        " · " +
        (key ? key + " 소환사 주문" : "스킬 사용")
      );
    }
    case "useItem":
      return (actor ?? "선수") + " · 아이템 사용";
    case "itemBuy":
      return (actor ?? "선수") + " · 아이템 구매";
    case "itemRemove":
      return (actor ?? "선수") + " · 아이템 제거";
    case "itemSet":
    case "itemSetResearch":
      return (actor ?? "선수") + " · 아이템 변경";
    case "inventorySnapshot":
      return (actor ?? "선수") + " · 인벤토리 동기화";
    case "targetHero":
      return (actor ?? "선수") + " → " + (target ?? "챔피언") + " 타깃";
    case "basicAttackSource":
      return (actor ?? "선수") + " · 기본 공격";
    case "unitApplyDamageTarget":
      return (actor ?? target ?? "선수") + " · 피해 이벤트";
    case "championDeath":
      return (actor ?? target ?? "선수") + " · 사망";
    case "championKillSummary":
      return (actor ?? "선수") + " · 처치";
    case "buffSubjectAdd":
      return (actor ?? "선수") + " · 버프 획득";
    case "buffSubjectRemove":
      return (actor ?? "선수") + " · 버프 종료";
    default:
      return (actor ? actor + " · " : "") + event.kind;
  }
}

function percent(value: number | null, maximum: number | null) {
  if (value == null || maximum == null || maximum <= 0) return 0;
  return Math.max(0, Math.min(100, (value / maximum) * 100));
}

export function ReplayViewer({
  match,
  connected,
}: {
  match: Match;
  connected: boolean;
}) {
  const [replay, setReplay] = useState<MatchReplay | null>(null);
  const [busy, setBusy] = useState<
    "reading" | "downloading" | "analyzing" | null
  >("reading");
  const [error, setError] = useState<string | null>(null);
  const [time, setTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const [selected, setSelected] = useState<number | null>(null);
  const generation = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const duration = replay?.durationMs ?? match.duration * 1000;
  const supportedMap = match.mapId === 11;
  const ready = replay?.status === "ready" && supportedMap;

  const load = useCallback(
    async (download = false) => {
      const request = ++generation.current;
      clearTimeout(timer.current);
      setBusy(download ? "downloading" : "reading");
      setError(null);
      setPlaying(false);
      try {
        if (download) await downloadMatchReplay(match.id, match.version);
        const started = Date.now();
        const read = async (): Promise<void> => {
          if (request !== generation.current) return;
          try {
            const result = await matchReplay(match.id);
            if (request !== generation.current) return;
            if (
              download &&
              result.status === "missing" &&
              Date.now() - started < 45_000
            ) {
              timer.current = setTimeout(() => void read(), 1500);
              return;
            }
            if (result.status === "processing" && result.jobId) {
              setReplay(result);
              setBusy("analyzing");
              const jobId = result.jobId;
              const poll = async (): Promise<void> => {
                if (request !== generation.current) return;
                try {
                  const next = await matchReplayAnalysis(match.id, jobId);
                  if (request !== generation.current) return;
                  setReplay(next);
                  if (next.status === "processing") {
                    setBusy("analyzing");
                    timer.current = setTimeout(() => void poll(), 1200);
                    return;
                  }
                  setTime(0);
                  setSelected(
                    next.participants?.[0]?.participantIndex ?? null,
                  );
                  setBusy(null);
                } catch (caught) {
                  if (request !== generation.current) return;
                  setError(
                    caught instanceof Error ? caught.message : String(caught),
                  );
                  setBusy(null);
                }
              };
              timer.current = setTimeout(() => void poll(), 800);
              return;
            }
            setReplay(result);
            setTime(0);
            setSelected(result.participants?.[0]?.participantIndex ?? null);
            if (download && result.status === "missing") {
              setError(
                "아직 다운로드가 완료되지 않았습니다. 잠시 후 다시 확인하세요.",
              );
            }
            setBusy(null);
          } catch (caught) {
            if (request !== generation.current) return;
            if (download && Date.now() - started < 45_000) {
              timer.current = setTimeout(() => void read(), 1500);
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
    },
    [match.id, match.version],
  );

  useEffect(() => {
    void load();
    return () => {
      generation.current++;
      clearTimeout(timer.current);
    };
  }, [load]);

  useEffect(() => {
    if (!playing || !ready) return;
    let previous = performance.now();
    const interval = setInterval(() => {
      const now = performance.now();
      const elapsed = Math.min(now - previous, 1000) * speed;
      previous = now;
      setTime((current) => Math.min(duration, current + elapsed));
    }, 50);
    return () => clearInterval(interval);
  }, [playing, ready, duration, speed]);

  useEffect(() => {
    if (time >= duration) setPlaying(false);
  }, [time, duration]);

  function seek(next: number) {
    setTime(Math.max(0, Math.min(duration, next)));
  }

  function togglePlay() {
    if (time >= duration) setTime(0);
    setPlaying((value) => !value);
  }

  if (busy || error || !ready) {
    const title =
      busy === "downloading"
        ? "리플레이 다운로드 중"
        : busy === "analyzing"
          ? "서버에서 리플레이 분석 중"
          : busy
            ? "리플레이를 읽고 있습니다"
            : error
              ? "리플레이를 불러오지 못했습니다"
              : replay?.status === "missing"
                ? "저장된 리플레이가 없습니다"
                : !supportedMap
                  ? "이 맵은 아직 재생할 수 없습니다"
                  : replay?.status === "unsupported"
                    ? "이 패치는 아직 재생할 수 없습니다"
                    : "리플레이 상태를 읽을 수 없습니다";
    const analysisProgress =
      replay?.progress == null ? "" : " · " + Math.round(replay.progress) + "%";
    const detail =
      busy === "analyzing"
        ? (replay?.stage ?? "ROFL 서버 분석 중") + analysisProgress
        : busy
          ? "준비되면 이 화면에서 볼 수 있습니다."
          : (error ??
            (replay?.status === "missing"
              ? "롤 클라이언트에서 제공하는 리플레이를 내려받습니다."
              : replay?.status === "unsupported"
                ? (replay.remoteError ??
                  "리플레이 버전 " +
                    (replay.version ?? "알 수 없음") +
                    " · 경기 결과는 확인할 수 있습니다.")
                : "ROFL에서 복원 가능한 상태가 아직 없습니다."));
    return (
      <div
        className="match-empty replay-empty"
        role={error ? "alert" : "status"}
        aria-busy={!!busy}
      >
        <span className="replay-empty-symbol" aria-hidden="true">
          {busy ? "···" : "▷"}
        </span>
        <strong>{title}</strong>
        <p>{detail}</p>
        {!busy && (
          <div className="match-actions">
            {replay?.status === "missing" && connected && (
              <button
                className="match-button match-button-primary"
                onClick={() => void load(true)}
              >
                리플레이 다운로드
              </button>
            )}
            <button className="match-button" onClick={() => void load()}>
              다시 확인
            </button>
          </div>
        )}
        {!busy && replay?.status === "missing" && !connected && (
          <p>다운로드하려면 롤 클라이언트를 실행하세요.</p>
        )}
      </div>
    );
  }

  const participantList = replay.participants ?? [];
  const participants = new Map(
    participantList.map((participant) => [
      participant.participantIndex,
      participant,
    ]),
  );
  const selectedIndex =
    selected ?? participantList[0]?.participantIndex ?? null;
  const selectedParticipant =
    selectedIndex == null ? undefined : participants.get(selectedIndex);
  const states = replayStatesAt(replay, time);
  const selectedState =
    selectedIndex == null ? undefined : states.get(selectedIndex);

  const recentEvents = (replay.events ?? [])
    .filter(
      (event) =>
        event.atMs <= time &&
        !HIDDEN_RECENT_EVENT_KINDS.has(event.kind) &&
        (selectedIndex == null ||
          event.participantIndex === selectedIndex ||
          event.targetParticipantIndex === selectedIndex),
    )
    .slice(-8)
    .reverse();

  const majorEvents = (replay.events ?? []).filter((event) =>
    MAJOR_EVENT_KINDS.has(event.kind),
  );
  const currentItems = selectedState?.items.filter(Boolean) ?? [];
  const statEntries = selectedState
    ? COMBAT_STAT_LABELS.map(
        ([key, label]) => [key, label, selectedState.stats[key]] as const,
      ).filter(([, , value]) => value != null)
    : [];
  const capabilityLabels = [
    replay.capabilities?.movement && "이동",
    replay.capabilities?.levels && "레벨",
    replay.capabilities?.skills && "스킬",
    replay.capabilities?.inventory && "아이템",
    replay.capabilities?.resources && "HP/자원",
    replay.capabilities?.combatStats && "전투 스탯",
    replay.capabilities?.deaths && "사망",
  ].filter(Boolean);

  return (
    <section
      className="replay-viewer"
      aria-label="경기 리플레이"
      tabIndex={0}
      onKeyDown={(event) => {
        if ((event.target as HTMLElement).closest("button, input, select")) {
          return;
        }
        if (event.code === "Space") {
          event.preventDefault();
          togglePlay();
        }
        if (event.code === "ArrowLeft") {
          event.preventDefault();
          seek(time - 10000);
        }
        if (event.code === "ArrowRight") {
          event.preventDefault();
          seek(time + 10000);
        }
      }}
    >
      <div className="replay-caption">
        <span>ROFL 리플레이 뷰어</span>
        <span>
          {replay.events?.length ?? 0} events
          {capabilityLabels.length
            ? " · " + capabilityLabels.join(" · ")
            : ""}
        </span>
      </div>

      <div className="replay-stage">
        <div
          className="replay-map"
          style={{ backgroundImage: "url(" + mapImage + ")" }}
          aria-label={matchTime(time / 1000) + " 선수 위치"}
        >
          {participantList.map((participant) => {
            const point = replayPosition(
              replay.movements ?? [],
              participant.participantIndex,
              time,
            );
            if (!point) return null;
            const markerClass =
              "replay-marker " +
              (participant.team === 100 ? "blue" : "red") +
              (selectedIndex === participant.participantIndex
                ? " selected"
                : "") +
              (selectedIndex !== null &&
              selectedIndex !== participant.participantIndex
                ? " muted"
                : "");
            return (
              <button
                key={participant.participantIndex}
                type="button"
                className={markerClass}
                style={{
                  left: point.x + "%",
                  top: point.y + "%",
                }}
                aria-label={participantName(participant) + " 위치"}
                aria-pressed={selectedIndex === participant.participantIndex}
                title={
                  participantName(participant) +
                  " · " +
                  (participant.champion ?? "챔피언")
                }
                onClick={() => setSelected(participant.participantIndex)}
              >
                {participant.participantIndex + 1}
              </button>
            );
          })}
          <span className="replay-map-clock">{matchTime(time / 1000)}</span>
        </div>

        <aside className="replay-sidebar">
          <section className="replay-inspector">
            <div className="replay-inspector-head">
              <span
                className={
                  "replay-champion-badge " +
                  (selectedParticipant?.team === 100 ? "blue" : "red")
                }
              >
                {(selectedParticipant?.champion ?? "?").slice(0, 2)}
              </span>
              <div>
                <strong>
                  {selectedParticipant?.champion ??
                    participantName(
                      selectedParticipant,
                      selectedIndex ?? undefined,
                    )}
                </strong>
                <small>
                  {selectedParticipant?.name ??
                    participantName(
                      selectedParticipant,
                      selectedIndex ?? undefined,
                    )}
                  {selectedParticipant?.role
                    ? " · " + selectedParticipant.role
                    : ""}
                </small>
              </div>
              <em>
                {selectedState?.level != null
                  ? "Lv." + selectedState.level
                  : "Lv.?"}
              </em>
            </div>

            {selectedState?.maxHealth != null && (
              <div className="replay-resource-block">
                <div className="replay-bar health">
                  <span
                    style={{
                      width:
                        percent(
                          selectedState.health,
                          selectedState.maxHealth,
                        ) + "%",
                    }}
                  />
                  <b>
                    {Math.round(selectedState.health ?? 0)} /{" "}
                    {Math.round(selectedState.maxHealth)}
                  </b>
                </div>
                {(selectedState.allShield ?? 0) > 0 && (
                  <small>
                    실드 {Math.round(selectedState.allShield ?? 0)}
                  </small>
                )}
                {selectedState.maxResource != null && (
                  <div className="replay-bar resource">
                    <span
                      style={{
                        width:
                          percent(
                            selectedState.resource,
                            selectedState.maxResource,
                          ) + "%",
                      }}
                    />
                    <b>
                      {Math.round(selectedState.resource ?? 0)} /{" "}
                      {Math.round(selectedState.maxResource)}
                    </b>
                  </div>
                )}
              </div>
            )}

            <div className="replay-loadout-row">
              <SpellPair
                spells={selectedParticipant?.summonerSpells ?? []}
                version={match.version}
                compact
              />
              <div className="replay-skills" aria-label="스킬 레벨">
                {SKILL_KEYS.map((key) => (
                  <span key={key}>
                    <b>{key}</b>
                    <em>{selectedState?.skills[key] ?? 0}</em>
                  </span>
                ))}
              </div>
            </div>

            <div className="replay-current-items">
              <small>현재 복원 인벤토리</small>
              <div>
                {Array.from({ length: 7 }, (_, slot) => (
                  <ItemIcon
                    key={slot}
                    id={selectedState?.items[slot] ?? 0}
                    version={match.version}
                    small
                  />
                ))}
              </div>
              {!currentItems.length && (
                <p>이 시점의 인벤토리 패킷은 아직 복원되지 않았습니다.</p>
              )}
            </div>

            {!!selectedParticipant?.finalItems?.length && (
              <div className="replay-final-build">
                <small>최종 빌드 참고</small>
                <div>
                  {selectedParticipant.finalItems
                    .slice(0, 7)
                    .map((item, index) => (
                      <ItemIcon
                        key={item + "-" + index}
                        id={item}
                        version={match.version}
                        small
                      />
                    ))}
                </div>
              </div>
            )}

            {!!statEntries.length && (
              <div className="replay-stat-grid">
                {statEntries.slice(0, 8).map(([key, label, value]) => (
                  <span key={key}>
                    <small>{label}</small>
                    <strong>{Number(value).toFixed(1)}</strong>
                  </span>
                ))}
              </div>
            )}
          </section>

          <div className="replay-roster" aria-label="선수 선택">
            {participantList.map((participant) => {
              const state = states.get(participant.participantIndex);
              return (
                <button
                  type="button"
                  key={participant.participantIndex}
                  className={
                    "replay-player " +
                    (participant.team === 100 ? "blue" : "red") +
                    (selectedIndex === participant.participantIndex
                      ? " selected"
                      : "")
                  }
                  aria-pressed={
                    selectedIndex === participant.participantIndex
                  }
                  onClick={() => setSelected(participant.participantIndex)}
                >
                  <span className="replay-player-number">
                    {participant.participantIndex + 1}
                  </span>
                  <span>
                    <strong>
                      {participant.champion ??
                        "선수 " + (participant.participantIndex + 1)}
                    </strong>
                    <small>
                      {participant.name ?? participant.role ?? ""}
                      {state?.level != null ? " · Lv." + state.level : ""}
                    </small>
                  </span>
                </button>
              );
            })}
          </div>

          <section className="replay-events">
            <header>
              <strong>최근 이벤트</strong>
              <span>{matchTime(time / 1000)}</span>
            </header>
            {recentEvents.length ? (
              <ol>
                {recentEvents.map((event, index) => (
                  <li key={event.atMs + "-" + event.kind + "-" + index}>
                    <button type="button" onClick={() => seek(event.atMs)}>
                      <time>{matchTime(event.atMs / 1000)}</time>
                      <span>{eventLabel(event, participants)}</span>
                    </button>
                  </li>
                ))}
              </ol>
            ) : (
              <p>이 시점 이전에 표시할 이벤트가 없습니다.</p>
            )}
          </section>
        </aside>
      </div>

      <div className="replay-controls">
        <div className="replay-timeline">
          <input
            type="range"
            min={0}
            max={duration}
            step={250}
            value={time}
            onChange={(event) => seek(Number(event.target.value))}
            aria-label="재생 위치"
            aria-valuetext={
              matchTime(time / 1000) + " / " + matchTime(duration / 1000)
            }
          />
          <div className="replay-event-track" aria-label="주요 이벤트">
            {majorEvents.slice(0, 500).map((event, index) => (
              <button
                key={event.atMs + "-" + event.kind + "-" + index}
                type="button"
                className={"replay-event-tick " + event.kind}
                style={{
                  left:
                    (duration > 0
                      ? Math.max(
                          0,
                          Math.min(100, (event.atMs / duration) * 100),
                        )
                      : 0) + "%",
                }}
                title={
                  matchTime(event.atMs / 1000) +
                  " " +
                  eventLabel(event, participants)
                }
                onClick={() => seek(event.atMs)}
                tabIndex={-1}
              />
            ))}
          </div>
        </div>

        <div className="replay-control-row">
          <div className="match-actions">
            <button
              type="button"
              className="match-button"
              onClick={() => seek(time - 10000)}
              aria-label="10초 이전"
            >
              −10초
            </button>
            <button
              type="button"
              className="match-button match-button-primary replay-play"
              onClick={togglePlay}
              aria-label={playing ? "일시정지" : "재생"}
            >
              {playing ? "Ⅱ" : "▶"}
            </button>
            <button
              type="button"
              className="match-button"
              onClick={() => seek(time + 10000)}
              aria-label="10초 이후"
            >
              +10초
            </button>
          </div>
          <span className="replay-time">
            {matchTime(time / 1000)}{" "}
            <span>/ {matchTime(duration / 1000)}</span>
          </span>
          <select
            aria-label="재생 속도"
            value={speed}
            onChange={(event) => setSpeed(Number(event.target.value))}
          >
            {[0.25, 0.5, 1, 2, 4, 8].map((value) => (
              <option key={value} value={value}>
                {value}×
              </option>
            ))}
          </select>
        </div>
      </div>
    </section>
  );
}
