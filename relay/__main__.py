# region [Imports]
"""`uv run yummi-lcu-relay` 진입점."""

from __future__ import annotations

import logging

import uvicorn

from relay import config
from relay.app import MAX_AGENT_MESSAGE_BYTES
from relay.error_monitor import DiscordErrorHandler
from relay.logging_safety import RedactingFormatter

# endregion


def main() -> None:
    stream_handler = logging.StreamHandler()
    stream_handler.setFormatter(
        RedactingFormatter("%(asctime)s %(levelname)s [%(name)s] %(message)s")
    )
    logging.basicConfig(level=logging.INFO, handlers=[stream_handler])
    error_handler = DiscordErrorHandler()
    error_handler.setLevel(logging.ERROR)
    error_handler.setFormatter(RedactingFormatter("%(levelname)s [%(name)s]: %(message)s"))
    logging.getLogger().addHandler(error_handler)
    uvicorn.run(
        "relay.app:app",
        host=config.relay_host(),
        port=config.relay_port(),
        reload=False,
        ws_max_size=MAX_AGENT_MESSAGE_BYTES,
        # The Agent already owns connection liveness with an application-level
        # heartbeat (25s ping / 10s timeout) and reconnects itself. Running
        # Uvicorn's protocol-level ping watchdog in parallel can terminate an
        # otherwise recoverable Agent connection first, surfacing as code 1005.
        ws_ping_interval=None,
        # Discord returns its one-time authorization code in the callback query string.
        # Keep application diagnostics, but do not persist sensitive queries in Uvicorn access logs.
        access_log=False,
    )


if __name__ == "__main__":
    main()
