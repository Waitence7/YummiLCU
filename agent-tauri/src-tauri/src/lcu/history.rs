use std::{
    collections::HashSet,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::{stream, StreamExt};
use reqwest::Method;
use serde_json::{json, Value};

use super::LcuClient;
use crate::error::{AgentError, AgentResult};

const HISTORY_PAGE_SIZE: usize = 20;
const MAX_HISTORY_MATCHES: usize = 200;
const DETAIL_CONCURRENCY: usize = 5;
const CACHE_SCHEMA: u64 = 1;
const MAX_CACHE_BYTES: u64 = 8 * 1024 * 1024;

impl LcuClient {
    pub(crate) async fn replay_directory(&self) -> AgentResult<Option<String>> {
        self.request(Method::GET, "/lol-replays/v1/rofls/path", None)
            .await
            .map(|value| value.as_str().map(str::to_owned))
    }

    pub(crate) async fn download_history_replay(&self, game_id: &str) -> AgentResult<()> {
        self.request(
            Method::POST,
            &format!("/lol-replays/v1/rofls/{game_id}/download/graceful"),
            Some(json!({"componentType": "replay-button_end-of-game"})),
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn match_history(&self, offset: u32) -> AgentResult<(String, Value)> {
        validate_history_offset(offset)?;

        let summoner = self
            .request(Method::GET, "/lol-summoner/v1/current-summoner", None)
            .await?;
        let puuid = summoner
            .get("puuid")
            .and_then(Value::as_str)
            .filter(|value| {
                !value.is_empty()
                    && value
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
            .ok_or_else(|| AgentError::Lcu("소환사 정보를 확인하지 못했습니다.".into()))?
            .to_owned();

        let response = self
            .request(
                Method::GET,
                &format!(
                    "/lol-match-history/v1/products/lol/{puuid}/matches?begIndex={offset}&endIndex={}",
                    offset + HISTORY_PAGE_SIZE as u32
                ),
                None,
            )
            .await?;
        let games = response
            .pointer("/games/games")
            .and_then(Value::as_array)
            .ok_or_else(|| AgentError::Lcu("전적 응답을 읽지 못했습니다.".into()))?;

        let champions = self
            .request(
                Method::GET,
                "/lol-game-data/assets/v1/champion-summary.json",
                None,
            )
            .await
            .unwrap_or(Value::Null);

        // The products/lol history list is intentionally compact and can contain
        // only the queried player's participant row. Fetch each game's detail so
        // the result screen has the complete roster (allies and opponents).
        let puuid_ref = puuid.as_str();
        let matches = stream::iter(games.iter().cloned())
            .map(|summary| {
                let champions = &champions;
                let puuid = puuid_ref;
                async move {
                    let game_id = game_id_string(&summary);
                    let detail = if game_id.is_empty() {
                        None
                    } else {
                        self.request(
                            Method::GET,
                            &format!("/lol-match-history/v1/games/{game_id}"),
                            None,
                        )
                        .await
                        .ok()
                    };
                    normalize_match(detail.as_ref().unwrap_or(&summary), puuid, champions)
                }
            })
            .buffered(DETAIL_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

        let summoner_name = summoner
            .get("gameName")
            .or_else(|| summoner.get("displayName"))
            .and_then(Value::as_str)
            .unwrap_or("내 경기");

        Ok((
            puuid,
            json!({
                "summoner": summoner_name,
                "matches": matches,
                "hasMore": games.len() >= HISTORY_PAGE_SIZE
                    && offset + HISTORY_PAGE_SIZE as u32 < MAX_HISTORY_MATCHES as u32,
                "source": "live",
                "savedAt": now_ms(),
            }),
        ))
    }
}

pub(crate) async fn save_match_history_page(account_key: &str, page: &Value) -> AgentResult<()> {
    let path = history_cache_path();
    let existing = read_cache_value(&path).await?;
    let merged = merge_cache_value(existing.as_ref(), account_key, page, now_ms());

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let bytes = serde_json::to_vec(&merged)?;
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return Err(AgentError::Lcu("저장할 전적 데이터가 너무 큽니다.".into()));
    }

    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    tokio::fs::write(&temp, bytes).await?;
    if tokio::fs::metadata(&path).await.is_ok() {
        let _ = tokio::fs::remove_file(&path).await;
    }
    tokio::fs::rename(&temp, &path).await?;
    Ok(())
}

pub(crate) async fn load_cached_match_history(offset: u32) -> AgentResult<Option<Value>> {
    validate_history_offset(offset)?;
    let Some(cache) = read_cache_value(&history_cache_path()).await? else {
        return Ok(None);
    };
    Ok(cached_page_from_value(&cache, offset))
}

pub(crate) fn empty_match_history() -> Value {
    json!({
        "summoner": "내 경기",
        "matches": [],
        "hasMore": false,
        "source": "none",
        "savedAt": Value::Null,
    })
}

fn validate_history_offset(offset: u32) -> AgentResult<()> {
    if offset as usize >= MAX_HISTORY_MATCHES || offset as usize % HISTORY_PAGE_SIZE != 0 {
        return Err(AgentError::Lcu(
            "최근 200경기까지 조회할 수 있습니다.".into(),
        ));
    }
    Ok(())
}

fn history_cache_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("YummiAgent")
        .join("match-history-cache.json")
}

async fn read_cache_value(path: &std::path::Path) -> AgentResult<Option<Value>> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CACHE_BYTES {
        return Ok(None);
    }

    let bytes = tokio::fs::read(path).await?;
    let value: Value = serde_json::from_slice(&bytes)?;
    if value.get("schema").and_then(Value::as_u64) != Some(CACHE_SCHEMA)
        || value.get("matches").and_then(Value::as_array).is_none()
    {
        return Ok(None);
    }
    Ok(Some(value))
}

fn merge_cache_value(
    existing: Option<&Value>,
    account_key: &str,
    page: &Value,
    saved_at: u64,
) -> Value {
    let same_account = existing
        .and_then(|value| value.get("accountKey"))
        .and_then(Value::as_str)
        == Some(account_key);

    let mut matches = page
        .get("matches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if same_account {
        if let Some(previous) = existing
            .and_then(|value| value.get("matches"))
            .and_then(Value::as_array)
        {
            matches.extend(previous.iter().cloned());
        }
    }

    let mut seen = HashSet::new();
    matches.retain(|entry| {
        let id = match_id(entry);
        !id.is_empty() && seen.insert(id)
    });
    matches.sort_by(|left, right| match_created_at(right).cmp(&match_created_at(left)));
    matches.truncate(MAX_HISTORY_MATCHES);

    json!({
        "schema": CACHE_SCHEMA,
        "accountKey": account_key,
        "summoner": page
            .get("summoner")
            .cloned()
            .or_else(|| existing.and_then(|value| value.get("summoner")).cloned())
            .unwrap_or_else(|| Value::String("내 경기".into())),
        "savedAt": saved_at,
        "matches": matches,
    })
}

fn cached_page_from_value(cache: &Value, offset: u32) -> Option<Value> {
    let matches = cache.get("matches")?.as_array()?;
    let start = offset as usize;
    if start > matches.len() {
        return Some(json!({
            "summoner": cache.get("summoner").cloned().unwrap_or_else(|| Value::String("내 경기".into())),
            "matches": [],
            "hasMore": false,
            "source": "cache",
            "savedAt": cache.get("savedAt").cloned().unwrap_or(Value::Null),
        }));
    }
    let end = (start + HISTORY_PAGE_SIZE).min(matches.len());
    Some(json!({
        "summoner": cache.get("summoner").cloned().unwrap_or_else(|| Value::String("내 경기".into())),
        "matches": matches[start..end].to_vec(),
        "hasMore": end < matches.len(),
        "source": "cache",
        "savedAt": cache.get("savedAt").cloned().unwrap_or(Value::Null),
    }))
}

fn match_created_at(value: &Value) -> u64 {
    value.get("createdAt").and_then(Value::as_u64).unwrap_or(0)
}

fn match_id(value: &Value) -> String {
    value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn game_id_string(game: &Value) -> String {
    game.get("gameId")
        .and_then(Value::as_u64)
        .map(|id| id.to_string())
        .or_else(|| {
            game.get("gameId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn normalize_match(game: &Value, puuid: &str, champions: &Value) -> Value {
    let identities = game.get("participantIdentities").and_then(Value::as_array);

    let me = identities
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.pointer("/player/puuid").and_then(Value::as_str) == Some(puuid)
                    || row.get("puuid").and_then(Value::as_str) == Some(puuid)
            })
        })
        .and_then(|row| row.get("participantId"))
        .cloned()
        .unwrap_or(Value::Null);

    let players = game
        .get("participants")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|participant| {
            let id = participant
                .get("participantId")
                .cloned()
                .unwrap_or(Value::Null);
            let identity = identities
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row.get("participantId") == Some(&id))
                })
                .and_then(|row| row.get("player"))
                .unwrap_or(&Value::Null);

            let (name, tag_line) = player_name(identity);
            let champion_id = participant
                .get("championId")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let champion = champions
                .as_array()
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row.get("id").and_then(Value::as_i64) == Some(champion_id))
                })
                .and_then(|row| row.get("name"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("챔피언 {champion_id}"));
            let stats = participant.get("stats").unwrap_or(&Value::Null);
            let timeline = participant.get("timeline").unwrap_or(&Value::Null);
            let cs = stats
                .get("totalMinionsKilled")
                .and_then(Value::as_i64)
                .map(|cs| {
                    cs + stats
                        .get("neutralMinionsKilled")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                });
            let items = (0..7)
                .map(|slot| {
                    stats
                        .get(format!("item{slot}"))
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                })
                .collect::<Vec<_>>();

            json!({
                "id": id,
                "name": name,
                "tagLine": tag_line,
                "championId": champion_id,
                "champion": champion,
                "team": participant.get("teamId").cloned().unwrap_or(Value::Null),
                "win": stats.get("win").cloned().unwrap_or(Value::Null),
                "level": stats.get("champLevel").cloned().unwrap_or(Value::Null),
                "kills": stats.get("kills").cloned().unwrap_or(Value::Null),
                "deaths": stats.get("deaths").cloned().unwrap_or(Value::Null),
                "assists": stats.get("assists").cloned().unwrap_or(Value::Null),
                "cs": cs,
                "gold": stats.get("goldEarned").cloned().unwrap_or(Value::Null),
                "damage": stats.get("totalDamageDealtToChampions").cloned().unwrap_or(Value::Null),
                "vision": stats.get("visionScore").cloned().unwrap_or(Value::Null),
                "lane": timeline.get("lane").cloned().unwrap_or(Value::Null),
                "role": timeline.get("role").cloned().unwrap_or(Value::Null),
                "summonerSpells": [
                    participant.get("spell1Id").cloned().unwrap_or(Value::Null),
                    participant.get("spell2Id").cloned().unwrap_or(Value::Null)
                ],
                "items": items,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "id": game_id_string(game),
        "createdAt": game.get("gameCreation").cloned().unwrap_or(Value::Null),
        "duration": game.get("gameDuration").and_then(Value::as_u64).unwrap_or(0),
        "queueId": game.get("queueId").and_then(Value::as_i64).unwrap_or(-1),
        "mapId": game.get("mapId").and_then(Value::as_i64).unwrap_or(0),
        "version": game.get("gameVersion").and_then(Value::as_str).unwrap_or(""),
        "me": me,
        "players": players,
    })
}

fn player_name(identity: &Value) -> (String, Option<String>) {
    let game_name = identity
        .get("gameName")
        .or_else(|| identity.get("riotIdGameName"))
        .or_else(|| identity.get("summonerName"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("알 수 없는 소환사")
        .trim();

    let tag_line = identity
        .get("tagLine")
        .or_else(|| identity.get("riotIdTagLine"))
        .or_else(|| identity.get("riotIdTagline"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if game_name.contains('#') {
        return (game_name.to_owned(), tag_line.map(str::to_owned));
    }
    match tag_line {
        Some(tag) => (format!("{game_name}#{tag}"), Some(tag.to_owned())),
        None => (game_name.to_owned(), None),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_full_roster_and_riot_ids() {
        let game = json!({
            "gameId": 123,
            "gameCreation": 9000,
            "participants": [
                {
                    "participantId": 1,
                    "championId": 99,
                    "teamId": 100,
                    "spell1Id": 4,
                    "spell2Id": 14,
                    "stats": {"win": true, "champLevel": 18, "kills": 3, "deaths": 1, "assists": 9}
                },
                {
                    "participantId": 7,
                    "championId": 103,
                    "teamId": 100,
                    "stats": {
                        "win": true,
                        "champLevel": 17,
                        "kills": 10,
                        "deaths": 2,
                        "assists": 8,
                        "totalMinionsKilled": 150,
                        "neutralMinionsKilled": 12,
                        "goldEarned": 12345,
                        "totalDamageDealtToChampions": 23456
                    }
                },
                {
                    "participantId": 8,
                    "championId": 1,
                    "teamId": 200,
                    "stats": {"win": false, "champLevel": 16, "kills": 2, "deaths": 8, "assists": 4}
                }
            ],
            "participantIdentities": [
                {"participantId": 1, "player": {"puuid": "ally", "gameName": "아군", "tagLine": "KR1"}},
                {"participantId": 7, "player": {"puuid": "me", "gameName": "테스트", "tagLine": "7777"}},
                {"participantId": 8, "player": {"puuid": "enemy", "gameName": "적군", "tagLine": "KR2"}}
            ]
        });
        let result = normalize_match(
            &game,
            "me",
            &json!([
                {"id": 99, "name": "럭스"},
                {"id": 103, "name": "아리"},
                {"id": 1, "name": "애니"}
            ]),
        );

        assert_eq!(result["id"], "123");
        assert_eq!(result["me"], 7);
        assert_eq!(result["players"].as_array().unwrap().len(), 3);
        assert_eq!(result["players"][0]["name"], "아군#KR1");
        assert_eq!(result["players"][1]["name"], "테스트#7777");
        assert_eq!(result["players"][1]["champion"], "아리");
        assert_eq!(result["players"][1]["level"], 17);
        assert_eq!(result["players"][1]["cs"], 162);
        assert_eq!(result["players"][2]["team"], 200);
    }

    #[test]
    fn cache_merges_pages_deduplicates_and_sorts_newest_first() {
        let existing = json!({
            "schema": CACHE_SCHEMA,
            "accountKey": "me",
            "summoner": "테스트",
            "savedAt": 1,
            "matches": [
                {"id": "2", "createdAt": 200},
                {"id": "1", "createdAt": 100}
            ]
        });
        let page = json!({
            "summoner": "테스트",
            "matches": [
                {"id": "3", "createdAt": 300},
                {"id": "2", "createdAt": 200}
            ]
        });

        let merged = merge_cache_value(Some(&existing), "me", &page, 99);
        let matches = merged["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0]["id"], "3");
        assert_eq!(matches[1]["id"], "2");
        assert_eq!(matches[2]["id"], "1");
        assert_eq!(merged["savedAt"], 99);
    }

    #[test]
    fn cache_is_reset_when_account_changes() {
        let existing = json!({
            "schema": CACHE_SCHEMA,
            "accountKey": "old",
            "matches": [{"id": "1", "createdAt": 100}]
        });
        let page = json!({
            "summoner": "새 계정",
            "matches": [{"id": "9", "createdAt": 900}]
        });
        let merged = merge_cache_value(Some(&existing), "new", &page, 2);
        assert_eq!(merged["matches"].as_array().unwrap().len(), 1);
        assert_eq!(merged["matches"][0]["id"], "9");
    }

    #[test]
    fn cached_page_is_bounded_to_twenty_matches() {
        let matches = (0..45)
            .map(|index| json!({"id": index.to_string(), "createdAt": 1000 - index}))
            .collect::<Vec<_>>();
        let cache = json!({
            "schema": CACHE_SCHEMA,
            "accountKey": "me",
            "summoner": "테스트",
            "savedAt": 7,
            "matches": matches
        });

        let page = cached_page_from_value(&cache, 20).unwrap();
        assert_eq!(page["matches"].as_array().unwrap().len(), 20);
        assert_eq!(page["hasMore"], true);
        assert_eq!(page["source"], "cache");
    }
}
