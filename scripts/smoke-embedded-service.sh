#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
runtime=$(mktemp -d "${TMPDIR:-/tmp}/hex-embedded.XXXXXX")
lease="$runtime/host-lease"
endpoint="$runtime/endpoint.json"
stderr="$runtime/service.log"
pid=

cleanup() {
  exec 3>&- 2>/dev/null || true
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi
  rm -rf "$runtime"
}
trap cleanup EXIT INT TERM

cargo build --manifest-path "$root/Cargo.toml"
mkfifo "$lease"
HEX_APPLICATION_SUPPORT_DIR="$runtime/support" \
  "$root/target/debug/voice-control" service --embedded \
  <"$lease" >"$endpoint" 2>"$stderr" &
pid=$!
exec 3>"$lease"

attempts=200
while [ ! -s "$endpoint" ]; do
  attempts=$((attempts - 1))
  if [ "$attempts" -eq 0 ] || ! kill -0 "$pid" 2>/dev/null; then
    cat "$stderr" >&2
    echo "Timed out waiting for the embedded service handshake." >&2
    exit 1
  fi
  sleep 0.05
done

type=$(jq -r .type "$endpoint")
url=$(jq -r .url "$endpoint")
token=$(jq -r .token "$endpoint")
api_version=$(jq -r .apiVersion "$endpoint")
reported_pid=$(jq -r .pid "$endpoint")
if [ "$type" != "ready" ] || [ "$api_version" != "1" ] || [ "$reported_pid" != "$pid" ]; then
  echo "Invalid embedded service handshake:" >&2
  cat "$endpoint" >&2
  exit 1
fi
if [ -e "$runtime/support/local-api.json" ]; then
  echo "Embedded service unexpectedly published a discovery file." >&2
  exit 1
fi

health=$(curl --fail --silent --show-error \
  -H "Authorization: Bearer $token" \
  "$url/health")
models=$(curl --fail --silent --show-error \
  -H "Authorization: Bearer $token" \
  "$url/models")
jq -e '.apiVersion == "1" and .version != null' >/dev/null <<EOF
$health
EOF
jq -e 'length > 0 and .[0].id == "parakeet_v2"' >/dev/null <<EOF
$models
EOF

exec 3>&-
attempts=200
while kill -0 "$pid" 2>/dev/null; do
  attempts=$((attempts - 1))
  if [ "$attempts" -eq 0 ]; then
    cat "$stderr" >&2
    echo "Embedded service did not stop when the host lease closed." >&2
    exit 1
  fi
  sleep 0.05
done
wait "$pid"
pid=

echo "Embedded service smoke test passed."
