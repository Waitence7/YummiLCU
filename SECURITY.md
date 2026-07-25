# 보안 정책

## 공개 범위

이 저장소는 Yummi LCU Agent의 공개 Windows 클라이언트 저장소입니다. 다음 영역이 공개 검토와 취약점 제보 범위에 포함됩니다.

- Tauri/Rust 클라이언트 코드
- React 사용자 인터페이스 코드
- 로컬 League Client LCU API 연결
- Yummi Relay 클라이언트 프로토콜 처리
- 로컬 설정, 세션 저장, 업데이트 처리
- Windows 설치 파일 및 CI 빌드 설정

Yummi Relay 서버, YummiBot, 웹 API, 데이터베이스, 서버 인프라, 운영 환경의 부정 사용 방지 로직은 이 저장소에 포함되지 않습니다.

## 민감 정보

비밀값이나 로컬 실행 설정은 커밋하지 않습니다. 다음 정보는 소스 코드와 빌드 산출물에 포함되면 안 됩니다.

- Discord Bot 토큰
- Relay 내부 인증 토큰
- OAuth Client Secret
- 데이터베이스 비밀번호
- 코드 서명 개인키
- 업데이트 manifest 서명 개인키
- 운영 서버 환경 변수
- League Client lockfile 원문 또는 LCU 비밀번호

클라이언트는 서버 권한 검증을 위해 내장된 공유 비밀값에 의존하지 않아야 합니다. 서버 인증과 권한 검증은 이 저장소 밖의 Yummi 서버 구성요소에서 수행됩니다.

## 클라이언트 보안 기준

- Agent는 사용자의 컴퓨터에서 실행 중인 로컬 League Client LCU HTTPS API에만 연결합니다.
- LCU lockfile 비밀번호와 로컬 LCU 인증 토큰은 Yummi Relay로 전송하지 않습니다.
- Discord OAuth 토큰은 Agent가 저장하거나 전달하지 않습니다.
- Relay 통신은 명령 전달, 처리 결과 반환, 세션 상태 유지를 위한 WebSocket 메시지로 제한합니다.
- 자동 업데이트는 HTTPS 업데이트 URL, SHA-256 검증, 서명된 Tauri manifest를 요구합니다.
- 로컬 세션 저장은 가능한 경우 Windows DPAPI를 사용합니다.

## 취약점 제보

이 저장소에서 GitHub private vulnerability reporting이 활성화되어 있으면 해당 기능을 사용합니다. 사용할 수 없다면 exploit 세부 정보, 비밀값, private token, 사용자 데이터를 포함하지 않는 최소한의 이슈로 영향을 받는 영역만 알려주세요.

비공개 Relay 서버, YummiBot, 데이터베이스, 운영 인프라와 관련된 제보는 운영 세부 정보를 공개 이슈에 쓰지 마세요. 해당 구성요소는 이 저장소의 오픈소스 공개 범위 밖입니다.
