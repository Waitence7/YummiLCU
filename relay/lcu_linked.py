"""LCU 에이전트 1회 이상 연결한 Discord 계정 (Redis SET)."""

from __future__ import annotations

import redis.asyncio as redis

LCU_EVER_LINKED_KEY = "lcu:ever_linked"


async def mark_lcu_linked(r: redis.Redis, discord_id: int) -> None:
    uid = int(discord_id)
    if uid <= 0:
        return
    await r.sadd(LCU_EVER_LINKED_KEY, str(uid))


async def is_lcu_linked(r: redis.Redis, discord_id: int) -> bool:
    uid = int(discord_id)
    if uid <= 0:
        return False
    return bool(await r.sismember(LCU_EVER_LINKED_KEY, str(uid)))


async def lcu_linked_map(r: redis.Redis, discord_ids: list[int]) -> dict[int, bool]:
    ids = sorted({int(x) for x in discord_ids if int(x) > 0})
    if not ids:
        return {}
    pipe = r.pipeline()
    for did in ids:
        pipe.sismember(LCU_EVER_LINKED_KEY, str(did))
    results = await pipe.execute()
    return {did: bool(flag) for did, flag in zip(ids, results, strict=True)}
