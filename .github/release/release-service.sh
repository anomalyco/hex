#!/bin/sh
# Publishes Hex to the Anomaly release service with curl and the workflow's
# GitHub OIDC token (anomalyco/release docs/publishing.md). Names follow
# docs/plans/release-service.md. Runs only in the Release workflow's publish
# jobs, which have `id-token: write`.
#
#   release-service.sh publish <dist> <notes-url>   # upload every target, then the release
#   release-service.sh distribution <kind> <version>  # a package manager now serves <version>
#
# RELEASE is the service origin, https://anoma.ly/release (its dev stage only
# tests the service itself); HEX_VERSION the version the files were built
# as and name (staging builds X.Y.Z-dev.<run> for the dev channel);
# HEX_RELEASE_CHANNEL the channel (default stable). `publish` writes
# <dist>/manifest.json after checking every file the service serves against
# the local bytes. Publishing the channel's current version again succeeds
# only when the service already has exactly these files, so a rerun is safe.
set -eu

: "${RELEASE:?Set RELEASE to the release service origin}"
: "${HEX_VERSION:?Set HEX_VERSION}"
channel=${HEX_RELEASE_CHANNEL:-stable}
release_version=$HEX_VERSION
app=hex
artifact=desktop
# One request stays under Cloudflare's 100 MB limit; bigger files go in parts.
single_limit=$((95 * 1024 * 1024))
part_size=90m

case "$RELEASE" in
  https://anoma.ly/release) ;;
  *)
    echo "RELEASE must be https://anoma.ly/release." >&2
    exit 1
    ;;
esac
for command in curl jq sha256sum split; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Publishing requires $command." >&2
    exit 1
  }
done
: "${ACTIONS_ID_TOKEN_REQUEST_URL:?Run in a GitHub Actions job with id-token: write}"
: "${ACTIONS_ID_TOKEN_REQUEST_TOKEN:?Run in a GitHub Actions job with id-token: write}"

temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

# The token stays in a curl config file, never in argv or the log.
token_config="$temporary/token.curl"
(
  umask 077
  token=$(curl -fsS -H "Authorization: Bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
    "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=$RELEASE" | jq -er .value)
  printf 'header = "Authorization: Bearer %s"\n' "$token" > "$token_config"
)

# request <curl arguments>: an authenticated request; prints the body and
# sets $status.
request() {
  status=$(curl -sS --config "$token_config" --retry 3 --retry-all-errors \
    -o "$temporary/response" -w '%{http_code}' "$@") || {
    echo "Request to the release service failed: $*" >&2
    exit 1
  }
  cat "$temporary/response"
}

# api <curl arguments>: a request that must succeed.
api() {
  request "$@"
  case "$status" in
    2??) ;;
    *)
      echo "The release service answered $status: $(cat "$temporary/response")" >&2
      exit 1
      ;;
  esac
}

# The package managers that mirror Hex, each as the service lists it.
distributions='[
  {"kind": "aur", "package": "hex-bin", "command": "yay -Syu hex-bin"},
  {"kind": "deb-x64", "package": "hex", "command": "curl -fLo /tmp/hex.deb https://anoma.ly/release/hex/desktop/stable/linux-x64-deb && sudo apt install /tmp/hex.deb"},
  {"kind": "deb-arm64", "package": "hex", "command": "curl -fLo /tmp/hex.deb https://anoma.ly/release/hex/desktop/stable/linux-arm64-deb && sudo apt install /tmp/hex.deb"},
  {"kind": "rpm-x64", "package": "hex", "command": "curl -fLo /tmp/hex.rpm https://anoma.ly/release/hex/desktop/stable/linux-x64-rpm && sudo dnf install /tmp/hex.rpm"},
  {"kind": "rpm-arm64", "package": "hex", "command": "curl -fLo /tmp/hex.rpm https://anoma.ly/release/hex/desktop/stable/linux-arm64-rpm && sudo dnf install /tmp/hex.rpm"},
  {"kind": "brew", "cask": "anomalyco/tap/hex", "command": "brew upgrade --cask anomalyco/tap/hex"},
  {"kind": "nix", "attribute": "github:anomalyco/hex", "command": "nix profile upgrade hex"}
]'

# resolve <url>: a target URL from the manifest, an absolute path on the
# service's host, as a full URL.
resolve() {
  case "$1" in
    /*) printf '%s%s\n' "${RELEASE%/release}" "$1" ;;
    *)
      echo "Unexpected target URL $1." >&2
      exit 1
      ;;
  esac
}

sha256_of() {
  sha256sum < "$1" | awk '{ print $1 }'
}

# upload <file> <name>: stores the file as <name> (the service's path is
# files/<sha256>/<name>, and downloads are saved as <name>), refused unless
# the service computes the same SHA-256.
upload() {
  file=$1
  name=$2
  sha256=$(sha256_of "$file")
  size=$(wc -c < "$file" | tr -d ' ')
  if [ "$size" -le "$single_limit" ]; then
    api -X PUT -H "x-checksum-sha256: $sha256" --data-binary @"$file" \
      "$RELEASE/$app/files/$name" > /dev/null
  else
    upload_id=$(api -X POST "$RELEASE/$app/files/$name?uploads" | jq -er .upload)
    rm -rf "$temporary/parts"
    mkdir "$temporary/parts"
    split -b "$part_size" "$file" "$temporary/parts/part."
    part=1
    for chunk in "$temporary"/parts/part.*; do
      api -X PUT --data-binary @"$chunk" "$RELEASE/$app/files/$name?upload=$upload_id&part=$part" > /dev/null
      part=$((part + 1))
    done
    api -X POST -H "x-checksum-sha256: $sha256" "$RELEASE/$app/files/$name?upload=$upload_id" > /dev/null
  fi
  echo "Uploaded $name ($size bytes, $sha256)." >&2
}

publish() {
  dist=$1
  notes_url=$2
  version=$HEX_VERSION
  manifest_url="$RELEASE/$app/$artifact/$channel.json"
  # target=local file=published name, as docs/plans/release-service.md pins.
  # Published names carry no version or target: the sha256 in the path
  # already tells files apart, and a download is saved under that name. A
  # run without a platform's signed build publishes the others.
  pinned='{}'
  for entry in \
    "macos-arm64=Hex-$version-macos-arm64.zip=Hex.zip" \
    "macos-arm64-dmg=Hex-$version-macos-arm64.dmg=Hex.dmg" \
    "windows-x64=Hex-$version-windows-x64.exe=hex.exe" \
    "windows-x64-setup=Hex-Setup-$version-windows-x64.exe=HexSetup.exe" \
    "windows-arm64=Hex-$version-windows-arm64.exe=hex.exe" \
    "windows-arm64-setup=Hex-Setup-$version-windows-arm64.exe=HexSetup.exe" \
    "linux-x64=hex-$version-linux-x64=hex" \
    "linux-x64-deb=hex-$version-linux-x64.deb=hex.deb" \
    "linux-x64-rpm=hex-$version-linux-x64.rpm=hex.rpm" \
    "linux-arm64=hex-$version-linux-arm64=hex" \
    "linux-arm64-deb=hex-$version-linux-arm64.deb=hex.deb" \
    "linux-arm64-rpm=hex-$version-linux-arm64.rpm=hex.rpm"
  do
    target=${entry%%=*}
    rest=${entry#*=}
    local_name=${rest%%=*}
    name=${rest#*=}
    [ -f "$dist/$local_name" ] || continue
    upload "$dist/$local_name" "$name"
    pinned=$(printf '%s' "$pinned" | jq --arg target "$target" --arg file "$name" \
      --arg local "$local_name" --arg sha256 "$(sha256_of "$dist/$local_name")" \
      '.[$target] = {file: $file, sha256: $sha256, local: $local}')
  done
  test "$(printf '%s' "$pinned" | jq 'length')" -gt 0
  jq -n \
    --arg channel "$channel" \
    --arg version "$release_version" \
    --arg notes_url "$notes_url" \
    --argjson targets "$(printf '%s' "$pinned" | jq 'map_values({file, sha256})')" \
    --argjson distributions "$distributions" \
    '{channel: $channel, version: $version, notes_url: $notes_url, targets: $targets,
      distributions: ([{kind: "direct"}] + $distributions)}' > "$temporary/release.json"
  request -H 'content-type: application/json' --data-binary @"$temporary/release.json" \
    "$RELEASE/$app/$artifact/releases" > "$dist/manifest.json"
  case "$status" in
    2??) ;;
    400)
      # A rerun: the channel already has this version; the checks below
      # require it to be exactly these files.
      grep -Fq "$release_version is already the current release" "$dist/manifest.json" || {
        echo "The release service answered 400: $(cat "$dist/manifest.json")" >&2
        exit 1
      }
      echo "$channel already has $release_version; checking that it is these files." >&2
      curl -fsS "$manifest_url" -o "$dist/manifest.json"
      ;;
    *)
      echo "The release service answered $status: $(cat "$dist/manifest.json")" >&2
      exit 1
      ;;
  esac

  # The service must serve exactly these bytes at the URLs it published.
  test "$(jq -r '"\(.app) \(.artifact) \(.channel) \(.release.version)"' "$dist/manifest.json")" = "$app $artifact $channel $release_version"
  for target in $(printf '%s' "$pinned" | jq -r 'keys[]'); do
    name=$(printf '%s' "$pinned" | jq -r --arg target "$target" '.[$target].local')
    expected=$(sha256_of "$dist/$name")
    entry=$(jq -c --arg target "$target" '.release.targets[$target]' "$dist/manifest.json")
    test "$(printf '%s' "$entry" | jq -r .sha256)" = "$expected"
    test "$(printf '%s' "$entry" | jq -r .size)" = "$(wc -c < "$dist/$name" | tr -d ' ')"
    url=$(resolve "$(printf '%s' "$entry" | jq -r .url)")
    curl -fsS --retry 3 "$url" -o "$temporary/served"
    test "$(sha256_of "$temporary/served")" = "$expected"
    echo "$target $url" >&2
  done
  curl -fsS "$manifest_url" | jq -e --arg version "$release_version" '.release.version == $version' > /dev/null
  echo "Published Hex $release_version to $manifest_url." >&2
}

distribution() {
  kind=$1
  version=$2
  # The whole entry, so recording a version never drops its command.
  printf '%s' "$distributions" | jq -e --arg kind "$kind" --arg version "$version" \
    'map(select(.kind == $kind))[0] // error("unknown distribution \($kind)") | . + {version: $version}' \
    > "$temporary/distribution.json"
  api -H 'content-type: application/json' --data-binary @"$temporary/distribution.json" \
    "$RELEASE/$app/$artifact/$channel/distribution" \
    | jq -e --arg kind "$kind" --arg version "$version" \
      '.release.distributions | any(.kind == $kind and .version == $version)' > /dev/null
  echo "Recorded that $kind serves Hex $version." >&2
}

case "${1:-}" in
  publish)
    [ $# -eq 3 ] || { echo "Usage: $0 publish <dist> <notes-url>" >&2; exit 1; }
    publish "$2" "$3"
    ;;
  distribution)
    [ $# -eq 3 ] || { echo "Usage: $0 distribution <kind> <version>" >&2; exit 1; }
    distribution "$2" "$3"
    ;;
  *)
    echo "Usage: $0 publish <dist> <notes-url> | distribution <kind> <version>" >&2
    exit 1
    ;;
esac
