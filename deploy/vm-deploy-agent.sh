#!/usr/bin/env bash
# Ubuntu VM: GitHub Actions(Windows)에서 빌드한 artifact를 VM에서 직접 받아 배포
#
#   gh auth login   # 최초 1회
#   ./deploy/vm-deploy-agent.sh              # 빌드 트리거 + 대기 + 배포
#   ./deploy/vm-deploy-agent.sh --download-only   # 마지막 성공 빌드만 받아 배포
#
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

REPO="${GITHUB_REPO:-Waitence7/YummiLCU}"
WORKFLOW_FILE="build-yummi-agent.yml"
DOWNLOAD_ONLY=false
TRIGGER_BUILD=true
NOTES="${AGENT_RELEASE_NOTES:-}"
LCU="${VM_LCU_PATH:-/home/ubuntu/Yummi/YummiLcu}"
WWW="${AGENT_WWW_DIR:-/var/www/yummi-agent}"
PUBLIC="${AGENT_PUBLIC_URL:-https://yummi.duckdns.org}"

for arg in "$@"; do
  case "$arg" in
    --download-only) DOWNLOAD_ONLY=true; TRIGGER_BUILD=false ;;
    --no-trigger) TRIGGER_BUILD=false ;;
    -h|--help)
      echo "Usage: $0 [--download-only] [--no-trigger]"
      echo "  VM에서 에이전트 zip·설치파일·bootstrapper·manifest 배포"
      exit 0
      ;;
  esac
done

if ! command -v gh >/dev/null 2>&1; then
  echo "gh CLI 필요: sudo apt install gh && gh auth login" >&2
  exit 1
fi

if ! gh auth status >/dev/null 2>&1; then
  echo "gh 미로그인 — manual-deploy-from-ci.py 로 artifact 배포"
  exec python3 "$ROOT/deploy/manual-deploy-from-ci.py"
fi

find_latest_build_run() {
  local id build_result
  while IFS= read -r id; do
    [[ -z "$id" ]] && continue
    build_result="$(gh run view "$id" --repo "$REPO" --json jobs -q '.jobs[] | select(.name=="build") | .conclusion' 2>/dev/null || true)"
    if [[ "$build_result" == "success" ]]; then
      echo "$id"
      return 0
    fi
  done < <(gh run list --workflow="$WORKFLOW_FILE" --repo "$REPO" --limit 15 --json databaseId -q '.[].databaseId')
  return 1
}

WORKDIR="$(mktemp -d /tmp/yummi-agent-dl.XXXXXX)"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

if [[ "$TRIGGER_BUILD" == true ]]; then
  echo "==> GitHub Actions 빌드 시작 ($REPO / $WORKFLOW_FILE)"
  if [[ -n "$NOTES" ]]; then
    gh workflow run "$WORKFLOW_FILE" --repo "$REPO" -f "release_notes=$NOTES"
  else
    gh workflow run "$WORKFLOW_FILE" --repo "$REPO"
  fi
  sleep 3
  RUN_ID="$(gh run list --workflow="$WORKFLOW_FILE" --repo "$REPO" --limit 1 --json databaseId -q '.[0].databaseId')"
  echo "==> 빌드 대기 (run $RUN_ID) …"
  gh run watch "$RUN_ID" --repo "$REPO" || true
  RUN_ID="$(find_latest_build_run)" || { echo "build job 성공 run을 찾을 수 없습니다." >&2; exit 1; }
else
  RUN_ID="$(find_latest_build_run)" || { echo "build job 성공 run을 찾을 수 없습니다." >&2; exit 1; }
  echo "==> 최근 build 성공 run 사용 (run $RUN_ID, deploy-vm 실패 run 포함)"
fi

echo "==> Artifacts 다운로드"
gh run download "$RUN_ID" --repo "$REPO" -n YummiAgent-win-x64-portable -D "$WORKDIR"
gh run download "$RUN_ID" --repo "$REPO" -n YummiAgent-Setup -D "$WORKDIR" 2>/dev/null || true
gh run download "$RUN_ID" --repo "$REPO" -n YummiAgent-Bootstrapper -D "$WORKDIR" 2>/dev/null || true
gh run download "$RUN_ID" --repo "$REPO" -n agent-version-json -D "$WORKDIR/manifest" 2>/dev/null || true

ZIP="$(find "$WORKDIR" -maxdepth 1 \( -name 'YummiAgent-win-x64-portable.zip' -o -name '*.zip' \) -print -quit)"
if [[ -z "$ZIP" || ! -f "$ZIP" ]]; then
  echo "zip을 찾을 수 없습니다: $WORKDIR" >&2
  ls -laR "$WORKDIR" >&2 || true
  exit 1
fi

MANIFEST="$WORKDIR/manifest/agent-version.json"
if [[ ! -f "$MANIFEST" ]]; then
  MANIFEST="$WORKDIR/agent-version-json/agent-version.json"
fi
SETUP="$(find "$WORKDIR" -maxdepth 1 \( -name 'YummiAgent-Setup.exe' -o -name 'YummiAgent-Setup-*.exe' \) -print -quit || true)"
BOOTSTRAPPER="$(find "$WORKDIR" -maxdepth 1 -name 'setup.exe' -print -quit || true)"

if [[ ! -f "$MANIFEST" ]]; then
  echo "==> manifest 없음 — agent-publish.sh로 생성"
  if [[ -n "$SETUP" ]]; then
    "$ROOT/deploy/agent-publish.sh" "$ZIP" "$SETUP" "$BOOTSTRAPPER"
  else
    "$ROOT/deploy/agent-publish.sh" "$ZIP"
  fi
  exit 0
fi

VER="$(python3 -c "import json; print(json.load(open('$MANIFEST'))['version'])")"
LATEST_MANIFEST="$WORKDIR/manifest/latest.json"
if [[ ! -f "$LATEST_MANIFEST" ]]; then
  LATEST_MANIFEST="$WORKDIR/agent-version-json/latest.json"
fi

echo "==> 배포 v${VER}"
sudo mkdir -p "$WWW/files"

sudo cp "$ZIP" "$WWW/YummiAgent.zip"
sudo chmod 644 "$WWW/YummiAgent.zip"

PATCH="$(find "$WORKDIR" -maxdepth 1 -name 'YummiAgent-patch.zip' -print -quit || true)"
if [[ -n "$PATCH" && -f "$PATCH" ]]; then
  sudo cp "$PATCH" "$WWW/YummiAgent-patch.zip"
  sudo chmod 644 "$WWW/YummiAgent-patch.zip"
  echo "patch zip → $WWW/YummiAgent-patch.zip ($(du -h "$PATCH" | cut -f1))"
else
  echo "WARN: patch zip 없음 (패치 자동 업데이트 비활성)"
fi

if [[ -n "$SETUP" ]]; then
  sudo cp "$SETUP" "$WWW/files/YummiAgent-Setup-${VER}.exe"
  sudo cp "$SETUP" "$WWW/latest"
  sudo chmod 644 "$WWW/files/YummiAgent-Setup-${VER}.exe" "$WWW/latest"
else
  echo "WARN: installer exe 없음 (Inno 빌드 스킵?)"
fi

if [[ -n "$BOOTSTRAPPER" ]]; then
  sudo cp "$BOOTSTRAPPER" "$WWW/setup.exe"
  sudo chmod 644 "$WWW/setup.exe"
else
  echo "WARN: bootstrapper setup.exe 없음"
fi

mkdir -p "$LCU/deploy"
cp "$MANIFEST" "$LCU/deploy/agent-version.json"
sudo cp "$MANIFEST" "$WWW/agent-version.json"
sudo chmod 644 "$WWW/agent-version.json"

if [[ -f "$LATEST_MANIFEST" ]]; then
  cp "$LATEST_MANIFEST" "$LCU/deploy/latest.json"
  sudo cp "$LATEST_MANIFEST" "$WWW/latest.json"
  sudo chmod 644 "$WWW/latest.json"
elif [[ -n "$SETUP" ]]; then
  SHA256="$(sha256sum "$SETUP" | awk '{print $1}')"
  python3 - "$VER" "$SHA256" "$PUBLIC" "$LCU/deploy/latest.json" <<'PY'
import json, sys
ver, sha, public, path = sys.argv[1:5]
base = public.rstrip('/')
data = {
    "version": ver,
    "url": f"{base}/agent/files/YummiAgent-Setup-{ver}.exe",
    "sha256": sha,
}
with open(path, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2, ensure_ascii=False)
    f.write("\n")
PY
  sudo cp "$LCU/deploy/latest.json" "$WWW/latest.json"
  sudo chmod 644 "$WWW/latest.json"
fi

echo ""
echo "배포 완료 (v${VER}). 확인:"
echo "  curl -s ${PUBLIC%/}/agent/version.json"
echo "  curl -s ${PUBLIC%/}/agent/latest.json"
echo "  ls -lh $WWW/setup.exe"
echo "  ls -lh $WWW/files/YummiAgent-Setup-${VER}.exe"
echo "  ls -lh $WWW/YummiAgent.zip"
