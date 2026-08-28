#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} != --inside ]]; then
  if [[ $# != 3 || $(id -u) == 0 ]]; then
    echo "Usage: $0 /path/to/voice-control /path/to/model.gguf /path/to/jfk.wav (non-root)" >&2
    exit 1
  fi
  for command in cc pkg-config pulseaudio pactl paplay xdotool xvfb-run timeout; do
    command -v "$command" >/dev/null
  done
  exec timeout --kill-after=10 180 xvfb-run -a --server-args="-screen 0 1280x900x24 -noreset" \
    "$0" --inside "$(realpath "$1")" "$(realpath "$2")" "$(realpath "$3")"
fi
shift
binary=$1
model=$2
audio=$3
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
listener=
target=
pulse=
finish() {
  status=$?
  trap - EXIT INT TERM
  for pid in "$listener" "$target" "$pulse"; do
    if [[ -n "$pid" ]]; then kill "$pid" 2>/dev/null || true; fi
  done
  for ((attempt = 0; attempt < 100; attempt++)); do
    if [[ -z $(jobs -pr) ]]; then break; fi
    sleep 0.05
  done
  for pid in $(jobs -pr); do kill -KILL "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  if [[ $status != 0 ]]; then
    for log in "$work"/*.log "$work"/support/logs/live.ndjson; do
      if [[ -f "$log" ]]; then printf '\n%s\n' "$log"; cat "$log"; fi
    done
  fi
  rm -rf -- "$work"
  exit "$status"
}
trap finish EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
export HOME="$work/home" XDG_RUNTIME_DIR="$work/runtime"
export HEX_APPLICATION_SUPPORT_DIR="$work/support"
export PULSE_SERVER="unix:$work/pulse" PULSE_SOURCE=hex-fixture.monitor
# GPUI still needs software Vulkan; only inference uses the CPU in this fixture.
export GDK_BACKEND=x11 GGML_VK_VISIBLE_DEVICES=""
unset WAYLAND_DISPLAY VK_ICD_FILENAMES
mkdir -m 700 "$HOME" "$XDG_RUNTIME_DIR" "$HEX_APPLICATION_SUPPORT_DIR"
mkdir "$HEX_APPLICATION_SUPPORT_DIR/models"
ln -s "$model" "$HEX_APPLICATION_SUPPORT_DIR/models/$(basename "$model")"
printf 'pcm.!default { type pulse server "%s" }\n' "$PULSE_SERVER" > "$HOME/.asoundrc"
pulseaudio -n --daemonize=no --use-pid-file=no --exit-idle-time=-1 \
  --load="module-native-protocol-unix socket=$work/pulse auth-anonymous=1" \
  --load='module-null-sink sink_name=hex-fixture rate=16000 channels=1' \
  >"$work/pulse.log" 2>&1 &
pulse=$!
for ((attempt = 0; attempt < 100; attempt++)); do
  if pactl info >/dev/null 2>&1; then break; fi
  sleep 0.05
done
pactl info >/dev/null
cc "$root/tests/fixtures/wayland-paste-target.c" $(pkg-config --cflags --libs gtk+-3.0) -o "$work/target"
"$work/target" "$work/pasted.txt" >"$work/target.log" 2>&1 &
target=$!
window=$(xdotool search --sync --name '^Hex Wayland Test$' | head -n 1)
xdotool windowfocus --sync "$window"
"$binary" app --hidden >"$work/listener.log" 2>&1 &
listener=$!
echo 'Waiting for the model and virtual microphone...'
for ((attempt = 0; attempt < 1200; attempt++)); do
  kill -0 "$listener"
  if grep -q 'HEX dictation is ready' "$work/listener.log"; then break; fi
  sleep 0.05
done
grep -q 'HEX dictation is ready' "$work/listener.log"
xdotool search --onlyvisible --name '^HEX$' >/dev/null
xdotool windowfocus --sync "$window"
echo 'Replaying speech through the held dictation shortcut...'
xdotool keydown Alt_L keydown space
sleep 0.5
paplay --device=hex-fixture "$audio"
sleep 0.5
xdotool keyup space keyup Alt_L
echo 'Waiting for transcribed text in the focused GTK target...'
for ((attempt = 0; attempt < 1800; attempt++)); do
  kill -0 "$listener"
  if [[ -s "$work/pasted.txt" ]]; then break; fi
  sleep 0.05
done
grep -qi 'ask not what your country can do for you' "$work/pasted.txt"
grep -qi 'ask what you can do for your country' "$work/pasted.txt"
printf 'Captured, transcribed, and pasted: %s\n' "$(<"$work/pasted.txt")"
kill -TERM "$listener"
wait "$listener"
listener=
outputs=$(pactl list short source-outputs)
test -z "$outputs"
echo 'Linux virtual-microphone dictation and orderly shutdown passed.'
