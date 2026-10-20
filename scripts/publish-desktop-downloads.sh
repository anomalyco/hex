#!/bin/sh
# Publish the human-facing Linux and Windows downloads of a release whose
# signed feeds and artifacts are in <dist>:
#
#   1. HEX-Setup-<version>-x64.exe and -arm64.exe to the hex-releases bucket
#      (downloads.hex.kitlangton.dev/releases/), immutable;
#   2. those installers and the Linux executable (as HEX-<version>-x86_64-linux)
#      to the GitHub release app-v<version> as identical-byte mirrors.
#
# Nothing replaces different bytes. Uses this Mac's Wrangler login and gh
# (GH_TOKEN works).
#
#   scripts/publish-desktop-downloads.sh <dist> <version>
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
repo=${HEX_RELEASE_REPO:-anomalyco/hex}
bucket=${HEX_RELEASE_BUCKET:-hex-releases}
base_url=${HEX_RELEASE_BASE_URL:-https://downloads.hex.kitlangton.dev}
wrangler=${HEX_WRANGLER:-wrangler}
if [ $# -ne 2 ]; then
  echo "Usage: $0 <dist> <version>" >&2
  exit 1
fi
dist=$1
version=$2
tag="app-v$version"
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'
for command in cmp curl gh jq shasum "${wrangler%% *}"; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Publishing downloads requires $command." >&2
    exit 1
  fi
done

temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

# The Linux executable is the one the signed feed names.
"$root/scripts/release-feed.sh" verify "$dist" linux-update.json windows-update.json windows-aarch64-update.json
linux_artifact=$(jq -r .payload "$dist/linux-update.json" | openssl base64 -d -A | jq -r .artifact)
cp "$dist/$linux_artifact" "$temporary/HEX-$version-x86_64-linux"

installers=
for label in x64 arm64; do
  name="HEX-Setup-$version-$label.exe"
  path="$dist/$name"
  test -s "$path"
  test "$(head -c 2 "$path")" = MZ
  installers="$installers $path"
  remote="$temporary/remote-$name"
  status=$(curl --silent --show-error --proto '=https' --connect-timeout 10 --max-time 600 \
    --retry 3 --write-out '%{http_code}' --output "$remote" "$base_url/releases/$name?check=$(date +%s)")
  case "$status" in
    200)
      if ! cmp -s "$path" "$remote"; then
        echo "releases/$name already exists with different bytes." >&2
        exit 1
      fi
      echo "releases/$name is already published." >&2
      ;;
    404)
      $wrangler r2 object put "$bucket/releases/$name" --remote --file "$path" \
        --content-type application/vnd.microsoft.portable-executable \
        --cache-control 'public, max-age=31536000, immutable' \
        --content-disposition "attachment; filename=\"$name\""
      curl --fail --silent --show-error --proto '=https' --max-time 600 \
        "$base_url/releases/$name?check=$(date +%s)" -o "$remote"
      cmp -s "$path" "$remote"
      ;;
    *)
      echo "Could not inspect releases/$name (HTTP $status)." >&2
      exit 1
      ;;
  esac
  echo "$base_url/releases/$name $(shasum -a 256 "$path" | awk '{ print $1 }')"
done

# The GitHub mirror. App releases stay off "Latest" (see AGENTS.md).
if ! gh release view "$tag" -R "$repo" --json name >/dev/null 2>&1; then
  notes="$root/docs/releases/$version.md"
  test -f "$notes"
  gh release create "$tag" -R "$repo" --verify-tag --title "Hex $version" \
    --notes-file "$notes" --latest=false
fi
gh release edit "$tag" -R "$repo" --title "Hex $version" --latest=false >/dev/null
gh release view "$tag" -R "$repo" --json assets > "$temporary/release.json"
# shellcheck disable=SC2086
for path in $installers "$temporary/HEX-$version-x86_64-linux"; do
  name=$(basename "$path")
  if ! jq -e --arg name "$name" '.assets | any(.name == $name)' "$temporary/release.json" >/dev/null; then
    gh release upload "$tag" "$path" -R "$repo"
  fi
  rm -f "$temporary/mirror"
  curl --fail --location --silent --show-error --proto '=https' --proto-redir '=https' \
    --max-time 600 "https://github.com/$repo/releases/download/$tag/$name" -o "$temporary/mirror"
  if ! cmp -s "$path" "$temporary/mirror"; then
    echo "GitHub's $name differs from the release bytes; refusing to continue." >&2
    exit 1
  fi
  echo "https://github.com/$repo/releases/download/$tag/$name"
done
