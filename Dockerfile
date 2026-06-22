# syntax=docker/dockerfile:1.7

FROM python:3.13-slim

WORKDIR /app

ENV PYTHONUNBUFFERED=1
ENV UV_COMPILE_BYTECODE=1
ENV UV_LINK_MODE=copy
ENV PATH="/app/.venv/bin:$PATH"

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

RUN pip install --no-cache-dir uv

COPY pyproject.toml uv.lock README.md ./
COPY relay relay
COPY deploy deploy

RUN uv sync --frozen --no-dev

EXPOSE 8790
CMD ["yummi-lcu-relay"]
