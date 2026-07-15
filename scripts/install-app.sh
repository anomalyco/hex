#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
source_bundle=$("$root/scripts/build-app.sh")
install_dir="$HOME/Applications"
installed_bundle="$install_dir/HEX.app"
previous_bundle="$install_dir/Voice Control.app"

stop_bundle() {
  executable="$1/Contents/MacOS/voice-control-watch"
  if ! pgrep -f "$executable" >/dev/null; then
    return
  fi
  pkill -TERM -f "$executable" || true
  attempts=30
  while pgrep -f "$executable" >/dev/null; do
    if [ "$attempts" -eq 0 ]; then
      echo "Timed out waiting for $executable to stop." >&2
      exit 1
    fi
    attempts=$((attempts - 1))
    sleep 1
  done
}

mkdir -p "$install_dir"
stop_bundle "$installed_bundle"
stop_bundle "$previous_bundle"
rm -rf "$installed_bundle" "$previous_bundle"
/usr/bin/ditto "$source_bundle" "$installed_bundle"
/usr/bin/open "$installed_bundle"
echo "$installed_bundle"
