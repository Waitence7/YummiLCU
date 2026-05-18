# YummiLcu

Discord 봇([Yummibot](../Yummibot))과 유저 PC의 League Client (LCU)를 중계합니다.

## 구성

| 경로 | 설명 |
|------|------|
| `relay/` | FastAPI — OAuth, WebSocket, 봇용 internal HTTP |
| `agent/` | C# WinForms **포터블** 에이전트 (유저 PC) |
| `deploy/` | nginx, systemd, `agent-version.json`, `agent-publish.sh` |

배포 **B**: Yummibot `main.py`와 Relay는 **별 프로세스**. 봇은 `http://127.0.0.1:8790` 으로만 명령 전달.

---

## Relay 실행

```bash
cd YummiLcu
cp .env.example .env
# .env 편집 후
uv sync
uv run yummi-lcu-relay
```

공개 HTTPS/WSS는 Nginx로 `RELAY_PUBLIC_BASE_URL` → `127.0.0.1:8790` 프록시 (`deploy/nginx-waitence.conf` 참고).

## Yummibot 연동

`Yummibot/.env`:

```env
RELAY_INTERNAL_URL=http://127.0.0.1:8790
RELAY_INTERNAL_SECRET=<YummiLcu/.env 와 동일>
RELAY_PUBLIC_BASE_URL=https://yummi.duckdns.org
```

Discord Developer Portal → OAuth2 Redirect: `https://<도메인>/auth/discord/callback`  
(실제 경로는 Relay `auth/callback` 설정에 따름)

---

## 에이전트 (유저 PC)

**포터블** zip을 아무 폴더에 풀고 `YummiLcu.Agent.exe` 실행. .NET 별도 설치 불필요.

| 빌드 | 용도 |
|------|------|
| `agent/build.bat` | 기본 — 포터블 (~80–150MB) |
| `agent/build-slim.bat` | 작은 zip (~5–15MB), PC에 .NET 8 Desktop Runtime 필요 |

설정은 exe **옆** `agent.json` (`agent/agent.json.example` 참고).  
자세한 UI·lockfile: `agent/README.md`.

---

## 에이전트 자동 업데이트

### 동작 방식

```
[에이전트 시작]
    → GET {UpdateManifestUrl}  (보통 …/agent/version.json)
    → manifest.version > 설치된 exe 버전 ?
         아니오 → 그대로 실행
         예     → manifest.url 에서 zip 다운로드
                → 임시 폴더에 압축 해제
                → 보조 cmd가 2초 후 exe 폴더에 robocopy (agent.json 제외)
                → 새 exe 실행 후 기존 프로세스 종료
```

- **완전 자동**: 사용자가 zip 받을 필요 없음 (기본 `AutoUpdateEnabled: true`).
- **`agent.json`은 덮어쓰지 않음** — URL, lockfile, 옵션은 업데이트 후에도 유지.
- 업데이트 중 에이전트가 **잠깐 꺼졌다 켜짐** (롤·Relay 연결도 끊김).

### 서버 쪽 두 파일

| 파일 | 역할 | 제공 방법 |
|------|------|-----------|
| `deploy/agent-version.json` | 버전·다운로드 URL·릴리스 노트 | Relay `GET /agent/version.json` |
| `/var/www/yummi-agent/YummiAgent.zip` | 실제 포터블 zip | nginx 정적 (`/agent/YummiAgent.zip`) |

manifest 예시:

```json
{
  "version": "0.3.1",
  "url": "https://yummi.duckdns.org/agent/YummiAgent.zip",
  "notes": "변경 내용 한 줄"
}
```

- `version`은 **반드시** 배포하는 exe의 `YummiLcu.Agent.csproj` `<Version>` 과 맞추고, 유저 PC보다 **커야** 업데이트가 실행됩니다.
- Relay는 manifest 파일만 읽습니다. **Relay 재시작 없이** manifest·zip만 바꿔도 됩니다.

### PC 설정 (`agent.json`)

```json
{
  "RelayPublicBaseUrl": "https://yummi.duckdns.org",
  "UpdateManifestUrl": "https://yummi.duckdns.org/agent/version.json",
  "CheckUpdatesOnStartup": true,
  "AutoUpdateEnabled": true
}
```

| 필드 | 설명 |
|------|------|
| `CheckUpdatesOnStartup` | `false`면 시작 시 버전 확인 안 함 |
| `AutoUpdateEnabled` | `false`면 새 버전 알림만 없고, 수동 배포 zip으로만 갱신 |

### 운영자: 새 버전 배포

**1. 버전 올리기** — `agent/YummiLcu.Agent/YummiLcu.Agent.csproj` 의 `<Version>` 증가 (예: `0.3.1` → `0.3.2`).

**2. VM 한 줄** (zip + manifest 동시):

```bash
cd ~/Yummi/YummiLcu
chmod +x deploy/agent-publish.sh
./deploy/agent-publish.sh /path/to/YummiAgent-win-x64-portable.zip
```

- zip만 서버에 두고 manifest만 갱신: `./deploy/agent-publish.sh` (인자 생략)
- 노트 지정: `AGENT_RELEASE_NOTES="솔랭 자동" ./deploy/agent-publish.sh ./zip`

환경 변수 (선택): `AGENT_PUBLIC_URL`, `AGENT_ZIP_PATH` (기본 `/var/www/yummi-agent/YummiAgent.zip`).

**3. GitHub Actions** (선택 자동화)

- `main`에 `agent/**` push → Windows에서 포터블 zip 빌드 + `deploy/agent-version.json` 생성
- Artifacts: `YummiAgent-win-x64-portable.zip`
- VM 자동 배포: **Secrets** `VM_HOST`, `VM_USER`, `VM_SSH_KEY` + **Variable** `VM_DEPLOY_ENABLED` = `true`  
  (job 조건에서 secrets는 쓸 수 없어 Variable로 deploy job on/off)

---

## 알아둘 것

### 보안·설정

- **`agent.json`은 Git에 올리지 마세요** — Relay URL, lockfile 경로 등. 예시만 `agent.json.example` 커밋.
- **`RELAY_INTERNAL_SECRET`** 은 Relay·Yummibot만 공유. 유저 PC 에이전트에는 없음.
- Discord OAuth는 **Relay 공개 URL** 기준. 도메인 바꾸면 Developer Portal Redirect도 수정.

### lockfile

- LCU 연결은 **League 설치 폴더의 lockfile 파일** 경로가 맞아야 함.  
  예: `C:\Riot Games\League of Legends\lockfile`  
  Riot Client `Config\lockfile`(0KB)와 혼동하지 않기.
- 에이전트 UI에서 「파일」/「롤 폴더」로 지정 가능.

### LCU 명령 (whitelist)

- 허용 명령은 **`relay/actions.py`** 와 **`agent/.../AllowedActions.cs`** 에 동일하게 있어야 함.
- 새 action 추가 시 Relay·에이전트·Yummibot `lcu_relay.py` 를 함께 맞출 것.
- 디스코드 `/lcu` — 롤 시작, 솔랭/일겜 돌리기, 매칭 수락 등. 실행·매칭은 수 분 걸릴 수 있음.

### 매칭·클라이언트

- **솔랭** queue `420`, **일반 비공개** queue `400` (한국 클라 기준).
- 매칭 중 UI에 **경과 시간·예상 대기** 표시 (`/lol-matchmaking/v1/search`).
- `PreventQueueAfterDodge`: 닷지 후 로비 복귀 시 매칭 자동 취소 (에이전트 설정).

### nginx

- `/agent/version.json` → Relay 프록시
- `/agent/YummiAgent.zip` → **정적 파일** (`alias /var/www/yummi-agent/YummiAgent.zip`)  
  zip URL 404면 자동 업데이트 실패.

### 업데이트 트러블슈팅

| 증상 | 확인 |
|------|------|
| 업데이트 안 됨 | VM `agent-version.json` version > PC exe 버전? zip URL 브라우저에서 다운로드 되는지? |
| exe만 있고 설정 날아감 | 같은 폴더에 `agent.json` 있는지 (업데이트는 json 보존) |
| 개발 중 계속 재시작 | `AutoUpdateEnabled: false` 또는 manifest version 내리지 않기 |
| Actions만 쓰는 경우 | Artifacts zip을 VM에 올린 뒤 `agent-publish.sh` 실행 (Artifacts URL은 로그인 필요라 manifest에 쓰기 부적합) |

### 저장소

- GitHub에는 **이 레포(YummiLCU)만** — `agent/`, `relay/`, `deploy/`.
- Yummibot은 별도 경로/저장소.

---

## 빠른 체크리스트 (새 릴리스)

1. [ ] `csproj` `<Version>` 증가  
2. [ ] Windows `build.bat` 또는 Actions로 포터블 zip  
3. [ ] VM: `./deploy/agent-publish.sh <zip>`  
4. [ ] `https://<도메인>/agent/version.json` 에 새 version 확인  
5. [ ] `https://<도메인>/agent/YummiAgent.zip` 다운로드 확인  
6. [ ] 테스트 PC에서 에이전트 재실행 → 자동 업데이트·`agent.json` 유지 확인
