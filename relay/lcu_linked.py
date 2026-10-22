"""LCU 에이전트 1회 이상 연결 여부와 마지막 접속 시각 (Redis)."""

from __future__ import annotations

import time

import redis.asyncio as redis

LCU_EVER_LINKED_KEY = "lcu:ever_linked"
LCU_LAST_CONNECTED_AT_KEY = "lcu:last_connected_at_ms"
LCU_LAST_AGENT_VERSION_KEY = "lcu:last_agent_version"


async def mark_lcu_linked(r: redis.Redis, discord_id: int) -> None:
    """LCU Agent 연결 이력과 마지막 접속 시각을 함께 기록한다."""
    uid = int(discord_id)
    if uid <= 0:
        return

    now_ms = int(time.time() * 1000)
    pipe = r.pipeline()
    pipe.sadd(LCU_EVER_LINKED_KEY, str(uid))
    pipe.zadd(LCU_LAST_CONNECTED_AT_KEY, {str(uid): now_ms})
    await pipe.execute()


async def mark_lcu_agent_version(r: redis.Redis, discord_id: int, version: str) -> None:
    """마지막으로 확인된 LCU Agent 버전을 사용자별로 영구 저장한다."""
    uid = int(discord_id)
    clean = str(version or "").strip()[:64]
    if uid <= 0 or not clean:
        return
    await r.hset(LCU_LAST_AGENT_VERSION_KEY, str(uid), clean)


async def lcu_agent_version_map(
    r: redis.Redis,
    discord_ids: list[int],
) -> dict[int, str | None]:
    """여러 Discord 계정의 마지막 Agent 버전을 한 번에 조회한다."""
    ids = sorted({int(x) for x in discord_ids if int(x) > 0})
    if not ids:
        return {}
    raw = await r.hmget(LCU_LAST_AGENT_VERSION_KEY, [str(did) for did in ids])
    out: dict[int, str | None] = {}
    for did, value in zip(ids, raw, strict=True):
        if isinstance(value, bytes):
            value = value.decode("utf-8", errors="ignore")
        text = str(value).strip() if value is not None else ""
        out[did] = text or None
    return out


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


async def last_lcu_connected_at_ms(r: redis.Redis, discord_id: int) -> int | None:
    """마지막으로 LCU Agent 연결이 확인된 UNIX epoch ms를 반환한다."""
    uid = int(discord_id)
    if uid <= 0:
        return None
    score = await r.zscore(LCU_LAST_CONNECTED_AT_KEY, str(uid))
    return int(score) if score is not None else None


async def lcu_last_connected_map(
    r: redis.Redis,
    discord_ids: list[int],
) -> dict[int, int | None]:
    """여러 Discord 계정의 마지막 LCU Agent 접속 시각을 한 번에 조회한다."""
    ids = sorted({int(x) for x in discord_ids if int(x) > 0})
    if not ids:
        return {}

    pipe = r.pipeline()
    for did in ids:
        pipe.zscore(LCU_LAST_CONNECTED_AT_KEY, str(did))
    results = await pipe.execute()
    return {
        did: int(score) if score is not None else None
        for did, score in zip(ids, results, strict=True)
    }
