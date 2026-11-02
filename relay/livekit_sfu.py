from __future__ import annotations

import base64
import hashlib
import hmac
import json
import secrets
import time
from dataclasses import dataclass
from typing import Any

from relay import config

TOKEN_TTL_SEC = 10 * 60
ROOM_PREFIX = "yummi-live-"


def _b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def _jwt(payload: dict[str, Any], secret: str) -> str:
    header = {"alg": "HS256", "typ": "JWT"}
    encoded_header = _b64url(json.dumps(header, separators=(",", ":"), sort_keys=True).encode())
    encoded_payload = _b64url(json.dumps(payload, separators=(",", ":"), sort_keys=True).encode())
    signing_input = f"{encoded_header}.{encoded_payload}".encode("ascii")
    signature = hmac.new(secret.encode("utf-8"), signing_input, hashlib.sha256).digest()
    return f"{encoded_header}.{encoded_payload}.{_b64url(signature)}"


def room_name(discord_id: int) -> str:
    return f"{ROOM_PREFIX}{int(discord_id)}"


def configured() -> bool:
    return bool(config.livekit_public_url() and config.livekit_api_key() and config.livekit_api_secret())


@dataclass(frozen=True)
class LiveKitSession:
    url: str
    room: str
    identity: str
    token: str
    expires_at: int


@dataclass(frozen=True)
class LiveStreamProfile:
    name: str
    width: int
    height: int
    fps: int
    bitrate: int


_STREAM_PROFILES = {
    "data": LiveStreamProfile("data", 960, 540, 15, 1_500_000),
    "balanced": LiveStreamProfile("balanced", 1280, 720, 30, 4_000_000),
    "high": LiveStreamProfile("high", 1920, 1080, 30, 7_000_000),
    "ultra": LiveStreamProfile("ultra", 2560, 1440, 30, 10_000_000),
}


def stream_profile(value: str | None) -> LiveStreamProfile:
    key = (value or "balanced").strip().lower()
    profile = _STREAM_PROFILES.get(key)
    if profile is None:
        raise ValueError("unsupported live stream profile")
    return profile


def _token(*, identity: str, room: str, can_publish: bool, can_subscribe: bool, can_publish_data: bool, name: str) -> tuple[str, int]:
    api_key = config.livekit_api_key()
    api_secret = config.livekit_api_secret()
    if not api_key or not api_secret:
        raise RuntimeError("LiveKit is not configured")
    now = int(time.time())
    exp = now + TOKEN_TTL_SEC
    payload: dict[str, Any] = {
        "exp": exp,
        "iss": api_key,
        "nbf": now - 5,
        "sub": identity,
        "name": name,
        "video": {
            "roomJoin": True,
            "room": room,
            "canPublish": can_publish,
            "canSubscribe": can_subscribe,
            "canPublishData": can_publish_data,
            **({"canPublishSources": ["screen_share"]} if can_publish else {}),
            "canUpdateOwnMetadata": False,
        },
    }
    return _jwt(payload, api_secret), exp


def publisher_session(discord_id: int) -> LiveKitSession:
    room = room_name(discord_id)
    identity = f"agent-{int(discord_id)}"
    token, exp = _token(
        identity=identity,
        room=room,
        can_publish=True,
        can_subscribe=False,
        can_publish_data=False,
        name="Yummi Agent",
    )
    return LiveKitSession(config.livekit_public_url(), room, identity, token, exp)


def viewer_session(
    discord_id: int,
    *,
    can_publish_data: bool = True,
    name: str = "Yummi Web Viewer",
) -> LiveKitSession:
    room = room_name(discord_id)
    identity = f"viewer-{int(discord_id)}-{secrets.token_hex(8)}"
    token, exp = _token(
        identity=identity,
        room=room,
        can_publish=False,
        can_subscribe=True,
        can_publish_data=can_publish_data,
        name=name,
    )
    return LiveKitSession(config.livekit_public_url(), room, identity, token, exp)


def decode_unverified(token: str) -> dict[str, Any]:
    parts = token.split(".")
    if len(parts) != 3:
        raise ValueError("invalid JWT")
    raw = parts[1] + "=" * (-len(parts[1]) % 4)
    value = json.loads(base64.urlsafe_b64decode(raw.encode("ascii")))
    if not isinstance(value, dict):
        raise ValueError("invalid JWT claims")
    return value
