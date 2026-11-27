# region [Imports]
"""Relay command policy shared by HTTP validation and timeout handling."""

from __future__ import annotations

from dataclasses import dataclass

# endregion


@dataclass(frozen=True, slots=True)
class ActionPolicy:
    help: str
    timeout_sec: float = 30.0
    list_limits: tuple[tuple[str, int], ...] = ()
    allow_new_device: bool = False


ACTION_POLICIES: dict[str, ActionPolicy] = {
    "ping": ActionPolicy("연결 테스트", allow_new_device=True),
    "accept_match": ActionPolicy("매치 수락 (레디체크)", allow_new_device=True),
    "decline_match": ActionPolicy("매치 거절 (레디체크)"),
    "reconnect": ActionPolicy("게임 재접속"),
    "dodge": ActionPolicy("닷지 (LP 감소) + 옵션 시 매칭 중지"),
    "queue_start": ActionPolicy("매칭 큐 시작"),
    "queue_cancel": ActionPolicy("매칭 큐 취소"),
    "leave_lobby": ActionPolicy("로비 나가기", allow_new_device=True),
    "party_ready": ActionPolicy("파티 준비 ON"),
    "champ_reroll": ActionPolicy("챔프 리롤"),
    "champ_select_action": ActionPolicy("밴/픽 확정"),
    "set_summoner_spells": ActionPolicy("소환사 주문 변경"),
    "list_rune_pages": ActionPolicy("룬 페이지 목록"),
    "set_rune_page": ActionPolicy("룬 페이지 선택"),
    "get_current_rune_page": ActionPolicy("현재 룬 페이지"),
    "update_rune_page": ActionPolicy("룬 직접 구성"),
    "quit_client": ActionPolicy("롤 클라이언트 종료"),
    "set_status": ActionPolicy("상메 설정 (payload.text, 유니코드·줄바꿈)"),
    "reset_status": ActionPolicy("기본 상메(𝗬𝘂𝗺𝗺𝗶 𝗖𝗹𝗶𝗲𝗻𝘁)"),
    "claim_all_rewards": ActionPolicy("보상 일괄 수령 시도"),
    "launch_client": ActionPolicy("롤 클라이언트 실행", timeout_sec=300.0),
    "play_ranked_solo": ActionPolicy("롤 실행 + 솔랭 매칭", timeout_sec=300.0),
    "play_normal_draft": ActionPolicy("롤 실행 + 일반(비공개 선택) 매칭", timeout_sec=300.0),
    "create_ranked_lobby": ActionPolicy("솔랭 로비만 생성", allow_new_device=True),
    "create_normal_lobby": ActionPolicy("일반(비공개 선택) 로비만 생성", allow_new_device=True),
    "invite_party_members": ActionPolicy(
        "모집 확정 멤버 파티 초대 (payload.riot_ids)",
        timeout_sec=80.0,
        list_limits=(("riot_ids", 20),),
        allow_new_device=True,
    ),
    "check_party_members": ActionPolicy(
        "LCU 로비 참가 여부 확인 (payload.check_riot_ids)",
        list_limits=(("check_riot_ids", 20),),
        allow_new_device=True,
    ),
    "resolve_riot_identity": ActionPolicy(
        "LCU Riot ID 읽기 전용 계정 조회 (payload.riot_ids)",
        timeout_sec=45.0,
        list_limits=(("riot_ids", 5),),
    ),
    "add_match_friend": ActionPolicy("내전 참가자 친구 요청 (명시적 확인)", timeout_sec=12.0, list_limits=(("riot_ids", 1),)),
    "match_friend_presence": ActionPolicy("활성 내전 친구 온라인 확인 (read only)", timeout_sec=8.0, list_limits=(("riot_ids", 50),)),
    "kick_party_member": ActionPolicy(
        "파티원 강퇴 (payload.summoner_id)",
        allow_new_device=True,
    ),
}

ACTION_HELP: dict[str, str] = {name: policy.help for name, policy in ACTION_POLICIES.items()}
ALLOWED_ACTIONS: frozenset[str] = frozenset(ACTION_POLICIES)


def action_policy(action: str) -> ActionPolicy | None:
    return ACTION_POLICIES.get(action)


def is_allowed_action(action: str) -> bool:
    return action in ACTION_POLICIES


def validate_riot_identity_payload(payload: dict[str, object]) -> bool:
    ids = payload.get("riot_ids")
    return isinstance(ids, list) and 1 <= len(ids) <= 5 and all(
        isinstance(value, str)
        and 0 < len(value.encode("utf-8", errors="surrogatepass")) <= 256
        and value.count("#") == 1
        and all(part.strip() for part in value.split("#"))
        and all(ord(ch) >= 32 and not 127 <= ord(ch) <= 159 and not 0xD800 <= ord(ch) <= 0xDFFF for ch in value)
        for value in ids
    )


def validate_match_friend_presence_payload(payload: dict[str, object]) -> bool:
    ids = payload.get("riot_ids")
    return isinstance(ids, list) and 1 <= len(ids) <= 50 and all(
        isinstance(value, str)
        and 0 < len(value.encode("utf-8", errors="surrogatepass")) <= 128
        and value.count("#") == 1
        and all(part.strip() for part in value.split("#"))
        and all(ord(ch) >= 32 and not 127 <= ord(ch) <= 159 and not 0xD800 <= ord(ch) <= 0xDFFF for ch in value)
        for value in ids
    )
