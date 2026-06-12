# YummiLcu Agent (WPF)

단순 연결형 에이전트 — Relay WSS + 로컬 LCU (Discord 봇 명령 수신).

## 프로젝트 구조

```
agent/
├── YummiLcu.Core/          # LCU HTTP, Relay, 설정 (UI 없음)
│   ├── Lcu/                # LcuClient, AllowedActions, LcuQueue
│   └── Relay/              # RelaySession
├── YummiLcu.App/           # WPF 단순 UI
│   ├── MainWindow.xaml
│   └── ViewModels/AgentViewModel.cs
├── agent.json.example
└── build-installer.bat
```

## 빠른 시작

1. `dev-run.bat` — 에이전트 실행
2. `agent.json`에 `RelayPublicBaseUrl`, `LockfilePath` 설정
3. **연결 시작** → 브라우저 Discord 로그인 → Relay 유지
4. Discord `/lcu` 등 봇 명령으로 LCU 제어 (에이전트 UI에는 API 버튼 없음)

## 빌드

| 스크립트 | 결과 |
|----------|------|
| `build.bat` / `build-portable.bat` | 포터블 publish |
| `build-installer.bat` | Inno Setup + zip |
| `dev-run.bat` | 디버그 실행 |

실행 파일: `YummiLcu.App.exe` (현재 **v0.5.3**)

## 화면

| 요소 | 기능 |
|------|------|
| 연결 시작 / 중지 | Relay 세션 (자동 재연결) |
| Discord / Relay / LCU | 3단 연결 상태 표시 |
| Discord 재로그인 | 저장 세션 삭제 후 재인증 |
| lockfile | 롤 클라이언트 lockfile 경로 |
| 설정 | 닷지 방지, 기본 상메, Windows 시작 시 실행 |
| 로그 | UI + `%LocalAppData%\YummiAgent\agent.log` |
| 트레이 | 최소화 시 알림 영역, 더블클릭 복원 |

## 설정 (`agent.json`)

```json
{
  "RelayPublicBaseUrl": "https://yummi.duckdns.org",
  "LockfilePath": "%LocalAppData%\\Riot Games\\Riot Client\\Config\\lockfile",
  "PreventQueueAfterDodge": true,
  "ApplyDefaultStatusOnConnect": true
}
```

## Relay + 봇 응답

1. Discord 봇 → Relay `POST /internal/command`
2. Relay → 에이전트 WebSocket `command`
3. 에이전트 → LCU 실행 후 `command_result` 회신
4. Relay → 봇 HTTP `result` 포함 응답 → Discord에 실행 결과 표시

모집 **게임 초대하기**는 `invite_party_members` action으로 동일 경로를 사용합니다 (v0.5.1+).

## Relay 실시간 push (v0.5.2+)

에이전트가 LCU WebSocket 이벤트를 감지해 Relay로 push합니다. Relay는 봇 WebSocket(`/ws/bot`)으로 전달합니다.

| type | 용도 |
|------|------|
| `agent_hello` | 연결 시 버전·LCU 준비 상태 |
| `party_lobby_update` | 파티 로비 변경 → 모집 패널 실시간 갱신 |
| `gameflow_update` | 게임 단계 (Lobby, ReadyCheck, ChampSelect 등) |
| `ready_check_update` | 레디체크 상세 → Discord `/dm` 수락·거절 알림 |

`ready_check_update`가 없는 구버전 에이전트는 `gameflow_update`의 `ReadyCheck` 단계로 `/dm`이 동작합니다.

## 자동 업데이트 (v0.5.3+ 슬림)

| 패키지 | 용량(대략) | 용도 |
|--------|-----------|------|
| `YummiAgent.zip` | ~240KB | 슬림 전체 (framework-dependent, .NET 8 Desktop Runtime 별도) |
| `YummiAgent-patch.zip` | ~130KB | App.exe + Core.dll만 (슬림→슬림 패치) |
| `setup.exe` | ~1MB 이하 | 최신 설치 파일을 내려받는 부트스트래퍼
| `YummiAgent-Setup-*.exe` | ~2MB | 버전별 실제 설치 파일 (Inno Setup, .NET 8 Desktop Runtime 확인) |

구버전(**self-contained** 단일 exe 60MB+)은 슬림 zip으로 자동 갱신되지 않습니다. **설치 프로그램**으로 한 번 마이그레이션하세요.

manifest (`agent-version.json`)에 `patchUrl`·`patchFrom`이 있으면 해당 버전에서 패치 zip을 우선 사용합니다.

## 작동 원리 (상세)

[`docs/AGENT_MECHANISM.md`](../docs/AGENT_MECHANISM.md)  
보안: [`docs/SECURITY.md`](../docs/SECURITY.md)
