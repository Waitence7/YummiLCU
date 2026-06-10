# YummiLcu

Discord 봇([YummiBot](../YummiBot))과 유저 PC의 League Client (LCU)를 중계합니다.

## 구성

| 경로 | 설명 |
|------|------|
| `relay/` | FastAPI — OAuth, WebSocket, 봇용 internal HTTP |
| `agent/` | C# WPF 에이전트 (유저 PC, v0.5.3) |
| `deploy/` | nginx, systemd, `agent-version.json`, `agent-publish.sh` |

배포 **B**: YummiBot `main.py`와 Relay는 **별 프로세스**. 봇은 `http://127.0.0.1:8790` (HTTP + `/ws/bot`)으로 Relay와 통신.

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

## YummiBot 연동

`YummiBot/.env`:

```env
RELAY_INTERNAL_URL=http://127.0.0.1:8790
RELAY_INTERNAL_SECRET=<YummiLcu/.env 와 동일>
RELAY_PUBLIC_BASE_URL=https://yummi.duckdns.org
```

Discord Developer Portal → OAuth2 Redirect: `https://<도메인>/auth/discord/callback`  
(실제 경로는 Relay `auth/callback` 설정에 따름)

---

## 에이전트 (유저 PC)

- **처음 설치:** `YummiAgent-Setup-x.x.x.exe` (Inno Setup, .NET 8 Desktop Runtime 필요)  
- **자동 업데이트:** 서버 zip → 기존 설치 폴더에 덮어쓰기 (슬림 빌드 기준 ~240KB)

| 빌드 | 용도 |
|------|------|
| `agent/build.bat` / `build-installer.bat` | 슬림 publish + Inno Setup (~2MB 설치 파일) |
| `agent/build-slim.bat` | 슬림 zip만 (~240KB), PC에 .NET 8 Desktop Runtime 필요 |

설정은 exe **옆** `agent.json` (`agent/agent.json.example` 참고).  
UI·lockfile·실시간 push: [`agent/README.md`](agent/README.md).

### 봇 연동 기능 (요약)

| 기능 | 경로 |
|------|------|
| `/lcu` 패널 | 봇 HTTP → Relay → 에이전트 WS → LCU |
| 모집 **게임 초대하기** | `invite_party_members` |
| 모집 로비 실시간 갱신 | 에이전트 `party_lobby_update` → Relay `/ws/bot` → 봇 |
| `/dm` 매치 알림 | `ready_check_update` (또는 `gameflow_update` ReadyCheck) → DM 수락/거절 버튼 |

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

### 서버 쪽 파일

| 파일 | 역할 | 제공 방법 |
|------|------|-----------|
| `deploy/agent-version.json` | 버전·다운로드 URL·패치·SHA256 | Relay `GET /agent/version.json` |
| `/var/www/yummi-agent/YummiAgent.zip` | 슬림 전체 zip | nginx 정적 |
| `/var/www/yummi-agent/YummiAgent-patch.zip` | 슬림→슬림 패치 (선택) | nginx 정적 |
| `/var/www/yummi-agent/YummiAgent-Setup-*.exe` | 최초 설치 | nginx 정적 |

manifest 예시 (v0.5.3+):

```json
{
  "version": "0.5.3",
  "url": "https://yummi.duckdns.org/agent/YummiAgent.zip",
  "installerUrl": "https://yummi.duckdns.org/agent/YummiAgent-Setup-0.5.3.exe",
  "patchUrl": "https://yummi.duckdns.org/agent/YummiAgent-patch.zip",
  "patchFrom": "0.5.2",
  "notes": "변경 내용 한 줄",
  "sha256": "...",
  "patchSha256": "..."
}
```

- `version`은 **반드시** 배포하는 exe의 `YummiLcu.App.csproj` `<Version>` 과 맞추고, 유저 PC보다 **커야** 업데이트가 실행됩니다.
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

**1. 버전 올리기** — `agent/YummiLcu.App/YummiLcu.App.csproj` 의 `<Version>` 증가 (예: `0.4.0` → `0.4.1`).

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

- `main`에 `agent/**` push → Windows에서 슬림 zip·패치·설치 파일 빌드 + `deploy/agent-version.json` 생성
- Artifacts: `YummiAgent-Setup`, `YummiAgent-win-x64-portable`, `YummiAgent-patch` (해당 시)
- VM 자동 배포: **Secrets** `VM_HOST`, `VM_USER`, `VM_SSH_KEY` + **Variable** `VM_DEPLOY_ENABLED` = `true`  
  (UFW 등으로 SCP가 막히면 job이 skip될 수 있음)

**4. VM에서 수동 배포** (`gh auth login` 필요):

```bash
cd ~/Yummi/YummiLcu
./deploy/vm-deploy-agent.sh --download-only   # 마지막 성공 빌드 artifact → /var/www/yummi-agent/
```

---

## 알아둘 것

### 보안·설정

- **`agent.json`은 Git에 올리지 마세요** — Relay URL, lockfile 경로 등. 예시만 `agent.json.example` 커밋.
- **`RELAY_INTERNAL_SECRET`** 은 Relay·YummiBot만 공유. 유저 PC 에이전트에는 없음.
- Discord OAuth는 **Relay 공개 URL** 기준. 도메인 바꾸면 Developer Portal Redirect도 수정.

### lockfile

- LCU 연결은 **League 설치 폴더의 lockfile 파일** 경로가 맞아야 함.  
  예: `C:\Riot Games\League of Legends\lockfile`  
  Riot Client `Config\lockfile`(0KB)와 혼동하지 않기.
- 에이전트 UI에서 「파일」/「롤 폴더」로 지정 가능.

### LCU 명령 (whitelist)

- 허용 명령은 **`relay/actions.py`** 와 **`agent/.../AllowedActions.cs`** 에 동일하게 있어야 함.
- 새 action 추가 시 Relay·에이전트·YummiBot `lcu_relay.py` 를 함께 맞출 것.
- 디스코드 `/lcu` — 롤 시작, 솔랭/일겜 돌리기, 매칭 수락 등. 실행·매칭은 수 분 걸릴 수 있음.
- 디스코드 `/dm` — 매치 잡힘 시 DM 알림 (수락/거절 버튼). Redis + Relay `subscribe_match_dm` 구독.

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
| 구버전 self-contained | 60MB+ 단일 exe는 슬림 zip 자동 갱신 불가 → **Setup 설치**로 마이그레이션 |
| exe만 있고 설정 날아감 | 같은 폴더에 `agent.json` 있는지 (업데이트는 json 보존) |
| 개발 중 계속 재시작 | `AutoUpdateEnabled: false` 또는 manifest version 내리지 않기 |
| Actions deploy skip | `vm-deploy-agent.sh --download-only` 또는 `agent-publish.sh`로 수동 배포 |

### 저장소

- GitHub에는 **이 레포(YummiLCU)만** — `agent/`, `relay/`, `deploy/`.
- YummiBot은 별도 경로/저장소.

---

## 빠른 체크리스트 (새 릴리스)

1. [ ] `csproj` `<Version>` 증가  
2. [ ] Windows `build.bat` 또는 Actions로 포터블 zip  
3. [ ] VM: `./deploy/agent-publish.sh <zip>`  
4. [ ] `https://<도메인>/agent/version.json` 에 새 version 확인  
5. [ ] `https://<도메인>/agent/YummiAgent.zip` 다운로드 확인  
6. [ ] 테스트 PC에서 에이전트 재실행 → 자동 업데이트·`agent.json` 유지 확인
