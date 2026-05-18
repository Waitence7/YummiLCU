# region [Imports]
"""서버·에이전트 공통 action whitelist."""

from __future__ import annotations

# endregion

ACTION_HELP: dict[str, str] = {
    "ping": "연결 테스트",
    "accept_match": "매치 수락 (레디체크)",
    "decline_match": "매치 거절 (레디체크)",
    "reconnect": "게임 재접속",
    "dodge": "닷지 (LP 감소) + 옵션 시 매칭 중지",
    "queue_start": "매칭 큐 시작",
    "queue_cancel": "매칭 큐 취소",
    "leave_lobby": "로비 나가기",
    "party_ready": "파티 준비 ON",
    "champ_reroll": "챔프 리롤",
    "quit_client": "롤 클라이언트 종료",
    "set_status": "상메 설정 (payload.text, 유니코드·줄바꿈)",
    "reset_status": "기본 상메(𝗬𝘂𝗺𝗺𝗶 𝗖𝗹𝗶𝗲𝗻𝘁)",
    "claim_all_rewards": "보상 일괄 수령 시도",
}

ALLOWED_ACTIONS: frozenset[str] = frozenset(ACTION_HELP.keys())


def is_allowed_action(action: str) -> bool:
    return action in ALLOWED_ACTIONS
