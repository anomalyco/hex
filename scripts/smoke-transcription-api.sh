#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
support="$HOME/Library/Application Support/voice-control"
discovery="$support/local-api.json"
model=${HEX_SMOKE_MODEL:-parakeet_unified_en}
audio="${TMPDIR:-/tmp}/hex-transcription-smoke.wav"
events="${TMPDIR:-/tmp}/hex-transcription-prepare.ndjson"
service_pid=

cleanup() {
  if [ -n "$service_pid" ] && kill -0 "$service_pid" 2>/dev/null; then
    kill -INT "$service_pid" 2>/dev/null || true
    shutdown_attempts=100
    while kill -0 "$service_pid" 2>/dev/null && [ "$shutdown_attempts" -gt 0 ]; do
      shutdown_attempts=$((shutdown_attempts - 1))
      sleep 0.05
    done
    if kill -0 "$service_pid" 2>/dev/null; then
      kill -TERM "$service_pid" 2>/dev/null || true
      sleep 0.1
    fi
    if kill -0 "$service_pid" 2>/dev/null; then
      kill -KILL "$service_pid" 2>/dev/null || true
    fi
  fi
  if [ -n "$service_pid" ]; then
    wait "$service_pid" 2>/dev/null || true
  fi
  rm -f "$audio" "$audio.aiff" "$events"
}
trap cleanup EXIT INT TERM

if [ -e "$discovery" ]; then
  echo "Refusing to replace an existing local API discovery file: $discovery" >&2
  exit 1
fi

cargo build --manifest-path "$root/Cargo.toml"
"$root/target/debug/voice-control" service >/dev/null 2>&1 &
service_pid=$!

attempts=100
while [ ! -f "$discovery" ]; do
  attempts=$((attempts - 1))
  if [ "$attempts" -eq 0 ]; then
    echo "Timed out waiting for local API discovery." >&2
    exit 1
  fi
  sleep 0.05
done
port=$(jq -r .port "$discovery")
token=$(jq -r .token "$discovery")

curl --fail --silent --show-error --no-buffer \
  -X POST \
  -H "Authorization: Bearer $token" \
  "http://127.0.0.1:$port/models/$model/prepare" > "$events"
grep -Fq '"type":"ok"' "$events"

/usr/bin/say "Hello from the local HEX transcription service." -o "$audio.aiff"
/usr/bin/afconvert -f WAVE -d LEI16@16000 "$audio.aiff" "$audio"
rm -f "$audio.aiff"

result=$(curl --fail --silent --show-error \
  -X POST \
  -H "Authorization: Bearer $token" \
  -H "Content-Type: audio/wav" \
  --data-binary "@$audio" \
  "http://127.0.0.1:$port/transcriptions?model=$model&language=en")
printf '%s\n' "$result" | jq -e '.transcript | length > 0' >/dev/null
printf '%s\n' "$result" | jq .
