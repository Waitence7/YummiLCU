"""Append-only on-disk archive for redacted LCU/Agent operational records."""

from __future__ import annotations

import asyncio
import json
import logging
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

logger = logging.getLogger("yummi_lcu.relay.audit_archive")


def _archive_dir() -> Path | None:
    raw = os.getenv("LCU_AUDIT_DIR", "").strip()
    return Path(raw) if raw else None


def _write_record(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(payload, ensure_ascii=False, separators=(",", ":")))
        handle.write("\n")


async def append_archive_record(
    discord_id: int,
    kind: str,
    payload: dict[str, Any],
    *,
    occurred_at_ms: int | None = None,
) -> None:
    """Persist a redacted record to a daily NDJSON file.

    Best-effort by design: archive I/O must not interrupt live relay traffic.
    """
    directory = _archive_dir()
    if directory is None:
        return
    when_ms = int(occurred_at_ms or payload.get("occurred_at_ms") or 0)
    if when_ms <= 0:
        when_ms = int(datetime.now(tz=timezone.utc).timestamp() * 1000)
    dt = datetime.fromtimestamp(when_ms / 1000, tz=timezone.utc)
    record = {
        "occurred_at_ms": when_ms,
        "discord_id": int(discord_id),
        "kind": str(kind)[:128],
        "payload": payload,
    }
    path = directory / f"{dt:%Y-%m-%d}.ndjson"
    try:
        await asyncio.to_thread(_write_record, path, record)
    except Exception:
        logger.exception("LCU audit file append failed path=%s kind=%s", path, kind)
