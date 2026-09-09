use ruzstd::decoding::StreamingDecoder;
use serde_json::{json, Map, Value};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const ROFL_MAGIC: &[u8; 4] = b"RIOT";
const ROFL_FORMAT_V2: u16 = 2;
const CHUNK_HEADER_SIZE: usize = 17;
const SIGNATURE_SIZE: usize = 256;
const GAME_CHUNK_STREAM: u8 = 1;
const MAX_REPLAY_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 16 * 1024 * 1024;
const MAX_DECOMPRESSED_CHUNK_BYTES: usize = 64 * 1024 * 1024;
const MAX_REPLAY_CANDIDATES: usize = 32;
const REPLAY_RECENCY_WINDOW: Duration = Duration::from_secs(30 * 60);
const MOVEMENT_SAMPLE_STEP_MS: u32 = 1_000;
const MOVEMENT_CHUNK_MS: u32 = 180_000;
const MAX_MOVEMENT_RECORDS: usize = 500_000;
const MAX_WAYPOINTS_PER_RECORD: usize = 128;

// Verified against a real 26.17 replay (16.17.810.4348). Keep semantic
// decoding exact-build gated: ROFL transport/metadata remain patch independent.
const MOVEMENT_CLIENT_VERSION: &str = "16.17.810.4348";
const MOVEMENT_OPCODE_26_17: u16 = 0x04ee;

#[derive(Clone, Debug, Default)]
pub(crate) struct RoflMatchHint {
    pub(crate) game_id: String,
    participant_puuids: Vec<String>,
    participant_riot_ids: Vec<String>,
    replay_dir: Option<PathBuf>,
}

impl RoflMatchHint {
    pub(crate) fn from_eog(game_id: String, eog: &Value) -> Self {
        let mut participant_puuids = Vec::new();
        let mut participant_riot_ids = Vec::new();
        for participant in eog
            .get("participants")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for key in ["puuid", "PUUID"] {
                if let Some(value) = participant.get(key).and_then(Value::as_str) {
                    let normalized = value.trim().to_ascii_lowercase();
                    if !normalized.is_empty() && !participant_puuids.contains(&normalized) {
                        participant_puuids.push(normalized);
                    }
                }
            }
            let game_name = participant.get("gameName").and_then(Value::as_str);
            let tag_line = participant.get("tagLine").and_then(Value::as_str);
            if let (Some(game_name), Some(tag_line)) = (game_name, tag_line) {
                let normalized = normalize_riot_id(game_name, tag_line);
                if !normalized.is_empty() && !participant_riot_ids.contains(&normalized) {
                    participant_riot_ids.push(normalized);
                }
            }
        }
        Self {
            game_id,
            participant_puuids,
            participant_riot_ids,
            replay_dir: None,
        }
    }

    pub(crate) fn set_replay_dir(&mut self, value: &str) {
        let value = value.trim();
        if !value.is_empty() && value.len() <= 4_096 {
            self.replay_dir = Some(PathBuf::from(value));
        }
    }
}

#[derive(Clone, Debug)]
struct ReplayHeader {
    client_version: String,
    header_size: usize,
    protocol_digest: String,
}

#[derive(Clone, Debug)]
struct ReplayMetadata {
    game_length_ms: Option<u32>,
    participants: Vec<Map<String, Value>>,
}

#[derive(Clone, Debug)]
struct ReplayEnvelope {
    header: ReplayHeader,
    metadata: ReplayMetadata,
    signature_offset: usize,
}

#[derive(Clone, Debug)]
struct MovementRecord {
    timestamp_ms: u32,
    entity_id: u32,
    speed: f32,
    waypoints: Vec<(f32, f32)>,
}

#[derive(Clone, Debug)]
pub(crate) struct CollectedReplay {
    pub(crate) path: PathBuf,
    pub(crate) events: Vec<Value>,
}

pub(crate) fn collect_replay_bundle(hint: &RoflMatchHint) -> Result<Option<CollectedReplay>, String> {
    let Some(path) = find_matching_replay(hint)? else {
        return Ok(None);
    };
    let bytes = fs::read(&path).map_err(|error| format!("ROFL 읽기 실패: {error}"))?;
    if bytes.len() as u64 > MAX_REPLAY_FILE_BYTES {
        return Err("ROFL 파일이 허용 크기를 초과함".into());
    }
    let replay = parse_envelope(&bytes)?;
    let mut events = Vec::new();
    events.push(summary_event(hint, &path, &replay));

    if replay.header.client_version == MOVEMENT_CLIENT_VERSION {
        match decode_movement(&bytes, &replay) {
            Ok(movement) => events.extend(movement_events(hint, &replay, movement)),
            Err(error) => events.push(json!({
                "schemaVersion": 1,
                "kind": "semantic_status",
                "gameId": hint.game_id,
                "clientVersion": replay.header.client_version,
                "movement": {
                    "status": "decode_failed",
                    "error": error.chars().take(240).collect::<String>(),
                }
            })),
        }
    } else {
        events.push(json!({
            "schemaVersion": 1,
            "kind": "semantic_status",
            "gameId": hint.game_id,
            "clientVersion": replay.header.client_version,
            "movement": {
                "status": "unsupported_build",
                "supportedClientVersion": MOVEMENT_CLIENT_VERSION,
            }
        }));
    }
    Ok(Some(CollectedReplay { path, events }))
}

pub(crate) fn collect_replay_events(hint: &RoflMatchHint) -> Result<Option<Vec<Value>>, String> {
    Ok(collect_replay_bundle(hint)?.map(|bundle| bundle.events))
}

fn replay_dirs(hint: &RoflMatchHint) -> Vec<PathBuf> {
    let mut dirs_out = Vec::new();
    if let Some(replay_dir) = hint.replay_dir.as_ref() {
        dirs_out.push(replay_dir.clone());
    }
    if let Some(override_dir) = std::env::var_os("YUMMI_ROFL_REPLAY_DIR") {
        dirs_out.push(PathBuf::from(override_dir));
    }
    if let Some(documents) = dirs::document_dir() {
        dirs_out.push(documents.join("League of Legends").join("Replays"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs_out.push(
            home.join("Documents")
                .join("League of Legends")
                .join("Replays"),
        );
        dirs_out.push(
            home.join("OneDrive")
                .join("Documents")
                .join("League of Legends")
                .join("Replays"),
        );
    }
    let mut seen = HashSet::new();
    dirs_out.retain(|path| seen.insert(path.clone()));
    dirs_out
}

fn find_matching_replay(hint: &RoflMatchHint) -> Result<Option<PathBuf>, String> {
    let now = SystemTime::now();
    let mut candidates = Vec::new();
    for dir in replay_dirs(hint) {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("rofl"))
            {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.len() == 0 || meta.len() > MAX_REPLAY_FILE_BYTES {
                continue;
            }
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if now
                .duration_since(modified)
                .is_ok_and(|age| age > REPLAY_RECENCY_WINDOW)
            {
                continue;
            }
            candidates.push((modified, path));
        }
    }
    candidates.sort_by_key(|(modified, _)| Reverse(*modified));
    candidates.truncate(MAX_REPLAY_CANDIDATES);

    // Riot's normal replay names contain the numeric game id. Prefer that
    // deterministic match and avoid reading every recent replay file.
    if let Some((_, path)) = candidates.iter().find(|(_, path)| {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.contains(&hint.game_id))
    }) {
        return Ok(Some(path.clone()));
    }

    let mut best: Option<(i32, SystemTime, PathBuf)> = None;
    for (modified, path) in candidates {
        let Ok(bytes) = read_metadata_window(&path) else {
            continue;
        };
        let Ok(replay) = parse_envelope(&bytes) else {
            continue;
        };
        let filename_matches = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.contains(&hint.game_id));
        let replay_puuids: HashSet<String> = replay
            .metadata
            .participants
            .iter()
            .filter_map(|participant| stat_string(participant, "PUUID"))
            .map(|value| value.to_ascii_lowercase())
            .collect();
        let puuid_matches = hint
            .participant_puuids
            .iter()
            .filter(|puuid| replay_puuids.contains(*puuid))
            .count();
        let replay_riot_ids: HashSet<String> = replay
            .metadata
            .participants
            .iter()
            .filter_map(|participant| {
                Some(normalize_riot_id(
                    &stat_string(participant, "RIOT_ID_GAME_NAME")?,
                    &stat_string(participant, "RIOT_ID_TAG_LINE")?,
                ))
            })
            .collect();
        let riot_id_matches = hint
            .participant_riot_ids
            .iter()
            .filter(|riot_id| replay_riot_ids.contains(*riot_id))
            .count();
        // Never bind a replay merely because it is recent and has ten players.
        // Filename game-id is strongest; otherwise require a majority roster match.
        if !filename_matches && puuid_matches < 5 && riot_id_matches < 5 {
            continue;
        }
        let mut score = 0i32;
        if filename_matches {
            score += 200;
        }
        score += puuid_matches as i32 * 25;
        score += riot_id_matches as i32 * 20;
        if replay.metadata.participants.len() == 10 {
            score += 20;
        }
        if best.as_ref().is_none_or(|(best_score, best_modified, _)| {
            score > *best_score || (score == *best_score && modified > *best_modified)
        }) {
            best = Some((score, modified, path));
        }
    }
    Ok(best.map(|(_, _, path)| path))
}

fn read_metadata_window(path: &Path) -> Result<Vec<u8>, String> {
    // Metadata is at EOF, but the v2 header is at the front. Candidate files are
    // capped, so a bounded full read keeps the matching path simple and safe.
    let meta = fs::metadata(path).map_err(|error| error.to_string())?;
    if meta.len() > MAX_REPLAY_FILE_BYTES {
        return Err("ROFL too large".into());
    }
    fs::read(path).map_err(|error| error.to_string())
}

fn parse_envelope(bytes: &[u8]) -> Result<ReplayEnvelope, String> {
    if bytes.len() < 32 || !bytes.starts_with(ROFL_MAGIC) {
        return Err("ROFL magic 불일치".into());
    }
    let format = read_u16(bytes, 4)?;
    if format != ROFL_FORMAT_V2 {
        return Err(format!("지원하지 않는 ROFL 포맷: {format}"));
    }
    let version_len = *bytes.get(14).ok_or("ROFL header truncated")? as usize;
    let header_size = 15usize
        .checked_add(version_len)
        .ok_or("ROFL header overflow")?;
    if version_len == 0 || header_size > bytes.len() {
        return Err("ROFL client version header 오류".into());
    }
    let client_version = std::str::from_utf8(&bytes[15..header_size])
        .map_err(|_| "ROFL clientVersion UTF-8 오류")?
        .to_owned();
    let field_06 = read_u16(bytes, 6)?;
    let digest_tail = bytes.get(8..14).ok_or("ROFL protocol digest truncated")?;
    let protocol_digest = format!(
        "{:02x}{:02x}{}",
        field_06 as u8,
        (field_06 >> 8) as u8,
        hex(digest_tail)
    );

    let metadata_len = read_u32(bytes, bytes.len() - 4)? as usize;
    if metadata_len == 0 || metadata_len > MAX_METADATA_BYTES || metadata_len + 4 > bytes.len() {
        return Err("ROFL metadata 길이 오류".into());
    }
    let metadata_offset = bytes.len() - 4 - metadata_len;
    let signature_offset = metadata_offset
        .checked_sub(SIGNATURE_SIZE)
        .ok_or("ROFL signature boundary 오류")?;
    if signature_offset < header_size {
        return Err("ROFL chunk/signature boundary 오류".into());
    }
    let metadata_value: Value = serde_json::from_slice(&bytes[metadata_offset..bytes.len() - 4])
        .map_err(|error| format!("ROFL metadata JSON 오류: {error}"))?;
    let metadata_object = metadata_value
        .as_object()
        .ok_or("ROFL metadata root가 object가 아님")?;
    let game_length_ms = metadata_object
        .get("gameLength")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let participants = match metadata_object.get("statsJson") {
        Some(Value::String(raw)) if !raw.is_empty() => {
            let parsed: Value = serde_json::from_str(raw)
                .map_err(|error| format!("ROFL statsJson 오류: {error}"))?;
            parsed
                .as_array()
                .ok_or("ROFL statsJson가 array가 아님")?
                .iter()
                .filter_map(Value::as_object)
                .cloned()
                .collect()
        }
        _ => Vec::new(),
    };
    Ok(ReplayEnvelope {
        header: ReplayHeader {
            client_version,
            header_size,
            protocol_digest,
        },
        metadata: ReplayMetadata {
            game_length_ms,
            participants,
        },
        signature_offset,
    })
}

fn summary_event(hint: &RoflMatchHint, path: &Path, replay: &ReplayEnvelope) -> Value {
    let participants: Vec<Value> = replay
        .metadata
        .participants
        .iter()
        .enumerate()
        .map(|(index, participant)| summarize_participant(index, participant))
        .collect();
    json!({
        "schemaVersion": 1,
        "kind": "summary",
        "gameId": hint.game_id,
        "replay": {
            "formatVersion": ROFL_FORMAT_V2,
            "clientVersion": replay.header.client_version,
            "protocolDigest": replay.header.protocol_digest,
            "gameLengthMs": replay.metadata.game_length_ms,
            "participantCount": participants.len(),
            "fileName": path.file_name().and_then(|value| value.to_str()),
        },
        "participants": participants,
    })
}

fn summarize_participant(index: usize, raw: &Map<String, Value>) -> Value {
    let items: Vec<Value> = (0..=6)
        .filter_map(|slot| stat_i64(raw, &format!("ITEM{slot}")))
        .map(Value::from)
        .collect();
    let perks: Vec<Value> = (0..=5)
        .filter_map(|slot| stat_i64(raw, &format!("PERK{slot}")))
        .map(Value::from)
        .collect();
    let perk_details: Vec<Value> = (0..=5)
        .filter_map(|slot| {
            let id = stat_i64(raw, &format!("PERK{slot}"))?;
            Some(json!({
                "id": id,
                "var1": stat_i64(raw, &format!("PERK{slot}_VAR1")),
                "var2": stat_i64(raw, &format!("PERK{slot}_VAR2")),
                "var3": stat_i64(raw, &format!("PERK{slot}_VAR3")),
            }))
        })
        .collect();

    let damage = json!({
        "total": stat_i64(raw, "TOTAL_DAMAGE_DEALT"),
        "physical": stat_i64(raw, "PHYSICAL_DAMAGE_DEALT_PLAYER"),
        "magic": stat_i64(raw, "MAGIC_DAMAGE_DEALT_PLAYER"),
        "true": stat_i64(raw, "TRUE_DAMAGE_DEALT_PLAYER"),
        "champions": stat_i64(raw, "TOTAL_DAMAGE_DEALT_TO_CHAMPIONS"),
        "physicalToChampions": stat_i64(raw, "PHYSICAL_DAMAGE_DEALT_TO_CHAMPIONS"),
        "magicToChampions": stat_i64(raw, "MAGIC_DAMAGE_DEALT_TO_CHAMPIONS"),
        "trueToChampions": stat_i64(raw, "TRUE_DAMAGE_DEALT_TO_CHAMPIONS"),
        "buildings": stat_i64(raw, "TOTAL_DAMAGE_DEALT_TO_BUILDINGS"),
        "turrets": stat_i64(raw, "TOTAL_DAMAGE_DEALT_TO_TURRETS"),
        "objectives": stat_i64(raw, "TOTAL_DAMAGE_DEALT_TO_OBJECTIVES"),
        "epicMonsters": stat_i64(raw, "TOTAL_DAMAGE_DEALT_TO_EPIC_MONSTERS"),
        "taken": stat_i64(raw, "TOTAL_DAMAGE_TAKEN"),
        "takenFromChampions": stat_i64(raw, "TOTAL_DAMAGE_TAKEN_FROM_CHAMPIONS"),
        "takenFromBuildings": stat_i64(raw, "TOTAL_DAMAGE_TAKEN_FROM_BUILDINGS"),
        "physicalTaken": stat_i64(raw, "PHYSICAL_DAMAGE_TAKEN"),
        "magicTaken": stat_i64(raw, "MAGIC_DAMAGE_TAKEN"),
        "trueTaken": stat_i64(raw, "TRUE_DAMAGE_TAKEN"),
        "selfMitigated": stat_i64(raw, "TOTAL_DAMAGE_SELF_MITIGATED"),
    });
    let support = json!({
        "heal": stat_i64(raw, "TOTAL_HEAL"),
        "healOnTeammates": stat_i64(raw, "TOTAL_HEAL_ON_TEAMMATES"),
        "shieldOnTeammates": stat_i64(raw, "TOTAL_DAMAGE_SHIELDED_ON_TEAMMATES"),
        "unitsHealed": stat_i64(raw, "TOTAL_UNITS_HEALED"),
        "timeCcingOthers": stat_i64(raw, "TIME_CCING_OTHERS"),
        "ccDealt": stat_i64(raw, "TOTAL_TIME_CROWD_CONTROL_DEALT"),
        "ccDealtToChampions": stat_i64(raw, "TOTAL_TIME_CROWD_CONTROL_DEALT_TO_CHAMPIONS"),
    });
    let vision = json!({
        "score": stat_i64(raw, "VISION_SCORE"),
        "wardsPlaced": stat_i64(raw, "WARD_PLACED"),
        "wardsKilled": stat_i64(raw, "WARD_KILLED"),
        "controlWardsBought": stat_i64(raw, "VISION_WARDS_BOUGHT_IN_GAME"),
        "detectorWardsPlaced": stat_i64(raw, "WARD_PLACED_DETECTOR"),
        "sightWardsBought": stat_i64(raw, "SIGHT_WARDS_BOUGHT_IN_GAME"),
    });
    let objectives = json!({
        "dragonKills": stat_i64(raw, "DRAGON_KILLS"),
        "baronKills": stat_i64(raw, "BARON_KILLS"),
        "elderKills": stat_i64(raw, "ELDER_DRAGON_KILLS"),
        "riftHeraldKills": stat_i64(raw, "RIFT_HERALD_KILLS"),
        "atakhanKills": stat_i64(raw, "ATAKHAN_KILLS"),
        "objectivesStolen": stat_i64(raw, "OBJECTIVES_STOLEN"),
        "objectivesStolenAssists": stat_i64(raw, "OBJECTIVES_STOLEN_ASSISTS"),
        "turretsKilled": stat_i64(raw, "TURRETS_KILLED"),
        "turretTakedowns": stat_i64(raw, "TURRET_TAKEDOWNS"),
        "inhibitorsKilled": stat_i64(raw, "BARRACKS_KILLED"),
        "inhibitorTakedowns": stat_i64(raw, "BARRACKS_TAKEDOWNS"),
    });
    let multi_kills = json!({
        "double": stat_i64(raw, "DOUBLE_KILLS"),
        "triple": stat_i64(raw, "TRIPLE_KILLS"),
        "quadra": stat_i64(raw, "QUADRA_KILLS"),
        "penta": stat_i64(raw, "PENTA_KILLS"),
        "largest": stat_i64(raw, "LARGEST_MULTI_KILL"),
        "largestSpree": stat_i64(raw, "LARGEST_KILLING_SPREE"),
        "killingSprees": stat_i64(raw, "KILLING_SPREES"),
    });
    let time = json!({
        "playedSeconds": stat_i64(raw, "TIME_PLAYED"),
        "deadSeconds": stat_i64(raw, "TOTAL_TIME_SPENT_DEAD"),
        "longestLifeSeconds": stat_i64(raw, "LONGEST_TIME_SPENT_LIVING"),
        "disconnectedSeconds": stat_i64(raw, "TIME_SPENT_DISCONNECTED"),
    });
    let largest_damage = json!({
        "ability": stat_i64(raw, "LARGEST_ABILITY_DAMAGE"),
        "attack": stat_i64(raw, "LARGEST_ATTACK_DAMAGE"),
        "criticalStrike": stat_i64(raw, "LARGEST_CRITICAL_STRIKE"),
    });
    let pings = json!({
        "allIn": stat_i64(raw, "ALL_IN_PINGS"),
        "assistMe": stat_i64(raw, "ASSIST_ME_PINGS"),
        "danger": stat_i64(raw, "DANGER_PINGS"),
        "enemyMissing": stat_i64(raw, "ENEMY_MISSING_PINGS"),
        "enemyVision": stat_i64(raw, "ENEMY_VISION_PINGS"),
        "getBack": stat_i64(raw, "GET_BACK_PINGS"),
        "needVision": stat_i64(raw, "NEED_VISION_PINGS"),
        "onMyWay": stat_i64(raw, "ON_MY_WAY_PINGS"),
        "push": stat_i64(raw, "PUSH_PINGS"),
        "retreat": stat_i64(raw, "RETREAT_PINGS"),
        "visionCleared": stat_i64(raw, "VISION_CLEARED_PINGS"),
        "basic": stat_i64(raw, "BASIC_PINGS"),
        "command": stat_i64(raw, "COMMAND_PINGS"),
        "hold": stat_i64(raw, "HOLD_PINGS"),
    });
    let economy = json!({
        "itemsPurchased": stat_i64(raw, "ITEMS_PURCHASED"),
        "consumablesPurchased": stat_i64(raw, "CONSUMABLES_PURCHASED"),
        "turretPlateGold": stat_i64(raw, "Missions_GoldFromTurretPlatesTaken"),
        "structureGold": stat_i64(raw, "Missions_GoldFromStructuresDestroyed"),
        "takedownGold": stat_i64(raw, "Missions_TakedownGold"),
    });
    let analysis = json!({
        "lastTakedownSecond": stat_i64(raw, "LAST_TAKEDOWN_TIME"),
        "turretPlatesDestroyed": stat_i64(raw, "Missions_TurretPlatesDestroyed"),
        "takedownsBefore15": stat_i64(raw, "Missions_TakedownsBefore15Min"),
        "takedownsUnderTurret": stat_i64(raw, "Missions_TakedownsUnderTurret"),
        "takedownsAfterTeleport": stat_i64(raw, "Missions_TakedownsAfterTeleporting"),
        "usefulWards": stat_i64(raw, "Missions_PlaceUsefulWards"),
        "usefulControlWards": stat_i64(raw, "Missions_PlaceUsefulControlWards"),
        "immobilizeChampions": stat_i64(raw, "Missions_ImmobilizeChampions"),
        "roleBoundItem": stat_i64(raw, "ROLE_BOUND_ITEM"),
    });
    let flags = json!({
        "afk": stat_bool(raw, "WAS_AFK"),
        "leaver": stat_bool(raw, "WAS_LEAVER"),
        "earlySurrender": stat_bool(raw, "GAME_ENDED_IN_EARLY_SURRENDER"),
        "surrender": stat_bool(raw, "GAME_ENDED_IN_SURRENDER"),
        "teamEarlySurrendered": stat_bool(raw, "TEAM_EARLY_SURRENDERED"),
    });

    json!({
        "participantIndex": index,
        "champion": stat_string(raw, "SKIN"),
        "team": stat_i64(raw, "TEAM"),
        "position": stat_string(raw, "INDIVIDUAL_POSITION").or_else(|| stat_string(raw, "TEAM_POSITION")),
        "win": stat_string(raw, "WIN").map(|value| value.eq_ignore_ascii_case("win")),
        "level": stat_i64(raw, "LEVEL"),
        "kills": stat_i64(raw, "CHAMPIONS_KILLED"),
        "deaths": stat_i64(raw, "NUM_DEATHS"),
        "assists": stat_i64(raw, "ASSISTS"),
        "cs": stat_i64(raw, "MINIONS_KILLED"),
        "neutralCs": stat_i64(raw, "NEUTRAL_MINIONS_KILLED"),
        "enemyJungleCs": stat_i64(raw, "NEUTRAL_MINIONS_KILLED_ENEMY_JUNGLE"),
        "ownJungleCs": stat_i64(raw, "NEUTRAL_MINIONS_KILLED_YOUR_JUNGLE"),
        "goldEarned": stat_i64(raw, "GOLD_EARNED"),
        "goldSpent": stat_i64(raw, "GOLD_SPENT"),
        "xp": stat_i64(raw, "EXP"),
        "items": items,
        "summonerSpells": [stat_i64(raw, "SUMMONER_SPELL_1"), stat_i64(raw, "SUMMONER_SPELL_2")],
        "summonerSpellCasts": [stat_i64(raw, "SUMMON_SPELL1_CAST"), stat_i64(raw, "SUMMON_SPELL2_CAST")],
        "spellCasts": [stat_i64(raw, "SPELL1_CAST"), stat_i64(raw, "SPELL2_CAST"), stat_i64(raw, "SPELL3_CAST"), stat_i64(raw, "SPELL4_CAST")],
        "perks": perks,
        "perkDetails": perk_details,
        "keystoneId": stat_i64(raw, "KEYSTONE_ID"),
        "perkPrimaryStyle": stat_i64(raw, "PERK_PRIMARY_STYLE"),
        "perkSubStyle": stat_i64(raw, "PERK_SUB_STYLE"),
        "statPerks": [stat_i64(raw, "STAT_PERK_0"), stat_i64(raw, "STAT_PERK_1"), stat_i64(raw, "STAT_PERK_2")],
        "damage": damage,
        "support": support,
        "vision": vision,
        "objectives": objectives,
        "multiKills": multi_kills,
        "time": time,
        "largestDamage": largest_damage,
        "pings": pings,
        "economy": economy,
        "analysis": analysis,
        "flags": flags,
    })
}

fn decode_movement(bytes: &[u8], replay: &ReplayEnvelope) -> Result<Vec<MovementRecord>, String> {
    let mut chunk_cursor = replay.header.header_size;
    let mut records = Vec::new();
    while chunk_cursor < replay.signature_offset {
        if chunk_cursor + CHUNK_HEADER_SIZE > replay.signature_offset {
            return Err("ROFL chunk header truncated".into());
        }
        let stream_raw = read_u32(bytes, chunk_cursor + 5)?;
        let stream = ((stream_raw >> 24) & 0xff) as u8;
        let raw_size = read_u32(bytes, chunk_cursor + 9)? as usize;
        let compressed_size = read_u32(bytes, chunk_cursor + 13)? as usize;
        let body_offset = chunk_cursor + CHUNK_HEADER_SIZE;
        let stored_size = if compressed_size == 0 {
            raw_size
        } else {
            compressed_size
        };
        let body_end = body_offset
            .checked_add(stored_size)
            .ok_or("ROFL chunk size overflow")?;
        if body_end > replay.signature_offset {
            return Err("ROFL chunk out of bounds".into());
        }
        if stream == GAME_CHUNK_STREAM {
            if raw_size > MAX_DECOMPRESSED_CHUNK_BYTES {
                return Err("ROFL chunk decompressed size too large".into());
            }
            let body = if compressed_size == 0 {
                bytes[body_offset..body_end].to_vec()
            } else {
                let mut decoder = StreamingDecoder::new(&bytes[body_offset..body_end])
                    .map_err(|error| format!("ROFL Zstd init 실패: {error:?}"))?;
                let mut decoded = Vec::with_capacity(raw_size);
                decoder
                    .read_to_end(&mut decoded)
                    .map_err(|error| format!("ROFL Zstd decode 실패: {error}"))?;
                if decoded.len() != raw_size {
                    return Err(format!(
                        "ROFL chunk size mismatch: expected={raw_size} actual={}",
                        decoded.len()
                    ));
                }
                decoded
            };
            decode_game_chunk_blocks(&body, &mut records)?;
            if records.len() > MAX_MOVEMENT_RECORDS {
                return Err("ROFL movement record limit exceeded".into());
            }
        }
        chunk_cursor = body_end;
    }
    Ok(records)
}

fn decode_game_chunk_blocks(body: &[u8], records: &mut Vec<MovementRecord>) -> Result<(), String> {
    let mut cursor = 0usize;
    let mut timestamp = 0f32;
    let mut packet_id = 0u16;
    let mut param = 0u32;
    while cursor < body.len() {
        let marker = *body.get(cursor).ok_or("ROFL block marker truncated")?;
        cursor += 1;
        if marker & 0x80 != 0 {
            let delta = *body.get(cursor).ok_or("ROFL block delta truncated")?;
            cursor += 1;
            timestamp += f32::from(delta) * 0.001;
        } else {
            timestamp = read_f32(body, cursor)?;
            cursor += 4;
        }
        if !timestamp.is_finite() || timestamp < 0.0 {
            return Err("ROFL block timestamp invalid".into());
        }
        let length = if marker & 0x10 != 0 {
            let value = *body.get(cursor).ok_or("ROFL block length truncated")? as usize;
            cursor += 1;
            value
        } else {
            let value = read_u32(body, cursor)? as usize;
            cursor += 4;
            value
        };
        if marker & 0x40 == 0 {
            packet_id = read_u16(body, cursor)?;
            cursor += 2;
        }
        if marker & 0x20 != 0 {
            let delta = *body.get(cursor).ok_or("ROFL block param truncated")? as u32;
            cursor += 1;
            param = param.wrapping_add(delta);
        } else {
            param = read_u32(body, cursor)?;
            cursor += 4;
        }
        let end = cursor.checked_add(length).ok_or("ROFL block overflow")?;
        if end > body.len() {
            return Err("ROFL block payload truncated".into());
        }
        if packet_id == MOVEMENT_OPCODE_26_17 {
            if let Some(decoded) = decode_movement_wire(&body[cursor..end])? {
                parse_movement_payload(timestamp, &decoded, records)?;
            }
        }
        let _ = param;
        cursor = end;
    }
    Ok(())
}

fn decode_movement_wire(raw: &[u8]) -> Result<Option<Vec<u8>>, String> {
    if raw.is_empty() {
        return Ok(None);
    }
    // The exact-build semantic profile stubs the inherited base decoder, so
    // the first byte is the movement packet's bitfield. Bit 3 gates the byte vector.
    if (raw[0] >> 3) & 1 == 0 {
        return Ok(None);
    }
    let mut cursor = 1usize;
    let length = decode_var_u32(raw, &mut cursor)? as usize;
    if length == 0 {
        return Ok(Some(Vec::new()));
    }
    if length > 16 * 1024 * 1024 || length > raw.len().saturating_sub(cursor) {
        return Err("ROFL movement decoded length invalid".into());
    }
    let mut output = vec![0u8; length];
    let mut front = 0usize;
    let mut back = length - 1;
    while front < back {
        output[front] = movement_wire_byte(*raw.get(cursor).ok_or("movement payload truncated")?);
        cursor += 1;
        front += 1;
        output[back] = movement_wire_byte(*raw.get(cursor).ok_or("movement payload truncated")?);
        cursor += 1;
        back -= 1;
    }
    if front == back {
        output[front] = movement_wire_byte(*raw.get(cursor).ok_or("movement payload truncated")?);
    }
    Ok(Some(output))
}

fn decode_var_u32(raw: &[u8], cursor: &mut usize) -> Result<u32, String> {
    let mut shift = 0u32;
    let mut value = 0u32;
    loop {
        if shift >= 35 {
            return Err("movement varint overflow".into());
        }
        let byte = movement_wire_byte(*raw.get(*cursor).ok_or("movement varint truncated")?);
        *cursor += 1;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

fn movement_wire_byte(value: u8) -> u8 {
    let value = value.wrapping_add(0x3c);
    let value = !value;
    let value = value.wrapping_add(0x22);
    let value = value ^ 0x58;
    value.wrapping_sub(0x40)
}

fn parse_movement_payload(
    timestamp: f32,
    payload: &[u8],
    output: &mut Vec<MovementRecord>,
) -> Result<(), String> {
    let mut cursor = 0usize;
    while cursor < payload.len() {
        if payload.len() - cursor < 10 {
            return Err("movement record truncated".into());
        }
        let parsing_type = read_u16(payload, cursor)?;
        let entity_id = read_u32(payload, cursor + 2)?;
        let speed = read_f32(payload, cursor + 6)?;
        if !speed.is_finite() || speed < 0.0 {
            return Err("movement speed invalid".into());
        }
        cursor += 10;
        if parsing_type & 1 != 0 {
            cursor = cursor.checked_add(1).ok_or("movement cursor overflow")?;
            if cursor > payload.len() {
                return Err("movement optional field truncated".into());
            }
        }
        let count = ((parsing_type & 0xff) >> 1) as usize;
        if count == 0 || count > MAX_WAYPOINTS_PER_RECORD {
            return Err("movement waypoint count invalid".into());
        }
        let bitmap_start = cursor;
        let bitmap_size = if count > 1 { ((count - 2) >> 2) + 1 } else { 0 };
        cursor = cursor
            .checked_add(bitmap_size)
            .ok_or("movement bitmap overflow")?;
        if cursor > payload.len() {
            return Err("movement bitmap truncated".into());
        }
        let mut encoded = Vec::with_capacity(count);
        let mut previous_x = 0u16;
        let mut previous_y = 0u16;
        let mut bit_index = 0usize;
        for index in 0..count {
            let (x_delta, y_delta) = if index == 0 {
                (false, false)
            } else {
                let x = bitmap_bit(payload, bitmap_start, bit_index)?;
                bit_index += 1;
                let y = bitmap_bit(payload, bitmap_start, bit_index)?;
                bit_index += 1;
                (x, y)
            };
            (previous_x, cursor) = movement_coordinate(payload, cursor, previous_x, x_delta)?;
            (previous_y, cursor) = movement_coordinate(payload, cursor, previous_y, y_delta)?;
            encoded.push((previous_x, previous_y));
        }
        let waypoints = encoded
            .into_iter()
            .map(|(x, y)| {
                (
                    f32::from(x as i16) * 2.0 + 7358.0,
                    f32::from(y as i16) * 2.0 + 7412.0,
                )
            })
            .collect();
        output.push(MovementRecord {
            timestamp_ms: (timestamp * 1000.0).max(0.0).round() as u32,
            entity_id,
            speed,
            waypoints,
        });
    }
    Ok(())
}

fn bitmap_bit(payload: &[u8], start: usize, index: usize) -> Result<bool, String> {
    let byte = *payload
        .get(start + (index >> 3))
        .ok_or("movement bitmap bit truncated")?;
    Ok(byte & (1 << (index & 7)) != 0)
}

fn movement_coordinate(
    payload: &[u8],
    cursor: usize,
    previous: u16,
    delta: bool,
) -> Result<(u16, usize), String> {
    if delta {
        let value = *payload.get(cursor).ok_or("movement coordinate truncated")?;
        Ok((previous.wrapping_add(u16::from(value)), cursor + 1))
    } else {
        Ok((read_u16(payload, cursor)?, cursor + 2))
    }
}

fn movement_events(
    hint: &RoflMatchHint,
    replay: &ReplayEnvelope,
    records: Vec<MovementRecord>,
) -> Vec<Value> {
    if records.is_empty() {
        return vec![json!({
            "schemaVersion": 1,
            "kind": "semantic_status",
            "gameId": hint.game_id,
            "clientVersion": replay.header.client_version,
            "movement": {"status": "no_records", "opcode": MOVEMENT_OPCODE_26_17},
        })];
    }
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for record in &records {
        if (0x4000_0000..=0x4000_ffff).contains(&record.entity_id) {
            *counts.entry(record.entity_id).or_default() += 1;
        }
    }
    let mut fountain_entities: HashMap<u32, u8> = HashMap::new();
    for record in &records {
        if record.timestamp_ms > 2_000 || !(0x4000_0000..=0x4000_ffff).contains(&record.entity_id) {
            continue;
        }
        let Some(&(x, y)) = record.waypoints.first() else {
            continue;
        };
        if x <= 1_200.0 && y <= 1_200.0 {
            fountain_entities.insert(record.entity_id, 0);
        } else if x >= 13_500.0 && y >= 13_500.0 {
            fountain_entities.insert(record.entity_id, 1);
        }
    }
    let mut fountain_ids: Vec<u32> = fountain_entities.keys().copied().collect();
    fountain_ids.sort_unstable();
    let mut player_entities: Vec<u32> = Vec::new();
    for start in fountain_ids {
        let run: Vec<u32> = (0..10).map(|offset| start.saturating_add(offset)).collect();
        if !run.iter().all(|entity| fountain_entities.contains_key(entity)) {
            continue;
        }
        let blue = run
            .iter()
            .filter(|entity| fountain_entities.get(entity).copied() == Some(0))
            .count();
        let red = run
            .iter()
            .filter(|entity| fountain_entities.get(entity).copied() == Some(1))
            .count();
        if blue == 5 && red == 5 {
            player_entities = run;
            break;
        }
    }
    if player_entities.len() != 10 {
        let mut ranked: Vec<(u32, usize)> = counts.into_iter().collect();
        ranked.sort_by_key(|(entity, count)| (Reverse(*count), *entity));
        player_entities = ranked
            .into_iter()
            .take(10)
            .map(|(entity, _)| entity)
            .collect();
        player_entities.sort_unstable();
        let contiguous = player_entities
            .windows(2)
            .all(|pair| pair[1] == pair[0].saturating_add(1));
        if !contiguous {
            player_entities.clear();
        }
    }
    if player_entities.len() != 10 {
        return vec![json!({
            "schemaVersion": 1,
            "kind": "semantic_status",
            "gameId": hint.game_id,
            "clientVersion": replay.header.client_version,
            "movement": {"status": "player_detection_failed", "candidateCount": player_entities.len()},
        })];
    }
    let player_set: HashSet<u32> = player_entities.iter().copied().collect();
    let mut tracks: HashMap<u32, Vec<MovementRecord>> = HashMap::new();
    for record in records {
        if player_set.contains(&record.entity_id) {
            tracks.entry(record.entity_id).or_default().push(record);
        }
    }
    let game_length_ms = replay
        .metadata
        .game_length_ms
        .or_else(|| {
            tracks
                .values()
                .filter_map(|track| track.last().map(|record| record.timestamp_ms))
                .max()
        })
        .unwrap_or(0);
    if game_length_ms == 0 {
        return Vec::new();
    }
    let full_samples: Vec<(u32, Vec<u16>)> = player_entities
        .iter()
        .map(|entity| {
            let samples = sample_track(
                tracks.get(entity).map(Vec::as_slice).unwrap_or(&[]),
                game_length_ms,
                MOVEMENT_SAMPLE_STEP_MS,
            );
            (*entity, samples)
        })
        .collect();
    let part_count = game_length_ms.div_ceil(MOVEMENT_CHUNK_MS).max(1);
    let mut events = Vec::with_capacity(part_count as usize);
    for part in 0..part_count {
        let start_ms = part * MOVEMENT_CHUNK_MS;
        let end_ms = ((part + 1) * MOVEMENT_CHUNK_MS).min(game_length_ms);
        let start_index = (start_ms / MOVEMENT_SAMPLE_STEP_MS) as usize;
        let end_index = (end_ms / MOVEMENT_SAMPLE_STEP_MS) as usize + 1;
        let players: Vec<Value> = full_samples
            .iter()
            .enumerate()
            .map(|(participant_index, (entity_id, xy))| {
                let pair_start = start_index.saturating_mul(2).min(xy.len());
                let pair_end = end_index.saturating_mul(2).min(xy.len());
                json!({
                    "participantIndex": participant_index,
                    "entityId": entity_id,
                    "xy": &xy[pair_start..pair_end],
                })
            })
            .collect();
        events.push(json!({
            "schemaVersion": 1,
            "kind": "movement",
            "gameId": hint.game_id,
            "clientVersion": replay.header.client_version,
            "opcode": MOVEMENT_OPCODE_26_17,
            "stepMs": MOVEMENT_SAMPLE_STEP_MS,
            "startMs": start_ms,
            "endMs": end_ms,
            "part": part,
            "parts": part_count,
            "players": players,
        }));
    }
    events
}

fn sample_track(records: &[MovementRecord], game_length_ms: u32, step_ms: u32) -> Vec<u16> {
    let sample_count = game_length_ms.div_ceil(step_ms) as usize + 1;
    let mut output = Vec::with_capacity(sample_count * 2);
    if records.is_empty() {
        output.resize(sample_count * 2, 0);
        return output;
    }
    let mut event_index = 0usize;
    for sample in 0..sample_count {
        let time_ms = (sample as u32).saturating_mul(step_ms).min(game_length_ms);
        while event_index + 1 < records.len() && records[event_index + 1].timestamp_ms <= time_ms {
            event_index += 1;
        }
        let record = &records[event_index];
        let elapsed = time_ms.saturating_sub(record.timestamp_ms) as f32 / 1000.0;
        let (x, y) = interpolate_path(record, elapsed);
        output.push(x.round().clamp(0.0, u16::MAX as f32) as u16);
        output.push(y.round().clamp(0.0, u16::MAX as f32) as u16);
    }
    output
}

fn interpolate_path(record: &MovementRecord, elapsed_seconds: f32) -> (f32, f32) {
    let Some(&first) = record.waypoints.first() else {
        return (0.0, 0.0);
    };
    if record.waypoints.len() == 1 || record.speed <= 0.0 || elapsed_seconds <= 0.0 {
        return first;
    }
    let mut remaining = record.speed * elapsed_seconds;
    let mut current = first;
    for &next in record.waypoints.iter().skip(1) {
        let dx = next.0 - current.0;
        let dy = next.1 - current.1;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance <= f32::EPSILON {
            current = next;
            continue;
        }
        if remaining <= distance {
            let ratio = remaining / distance;
            return (current.0 + dx * ratio, current.1 + dy * ratio);
        }
        remaining -= distance;
        current = next;
    }
    current
}

fn normalize_riot_id(game_name: &str, tag_line: &str) -> String {
    let game_name = game_name
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let tag_line = tag_line
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if game_name.is_empty() || tag_line.is_empty() {
        String::new()
    } else {
        format!("{game_name}#{tag_line}")
    }
}

fn stat_string(raw: &Map<String, Value>, key: &str) -> Option<String> {
    raw.get(key).and_then(|value| match value {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn stat_i64(raw: &Map<String, Value>, key: &str) -> Option<i64> {
    raw.get(key).and_then(|value| match value {
        Value::String(value) => value.parse().ok(),
        Value::Number(value) => value.as_i64(),
        _ => None,
    })
}

fn stat_bool(raw: &Map<String, Value>, key: &str) -> Option<bool> {
    stat_i64(raw, key).map(|value| value != 0)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let raw: [u8; 2] = bytes
        .get(offset..offset + 2)
        .ok_or("u16 out of bounds")?
        .try_into()
        .map_err(|_| "u16 conversion failed")?;
    Ok(u16::from_le_bytes(raw))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let raw: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or("u32 out of bounds")?
        .try_into()
        .map_err(|_| "u32 conversion failed")?;
    Ok(u32::from_le_bytes(raw))
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f32, String> {
    Ok(f32::from_bits(read_u32(bytes, offset)?))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_wire_byte_matches_verified_26_17_examples() {
        assert_eq!(movement_wire_byte(0x72), 0xeb);
        assert_eq!(movement_wire_byte(0xcb), 0x02);
    }

    #[test]
    fn movement_payload_parser_decodes_spawn_coordinate() {
        let payload = hex_bytes("0300ae0000400000000001cff2b8f2");
        let mut records = Vec::new();
        parse_movement_payload(0.129, &payload, &mut records).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].entity_id, 0x4000_00ae);
        let (x, y) = records[0].waypoints[0];
        assert_eq!((x, y), (604.0, 612.0));
    }

    #[test]
    fn interpolation_moves_along_waypoints() {
        let record = MovementRecord {
            timestamp_ms: 0,
            entity_id: 1,
            speed: 100.0,
            waypoints: vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)],
        };
        assert_eq!(interpolate_path(&record, 0.5), (50.0, 0.0));
        assert_eq!(interpolate_path(&record, 1.5), (100.0, 50.0));
        assert_eq!(interpolate_path(&record, 3.0), (100.0, 100.0));
    }

    fn hex_bytes(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }
}
