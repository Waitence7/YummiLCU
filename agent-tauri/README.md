# Yummi LCU Agent (Tauri + Rust)

Windows 에이전트의 Tauri/Rust 구현입니다. Yummi Relay의 `/ws/agent` WebSocket 연결과 `agent.json` 설정 형식을 기준으로 합니다.

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

## 전적과 리플레이

- `전적` 탭에서 로그인한 롤 계정의 최근 경기를 20개씩 조회합니다(최대 200경기). 챔피언·소환사 검색과 게임 모드 필터를 제공합니다.
- 경기를 선택하면 팀별 최종 기록을 확인하고 `리플레이` 탭에서 로컬 ROFL의 동선을 재생할 수 있습니다. 재생·일시정지, 10초 이동, 배속, 시간 슬라이더, 선수 강조를 지원합니다.
- 작은 창에서는 목록과 상세를 전환하고, 780px 이상에서는 목록을 상세 옆에 유지합니다. 전적 탭의 연결 정보는 간결하게 표시합니다.
- 로컬 파일이 없으면 사용자가 누른 `리플레이 다운로드` 버튼으로 LCU에 다운로드를 요청합니다. 서버 보관 파일을 가져오는 기능은 포함하지 않습니다.
- 동선 재생은 기존 Rust 해석기가 지원하는 **16.17.810.4348 빌드의 소환사의 협곡**을 대상으로 합니다. 다른 패치·맵, 파일 누락, 해석 실패는 각각 안내하며 경기 결과는 계속 볼 수 있습니다. 최종 KDA나 피해량을 재생 시점의 수치로 표시하지 않습니다.
- 브라우저 확인: `NODE_ENV=development npm run dev`. 연결 시작 후 전적 탭을 열면 개발용 샘플이 표시됩니다. 첫 경기는 재생 가능, 두 번째는 미지원 패치, 세 번째는 파일 없음 상태입니다. 샘플은 일반 프로덕션 번들에 포함하지 않습니다.

지도 이미지는 YummiWeb의 `public/draft-sandbox/summoners-rift-base.webp`를 재사용합니다. 챔피언 초상화 로딩 실패 시 이름의 첫 글자를 표시합니다.

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
