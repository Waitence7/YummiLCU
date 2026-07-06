#!/usr/bin/env bash
# VM에서 에이전트 zip + installer + bootstrapper 배포 + manifest 동기화
#
#   ./deploy/agent-publish.sh
#   ./deploy/agent-publish.sh /path/to/portable.zip [/path/to/YummiAgent-Setup.exe] [/path/to/setup.exe]
#   AGENT_RELEASE_NOTES="솔랭 자동" ./deploy/agent-publish.sh ./YummiAgent.zip ./YummiAgent-Setup.exe ./setup.exe
#
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CSPROJ="$ROOT/agent/YummiLcu.App/YummiLcu.App.csproj"
MANIFEST="$ROOT/deploy/agent-version.json"
LATEST_MANIFEST="$ROOT/deploy/latest.json"
ZIP_SRC="${1:-}"
INSTALLER_SRC="${2:-}"
BOOTSTRAPPER_SRC="${3:-}"
ZIP_DST="${AGENT_ZIP_PATH:-/var/www/yummi-agent/YummiAgent.zip}"
WWW="${AGENT_WWW_DIR:-/var/www/yummi-agent}"
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
base = public.rstrip('/')
data = {
    "version": version,
    "url": f"{base}/agent/YummiAgent.zip",
    "installerUrl": f"{base}/agent/setup.exe",
    "notes": notes,
}
with open(path, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2, ensure_ascii=False)
    f.write("\n")
print(json.dumps(data, ensure_ascii=False))
PY

if [[ -n "$INSTALLER_SRC" ]]; then
  if [[ ! -f "$INSTALLER_SRC" ]]; then
    echo "installer 없음: $INSTALLER_SRC" >&2
    exit 1
  fi
  sudo mkdir -p "$WWW/files"
  INSTALLER_NAME="YummiAgent-Setup-${VERSION}.exe"
  sudo cp "$INSTALLER_SRC" "$WWW/files/$INSTALLER_NAME"
  sudo cp "$INSTALLER_SRC" "$WWW/latest"
  sudo chmod 644 "$WWW/files/$INSTALLER_NAME" "$WWW/latest"
  echo "installer → $WWW/files/$INSTALLER_NAME"
  echo "latest → $WWW/latest"

  SHA256="$(sha256sum "$INSTALLER_SRC" | awk '{print $1}')"
  python3 - "$VERSION" "$PUBLIC" "$NOTES" "$SHA256" "$LATEST_MANIFEST" <<'PY'
import json, sys
version, public, notes, sha256, path = sys.argv[1:6]
base = public.rstrip('/')
data = {
    "version": version,
    "url": f"{base}/agent/files/YummiAgent-Setup-{version}.exe",
    "sha256": sha256,
    "notes": notes,
}
with open(path, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2, ensure_ascii=False)
    f.write("\n")
print(json.dumps(data, ensure_ascii=False))
PY
fi

if [[ -n "$BOOTSTRAPPER_SRC" ]]; then
  if [[ ! -f "$BOOTSTRAPPER_SRC" ]]; then
    echo "bootstrapper 없음: $BOOTSTRAPPER_SRC" >&2
    exit 1
  fi
  sudo mkdir -p "$WWW"
  sudo cp "$BOOTSTRAPPER_SRC" "$WWW/setup.exe"
  sudo chmod 644 "$WWW/setup.exe"
  echo "bootstrapper → $WWW/setup.exe"
fi

PUBLIC_MANIFEST="${AGENT_MANIFEST_PATH:-$WWW/agent-version.json}"
sudo mkdir -p "$(dirname "$PUBLIC_MANIFEST")"
sudo cp "$MANIFEST" "$PUBLIC_MANIFEST"
sudo chmod 644 "$PUBLIC_MANIFEST"
echo "manifest → $MANIFEST"
echo "manifest → $PUBLIC_MANIFEST (nginx /agent/version.json)"
if [[ "$PUBLIC_MANIFEST" == "$WWW/agent-version.json" ]]; then
  sudo cp "$MANIFEST" "$WWW/version.json"
  sudo chmod 644 "$WWW/version.json"
  echo "manifest alias → $WWW/version.json"
fi

if [[ -f "$LATEST_MANIFEST" ]]; then
  sudo cp "$LATEST_MANIFEST" "$WWW/latest.json"
  sudo chmod 644 "$WWW/latest.json"
  echo "latest manifest → $WWW/latest.json"
fi

echo "완료. 설치 링크: ${PUBLIC%/}/agent/setup.exe"
