# region [Imports]
"""Discord OAuth2 (identify scope)."""

from __future__ import annotations

import logging
import urllib.parse
from typing import Any

import aiohttp

from relay import config

logger = logging.getLogger("yummi_lcu.relay.auth")
# endregion

DISCORD_API = "https://discord.com/api"
DISCORD_AUTHORIZE = "https://discord.com/api/oauth2/authorize"


def build_login_url(session_id: str) -> str:
    """Discord authorize URL을 만듭니다. state=session_id."""
    params = {
        "client_id": config.discord_client_id(),
        "redirect_uri": config.discord_oauth_redirect_uri(),
        "response_type": "code",
        "scope": "identify",
        "state": session_id,
    }
    return f"{DISCORD_AUTHORIZE}?{urllib.parse.urlencode(params)}"


async def exchange_code(session: aiohttp.ClientSession, code: str) -> dict[str, Any] | None:
    """authorization code → token. 실패 시 None."""
    data = {
        "client_id": config.discord_client_id(),
        "client_secret": config.discord_client_secret(),
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": config.discord_oauth_redirect_uri(),
    }
    headers = {"Content-Type": "application/x-www-form-urlencoded"}
    try:
        async with session.post(f"{DISCORD_API}/oauth2/token", data=data, headers=headers) as resp:
            if resp.status != 200:
                body = await resp.text()
                logger.error("OAuth token 실패 status=%s body=%s", resp.status, body[:500])
                return None
            return await resp.json()
    except Exception:
        logger.exception("OAuth token 요청 예외")
        return None


async def fetch_discord_user(session: aiohttp.ClientSession, access_token: str) -> dict[str, Any] | None:
    """Bearer access_token으로 @me 조회."""
    headers = {"Authorization": f"Bearer {access_token}"}
    try:
        async with session.get(f"{DISCORD_API}/users/@me", headers=headers) as resp:
            if resp.status != 200:
                return None
            return await resp.json()
    except Exception:
        logger.exception("Discord @me 조회 예외")
        return None


def parse_discord_id(user: dict[str, Any]) -> int | None:
    raw = user.get("id")
    if raw is None:
        return None
    try:
        return int(raw)
    except (TypeError, ValueError):
        return None
