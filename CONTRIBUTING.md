# 기여 안내

## 범위

이 저장소는 Yummi LCU Agent의 공개 Windows 클라이언트 변경을 받습니다.

- `agent-tauri/src-tauri/` 아래 Tauri/Rust Agent 코드
- `agent-tauri/src/` 아래 React UI 코드
- 클라이언트 프로토콜 타입과 명령 처리
- 로컬 설정, 세션, LCU, 업데이트 동작
- Windows 빌드와 설치 파일 workflow 설정

서버 측 Relay, YummiBot, 웹 API, 데이터베이스, 인프라, 운영 환경의 부정 사용 방지 로직은 이 저장소 범위가 아닙니다.

## 개발 검증

pull request를 열기 전에 다음 검증을 실행합니다.

```powershell
cd agent-tauri
npm install
npm run build
cd src-tauri
cargo test --locked
```

깨끗한 checkout이나 CI와 같은 환경에서는 `npm install` 대신 `npm ci`를 사용합니다. Linux에서 Rust 테스트를 실행할 때는 Tauri에 필요한 `pkg-config`와 GLib/WebKit 계열 시스템 패키지가 필요할 수 있습니다.

## 보안

이슈, pull request, 로그, 스크린샷, 테스트 fixture에 비밀값, token, private key, 로컬 `agent.json`, League Client lockfile 원문, 운영 서버 세부 정보를 포함하지 마세요. 취약점 제보 절차는 [SECURITY.md](SECURITY.md)를 따릅니다.
