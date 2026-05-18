# region [Imports]
"""`uv run yummi-lcu-relay` 진입점."""

from __future__ import annotations

import logging

import uvicorn

from relay import config

# endregion


def main() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s [%(name)s] %(message)s",
    )
    uvicorn.run(
        "relay.app:app",
        host=config.relay_host(),
        port=config.relay_port(),
        reload=False,
    )


if __name__ == "__main__":
    main()
