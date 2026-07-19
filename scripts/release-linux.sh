#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mode=${1:-prepare}
version=${HEX_VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$root/Cargo.toml" | head -1)}
bucket=${HEX_RELEASE_BUCKET:-hex-releases}
base_url=https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev
dist="$root/dist"
target_dir=${CARGO_TARGET_DIR:-"$root/target"}
feed="$dist/linux-update.json"
expected_public_key=bfad02e62208ff144b5c9d21c7e79c7c16c6904299a437d857303007cd4ff7d8
public_key_prefix=302a300506032b6570032100

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Linux releases require $1." >&2
    exit 1
  fi
}
for command in cargo curl file flock jq openssl sha256sum sort xxd; do
  require "$command"
done
if [ "$mode" = publish ]; then
  require wrangler
fi
if [ "$(uname -s)" != Linux ] || [ "$(uname -m)" != x86_64 ]; then
  echo "Linux releases must be built on x86_64 Linux." >&2
  exit 1
fi
if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "Stable Linux releases require a non-prerelease semantic version." >&2
  exit 1
fi

mkdir -p "$dist"
exec 9> "$dist/.release-linux.lock"
if ! flock -n 9; then
  echo "Another Linux release is in progress on this host." >&2
  exit 1
fi
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
payload="$temporary/payload.json"
signature="$temporary/signature"
public_der="$temporary/public.der"
public_pem="$temporary/public.pem"
printf '%s%s' "$public_key_prefix" "$expected_public_key" | xxd -r -p > "$public_der"
openssl pkey -pubin -inform DER -in "$public_der" -out "$public_pem"

verify_signed_feed() {
  feed_path=$1
  payload_path=$2
  signature_path=$3
  jq -er .payload "$feed_path" | openssl base64 -d -A > "$payload_path"
  jq -er .signature "$feed_path" | openssl base64 -d -A > "$signature_path"
  openssl pkeyutl -verify -rawin -pubin -inkey "$public_pem" \
    -in "$payload_path" -sigfile "$signature_path" >/dev/null
}

verify_prepared() {
  verify_signed_feed "$feed" "$payload" "$signature"
  artifact=$(jq -r .artifact "$payload")
  artifact_path="$dist/$artifact"
  test -x "$artifact_path"
  test "$(jq -r .schema_version "$payload")" = 1
  test "$(jq -r .channel "$payload")" = stable
  test "$(jq -r .target "$payload")" = x86_64-unknown-linux-gnu
  test "$(jq -r .version "$payload")" = "$version"
  bytes=$(wc -c < "$artifact_path" | tr -d ' ')
  sha256=$(sha256sum "$artifact_path" | awk '{ print $1 }')
  test "$artifact" = "HEX-$version-$sha256-x86_64-linux"
  test "$(jq -r .bytes "$payload")" = "$bytes"
  test "$(jq -r .sha256 "$payload")" = "$sha256"
  file "$artifact_path" | grep -E 'ELF 64-bit.*x86-64' >/dev/null
  reported_version=$("$artifact_path" --version | awk 'NF { value=$NF } END { print value }')
  test "$reported_version" = "$version"
}

published_version() {
  remote_feed="$temporary/remote-linux-update.json"
  status=$(curl --silent --show-error --proto '=https' --proto-redir '=https' \
    --header 'Cache-Control: no-cache' --write-out '%{http_code}' \
    --output "$remote_feed" "$base_url/linux-update.json")
  case "$status" in
    200)
      remote_payload="$temporary/remote-payload.json"
      remote_signature="$temporary/remote-signature"
      verify_signed_feed "$remote_feed" "$remote_payload" "$remote_signature"
      test "$(jq -r .schema_version "$remote_payload")" = 1
      test "$(jq -r .channel "$remote_payload")" = stable
      test "$(jq -r .target "$remote_payload")" = x86_64-unknown-linux-gnu
      remote_version=$(jq -r .version "$remote_payload")
      printf '%s\n' "$remote_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'
      printf '%s\n' "$remote_version"
      ;;
    404) ;;
    *)
      echo "Could not inspect the published Linux feed (HTTP $status)." >&2
      exit 1
      ;;
  esac
}

require_newer_than_published() {
  current=$(published_version)
  if [ -n "$current" ]; then
    newest=$(printf '%s\n%s\n' "$current" "$version" | sort -V | tail -1)
    if [ "$current" = "$version" ] || [ "$newest" != "$version" ]; then
      echo "Refusing to replace Linux stable $current with $version." >&2
      exit 1
    fi
  fi
}

if [ "$mode" = publish ]; then
  if [ ! -f "$feed" ]; then
    echo "Prepare and validate version $version before publishing it." >&2
    exit 1
  fi
  verify_prepared
  require_newer_than_published
  wrangler r2 object put "$bucket/releases/$artifact" --remote \
    --file "$artifact_path" \
    --content-type application/octet-stream
  remote_artifact="$temporary/$artifact"
  curl --fail --silent --show-error --proto '=https' --proto-redir '=https' \
    "$base_url/releases/$artifact" -o "$remote_artifact"
  cmp -s "$artifact_path" "$remote_artifact"
  # Recheck immediately before publishing the only mutable object.
  require_newer_than_published
  wrangler r2 object put "$bucket/linux-update.json" --remote \
    --file "$feed" \
    --content-type application/json
  curl --fail --silent --show-error --proto '=https' --proto-redir '=https' \
    --header 'Cache-Control: no-cache' "$base_url/linux-update.json" \
    -o "$temporary/published-linux-update.json"
  cmp -s "$feed" "$temporary/published-linux-update.json"
  echo "$base_url/releases/$artifact"
  exit 0
fi
if [ "$mode" != prepare ]; then
  echo "Usage: $0 [prepare|publish]" >&2
  exit 1
fi
if [ -z "${HEX_LINUX_SIGNING_KEY:-}" ]; then
  echo "Set HEX_LINUX_SIGNING_KEY to the Ed25519 private PEM." >&2
  exit 1
fi

key="$temporary/private.pem"
printf '%s\n' "$HEX_LINUX_SIGNING_KEY" > "$key"
chmod 600 "$key"
actual_public_key=$(openssl pkey -in "$key" -pubout -outform DER | tail -c 32 | xxd -p -c 64)
if [ "$actual_public_key" != "$expected_public_key" ]; then
  echo "HEX_LINUX_SIGNING_KEY does not match the public key embedded in HEX." >&2
  exit 1
fi

cargo build --locked --release --manifest-path "$root/Cargo.toml"
built="$target_dir/release/voice-control"
sha256=$(sha256sum "$built" | awk '{ print $1 }')
artifact="HEX-$version-$sha256-x86_64-linux"
artifact_path="$dist/$artifact"
install -m755 "$built" "$artifact_path"
bytes=$(wc -c < "$artifact_path" | tr -d ' ')
if [ "$bytes" -gt 536870912 ]; then
  echo "The Linux artifact exceeds the updater's 512 MiB limit." >&2
  exit 1
fi
jq -cn \
  --arg version "$version" \
  --arg artifact "$artifact" \
  --arg sha256 "$sha256" \
  --argjson bytes "$bytes" \
  '{schema_version:1,channel:"stable",target:"x86_64-unknown-linux-gnu",version:$version,artifact:$artifact,bytes:$bytes,sha256:$sha256}' > "$payload"
openssl pkeyutl -sign -rawin -inkey "$key" -in "$payload" -out "$signature"
payload_base64=$(openssl base64 -A -in "$payload")
signature_base64=$(openssl base64 -A -in "$signature")
jq -n \
  --arg payload "$payload_base64" \
  --arg signature "$signature_base64" \
  '{payload:$payload,signature:$signature}' > "$feed"

verify_prepared
echo "$artifact_path"
echo "$feed"
echo "Prepared but not published. Run $0 publish after validation." >&2
