#!/usr/bin/env bash
# Ubuntu VM: GitHub Actions(Windows)에서 빌드한 Tauri artifact를 받아 정적 배포합니다.
#
#   ./deploy/vm-deploy-tauri-agent.sh
#   ./deploy/vm-deploy-tauri-agent.sh --download-only
#
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

REPO="${GITHUB_REPO:-Waitence7/YummiLCU}"
WORKFLOW_FILE="build-tauri-agent.yml"
TRIGGER_BUILD=true
WWW="${AGENT_WWW_DIR:-/var/www/yummi-agent}"
PUBLIC="${AGENT_PUBLIC_URL:-https://yummi.duckdns.org}"
NOTES="${AGENT_RELEASE_NOTES:-}"
CHANNEL="${AGENT_UPDATE_CHANNEL:-stable}"
ROLLOUT_PERCENT="${AGENT_ROLLOUT_PERCENT:-100}"

for arg in "$@"; do
  case "$arg" in
    --download-only) TRIGGER_BUILD=false ;;
    -h|--help)
      echo "Usage: $0 [--download-only]"
      exit 0
      ;;
  esac
done

if ! command -v gh >/dev/null 2>&1; then
  echo "gh CLI 필요: sudo apt install gh && gh auth login" >&2
  exit 1
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

WORKDIR="$(mktemp -d /tmp/yummi-tauri-agent-dl.XXXXXX)"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

if [[ "$TRIGGER_BUILD" == true ]]; then
  echo "==> GitHub Actions Tauri 빌드 시작 ($REPO / $WORKFLOW_FILE)"
  args=(--repo "$REPO" -f "channel=$CHANNEL" -f "rollout_percent=$ROLLOUT_PERCENT")
  if [[ -n "$NOTES" ]]; then
    args+=(-f "release_notes=$NOTES")
  fi
  gh workflow run "$WORKFLOW_FILE" "${args[@]}"
  sleep 3
  RUN_ID="$(gh run list --workflow="$WORKFLOW_FILE" --repo "$REPO" --limit 1 --json databaseId -q '.[0].databaseId')"
  echo "==> 빌드 대기 (run $RUN_ID)"
  gh run watch "$RUN_ID" --repo "$REPO" || true
  RUN_ID="$(find_latest_build_run)" || { echo "Tauri build 성공 run을 찾을 수 없습니다." >&2; exit 1; }
else
  RUN_ID="$(find_latest_build_run)" || { echo "Tauri build 성공 run을 찾을 수 없습니다." >&2; exit 1; }
fi

echo "==> Artifacts 다운로드 (run $RUN_ID)"
gh run download "$RUN_ID" --repo "$REPO" -n YummiLcuTauri-win-x64-portable -D "$WORKDIR"
gh run download "$RUN_ID" --repo "$REPO" -n YummiLcuTauri-Installers -D "$WORKDIR/installers" 2>/dev/null || true
gh run download "$RUN_ID" --repo "$REPO" -n agent-version-json -D "$WORKDIR/manifest"

MANIFEST="$WORKDIR/manifest/agent-version.json"
if [[ ! -f "$MANIFEST" ]]; then
  echo "agent-version.json artifact 없음" >&2
  exit 1
fi
VER="$(python3 -c "import json; print(json.load(open('$MANIFEST'))['tauri']['version'])")"
ZIP="$WORKDIR/tauri-$VER.zip"
if [[ ! -f "$ZIP" ]]; then
  ZIP="$(find "$WORKDIR" -maxdepth 1 -name 'tauri-*.zip' -print -quit)"
fi
if [[ -z "${ZIP:-}" || ! -f "$ZIP" ]]; then
  echo "Tauri zip artifact 없음" >&2
  exit 1
fi
SETUP="$(find "$WORKDIR/installers" -maxdepth 1 -name '*.exe' -print -quit || true)"

echo "==> 배포 v$VER"
sudo mkdir -p "$WWW/releases/tauri"
sudo install -m 0644 "$ZIP" "$WWW/releases/tauri/tauri-$VER.zip"
sudo install -m 0644 "$ZIP" "$WWW/YummiLcuTauri.zip"
sudo install -m 0644 "$MANIFEST" "$WWW/agent-version.json"
sudo install -m 0644 "$MANIFEST" "$WWW/version.json"
mkdir -p "$ROOT/deploy"
cp "$MANIFEST" "$ROOT/deploy/agent-version.json"

if [[ -n "$SETUP" && -f "$SETUP" ]]; then
  sudo install -m 0644 "$SETUP" "$WWW/releases/tauri/Yummi-LCU-Agent-$VER-setup.exe"
  sudo install -m 0644 "$SETUP" "$WWW/releases/tauri/latest-setup.exe"
  sudo install -m 0644 "$SETUP" "$WWW/setup.exe"
fi

echo "배포 완료:"
echo "  $PUBLIC/agent/version.json"
echo "  $PUBLIC/agent/releases/tauri/tauri-$VER.zip"
echo "  $PUBLIC/agent/setup.exe"
