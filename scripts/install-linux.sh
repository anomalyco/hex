#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

cargo build --release
install -Dm755 target/release/voice-control "$HOME/.local/bin/hex"
install -Dm644 packaging/hex.desktop \
  "$HOME/.local/share/applications/hex.desktop"
install -Dm644 packaging/hex-autostart.desktop \
  "$HOME/.config/autostart/HEX.desktop"

printf 'Installed HEX to %s\n' "$HOME/.local/bin/hex"
printf 'Launch now with: hex app\n'
