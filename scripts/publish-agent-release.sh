#!/usr/bin/env bash
set -euo pipefail

release_dir="${1:-release}"
: "${GH_TOKEN:?GH_TOKEN is required}"
: "${AGENT_DEPLOY_TOKEN:?AGENT_DEPLOY_TOKEN is required}"
: "${RELEASE_CHANNEL:?RELEASE_CHANNEL is required}"
: "${RELEASE_LABEL:?RELEASE_LABEL is required}"
: "${TARGET_SHA:?TARGET_SHA is required}"

manifest="$release_dir/agent-version.json"
test -s "$manifest"
version="$(jq -r '.version' "$manifest")"
build_id="$(jq -r '.tauri.buildId // .buildId' "$manifest")"
manifest_channel="$(jq -r '.tauri.channel' "$manifest")"
manifest_label="$(jq -r '.tauri.releaseLabel' "$manifest")"
manifest_commit="$(jq -r '.tauri.commit' "$manifest")"
target_short="${TARGET_SHA:0:7}"
test -n "$version" && test "$version" != "null"
test -n "$build_id" && test "$build_id" != "null"
test "$manifest_channel" = "$RELEASE_CHANNEL"
test "$manifest_label" = "$RELEASE_LABEL"
test "$manifest_commit" = "$target_short"

installer="$release_dir/Yummi-LCU-Agent-${version}-setup.exe"
archive="$release_dir/tauri-${version}.zip"
test -s "$installer"
test -s "$archive"
cp "$installer" "$release_dir/Yummi-LCU-Agent-latest-setup.exe"
cp "$archive" "$release_dir/tauri-latest.zip"

tag="v${version}"
if [ "$RELEASE_CHANNEL" != "stable" ]; then tag="v${RELEASE_LABEL}"; fi
if ! gh release view "$tag" --repo "$GITHUB_REPOSITORY" >/dev/null 2>&1; then
  args=(
    "$tag"
    --target "$TARGET_SHA"
    --title "Yummi LCU Agent ${RELEASE_LABEL}"
    --notes "Yummi LCU Agent ${RELEASE_LABEL}."
  )
  if [ "$RELEASE_CHANNEL" != "stable" ]; then args+=(--prerelease); fi
  gh release create "${args[@]}" --repo "$GITHUB_REPOSITORY"
fi
gh release upload "$tag" \
  "$installer" \
  "$release_dir/Yummi-LCU-Agent-latest-setup.exe" \
  "$archive" \
  "$release_dir/tauri-latest.zip" \
  "$manifest" \
  --repo "$GITHUB_REPOSITORY" \
  --clobber

route_channel=""
if [ "$RELEASE_CHANNEL" != "stable" ]; then route_channel="/$RELEASE_CHANNEL"; fi

curl --fail-with-body --silent --show-error --retry 3 \
  -X PUT \
  -H "Authorization: Bearer $AGENT_DEPLOY_TOKEN" \
  -H "Content-Type: application/json" \
  --data-binary "@$manifest" \
  "https://yummi.duckdns.org/agent-deploy/manifest${route_channel}"

curl --fail-with-body --silent --show-error --retry 3 \
  -X PUT \
  -H "Authorization: Bearer $AGENT_DEPLOY_TOKEN" \
  -H "Content-Type: application/octet-stream" \
  --data-binary "@$installer" \
  "https://yummi.duckdns.org/agent-deploy/installer${route_channel}/${version}"

curl --fail-with-body --silent --show-error --retry 3 \
  -X PUT \
  -H "Authorization: Bearer $AGENT_DEPLOY_TOKEN" \
  -H "Content-Type: application/zip" \
  --data-binary "@$archive" \
  "https://yummi.duckdns.org/agent-deploy/archive${route_channel}/${version}"

if [ "$RELEASE_CHANNEL" = "stable" ]; then
  manifest_url="https://yummi.duckdns.org/agent/version.json"
  release_subdir=""
else
  manifest_url="https://yummi.duckdns.org/agent/releases/tauri/${RELEASE_CHANNEL}/version.json"
  release_subdir="/${RELEASE_CHANNEL}"
fi
public_manifest="$(curl --fail --silent --show-error --retry 3 "$manifest_url")"
jq -e \
  --arg version "$version" \
  --arg channel "$RELEASE_CHANNEL" \
  --arg release_label "$RELEASE_LABEL" \
  --arg build_id "$build_id" \
  '.tauri.version == $version and .version == $version and .tauri.channel == $channel and .tauri.releaseLabel == $release_label and .tauri.buildId == $build_id' \
  <<<"$public_manifest" >/dev/null
curl --fail --silent --show-error --retry 3 --head \
  "https://yummi.duckdns.org/agent/releases/tauri${release_subdir}/tauri-${version}.zip" >/dev/null
curl --fail --silent --show-error --retry 3 --head \
  "https://yummi.duckdns.org/agent/releases/tauri${release_subdir}/latest-setup.exe" >/dev/null

echo "Published ${RELEASE_LABEL} build ${build_id} (${manifest_commit})"
