# 보안 · 통신

## Relay (서버)

| 항목 | 조치 |
|------|------|
| `/internal/*` | nginx 에서 **127.0.0.1 만** 허용 (`deploy/nginx-waitence.conf`) |
| Internal secret | `RELAY_INTERNAL_SECRET` 32바이트+ 랜덤, `.env` 만 (git 제외) |
| Internal rate limit | Redis 카운터 (`relay:internal_auth_fail:{ip}`) |
| `/lcu/internal/` | dashboard nginx 예시에도 **127.0.0.1 만** (`nginx-yummi-dashboard.conf.example`) |
| HTTP 배포 중 | `nginx-waitence-http.conf` 에서도 `/internal/` 차단 |
| OAuth | `state` = 일회용 토큰 → Redis → `session_id` (피싱/세션 고정 완화) |
| OAuth 링크 코드 | Discord 로그인 후 **6자리 코드**를 에이전트에 입력해야 바인딩 (세션 고정 방지) |
| `/login` | 에이전트 WS 가 먼저 연결된 `session_id` 만 OAuth 시작 허용 |
| `/auth/status` | `discord_id` 미반환 (상태만) |
| WebSocket 인증 | `ws_token` / `RELAY_INTERNAL_SECRET` 은 **첫 JSON 메시지**로 전달 (URL 쿼리 미사용) |
| nginx `/ws/` 로그 | `yummi_ws_safe` 포맷 — URI 만 기록 (쿼리스트링 제외) |
| 공개 URL | `RELAY_PUBLIC_BASE_URL` 비로컬은 **HTTPS 필수** (relay 기동 시 검증) |
| Redis | 프로덕션: `requirepass` + `bind 127.0.0.1` 권장 |

## 에이전트 (PC)

| 항목 | 조치 |
|------|------|
| Relay / 업데이트 | `agent.json` 로드 시 공개 URL HTTP → HTTPS 승격 |
| 자동 업데이트 | zip URL **공식 HTTPS `/agent/` 경로만**; manifest **Ed25519 서명 + `sha256` 필수** |
| 세션 저장 | `relay-session.json` — Windows **DPAPI**(CurrentUser) 암호화, **14일 만료**, Relay URL 변경 시 재로그인 |
| LCU | 인증서 검증은 **127.0.0.1 / localhost 만** 우회 |
| lockfile | 로그에 lockfile 원문(비밀번호) 미포함 |

## 운영 체크리스트

1. VM 에 nginx 설정 반영 후 `nginx -t && systemctl reload nginx`
2. `.env` 에 `RELAY_INTERNAL_SECRET` 설정 (`.env.example` 참고)
3. Legacy 배포 시 `deploy/sync-agent-version.ps1` 로 `sha256` 포함 manifest 생성
4. Tauri 배포 시 `deploy/sync-tauri-agent-version.mjs` 로 서명된 `tauri` manifest 생성
4. `agent.json` / `.env` 커밋 금지
5. Relay·에이전트·봇을 **동일 버전**(v0.5.5+) 으로 함께 배포 (WS 프로토콜 변경)

## 잔여 리스크

- **LCU lockfile** = 해당 PC 계정 전체 제어 (로컬 신뢰 전제)
- **코드 서명** 미설정 빌드 — manifest 서명과 `sha256` 으로 업데이트 변조는 막지만 Windows publisher 신뢰는 낮음
- **Internal API** — `RELAY_INTERNAL_SECRET` 유출 시 연결된 모든 에이전트 명령 가능 (nginx + 시크릿 로테이션)
- **WebSocket** — `session_id` + `ws_token`(첫 메시지) + OAuth 링크 코드 3단계

## 검증 메모

1. 기존(평문/구버전) `relay-session.json` 이 있으면 에이전트 시작 시 **자동 로그인되지 않고** 브라우저 재로그인이 떠야 함
2. `agent.json` 기본값에서 `AutoUpdateEnabled` 는 `false` 여야 함
3. Bootstrapper 는 `https://yummi.duckdns.org` 이외 호스트 또는 64자리 hex가 아닌 `sha256` manifest 를 **거부**해야 함
4. Tauri updater 는 `tauri.signature` 가 없거나 public key 검증이 실패하면 업데이트를 **거부**해야 함
5. `YUMMI_AGENT_WINDOWS_SIGNING_THUMBPRINT` 를 빌드에 넣은 경우 updater 는 해당 publisher thumbprint 가 아닌 exe 를 **거부**해야 함

자세한 흐름: [`AGENT_MECHANISM.md`](AGENT_MECHANISM.md)
