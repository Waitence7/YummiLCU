"""Small final-line defense against accidentally logging credentials."""

from __future__ import annotations

import logging
import re

_SENSITIVE_VALUE = re.compile(
    r"(?i)(\b(?:authorization|password|secret|token|ws_token|session_token|oauth_code)\b\s*[=:]\s*)"
    r"(?:bearer\s+)?(?:\"[^\"]*\"|'[^']*'|[^\s,;}&]+)"
)
_BEARER = re.compile(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{8,}")
_URL_SECRET = re.compile(r"(?i)([?&](?:code|token|secret|access_token|refresh_token)=)[^&#\s]+")


def redact_log_text(value: str) -> str:
    value = _SENSITIVE_VALUE.sub(r"\1[REDACTED]", value)
    value = _BEARER.sub("Bearer [REDACTED]", value)
    return _URL_SECRET.sub(r"\1[REDACTED]", value)


class RedactingFormatter(logging.Formatter):
    def format(self, record: logging.LogRecord) -> str:
        return redact_log_text(super().format(record))
