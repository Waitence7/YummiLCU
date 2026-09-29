"""Bounded, redacted diagnostics for failed upstream HTTP requests."""

from __future__ import annotations

import json
from typing import Any

from relay.logging_safety import redact_log_text

MAX_UPSTREAM_ERROR_BYTES = 2_048
MAX_UPSTREAM_ERROR_CHARS = 512


def summarize_upstream_error_bytes(raw: bytes) -> str:
    truncated = len(raw) > MAX_UPSTREAM_ERROR_BYTES
    text = raw[:MAX_UPSTREAM_ERROR_BYTES].decode("utf-8", errors="replace").strip()
    if not text:
        return "empty_response"
    try:
        body: Any = json.loads(text)
    except (ValueError, TypeError):
        detail = text
    else:
        if isinstance(body, dict):
            fields = [
                f"{key}={body[key]}"
                for key in ("error", "code", "message", "detail")
                if isinstance(body.get(key), (str, int, float, bool))
            ]
            detail = " ".join(fields) if fields else "json_without_error_detail"
        elif isinstance(body, str):
            detail = body
        else:
            detail = "json_without_error_detail"
    detail = redact_log_text(" ".join(detail.split()))[:MAX_UPSTREAM_ERROR_CHARS]
    return f"{detail} [truncated]" if truncated else detail


async def upstream_error_detail(response: Any) -> str:
    try:
        raw = await response.content.read(MAX_UPSTREAM_ERROR_BYTES + 1)
    except Exception as error:
        return f"body_read_failed={type(error).__name__}"
    return summarize_upstream_error_bytes(raw)
