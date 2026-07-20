use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

fn empty_object() -> Value {
    json!({})
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub(crate) enum IncomingMessage {
    #[serde(rename = "command")]
    Command {
        action: String,
        request_id: String,
        #[serde(default = "empty_object")]
        payload: Value,
    },
    #[serde(rename = "session_bound")]
    SessionBound {
        discord_id: Option<u64>,
        discord_name: Option<String>,
        username: Option<String>,
        discord_avatar: Option<String>,
        avatar_url: Option<String>,
    },
    #[serde(rename = "pong")]
    Pong,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Ping,
    AcceptMatch,
    DeclineMatch,
    Reconnect,
    Dodge,
    QueueStart,
    QueueCancel,
    LeaveLobby,
    PartyReady,
    ChampReroll,
    ChampSelect,
    SetSummonerSpells,
    ListRunePages,
    SetRunePage,
    GetCurrentRunePage,
    UpdateRunePage,
    QuitClient,
    SetStatus,
    ResetStatus,
    ClaimAllRewards,
    LaunchClient,
    CreateRankedLobby,
    CreateNormalLobby,
    PlayRankedSolo,
    PlayNormalDraft,
    InvitePartyMembers,
    CheckPartyMembers,
}

impl Action {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "ping" => Self::Ping,
            "accept_match" => Self::AcceptMatch,
            "decline_match" => Self::DeclineMatch,
            "reconnect" => Self::Reconnect,
            "dodge" => Self::Dodge,
            "queue_start" => Self::QueueStart,
            "queue_cancel" => Self::QueueCancel,
            "leave_lobby" => Self::LeaveLobby,
            "party_ready" => Self::PartyReady,
            "champ_reroll" => Self::ChampReroll,
            "champ_select_action" => Self::ChampSelect,
            "set_summoner_spells" => Self::SetSummonerSpells,
            "list_rune_pages" => Self::ListRunePages,
            "set_rune_page" => Self::SetRunePage,
            "get_current_rune_page" => Self::GetCurrentRunePage,
            "update_rune_page" => Self::UpdateRunePage,
            "quit_client" => Self::QuitClient,
            "set_status" => Self::SetStatus,
            "reset_status" => Self::ResetStatus,
            "claim_all_rewards" => Self::ClaimAllRewards,
            "launch_client" => Self::LaunchClient,
            "create_ranked_lobby" => Self::CreateRankedLobby,
            "create_normal_lobby" => Self::CreateNormalLobby,
            "play_ranked_solo" => Self::PlayRankedSolo,
            "play_normal_draft" => Self::PlayNormalDraft,
            "invite_party_members" => Self::InvitePartyMembers,
            "check_party_members" => Self::CheckPartyMembers,
            _ => return None,
        })
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Ping => "ping",
            Self::AcceptMatch => "accept_match",
            Self::DeclineMatch => "decline_match",
            Self::Reconnect => "reconnect",
            Self::Dodge => "dodge",
            Self::QueueStart => "queue_start",
            Self::QueueCancel => "queue_cancel",
            Self::LeaveLobby => "leave_lobby",
            Self::PartyReady => "party_ready",
            Self::ChampReroll => "champ_reroll",
            Self::ChampSelect => "champ_select_action",
            Self::SetSummonerSpells => "set_summoner_spells",
            Self::ListRunePages => "list_rune_pages",
            Self::SetRunePage => "set_rune_page",
            Self::GetCurrentRunePage => "get_current_rune_page",
            Self::UpdateRunePage => "update_rune_page",
            Self::QuitClient => "quit_client",
            Self::SetStatus => "set_status",
            Self::ResetStatus => "reset_status",
            Self::ClaimAllRewards => "claim_all_rewards",
            Self::LaunchClient => "launch_client",
            Self::CreateRankedLobby => "create_ranked_lobby",
            Self::CreateNormalLobby => "create_normal_lobby",
            Self::PlayRankedSolo => "play_ranked_solo",
            Self::PlayNormalDraft => "play_normal_draft",
            Self::InvitePartyMembers => "invite_party_members",
            Self::CheckPartyMembers => "check_party_members",
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct CommandResult {
    #[serde(rename = "type")]
    message_type: &'static str,
    request_id: String,
    ok: bool,
    message: String,
    data: Value,
}

impl CommandResult {
    pub(crate) fn from_parts(
        request_id: String,
        ok: bool,
        message: impl Into<String>,
        data: Value,
    ) -> Self {
        Self {
            message_type: "command_result",
            request_id,
            ok,
            message: message.into(),
            data,
        }
    }

    pub(crate) fn success(request_id: String, message: impl Into<String>, data: Value) -> Self {
        Self::from_parts(request_id, true, message, data)
    }

    pub(crate) fn failure(request_id: String, message: impl Into<String>) -> Self {
        Self::from_parts(request_id, false, message, empty_object())
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct AuthMessage<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    ws_token: &'a str,
}

impl<'a> AuthMessage<'a> {
    pub(crate) fn new(ws_token: &'a str) -> Self {
        Self {
            message_type: "auth",
            ws_token,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AgentCapabilities {
    command_result_v2: bool,
    heartbeat: bool,
    relay_reconnect: bool,
    runes: bool,
    rewards: bool,
    party_invite: bool,
    party_lookup: bool,
    summoner_spells: bool,
    dodge: bool,
    launch_client: bool,
    update_progress: bool,
    gameflow_events: bool,
    ready_check_events: bool,
    champ_select_events: bool,
    party_events: bool,
    eog_events: bool,
}

impl AgentCapabilities {
    const fn current() -> Self {
        Self {
            command_result_v2: true,
            heartbeat: true,
            relay_reconnect: true,
            runes: true,
            rewards: true,
            party_invite: true,
            party_lookup: true,
            summoner_spells: true,
            dodge: true,
            launch_client: true,
            update_progress: true,
            gameflow_events: true,
            ready_check_events: true,
            champ_select_events: true,
            party_events: true,
            eog_events: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AgentHelloMessage {
    #[serde(rename = "type")]
    message_type: &'static str,
    version: &'static str,
    os: &'static str,
    lcu_ready: bool,
    protocol_version: u32,
    capabilities: AgentCapabilities,
}

impl AgentHelloMessage {
    pub(crate) const fn new(lcu_ready: bool) -> Self {
        Self {
            message_type: "agent_hello",
            version: env!("CARGO_PKG_VERSION"),
            os: "windows",
            lcu_ready,
            protocol_version: 1,
            capabilities: AgentCapabilities::current(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct AgentEventMessage {
    #[serde(rename = "type")]
    message_type: &'static str,
    data: Value,
}

impl AgentEventMessage {
    pub(crate) fn new(message_type: &'static str, data: Value) -> Self {
        Self { message_type, data }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct OAuthCodeMessage<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    code: &'a str,
}

impl<'a> OAuthCodeMessage<'a> {
    pub(crate) fn new(code: &'a str) -> Self {
        Self {
            message_type: "complete_oauth_link",
            code,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct PongMessage {
    #[serde(rename = "type")]
    message_type: &'static str,
}

impl PongMessage {
    pub(crate) const fn new() -> Self {
        Self {
            message_type: "pong",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_command_deserializes() {
        let message: IncomingMessage = serde_json::from_value(json!({
            "type": "command",
            "action": "play_normal_draft",
            "request_id": "request-42",
            "payload": {"source": "discord"}
        }))
        .unwrap();

        assert_eq!(
            message,
            IncomingMessage::Command {
                action: "play_normal_draft".into(),
                request_id: "request-42".into(),
                payload: json!({"source": "discord"}),
            }
        );
    }

    #[test]
    fn command_result_serializes_with_flat_relay_shape() {
        let value = serde_json::to_value(CommandResult::success(
            "request-42".into(),
            "일반(비공개) 매칭 시작",
            json!({}),
        ))
        .unwrap();

        assert_eq!(value["type"], "command_result");
        assert_eq!(value["request_id"], "request-42");
        assert_eq!(value["ok"], true);
        assert_eq!(value["message"], "일반(비공개) 매칭 시작");
        assert_eq!(value["data"], json!({}));
        assert!(value.get("result").is_none());
    }

    #[test]
    fn command_result_keeps_rune_and_party_data_at_top_level() {
        let value = serde_json::to_value(CommandResult::from_parts(
            "request-data".into(),
            true,
            "현재 룬 페이지",
            json!({"id": 42, "members": [{"riot_id": "Player#KR1"}]}),
        ))
        .unwrap();

        assert_eq!(value["data"]["id"], 42);
        assert_eq!(value["data"]["members"][0]["riot_id"], "Player#KR1");
        assert!(value.get("result").is_none());
    }

    #[test]
    fn agent_hello_serializes_current_capabilities() {
        let value = serde_json::to_value(AgentHelloMessage::new(true)).unwrap();

        assert_eq!(value["type"], "agent_hello");
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["os"], "windows");
        assert_eq!(value["lcu_ready"], true);
        assert_eq!(value["protocol_version"], 1);
        assert_eq!(value["capabilities"]["command_result_v2"], true);
        assert_eq!(value["capabilities"]["runes"], true);
        assert_eq!(value["capabilities"]["gameflow_events"], true);
        assert_eq!(value["capabilities"]["party_events"], true);
    }

    #[test]
    fn unknown_action_is_rejected_without_changing_known_names() {
        let relay_actions = [
            "ping",
            "accept_match",
            "decline_match",
            "reconnect",
            "dodge",
            "queue_start",
            "queue_cancel",
            "leave_lobby",
            "party_ready",
            "champ_reroll",
            "champ_select_action",
            "set_summoner_spells",
            "list_rune_pages",
            "set_rune_page",
            "get_current_rune_page",
            "update_rune_page",
            "quit_client",
            "set_status",
            "reset_status",
            "claim_all_rewards",
            "launch_client",
            "play_ranked_solo",
            "play_normal_draft",
            "create_ranked_lobby",
            "create_normal_lobby",
            "invite_party_members",
            "check_party_members",
        ];
        for action_name in relay_actions {
            let action = Action::parse(action_name).expect("Relay action must remain supported");
            assert_eq!(action.as_str(), action_name);
        }
        assert_eq!(Action::parse("arbitrary_http"), None);
    }
}
