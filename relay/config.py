# region [Imports]
"""Relay 환경 변수 로딩."""

from __future__ import annotations

import os
from pathlib import Path

from dotenv import load_dotenv

# endregion

_ROOT = Path(__file__).resolve().parents[1]
load_dotenv(_ROOT / ".env", override=False)


def _env(key: str, default: str = "") -> str:
    return (os.getenv(key) or default).strip()


def discord_client_id() -> str:
    return _env("DISCORD_CLIENT_ID")


def discord_client_secret() -> str:
    return _env("DISCORD_CLIENT_SECRET")


def discord_oauth_redirect_uri() -> str:
    return _env("DISCORD_OAUTH_REDIRECT_URI")


def relay_host() -> str:
    return _env("RELAY_HOST", "127.0.0.1")


def relay_port() -> int:
    return int(_env("RELAY_PORT", "8790"))


def relay_public_base_url() -> str:
    base = _env("RELAY_PUBLIC_BASE_URL")
    if not base:
        return f"http://{relay_host()}:{relay_port()}"
    return base.rstrip("/")


def relay_public_base_url_must_be_https() -> None:
    """공개 Relay URL 이 localhost 가 아니면 HTTPS 만 허용."""
    base = relay_public_base_url().lower()
    if "localhost" in base or "127.0.0.1" in base:
        return
    if not base.startswith("https://"):
        raise RuntimeError(
            "RELAY_PUBLIC_BASE_URL must use https:// for non-localhost deployments"
        )


def relay_internal_secret() -> str:
    return _env("RELAY_INTERNAL_SECRET")


def relay_session_ttl_sec() -> int:
    return int(_env("RELAY_SESSION_TTL_SEC", "600"))


def redis_url() -> str:
    return _env("REDIS_URL", "redis://127.0.0.1:6379/0")


def oauth_configured() -> bool:
    return bool(discord_client_id() and discord_client_secret() and discord_oauth_redirect_uri())
