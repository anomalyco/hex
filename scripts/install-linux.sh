#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

if ! command -v flock >/dev/null 2>&1; then
  echo "Linux installation requires flock from util-linux." >&2
  exit 1
fi
if [ "$(id -u)" -eq 0 ]; then
  echo "Install HEX as your desktop user, not root." >&2
  exit 1
fi

install_dir=${HEX_INSTALL_DIR:-"$HOME/.local/bin"}
binary="$install_dir/hex"
support_dir=${HEX_APPLICATION_SUPPORT_DIR:-"${XDG_DATA_HOME:-$HOME/.local/share}/voice-control"}
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
version_dir="$support_dir/versions/$version"
target_dir=${CARGO_TARGET_DIR:-target}

cargo build --locked --release
mkdir -p "$support_dir/updates"
chmod 700 "$support_dir" "$support_dir/updates"
exec 9> "$support_dir/updates/update.lock"
if ! flock -n 9; then
  echo "A HEX update is already in progress." >&2
  exit 1
fi
mkdir -p "$version_dir"
chmod 700 "$support_dir/versions" "$version_dir"
staged="$version_dir/.hex.install.$$"
trap 'rm -f "$staged"' EXIT HUP INT TERM
install -m755 "$target_dir/release/voice-control" "$staged"
mv -Tf "$staged" "$version_dir/hex"
mkdir -p "$install_dir"
rm -f "$support_dir/.current.next" "$install_dir/.hex.next"
ln -s "versions/$version" "$support_dir/.current.next"
mv -Tf "$support_dir/.current.next" "$support_dir/current"
ln -s "$support_dir/current/hex" "$install_dir/.hex.next"
mv -Tf "$install_dir/.hex.next" "$binary"
mkdir -p "$HOME/.local/share/applications" "$HOME/.config/autostart"
sed "s|@HEX_BIN@|$binary|g" packaging/hex.desktop \
  > "$HOME/.local/share/applications/hex.desktop"
sed "s|@HEX_BIN@|$binary|g" packaging/hex-autostart.desktop \
  > "$HOME/.config/autostart/HEX.desktop"

printf 'Installed HEX to %s\n' "$binary"
printf 'Launch now with: %s app\n' "$binary"
printf 'Signed updates will install automatically.\n'
