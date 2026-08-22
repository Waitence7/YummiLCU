# region [Imports]
"""`uv run yummi-lcu-relay` 진입점."""

from __future__ import annotations

import logging

import uvicorn

from relay import config
from relay.app import MAX_AGENT_MESSAGE_BYTES
from relay.error_monitor import DiscordErrorHandler

# endregion


def main() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s [%(name)s] %(message)s",
    )
    error_handler = DiscordErrorHandler()
    error_handler.setLevel(logging.ERROR)
    error_handler.setFormatter(logging.Formatter("%(levelname)s [%(name)s]: %(message)s"))
    logging.getLogger().addHandler(error_handler)
    uvicorn.run(
        "relay.app:app",
        host=config.relay_host(),
        port=config.relay_port(),
        reload=False,
        ws_max_size=MAX_AGENT_MESSAGE_BYTES,
        # Discord returns its one-time authorization code in the callback query string.
        # Keep application diagnostics, but do not persist sensitive queries in Uvicorn access logs.
        access_log=False,
    )


if __name__ == "__main__":
    main()
