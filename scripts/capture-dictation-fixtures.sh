#!/usr/bin/env bash

set -euo pipefail

destination="${1:-$HOME/Library/Application Support/voice-control/test-fixtures/dictation-protocol}"
recording_pid=""

names=(
  period-stop
  question-stop
  exclamation-send
  comma-stop
)

prompts=(
  "Say, I don't understand this. Say stop."
  "Say, is this working? Say stop."
  "Say, ship it! Say send."
  "Say, meet me at five, say stop."
)

cleanup() {
  if [[ -n "$recording_pid" ]] && kill -0 "$recording_pid" 2>/dev/null; then
    kill -INT "$recording_pid" 2>/dev/null || true
    wait "$recording_pid" 2>/dev/null || true
  fi
}

confirm() {
  local prompt="$1"
  local default="${2:-yes}"
  local reply
  if [[ "$default" == "yes" ]]; then
    read -r -p "$prompt [Y/n] " reply
    [[ ! "$reply" =~ ^[Nn]$ ]]
  else
    read -r -p "$prompt [y/N] " reply
    [[ "$reply" =~ ^[Yy]$ ]]
  fi
}

trap cleanup EXIT INT TERM

for command in rec play soxi; do
  if ! command -v "$command" >/dev/null 2>&1; then
    printf 'Missing %s. Install SoX before recording fixtures.\n' "$command" >&2
    exit 1
  fi
done

mkdir -p "$destination"

printf '\nHEX dictation protocol fixture recorder\n'
printf 'Audio stays local in:\n  %s\n\n' "$destination"
printf 'Use a normal speaking pace. Leave a short pause before the final control.\n'

for index in "${!names[@]}"; do
  name="${names[$index]}"
  prompt="${prompts[$index]}"
  output="$destination/$name.wav"

  while true; do
    printf '\n[%s/%s] %s\n' "$((index + 1))" "${#names[@]}" "$name"
    printf 'Say exactly:\n  %s\n' "$prompt"
    read -r -p 'Press Enter to start recording. '
    printf '\aRecording. Press Enter when you finish speaking.\n'

    rm -f "$output"
    rec -q -c 1 -r 16000 -e floating-point -b 32 "$output" &
    recording_pid=$!
    read -r
    if kill -0 "$recording_pid" 2>/dev/null; then
      kill -INT "$recording_pid" 2>/dev/null || true
    fi
    wait "$recording_pid" 2>/dev/null || true
    recording_pid=""

    if [[ ! -s "$output" ]]; then
      printf 'Recording failed; no audio was written.\n' >&2
      continue
    fi

    duration="$(soxi -D "$output")"
    printf 'Recorded %s seconds.\n' "$duration"
    if confirm 'Play it back?'; then
      play -q "$output"
    fi
    if confirm 'Keep this recording?'; then
      break
    fi
  done
done

printf '\nCaptured %s fixtures.\n' "${#names[@]}"
printf 'Run the fixture test with:\n\n'
printf '  VOICE_CONTROL_DICTATION_FIXTURE_DIR=%q cargo test dictation_protocol_audio -- --ignored --nocapture\n' "$destination"
