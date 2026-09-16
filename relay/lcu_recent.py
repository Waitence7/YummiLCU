"""최근 실제 LCU 데이터 수신 이력.

내전 모집 단계에서 순간적인 League Client 종료/재시작만으로 사용자를
'LCU 미사용'으로 단정하지 않도록, Relay가 실제 LCU 이벤트를 받은 최근 시각을
짧은 TTL로 기억합니다.
"""

from __future__ import annotations

import time

import redis.asyncio as redis

LCU_RECENT_DATA_TTL_SEC = 30 * 60
_LCU_RECENT_DATA_KEY = "lcu:recent_data:{discord_id}"


def _key(discord_id: int) -> str:
    return _LCU_RECENT_DATA_KEY.format(discord_id=int(discord_id))


async def mark_recent_lcu_data(
    r: redis.Redis,
    discord_id: int,
    *,
    occurred_at_ms: int | None = None,
) -> None:
    uid = int(discord_id)
    if uid <= 0:
        return
    timestamp_ms = int(occurred_at_ms if occurred_at_ms is not None else time.time() * 1000)
    await r.set(_key(uid), str(timestamp_ms), ex=LCU_RECENT_DATA_TTL_SEC)


async def recent_lcu_data_map(
    r: redis.Redis,
    discord_ids: list[int],
) -> dict[int, dict[str, int | bool | None]]:
    ids = sorted({int(value) for value in discord_ids if int(value) > 0})
    if not ids:
        return {}
    raw_values = await r.mget([_key(uid) for uid in ids])
    out: dict[int, dict[str, int | bool | None]] = {}
    for uid, raw in zip(ids, raw_values, strict=True):
        last_at_ms: int | None = None
        if raw is not None:
            try:
                last_at_ms = int(raw)
            except (TypeError, ValueError):
                last_at_ms = None
        out[uid] = {
            "recent_lcu_data": last_at_ms is not None,
            "last_lcu_data_at_ms": last_at_ms,
        }
    return out


async def recent_lcu_data(
    r: redis.Redis,
    discord_id: int,
) -> dict[str, int | bool | None]:
    uid = int(discord_id)
    return (await recent_lcu_data_map(r, [uid])).get(
        uid,
        {"recent_lcu_data": False, "last_lcu_data_at_ms": None},
    )
