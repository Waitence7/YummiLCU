#!/usr/bin/env bash
# VM에서 Legacy(C#)와 Tauri(Rust) 에이전트를 함께 배포합니다.
#
#   ./deploy/agent-publish.sh
#   ./deploy/agent-publish.sh /path/to/legacy.zip /path/to/tauri-portable.zip [legacy-installer]
#
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CSPROJ="$ROOT/agent/YummiLcu.App/YummiLcu.App.csproj"
MANIFEST="$ROOT/deploy/agent-version.json"
LEGACY_ZIP="${1:-}"
TAURI_ZIP="${2:-}"
INSTALLER_SRC="${3:-}"
WWW="${AGENT_WWW_DIR:-/var/www/yummi-agent}"
PUBLIC="${AGENT_PUBLIC_URL:-https://yummi.duckdns.org}"
NOTES="${AGENT_RELEASE_NOTES:-}"

if [[ ! -f "$CSPROJ" ]]; then
  echo "csproj 없음: $CSPROJ" >&2
  exit 1
fi

VERSION="$(grep -oP '(?<=<Version>)[^<]+' "$CSPROJ" | head -1)"
TAURI_VERSION="$(grep -m1 '^version = ' "$ROOT/agent-tauri/src-tauri/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')"
if [[ -z "$VERSION" ]]; then
  echo "csproj에서 <Version> 을 찾을 수 없습니다." >&2
  exit 1
fi

[[ -z "$NOTES" ]] && NOTES="Yummi Agent release"

publish_zip() {
  local kind="$1" version="$2" src="$3" executable="$4" alias="$5"
  [[ -z "$src" ]] && return 0
  if [[ ! -f "$src" ]]; then
    echo "$kind zip 없음: $src" >&2
    exit 1
  fi
  local dir="$WWW/releases/$kind" name="$kind-$version.zip"
  sudo mkdir -p "$dir"
  sudo cp "$src" "$dir/$name"
  sudo cp "$src" "$WWW/$alias"
  sudo chmod 644 "$dir/$name" "$WWW/$alias"
  sha256sum "$src" | awk '{print $1}' > "$ROOT/deploy/.${kind}.sha256"
  echo "$kind → $dir/$name"
}

publish_zip legacy "$VERSION" "$LEGACY_ZIP" "YummiLcu.App.exe" "YummiAgent.zip"
publish_zip tauri "$TAURI_VERSION" "$TAURI_ZIP" "yummi-lcu-tauri.exe" "YummiLcuTauri.zip"

legacy_sha="$(test -f "$ROOT/deploy/.legacy.sha256" && cat "$ROOT/deploy/.legacy.sha256" || true)"
tauri_sha="$(test -f "$ROOT/deploy/.tauri.sha256" && cat "$ROOT/deploy/.tauri.sha256" || true)"
rm -f "$ROOT/deploy/.legacy.sha256" "$ROOT/deploy/.tauri.sha256"

python3 - "$VERSION" "$TAURI_VERSION" "$PUBLIC" "$NOTES" "$legacy_sha" "$tauri_sha" "$MANIFEST" <<'PY'
import json, sys
legacy_v, tauri_v, public, notes, legacy_sha, tauri_sha, path = sys.argv[1:8]
base = public.rstrip('/')
def target(kind, version, filename, executable, sha):
    if not sha: return None
    return {"version": version, "url": f"{base}/agent/releases/{kind}/{kind}-{version}.zip",
            "sha256": sha, "executable": executable, "notes": notes}
legacy = target("legacy", legacy_v, "YummiAgent.zip", "YummiLcu.App.exe", legacy_sha)
tauri = target("tauri", tauri_v, "YummiLcuTauri.zip", "yummi-lcu-tauri.exe", tauri_sha)
data = {"schemaVersion": 2, "notes": notes, "legacy": legacy, "tauri": tauri}
# Root compatibility makes pre-v2 C# agents continue to update from the same URL.
if legacy:
    data.update({"version": legacy["version"], "url": legacy["url"], "sha256": legacy["sha256"]})
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

echo "완료. Legacy와 Tauri는 각자 manifest 대상과 배포 경로를 자동 선택합니다."
