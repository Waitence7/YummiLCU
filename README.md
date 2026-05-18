# YummiLcu

Discord 봇([Yummibot](../Yummibot))과 유저 PC의 League Client (LCU)를 중계합니다.

## 구성

| 경로 | 설명 |
|------|------|
| `relay/` | FastAPI — OAuth, WebSocket, 봇용 internal HTTP |
| `agent/` | C# WinForms 포터블 에이전트 (유저 PC) |

배포 **B**: Yummibot `main.py`와 Relay는 **별 프로세스**. 봇은 `http://127.0.0.1:8790` 으로만 명령 전달.

## Relay 실행

```bash
cd YummiLcu
cp .env.example .env
# .env 편집 후
uv sync
uv run yummi-lcu-relay
# 또는
uv run uvicorn relay.app:app --host 127.0.0.1 --port 8790
```

공개 HTTPS/WSS는 Nginx 등으로 `RELAY_PUBLIC_BASE_URL` → `127.0.0.1:8790` 프록시.

## Yummibot 연동

`Yummibot/.env` 에 추가:

```env
RELAY_INTERNAL_URL=http://127.0.0.1:8790
RELAY_INTERNAL_SECRET=<YummiLcu/.env 의 RELAY_INTERNAL_SECRET 과 동일>
RELAY_PUBLIC_BASE_URL=https://relay.example.com
```

Discord Developer Portal → OAuth2 Redirect: `https://relay.example.com/auth/callback`

## 에이전트

`agent/` — Windows에서 빌드 후 배포. 자세한 내용은 `agent/README.md`.
