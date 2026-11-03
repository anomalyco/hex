#!/usr/bin/env bash
# legacy-feeds.sh <check|publish> <version>: points the frozen Linux and
# Windows feeds that 2.3.1 installs read (https://anoma.ly/hex/files/
# {linux,windows,windows-aarch64}-update.json, which Kit's
# downloads.hex.kitlangton.dev redirects to) at <version>, the release
# service's current stable release. 2.3.1 installs it into its old folder,
# and the new Hex moves itself into the managed layout from there
# (src/updater/legacy.rs).
#
# Each feed is 2.3.1's format: {"payload","signature"}, both base64, an
# ed25519 signature by HEX_RELEASE_SIGNING_KEY (linux-signing.pub) over the
# payload's bytes. The payload names a content-addressed copy of the
# release's executable under releases/, uploaded first and never replaced
# with different bytes. Feeds go last. `check` builds, signs, and verifies
# everything without uploading.
. "$(dirname "$0")/lib.sh"

mode=${1:-}
version=${2:-}
case "$mode" in check | publish) ;; *) die "Usage: $0 <check|publish> <version>" ;; esac
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "Legacy feeds take a stable version, not '$version'."
: "${HEX_RELEASE_SIGNING_KEY:?Set HEX_RELEASE_SIGNING_KEY}"
release=$(cd "$(dirname "$0")" && pwd)
service=https://anoma.ly/release
files=https://anoma.ly/hex/files
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT

fetch() {
  curl --fail --silent --show-error --proto '=https' --proto-redir '=https' \
    --connect-timeout 10 --max-time "${3:-60}" --retry 3 \
    --header 'Cache-Control: no-cache' "$1" -o "$2"
}

(umask 077 && printf '%s\n' "$HEX_RELEASE_SIGNING_KEY" > "$temporary/key.pem")
openssl pkey -in "$temporary/key.pem" -pubout > "$temporary/key.pub"
cmp -s <(openssl pkey -pubin -in "$temporary/key.pub" -outform DER) \
  <(openssl pkey -pubin -in "$release/linux-signing.pub" -outform DER) ||
  die "HEX_RELEASE_SIGNING_KEY does not match linux-signing.pub, the key 2.3.1 trusts."

fetch "$service/hex/desktop/stable.json" "$temporary/stable.json"
[ "$(jq -r '"\(.schema) \(.app) \(.artifact) \(.channel)"' "$temporary/stable.json")" = "1 hex desktop stable" ] ||
  die "$service/hex/desktop/stable.json is not Hex's stable manifest."
current=$(jq -er .release.version "$temporary/stable.json")
[ "$current" = "$version" ] || die "Stable is $current, not $version; legacy feeds follow the current stable release."

# <feed> <service target> <2.3.1 target triple> <artifact suffix>
feeds=(
  "linux-update.json linux-x64 x86_64-unknown-linux-gnu x86_64-linux"
  "windows-update.json windows-x64 x86_64-pc-windows-msvc x86_64-windows.exe"
  "windows-aarch64-update.json windows-arm64 aarch64-pc-windows-msvc aarch64-windows.exe"
)
for entry in "${feeds[@]}"; do
  read -r feed target triple suffix <<< "$entry"
  url=$(jq -er --arg t "$target" '.release.targets[$t].url' "$temporary/stable.json") || die "$version has no $target."
  bytes=$(jq -er --arg t "$target" '.release.targets[$t].size' "$temporary/stable.json")
  sha=$(jq -er --arg t "$target" '.release.targets[$t].sha256' "$temporary/stable.json")
  [[ "$url" =~ ^/release/hex/files/[0-9a-f]{64}/[A-Za-z0-9._-]+$ ]] || die "$target names an unexpected URL."
  [[ "$sha" =~ ^[0-9a-f]{64}$ && "$bytes" =~ ^[0-9]+$ ]] || die "$target has a malformed size or SHA-256."
  executable="$temporary/$target"
  fetch "https://anoma.ly$url" "$executable" 600
  [ "$(wc -c < "$executable" | tr -d ' ')" = "$bytes" ] || die "$target has the wrong size."
  [ "$(sha256 "$executable" | cut -c1-64)" = "$sha" ] || die "$target has the wrong SHA-256."
  case "$suffix" in
    *.exe) [ "$(head -c 2 "$executable")" = MZ ] || die "$target is not a Windows executable." ;;
    *) [ "$(head -c 4 "$executable" | od -An -tx1 | tr -d ' \n')" = 7f454c46 ] || die "$target is not an ELF executable." ;;
  esac
  # A build without src/updater/legacy.rs would run from 2.3.1's folder and
  # never update again; its log message is the marker.
  grep -aFq 'moving a 2.3.1 install into the managed layout' "$executable" ||
    die "$version cannot move a 2.3.1 install into the managed layout; publishing it would strand those installs."
  artifact="HEX-$version-$sha-$suffix"
  jq -cn --arg target "$triple" --arg version "$version" --arg artifact "$artifact" \
    --argjson bytes "$bytes" --arg sha256 "$sha" \
    '{schema_version: 1, channel: "stable", target: $target, version: $version, artifact: $artifact, bytes: $bytes, sha256: $sha256}' \
    | tr -d '\n' > "$temporary/$feed.payload"
  openssl pkeyutl -sign -rawin -inkey "$temporary/key.pem" -in "$temporary/$feed.payload" -out "$temporary/$feed.sig"
  openssl pkeyutl -verify -rawin -pubin -inkey "$release/linux-signing.pub" \
    -in "$temporary/$feed.payload" -sigfile "$temporary/$feed.sig" > /dev/null || die "The $feed signature does not verify."
  jq -cn --arg payload "$(openssl base64 -A -in "$temporary/$feed.payload")" \
    --arg signature "$(openssl base64 -A -in "$temporary/$feed.sig")" \
    '{payload: $payload, signature: $signature}' > "$temporary/$feed"

  # 2.3.1 refuses anything but a newer version, so an older one is a mistake.
  fetch "$files/$feed?check=$(date +%s)" "$temporary/live-$feed"
  live=$(jq -er .payload "$temporary/live-$feed" | openssl base64 -d -A | jq -er .version)
  newest=$(printf '%s\n%s\n' "$live" "$version" | sort -V | tail -1)
  if [ "$live" != "$version" ] && [ "$newest" != "$version" ]; then
    die "$feed serves $live, newer than $version."
  fi
  echo "$feed: $live -> $version ($artifact, $bytes bytes)"
  {
    echo "### $feed"
    echo "$live → $version, \`releases/$artifact\`"
  } >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
  printf '%s %s %s\n' "$feed" "$artifact" "$executable" >> "$temporary/plan"
done

[ "$mode" = publish ] || exit 0

# Every executable first, so no feed ever names a missing file.
while read -r feed artifact executable; do
  status=$(curl --silent --show-error --proto '=https' --connect-timeout 10 --max-time 600 --retry 3 \
    --write-out '%{http_code}' --output "$temporary/remote" "$files/releases/$artifact?check=$(date +%s)")
  case "$status" in
    200) cmp -s "$executable" "$temporary/remote" || die "releases/$artifact exists with different bytes." ;;
    404)
      sh "$release/r2-put.sh" "releases/$artifact" "$executable" application/octet-stream 'public, max-age=31536000, immutable'
      fetch "$files/releases/$artifact?check=$(date +%s)" "$temporary/remote" 600
      cmp -s "$executable" "$temporary/remote" || die "releases/$artifact did not upload intact."
      ;;
    *) die "Could not inspect releases/$artifact (HTTP $status)." ;;
  esac
done < "$temporary/plan"
while read -r feed _ _; do
  sh "$release/r2-put.sh" "$feed" "$temporary/$feed" application/json no-cache
  fetch "$files/$feed?check=$(date +%s)" "$temporary/remote"
  cmp -s "$temporary/$feed" "$temporary/remote" || die "$feed did not publish intact."
done < "$temporary/plan"
