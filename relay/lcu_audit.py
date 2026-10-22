"""Durable per-user LCU/Agent audit state stored in Redis AOF.

These keys intentionally have no TTL. Production Redis is configured with AOF
and a persistent volume, so lifecycle metadata survives relay/container deploys.
Raw diagnostics/log chunks are persisted by relay.app; this module keeps a small
query-friendly state plus an append-only lifecycle history.
"""

from __future__ import annotations

import json
import time
from typing import Any

import redis.asyncio as redis

from relay.audit_archive import append_archive_record

LCU_AGENT_AUDIT_USERS_KEY = "lcu:agent_audit_users:v1"
LCU_AGENT_STATE_KEY = "lcu:agent_state:v1:{discord_id}"
LCU_AGENT_HISTORY_KEY = "lcu:agent_history:v1:{discord_id}"


def _state_key(discord_id: int) -> str:
    return LCU_AGENT_STATE_KEY.format(discord_id=int(discord_id))


def _history_key(discord_id: int) -> str:
    return LCU_AGENT_HISTORY_KEY.format(discord_id=int(discord_id))


def _clean_value(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bool):
        return "1" if value else "0"
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return str(value)
    if isinstance(value, (dict, list, tuple)):
        return json.dumps(value, ensure_ascii=False, separators=(",", ":"))[:4096]
    return str(value)[:4096]


async def remember_agent_state(r: redis.Redis, discord_id: int, **fields: Any) -> None:
    uid = int(discord_id)
    if uid <= 0:
        return
    mapping = {str(key)[:128]: _clean_value(value) for key, value in fields.items() if value is not None}
    if not mapping:
        return
    # Some unit/integration stubs intentionally expose no Redis mutation API.
    # Audit storage must never break the live LCU path if the auxiliary store is unavailable.
    if not all(hasattr(r, name) for name in ("sadd", "hset")):
        return
    await r.sadd(LCU_AGENT_AUDIT_USERS_KEY, str(uid))
    await r.hset(_state_key(uid), mapping=mapping)


async def append_agent_history(
    r: redis.Redis,
    discord_id: int,
    event: str,
    *,
    occurred_at_ms: int | None = None,
    **fields: Any,
) -> None:
    uid = int(discord_id)
    if uid <= 0:
        return
    when = int(occurred_at_ms if occurred_at_ms is not None else time.time() * 1000)
    payload: dict[str, Any] = {
        "occurred_at_ms": when,
        "event": str(event or "unknown")[:128],
    }
    for key, value in fields.items():
        if value is None:
            continue
        if isinstance(value, (bool, int, float)):
            payload[str(key)[:128]] = value
        elif isinstance(value, (dict, list, tuple)):
            payload[str(key)[:128]] = json.loads(_clean_value(value))
        else:
            payload[str(key)[:128]] = str(value)[:4096]
    encoded = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    await append_archive_record(uid, "lifecycle", payload, occurred_at_ms=when)
    if not all(hasattr(r, name) for name in ("sadd", "rpush")):
        return
    await r.sadd(LCU_AGENT_AUDIT_USERS_KEY, str(uid))
    # Append-only by design: no EXPIRE/LTRIM. Redis AOF + persistent volume is the archive.
    await r.rpush(_history_key(uid), encoded)


async def agent_state_map(r: redis.Redis, discord_ids: list[int]) -> dict[int, dict[str, str]]:
    ids = sorted({int(value) for value in discord_ids if int(value) > 0})
    if not ids:
        return {}
    if not hasattr(r, "pipeline"):
        return {uid: {} for uid in ids}
    pipe = r.pipeline()
    for uid in ids:
        pipe.hgetall(_state_key(uid))
    rows = await pipe.execute()
    return {uid: dict(row or {}) for uid, row in zip(ids, rows, strict=True)}
