# Yummi LCU Agent (Tauri + Rust)

Windows 에이전트의 Tauri 전환 작업본입니다. 기존 Python Relay의 `/ws/agent` 연결과 `agent.json` 설정 형식을 기준으로 합니다.

## 개발

```powershell
npm install
npm run build
cd src-tauri
cargo check
```

Tauri 개발 실행에는 Windows WebView2와 Rust MSVC 툴체인이 필요합니다.

## 현재 구현

- React 19 + Tailwind CSS v4 탭 UI (내전 · 편의기능 · 음성(예정) · 로그)
- 브라우저 개발용 목 브리지 (`npm run dev` 후 Tauri 없이 UI 확인 가능, `src/api/mock.ts`)
- Relay WSS 연결 및 `auth`/`command_result` 메시지
- lockfile 파싱, loopback LCU HTTPS Basic 인증
- 기존 action whitelist와 핵심 queue/lobby/match/status/champ-select endpoint
- DPAPI 세션 저장 포맷(`V: 3`) 호환 기반
- HTTPS URL 보정, 자동 재연결, 300개 로그 제한
- 자체 updater: signed `agent-version.json` tauri 블록, zip/file SHA-256 검증, 채널/rollout, rollback-safe install

실제 League Client와 Relay를 이용한 smoke test는 해당 Windows PC의 `agent.json`과 lockfile 설정 후 수행해야 합니다.

## 배포

Windows Actions workflow:

```powershell
gh workflow run build-tauri-agent.yml -f channel=stable -f rollout_percent=100
```

필수:

- `YUMMI_AGENT_MANIFEST_SIGNING_KEY`
- `YUMMI_AGENT_MANIFEST_PUBLIC_KEY`

선택:

- `WINDOWS_CERTIFICATE`
- `WINDOWS_CERTIFICATE_PASSWORD`
- `YUMMI_AGENT_WINDOWS_SIGNING_THUMBPRINT`
