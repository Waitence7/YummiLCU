"""Relay 오류를 공용 Discord webhook으로 전달합니다."""

from __future__ import annotations

import asyncio
import logging
import os
import time

import aiohttp

from relay.logging_safety import redact_log_text

_WEBHOOK_URL = os.getenv("DISCORD_ERROR_WEBHOOK_URL", "").strip()
_COOLDOWN_SEC = float(os.getenv("DISCORD_ERROR_MONITOR_COOLDOWN_SEC", "300"))
_recent: dict[str, float] = {}


def _truncate(value: str, limit: int = 1900) -> str:
    return value if len(value) <= limit else f"{value[: limit - 3]}..."


def _should_notify(fingerprint: str) -> bool:
    now = time.monotonic()
    last = _recent.get(fingerprint, 0.0)
    if now - last < _COOLDOWN_SEC:
        return False
    _recent[fingerprint] = now
    return True


async def notify_error(context: str, error: BaseException | str) -> None:
    if not _WEBHOOK_URL:
        return
    if isinstance(error, BaseException):
        message = str(error) or type(error).__name__
    else:
        message = str(error)
    message = redact_log_text(message)
    context = redact_log_text(context)
    if not _should_notify(f"{context}|{message[:200]}"):
        return

    payload = {
        "username": "Yummi LCU Relay Error Monitor",
        "content": _truncate(f"🚨 {context}\nerror: {message}"),
        "allowed_mentions": {"parse": []},
    }
    try:
        timeout = aiohttp.ClientTimeout(total=8)
        async with aiohttp.ClientSession(timeout=timeout) as session:
            async with session.post(_WEBHOOK_URL, json=payload) as response:
                if response.status >= 400:
                    logging.getLogger("yummi_lcu.error_monitor").warning(
                        "Discord error webhook failed status=%s", response.status
                    )
    except Exception:
        logging.getLogger("yummi_lcu.error_monitor").warning(
            "Discord error webhook request failed", exc_info=True
        )


class DiscordErrorHandler(logging.Handler):
    """Root ERROR 로그를 현재 asyncio loop에서 비동기 전송합니다."""

    def emit(self, record: logging.LogRecord) -> None:
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            return
        loop.create_task(notify_error(record.name, self.format(record)))
