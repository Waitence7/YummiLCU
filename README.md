# Yummi LCU Agent

Yummi LCU Agent는 Windows에서 실행되는 Tauri/Rust 기반 클라이언트 애플리케이션입니다. Discord에서 요청된 기능을 사용자의 로컬 League Client LCU API와 연결합니다.

## 프로그램 기능

- Discord에서 요청된 LCU 명령을 사용자의 로컬 League Client에 전달합니다.
- 매치 수락/거절, 재접속, 닷지, 큐 시작/취소, 로비 나가기, 파티 준비 상태 변경을 처리합니다.
- 랭크 솔로/일반 교차 선택 로비 생성, 파티원 초대, 파티원 상태 확인을 지원합니다.
- 챔피언 선택, 소환사 주문 설정, 룬 페이지 조회/수정, 보상 수령, 상태 메시지 설정을 지원합니다.
- League Client 상태, 준비 확인, 챔피언 선택, 로비/파티, 게임 종료 요약 등 요청된 이벤트를 Relay로 전달합니다.
- Relay 세션 저장, 자동 재연결, 서명된 업데이트 manifest 검증, SHA-256 검증 기반 업데이트 처리를 제공합니다.

## 설치 방법

1. [GitHub Releases](https://github.com/Waitence7/YummiLCU/releases/latest)에서 최신 Windows 릴리스를 다운로드합니다.
2. 설치 파일(`setup.exe`, `.msi`)이 제공되면 실행합니다. 포터블 zip만 제공되는 릴리스는 압축을 풀고 `yummi-lcu-tauri.exe` 또는 포함된 실행 파일을 실행합니다.
3. League Client를 실행한 상태에서 Agent를 시작합니다.
4. Discord에서 Yummi가 안내하는 연결 절차에 따라 Agent와 Discord 세션을 연결합니다.

현재 공개 릴리스에는 다운로드 가능한 Windows zip asset이 포함되어 있습니다. 새 릴리스의 파일명은 빌드 방식에 따라 달라질 수 있으므로 릴리스 페이지의 최신 asset을 기준으로 확인하세요.

## 오픈소스 공개 범위

이 저장소에는 Yummi LCU Agent Windows 애플리케이션을 빌드하는 데 필요한 클라이언트 소스 코드와 빌드 설정이 포함되어 있습니다.

공개되는 주요 구성요소는 다음과 같습니다.

- Tauri 및 Rust 기반 Windows Agent
- 사용자 인터페이스 코드
- League Client의 로컬 LCU API 연결 코드
- Yummi Relay와 통신하는 클라이언트 코드
- Relay 요청 및 응답 데이터 형식
- 설정, 세션, 업데이트 처리 코드
- 설치 파일 생성 및 CI 빌드 설정

Yummi LCU Agent와 통신하는 다음 서버 측 구성요소는 이 저장소의 공개 범위에 포함되지 않습니다.

- Yummi Relay 서버 구현
- YummiBot 서버 코드
- 웹 API 및 관리자용 내부 API
- 데이터베이스 및 서버 인프라
- 운영 환경의 보안·부정 사용 방지 로직

서버 측 구성요소는 Agent 설치 파일에 포함되지 않으며, 별도의 서버 환경에서 실행됩니다.

## 저장소 구성

| 경로 | 설명 |
|------|------|
| `agent-tauri/` | Tauri/Rust Windows Agent 소스 코드 |
| `scripts/sync-tauri-agent-version.mjs` | CI에서 업데이트 manifest를 생성하고 서명하는 스크립트 |
| `.github/workflows/build-tauri-agent.yml` | Windows Agent 빌드 및 릴리스 산출물 생성 workflow |
| `SECURITY.md` | 공개 범위와 보안 보고 안내 |
| `LICENSE` | GNU Affero General Public License v3.0 |

## 데이터 흐름

Yummi LCU Agent는 Discord에서 요청된 기능을 사용자의 로컬 League Client와 연결합니다.

```text
Discord 사용자
    ↓
YummiBot
    ↓
Yummi Relay Server
    ↓
Yummi LCU Agent
    ↕
League Client LCU API
```

Yummi LCU Agent는 사용자의 컴퓨터에서 실행되며, League Client가 로컬에서 제공하는 LCU HTTPS API에만 연결합니다.

Agent는 게임 화면이나 게임 프로세스를 직접 조작하지 않으며, 안티치트 또는 보안 기능을 우회하지 않습니다.

## 개인정보 및 데이터 전송

Yummi LCU Agent는 기능 제공을 위해 Yummi Relay Server와 암호화된 WebSocket 연결을 사용할 수 있습니다.

이 연결은 다음 목적으로 사용됩니다.

- Discord에서 요청된 명령 전달
- 명령 처리 결과 반환
- Agent와 Discord 세션 연결 상태 유지
- 요청된 로비 및 매치 관련 기능 처리

Agent가 Relay로 전송할 수 있는 정보는 다음 범위로 제한됩니다.

- Agent 세션 인증용 `ws_token`
- 명령 처리 성공/실패 여부와 처리 결과
- 요청된 LCU 상태 이벤트: gameflow, ready check, champion select, lobby/party, participant status, end-of-game 요약
- 사용자가 요청한 로비/매치 기능 처리에 필요한 최소한의 Riot ID, 파티원 상태, 선택 상태, 매치 상태 정보

League Client의 로컬 인증 비밀번호, LCU 인증 토큰 및 사용자의 Discord OAuth 토큰은 Yummi Relay Server로 전송되지 않습니다.

Agent는 게임 화면, 게임 프로세스 메모리, 키 입력, 음성 데이터, 로컬 파일 목록을 수집하거나 전송하지 않습니다.

Relay 세션은 사용자의 PC에 로컬 파일로 저장될 수 있으며, Windows에서는 가능한 경우 DPAPI로 보호됩니다. 저장되는 값은 세션 식별자, `ws_token`, 저장 시각, Relay URL입니다.

사용자는 Agent를 종료하거나 연결을 해제하여 Relay 통신을 중지할 수 있습니다.

## 보안

다음 정보는 소스 코드와 배포 파일에 포함되지 않습니다.

- Discord Bot 토큰
- Relay 내부 인증 토큰
- OAuth Client Secret
- 데이터베이스 비밀번호
- 코드 서명 개인키
- 업데이트 서명 개인키
- 운영 서버 환경 변수

서버 인증과 권한 검증은 클라이언트에 포함된 비밀값에 의존하지 않고 서버 측에서 수행됩니다.

## Code signing policy

공식 Windows 릴리스 산출물은 GitHub Actions의 `build-tauri-agent.yml` workflow에서 생성합니다.

- 업데이트 manifest는 `YUMMI_AGENT_MANIFEST_SIGNING_KEY`로 서명하며, 클라이언트는 내장된 공개키로 manifest 서명을 검증합니다.
- 업데이트 zip과 실행 파일은 manifest의 SHA-256 값과 일치해야 합니다.
- Windows Authenticode 서명은 `WINDOWS_CERTIFICATE`와 `WINDOWS_CERTIFICATE_PASSWORD` secret이 설정된 릴리스에서 적용합니다.
- `YUMMI_AGENT_WINDOWS_SIGNING_THUMBPRINT`가 설정된 경우, CI는 가져온 인증서의 thumbprint가 일치하지 않으면 실패해야 합니다.
- 인증서가 설정되지 않은 릴리스 산출물은 unsigned build로 간주하며, 공개 릴리스 노트나 asset 설명에서 signed/unsigned 상태를 구분해야 합니다.
- code signing 인증서, manifest signing private key, 인증서 비밀번호는 GitHub Secrets에만 보관하고 저장소에 커밋하지 않습니다.

릴리스 권한, GitHub Secrets 접근 권한, branch 보호 설정 변경 권한이 있는 maintainer 계정은 GitHub 2단계 인증을 활성화해야 합니다.

## 개발

```powershell
cd agent-tauri
npm install
npm run build
cd src-tauri
cargo check
```

Tauri 개발 실행에는 Windows WebView2와 Rust MSVC 툴체인이 필요합니다.

## 빌드 및 릴리스 산출물

Windows용 Agent 빌드는 GitHub Actions의 `build-tauri-agent.yml` 워크플로에서 수행합니다. 이 공개 저장소의 workflow는 Windows 설치 파일, 포터블 zip, 서명된 업데이트 manifest를 산출물로 만들며, 서버 배포는 포함하지 않습니다.

```powershell
gh workflow run build-tauri-agent.yml -f channel=stable -f rollout_percent=100
```

필수 설정:

- `YUMMI_AGENT_MANIFEST_SIGNING_KEY`
- `YUMMI_AGENT_MANIFEST_PUBLIC_KEY`

선택 설정:

- `WINDOWS_CERTIFICATE`
- `WINDOWS_CERTIFICATE_PASSWORD`
- `YUMMI_AGENT_WINDOWS_SIGNING_THUMBPRINT`

## 제3자 서비스 안내

Yummi LCU Agent는 Riot Games 또는 Discord의 공식 제품이 아니며, Riot Games 또는 Discord의 보증이나 후원을 받지 않습니다.

League of Legends 및 Riot Games 관련 명칭과 자산의 권리는 해당 권리자에게 있습니다.

## 라이선스

이 저장소는 GNU Affero General Public License v3.0에 따라 공개됩니다. 자세한 내용은 [LICENSE](LICENSE)를 확인하세요.
