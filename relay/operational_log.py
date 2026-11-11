"""Low-volume diagnostics for best-effort operational fallbacks."""

import logging
import threading
import time

_COOLDOWN_SEC = 300
_MAX_OPERATIONS = 128
_recent: dict[tuple[str, str], tuple[float, int]] = {}
_lock = threading.Lock()


def _log_operational_failure(
    logger: logging.Logger, operation: str, error: BaseException | None = None,
    *, status: int | None = None, level: int = logging.WARNING,
) -> None:
    """Fixed operation keys bound both output volume and in-memory state."""
    now = time.monotonic()
    key = (logger.name, operation)
    with _lock:
        previous = _recent.get(key)
        if previous is not None and now - previous[0] < _COOLDOWN_SEC:
            _recent[key] = (previous[0], previous[1] + 1)
            return
        if previous is None and len(_recent) >= _MAX_OPERATIONS:
            del _recent[next(iter(_recent))]
        _recent[key] = (now, 0)
    # Do not include exception messages, URLs, credentials or request bodies.
    logger.log(
        level,
        "operational failure operation=%s error_type=%s status=%s suppressed=%s",
        operation, type(error).__name__ if error is not None else "none",
        status, previous[1] if previous else 0,
    )
