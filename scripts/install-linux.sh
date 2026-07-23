#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
installer="$root/scripts/install-linux-release.sh"

if [ "${1:-install}" = uninstall ]; then
  exec "$installer" uninstall
fi
if [ "${1:-install}" != install ]; then
  echo "Usage: $0 [install|uninstall]" >&2
  exit 1
fi

version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$root/Cargo.toml" | head -1)
target_dir=${CARGO_TARGET_DIR:-"$root/target"}
cargo build --locked --release --manifest-path "$root/Cargo.toml"

HEX_INSTALL_ARTIFACT="$target_dir/release/voice-control" \
HEX_INSTALL_VERSION="$version" \
  exec "$installer"
