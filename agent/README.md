# YummiLcu Agent (Windows)

WinForms 포터블 에이전트 — Relay WSS + 로컬 LCU.

## 빌드 (Windows)

```bat
build.bat
```

또는 `build-portable.bat` — **포터블** (~80–150MB, .NET 런타임 설치 불필요).  
작은 zip이 필요하면 `build-slim.bat` (~5–15MB, PC에 .NET 8 Desktop Runtime 필요).

출력: `YummiLcu.Agent\bin\Release\net8.0-windows\win-x64\publish\YummiLcu.Agent.exe`

## 설정

실행 파일 옆 `agent.json` (선택):

```json
{
  "RelayPublicBaseUrl": "https://yummi.duckdns.org",
  "LockfilePath": "%LocalAppData%\\Riot Games\\Riot Client\\Config\\lockfile",
  "AuthPollIntervalMs": 1500
}
```

`LockfilePath`: lockfile이 있는 **파일** 전체 경로 (폴더가 아님). 끝에 `\lockfile` 포함.

미설정 시 기본 `http://127.0.0.1:8790` (로컬 Relay 테스트용).

## 동작

1. UUID `session_id` 생성 → 브라우저 `/login?session_id=...`
2. WSS `/ws/agent?session_id=...` + `/auth/status` 폴링
3. lockfile → LCU HTTPS
4. WS `command` → whitelist만 LCU 호출
