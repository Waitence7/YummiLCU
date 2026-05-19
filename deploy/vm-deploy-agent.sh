#!/usr/bin/env bash
# Ubuntu VM: GitHub Actions(Windows)에서 포터블 zip 빌드 → 이 서버에 배포
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
ARTIFACT_NAME="YummiAgent-win-x64-portable"
DOWNLOAD_ONLY=false
TRIGGER_BUILD=true
NOTES="${AGENT_RELEASE_NOTES:-}"

for arg in "$@"; do
  case "$arg" in
    --download-only) DOWNLOAD_ONLY=true; TRIGGER_BUILD=false ;;
    --no-trigger) TRIGGER_BUILD=false ;;
    -h|--help)
      echo "Usage: $0 [--download-only] [--no-trigger]"
      echo "  VM에서 에이전트 zip 배포 (빌드는 GitHub Windows 러너)"
      exit 0
      ;;
  esac
done

if ! command -v gh >/dev/null 2>&1; then
  echo "gh CLI 필요: sudo apt install gh && gh auth login" >&2
  exit 1
fi

if ! gh auth status >/dev/null 2>&1; then
  echo "GitHub 로그인 필요: gh auth login" >&2
  exit 1
fi

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
  gh run watch "$RUN_ID" --repo "$REPO" --exit-status
else
  RUN_ID="$(gh run list --workflow="$WORKFLOW_FILE" --repo "$REPO" --limit 1 --status success --json databaseId -q '.[0].databaseId')"
  if [[ -z "$RUN_ID" || "$RUN_ID" == "null" ]]; then
    echo "성공한 빌드 run이 없습니다. 먼저: $0 (트리거 없이)" >&2
    echo "또는 GitHub Actions에서 workflow를 한 번 실행하세요." >&2
    exit 1
  fi
  echo "==> 마지막 성공 빌드 사용 (run $RUN_ID)"
fi

echo "==> Artifact 다운로드"
gh run download "$RUN_ID" --repo "$REPO" -n "$ARTIFACT_NAME" -D "$WORKDIR"

ZIP="$WORKDIR/${ARTIFACT_NAME}.zip"
[[ -f "$ZIP" ]] || ZIP="$(find "$WORKDIR" -name '*.zip' -print -quit)"
if [[ -z "$ZIP" || ! -f "$ZIP" ]]; then
  echo "zip을 찾을 수 없습니다: $WORKDIR" >&2
  ls -laR "$WORKDIR" >&2 || true
  exit 1
fi

echo "==> VM 배포 (agent-publish.sh)"
"$ROOT/deploy/agent-publish.sh" "$ZIP"

echo ""
echo "배포 완료. 확인:"
echo "  curl -s https://yummi.duckdns.org/agent/version.json"
echo "  ls -lh /var/www/yummi-agent/YummiAgent.zip"
