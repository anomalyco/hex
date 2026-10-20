#!/bin/sh
set -eu

# Mirror prepared, signed bytes; never rebuild or overwrite an existing asset.
version=${1:?Usage: publish-app-mirror.sh VERSION DMG NOTES COMMIT}
artifact_path=${2:?Pass the prepared signed DMG}
notes=${3:?Pass release notes}
commit=${4:?Pass the full release commit}
repo=anomalyco/hex
tag="app-v$version"
artifact="HEX-$version-arm64.dmg"
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'
test "$(basename "$artifact_path")" = "$artifact"
test -f "$artifact_path"
test -f "$notes"

temporary=$(mktemp -d "${TMPDIR:-/tmp}/hex-app-mirror.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
if ! gh release view "$tag" --repo "$repo" --json assets > "$temporary/release.json"; then
  gh release create "$tag" --repo "$repo" --target "$commit" \
    --title "Hex $version" --notes-file "$notes" --draft --latest=false
  gh release view "$tag" --repo "$repo" --json assets > "$temporary/release.json"
fi
if ! jq -e --arg name "$artifact" '.assets | any(.name == $name)' "$temporary/release.json" >/dev/null; then
  gh release upload "$tag" "$artifact_path" --repo "$repo"
fi
gh release download "$tag" --repo "$repo" --pattern "$artifact" --dir "$temporary"
if ! cmp -s "$artifact_path" "$temporary/$artifact"; then
  echo "GitHub's $artifact differs from the prepared DMG; refusing to replace it." >&2
  exit 1
fi
gh release edit "$tag" --repo "$repo" --draft=false --latest=false
url="https://github.com/$repo/releases/download/$tag/$artifact"
curl --fail --location --silent --show-error --proto '=https' --proto-redir '=https' \
  --connect-timeout 15 --max-time 600 "$url" -o "$temporary/public.dmg"
cmp -s "$artifact_path" "$temporary/public.dmg"
echo "$url"
