# Yummi LCU Agent

Yummi LCU Agent는 Windows에서 실행되는 Tauri/Rust 기반 클라이언트 애플리케이션입니다. Discord에서 요청된 기능을 사용자의 로컬 League Client LCU API와 연결합니다.

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

## 외부 통신

Yummi LCU Agent는 기능 제공을 위해 Yummi Relay Server와 암호화된 WebSocket 연결을 사용할 수 있습니다.

이 연결은 다음 목적으로 사용됩니다.

- Discord에서 요청된 명령 전달
- 명령 처리 결과 반환
- Agent와 Discord 세션 연결 상태 유지
- 요청된 로비 및 매치 관련 기능 처리

League Client의 로컬 인증 비밀번호, LCU 인증 토큰 및 사용자의 Discord 인증 토큰은 Yummi Relay Server로 전송되지 않습니다.

Agent가 외부로 전송하는 정보는 요청을 처리하는 데 필요한 최소한의 명령 정보와 처리 결과로 제한됩니다.

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
