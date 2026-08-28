#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == --inside ]]; then
  target=
  keyboard=
  app=
  finish() {
    status=$?
    trap - EXIT
    if [[ -n "$target" ]]; then kill "$target" 2>/dev/null || true; fi
    if [[ -n "$keyboard" ]]; then kill "$keyboard" 2>/dev/null || true; fi
    if [[ -n "$app" ]]; then kill "$app" 2>/dev/null || true; fi
    printf '%s\n' "$status" > "$HEX_WAYLAND_TEST_RESULT"
    swaymsg exit >/dev/null 2>&1 || true
    exit "$status"
  }
  trap finish EXIT

  # Seed the isolated compositor's keyboard state before any window exists.
  wtype -k v -p VoidSymbol -s 60000 &
  keyboard=$!
  for ((attempt = 0; attempt < 100; attempt++)); do
    if swaymsg -t get_inputs -r | grep -q wlr_virtual_keyboard; then break; fi
    sleep 0.05
  done
  "$HEX_WAYLAND_TEST_TARGET" "$HEX_WAYLAND_PASTE_OUTPUT" &
  target=$!
  for ((attempt = 0; attempt < 100; attempt++)); do
    if [[ -f "$HEX_WAYLAND_PASTE_OUTPUT.ready" ]]; then break; fi
    sleep 0.05
  done
  test -f "$HEX_WAYLAND_PASTE_OUTPUT.ready"
  "$HEX_WAYLAND_TEST_BINARY" --exact linux_paste::tests::native_wayland_clipboard_shortcut \
    --ignored --nocapture --test-threads=1
  kill "$target"
  wait "$target" 2>/dev/null || true
  target=

  # The app-window check uses compositor IPC, not keyboard input. Remove the
  # synthetic seat first rather than expose a transient, keymap-less keyboard.
  kill "$keyboard"
  wait "$keyboard" 2>/dev/null || true
  keyboard=
  for ((attempt = 0; attempt < 100; attempt++)); do
    if ! swaymsg -t get_inputs -r | grep wlr_virtual_keyboard >/dev/null; then break; fi
    sleep 0.05
  done
  "$HEX_WAYLAND_APP_BINARY" app --hidden >"$HEX_WAYLAND_APP_LOG" 2>&1 &
  app=$!
  for ((attempt = 0; attempt < 200; attempt++)); do
    kill -0 "$app"
    if swaymsg -t get_tree -r | grep '"name": "HEX"' >/dev/null; then break; fi
    sleep 0.05
  done
  swaymsg -t get_tree -r | grep '"name": "HEX"' >/dev/null
  swaymsg '[app_id="hex"] kill' >/dev/null
  wait "$app"
  app=
  exit 0
fi

if [[ $# != 2 || $(id -u) == 0 ]]; then
  echo "Usage: $0 /path/to/compiled-test /path/to/voice-control (run as an unprivileged user)" >&2
  exit 1
fi
for command in cc pkg-config sway swaymsg wtype wl-copy dbus-run-session timeout; do
  command -v "$command" >/dev/null
done
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
export HEX_WAYLAND_TEST_BINARY
HEX_WAYLAND_TEST_BINARY=$(realpath -- "$1")
export HEX_WAYLAND_APP_BINARY
HEX_WAYLAND_APP_BINARY=$(realpath -- "$2")
export HEX_WAYLAND_TEST_SCRIPT="$root/scripts/test-wayland-paste.sh"
export HEX_WAYLAND_TEST_TARGET="$work/target"
export HEX_WAYLAND_PASTE_OUTPUT="$work/pasted.txt"
export HEX_WAYLAND_TEST_RESULT="$work/result"
export HEX_WAYLAND_APP_LOG="$work/hex.log"
export XDG_RUNTIME_DIR="$work/runtime"
export XDG_DATA_HOME="$work/data"
mkdir -m 700 "$XDG_RUNTIME_DIR"
cc "$root/tests/fixtures/wayland-paste-target.c" $(pkg-config --cflags --libs gtk+-3.0) -o "$HEX_WAYLAND_TEST_TARGET"
unset DISPLAY WAYLAND_DISPLAY SWAYSOCK
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 GDK_BACKEND=wayland
if ! timeout 25 dbus-run-session -- sway --config "$root/tests/fixtures/wayland-paste.conf" >"$work/compositor.log" 2>&1 \
  || [[ ! -f "$work/result" || $(<"$work/result") != 0 ]]; then
  cat "$work/compositor.log"
  if [[ -f "$work/hex.log" ]]; then cat "$work/hex.log"; fi
  exit 1
fi
echo "Native Wayland clipboard insertion and tray-less app checks passed."
