#!/usr/bin/env python3
"""Losslessly shrink legacy Redis Agent log entries in place.

Only run after verifying a durable RDB/AOF backup. No key is deleted and list
ordering, TTL and length remain unchanged. Redis uses compare-and-set LSET so
concurrent LPUSH/LTRIM never replaces the wrong element.
"""
from __future__ import annotations

import argparse
import base64
import os
import time
import zlib

import redis

CAS = """
local current = redis.call('LINDEX', KEYS[1], tonumber(ARGV[1]))
if current ~= ARGV[2] then return 0 end
redis.call('LSET', KEYS[1], tonumber(ARGV[1]), ARGV[3])
return 1
"""
PREFIX = b'z:'


def encode(raw: bytes) -> bytes | None:
    if raw.startswith(PREFIX):
        return None
    compact = PREFIX + base64.b64encode(zlib.compress(raw, level=3))
    return compact if len(compact) < len(raw) else None


def compact(redis_client: redis.Redis, limit_keys: int | None, batch_size: int = 32, max_seconds: float | None = None):
    cas = redis_client.register_script(CAS)
    seen = updated = skipped = bytes_saved = 0
    started = time.monotonic()
    for key in redis_client.scan_iter(match='relay:agent_logs:*', count=100):
        if (limit_keys is not None and seen >= limit_keys) or (max_seconds is not None and time.monotonic() - started >= max_seconds):
            break
        seen += 1
        length = redis_client.llen(key)
        key_updated = key_skipped = key_saved = 0
        for first in range(0, length, batch_size):
            items = redis_client.lrange(key, first, first + batch_size - 1)
            prepared = []
            for offset, raw in enumerate(items):
                encoded = encode(raw)
                if encoded is None:
                    continue
                prepared.append((first + offset, raw, encoded))
            if not prepared:
                continue
            pipe = redis_client.pipeline(transaction=False)
            for idx, raw, encoded in prepared:
                cas(keys=[key], args=[idx, raw, encoded], client=pipe)
            results = pipe.execute()
            for (_idx, raw, encoded), ok in zip(prepared, results, strict=True):
                if int(ok) == 1:
                    key_updated += 1
                    key_saved += len(raw) - len(encoded)
                else:
                    key_skipped += 1
            # Keep production Redis responsive during large one-off migration.
            time.sleep(0.002)
        updated += key_updated
        skipped += key_skipped
        bytes_saved += key_saved
        print(
            f'key={seen} entries={length} compressed={key_updated} '
            f'concurrent_skips={key_skipped} saved_mib={key_saved/1048576:.2f} '
            f'elapsed_s={time.monotonic()-started:.1f}',
            flush=True,
        )
    print(
        f'DONE keys={seen} compressed={updated} concurrent_skips={skipped} '
        f'saved_gib={bytes_saved/1073741824:.3f} elapsed_s={time.monotonic()-started:.1f}',
        flush=True,
    )


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--limit-keys', type=int)
    parser.add_argument('--max-seconds', type=float)
    args = parser.parse_args()
    if not args.apply:
        parser.error('safety guard: use --apply after validating a backup')
    if not os.environ.get('REDIS_URL'):
        parser.error('REDIS_URL not available')
    compact(redis.Redis.from_url(os.environ['REDIS_URL'], decode_responses=False), args.limit_keys, max_seconds=args.max_seconds)
