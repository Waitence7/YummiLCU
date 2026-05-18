#!/usr/bin/env bash
# VM에서 에이전트 zip 배포 + version.json 동기화 (한 번에)
#
#   ./deploy/agent-publish.sh                          # manifest만 csproj 버전으로 갱신
#   ./deploy/agent-publish.sh /path/to/YummiAgent-win-x64-portable.zip
#   AGENT_RELEASE_NOTES="솔랭 자동" ./deploy/agent-publish.sh ./YummiAgent.zip
#
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CSPROJ="$ROOT/agent/YummiLcu.Agent/YummiLcu.Agent.csproj"
MANIFEST="$ROOT/deploy/agent-version.json"
ZIP_SRC="${1:-}"
ZIP_DST="${AGENT_ZIP_PATH:-/var/www/yummi-agent/YummiAgent.zip}"
PUBLIC="${AGENT_PUBLIC_URL:-https://yummi.duckdns.org}"
NOTES="${AGENT_RELEASE_NOTES:-}"

if [[ ! -f "$CSPROJ" ]]; then
  echo "csproj 없음: $CSPROJ" >&2
  exit 1
fi

VERSION="$(grep -oP '(?<=<Version>)[^<]+' "$CSPROJ" | head -1)"
if [[ -z "$VERSION" ]]; then
  echo "csproj에서 <Version> 을 찾을 수 없습니다." >&2
  exit 1
fi

[[ -z "$NOTES" ]] && NOTES="Yummi Agent $VERSION"

if [[ -n "$ZIP_SRC" ]]; then
  if [[ ! -f "$ZIP_SRC" ]]; then
    echo "zip 없음: $ZIP_SRC" >&2
    exit 1
  fi
  sudo mkdir -p "$(dirname "$ZIP_DST")"
  sudo cp "$ZIP_SRC" "$ZIP_DST"
  sudo chmod 644 "$ZIP_DST"
  echo "zip → $ZIP_DST"
fi

python3 - "$VERSION" "$PUBLIC" "$NOTES" "$MANIFEST" <<'PY'
import json, sys
version, public, notes, path = sys.argv[1:5]
data = {
    "version": version,
    "url": f"{public.rstrip('/')}/agent/YummiAgent.zip",
    "notes": notes,
}
with open(path, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2, ensure_ascii=False)
    f.write("\n")
print(json.dumps(data, ensure_ascii=False))
PY

echo "manifest → $MANIFEST (Relay가 이 파일을 서빙)"
echo "완료. PC 에이전트는 다음 실행 시 v$VERSION 자동 적용."
