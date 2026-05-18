# YummiLcu Agent (Windows)

WinForms 포터블 에이전트 — Relay WSS + 로컬 LCU.

## 빌드 (Windows)

| 스크립트 | 결과 |
|----------|------|
| `build.bat` | 포터블 publish 폴더 |
| `build-installer.bat` | **Inno Setup 설치 프로그램** + zip (자동 업데이트용) |
| `build-slim.bat` | 작은 zip (.NET 8 Runtime 필요) |

`build-installer.bat` 필요: [Inno Setup 6](https://jrsoftware.org/isdl.php)

출력:

- `installer\output\YummiAgent-Setup-<버전>.exe` — **처음 설치용**
- `installer\output\YummiAgent-win-x64-portable.zip` — 자동 업데이트용

다운로드 URL (manifest): `installerUrl` → `…/agent/YummiAgent-Setup-0.3.1.exe`

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
