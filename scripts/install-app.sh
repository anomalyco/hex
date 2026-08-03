#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
source_bundle=$("$root/scripts/build-app.sh")
install_dir="$HOME/Applications"
installed_bundle="$install_dir/$(basename "$source_bundle")"

stop_bundle() {
  bundle="$1"
  [ -d "$bundle" ] || return 0
  executable_name=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$bundle/Contents/Info.plist")
  executable="$bundle/Contents/MacOS/$executable_name"
  if ! pgrep -f "$executable" >/dev/null; then
    return 0
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
for previous_bundle in "$install_dir/HEX.app" "$install_dir/Hex.app" "$install_dir/Voice Control.app"; do
  if [ "$previous_bundle" != "$installed_bundle" ]; then
    stop_bundle "$previous_bundle"
    rm -rf "$previous_bundle"
  fi
done
rm -rf "$installed_bundle"
/usr/bin/ditto "$source_bundle" "$installed_bundle"
/usr/bin/open "$installed_bundle"
echo "$installed_bundle"
