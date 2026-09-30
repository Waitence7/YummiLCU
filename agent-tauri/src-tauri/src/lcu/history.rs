use reqwest::Method;
use serde_json::{json, Value};

use super::LcuClient;
use crate::error::{AgentError, AgentResult};

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

    pub(crate) async fn match_history(&self, offset: u32) -> AgentResult<Value> {
        if offset > 180 || offset % 20 != 0 {
            return Err(AgentError::Lcu(
                "최근 200경기까지 조회할 수 있습니다.".into(),
            ));
        }
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
            .ok_or_else(|| AgentError::Lcu("소환사 정보를 확인하지 못했습니다.".into()))?;
        let response = self.request(Method::GET,
            &format!("/lol-match-history/v1/products/lol/{puuid}/matches?begIndex={offset}&endIndex={}", offset + 20), None).await?;
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
        let matches: Vec<Value> = games
            .iter()
            .map(|game| normalize_match(game, puuid, &champions))
            .collect();
        Ok(json!({
            "summoner": summoner.get("gameName").or_else(|| summoner.get("displayName")).and_then(Value::as_str).unwrap_or("내 경기"),
            "matches": matches,
            "hasMore": games.len() >= 20 && offset + 20 < 200,
        }))
    }
}

fn normalize_match(game: &Value, puuid: &str, champions: &Value) -> Value {
    let identities = game.get("participantIdentities").and_then(Value::as_array);
    let me = identities
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.pointer("/player/puuid").and_then(Value::as_str) == Some(puuid))
        })
        .and_then(|row| row.get("participantId"))
        .cloned()
        .unwrap_or(Value::Null);
    let players: Vec<Value> = game.get("participants").and_then(Value::as_array)
        .into_iter().flatten().map(|participant| {
            let id = &participant["participantId"];
            let identity = identities.and_then(|rows| rows.iter().find(|row| &row["participantId"] == id))
                .map(|row| &row["player"]).unwrap_or(&Value::Null);
            let name = identity.get("gameName").or_else(|| identity.get("riotIdGameName"))
                .or_else(|| identity.get("summonerName")).and_then(Value::as_str).unwrap_or("알 수 없는 소환사");
            let champion_id = participant["championId"].as_i64().unwrap_or(0);
            let champion = champions.as_array().and_then(|rows| rows.iter().find(|row| row["id"].as_i64() == Some(champion_id)))
                .and_then(|row| row["name"].as_str()).map(str::to_owned).unwrap_or_else(|| format!("챔피언 {champion_id}"));
            let stats = &participant["stats"];
            let cs = stats["totalMinionsKilled"].as_i64().map(|cs| cs + stats["neutralMinionsKilled"].as_i64().unwrap_or(0));
            let items: Vec<i64> = (0..7).map(|slot| stats[format!("item{slot}")].as_i64().unwrap_or(0)).collect();
            json!({ "id": id, "name": name, "championId": champion_id, "champion": champion,
                "team": participant["teamId"], "win": stats["win"],
                "kills": stats["kills"], "deaths": stats["deaths"], "assists": stats["assists"],
                "cs": cs, "gold": stats["goldEarned"], "damage": stats["totalDamageDealtToChampions"], "items": items })
        }).collect();
    let game_id = game["gameId"]
        .as_u64()
        .map(|id| id.to_string())
        .or_else(|| game["gameId"].as_str().map(str::to_owned))
        .unwrap_or_default();
    json!({ "id": game_id, "createdAt": game["gameCreation"], "duration": game["gameDuration"].as_u64().unwrap_or(0),
        "queueId": game["queueId"].as_i64().unwrap_or(-1), "mapId": game["mapId"].as_i64().unwrap_or(0),
        "version": game["gameVersion"].as_str().unwrap_or(""), "me": me, "players": players })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_identity_by_id_and_preserves_unknown_result() {
        let game = json!({ "gameId": 123, "participants": [
            { "participantId": 1, "championId": 99, "teamId": 200, "stats": {"win": false} },
            { "participantId": 7, "championId": 103, "teamId": 100, "stats": {"totalMinionsKilled": 150, "neutralMinionsKilled": 12} }
        ], "participantIdentities": [{"participantId": 7, "player": {"puuid": "me", "gameName": "테스트"}}] });
        let result = normalize_match(&game, "me", &json!([{"id": 103, "name": "아리"}]));
        assert_eq!(result["id"], "123");
        assert_eq!(result["me"], 7);
        assert_eq!(result["players"][1]["champion"], "아리");
        assert_eq!(result["players"][1]["cs"], 162);
        assert!(result["players"][1]["win"].is_null());
        assert!(normalize_match(&game, "missing", &Value::Null)["me"].is_null());
    }
}
