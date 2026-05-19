# 보안 · 통신

## Relay (서버)

| 항목 | 조치 |
|------|------|
| `/internal/*` | nginx 에서 **127.0.0.1 만** 허용 (`deploy/nginx-waitence.conf`) |
| Internal secret | `RELAY_INTERNAL_SECRET` 32바이트+ 랜덤, `.env` 만 (git 제외) |
| OAuth | `state` = 일회용 토큰 → Redis → `session_id` (피싱/세션 고정 완화) |
| `/auth/status` | `discord_id` 미반환 (상태만) |
| 공개 URL | `RELAY_PUBLIC_BASE_URL` 비로컬은 **HTTPS 필수** (relay 기동 시 검증) |

## 에이전트 (PC)

| 항목 | 조치 |
|------|------|
| Relay / 업데이트 | `agent.json` 로드 시 공개 URL HTTP → HTTPS 승격 |
| 자동 업데이트 | zip URL **https 만**; manifest `sha256` 있으면 검증 |
| LCU | 인증서 검증은 **127.0.0.1 / localhost 만** 우회 |
| lockfile | 로그에 lockfile 원문(비밀번호) 미포함 |

## 운영 체크리스트

1. VM 에 nginx 설정 반영 후 `nginx -t && systemctl reload nginx`
2. `.env` 에 `RELAY_INTERNAL_SECRET` 설정 (`.env.example` 참고)
3. 배포 시 `deploy/sync-agent-version.ps1` 로 `sha256` 포함 manifest 생성
4. `agent.json` / `.env` 커밋 금지

## 잔여 리스크

- **LCU lockfile** = 해당 PC 계정 전체 제어 (로컬 신뢰 전제)
- **코드 서명** 없는 zip 업데이트 — `sha256` 으로 무결성만 검증 (서버·manifest 보호 필요)
- **WebSocket** — `session_id` UUID + OAuth 바인딩에 의존

자세한 흐름: [`AGENT_MECHANISM.md`](AGENT_MECHANISM.md)
