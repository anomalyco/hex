#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

install_dir=${HEX_INSTALL_DIR:-"$HOME/.local/bin"}
binary="$install_dir/hex"

cargo build --release
install -Dm755 target/release/voice-control "$binary"
mkdir -p "$HOME/.local/share/applications" "$HOME/.config/autostart"
sed "s|@HEX_BIN@|$binary|g" packaging/hex.desktop \
  > "$HOME/.local/share/applications/hex.desktop"
sed "s|@HEX_BIN@|$binary|g" packaging/hex-autostart.desktop \
  > "$HOME/.config/autostart/HEX.desktop"

printf 'Installed HEX to %s\n' "$binary"
printf 'Launch now with: %s app\n' "$binary"
