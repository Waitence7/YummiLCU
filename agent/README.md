# YummiLcu Agent (WPF)

WPF + MVVM 에이전트 — Relay WSS + 로컬 LCU.

## 프로젝트 구조

```
agent/
├── YummiLcu.Core/          # LCU HTTP, Relay, 설정 (UI 없음)
│   ├── Lcu/                # LcuClient, LcuConnector, LcuQueue
│   ├── Lcu/Models/         # DTO (Summoner, Lobby, ChampSelect…)
│   └── Relay/              # RelaySession
├── YummiLcu.App/           # WPF UI
│   ├── Themes/             # CatTheme, CyberTheme, ClassicTheme
│   ├── Views/              # HomePage, LobbyPage, ChampSelectPage
│   ├── ViewModels/         # MVVM (CommunityToolkit.Mvvm)
│   └── Services/           # ThemeService, NavigationService
├── agent.json.example
└── build-installer.bat
```

## 빠른 시작 (개발)

1. **롤 없이 UI만 테스트:** `dev-run-test.bat` 또는 `dotnet run --project YummiLcu.App -- --test`
2. 일반 실행: `dev-run.bat` — 좌측 **테스트 모드 (롤 불필요)** 체크해도 동일
3. `agent.json`에 `"UiTestMode": true` 저장 시 다음 실행부터 자동 테스트 모드
4. 실제 LCU: 테스트 모드 **끄고** lockfile 경로 설정 → **홈**에서 LCU 연결

## 빌드

| 스크립트 | 결과 |
|----------|------|
| `build.bat` / `build-portable.bat` | 포터블 publish |
| `build-installer.bat` | Inno Setup + zip |
| `dev-run.bat` | 디버그 실행 |
| `dev-run-test.bat` | 테스트 모드 (`--test`, 롤 불필요) |

실행 파일: `YummiLcu.App.exe`

## 화면

| 메뉴 | 기능 |
|------|------|
| 홈 | 소환사 정보, 상메, lockfile |
| 로비 | 로비 생성, 게임 찾기, 친구 목록 |
| 챔프 선택 | 세션 폴링, 픽/밴, 룬 페이지 |

## 테마

`Themes/CatTheme.xaml`, `CyberTheme.xaml`, `ClassicTheme.xaml` — 동일 Key (`LcuBg`, `LcuPanel`, `LcuAccent`, `LcuSubAccent`, `LcuText`).

UI는 `{DynamicResource LcuBg}` 등으로 바인딩. 좌측 하단에서 테마 전환.

## 설정 (`agent.json`)

```json
{
  "RelayPublicBaseUrl": "https://yummi.duckdns.org",
  "LockfilePath": "%LocalAppData%\\Riot Games\\Riot Client\\Config\\lockfile",
  "UiTestMode": false,
  "PreventQueueAfterDodge": true,
  "ApplyDefaultStatusOnConnect": true
}
```

## LCU API (주요)

| 기능 | 엔드포인트 |
|------|------------|
| 소환사 | `GET /lol-summoner/v1/current-summoner` |
| 상메 | `GET/PUT /lol-chat/v1/me` |
| 로비 | `GET/POST/DELETE /lol-lobby/v2/lobby` |
| 매칭 | `POST/DELETE .../matchmaking/search` |
| 친구 | `GET /lol-chat/v1/friends` |
| 챔프선 | `GET /lol-champ-select/v1/session` |
| 픽/밴 | `PATCH .../session/actions/{id}` |
| 룬 | `GET /lol-perks/v1/pages` |

클라이언트 버전에 따라 응답 필드가 다를 수 있습니다.

## Relay

좌측 **Relay 시작** → Discord 로그인 → WS 명령으로 LCU 제어 (기존과 동일).

## 작동 원리 (상세)

전체 아키텍처·LCU·Relay·MVVM·테스트 모드·자동 업데이트 설명: [`docs/AGENT_MECHANISM.md`](../docs/AGENT_MECHANISM.md)  
보안·통신: [`docs/SECURITY.md`](../docs/SECURITY.md)
