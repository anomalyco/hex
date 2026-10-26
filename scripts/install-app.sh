#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
source_bundle=$("$root/scripts/build-app.sh")
install_dir="/Applications"
installed_bundle="$install_dir/$(basename "$source_bundle")"

stop_bundle() {
  bundle="$1"
  [ -d "$bundle" ] || return 0
  executable_name=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$bundle/Contents/Info.plist")
  executable="$bundle/Contents/MacOS/$executable_name"
  if ! LC_ALL=C pgrep -f "$executable" >/dev/null; then
    return 0
  fi
  LC_ALL=C pkill -TERM -f "$executable" || true
  attempts=30
  while LC_ALL=C pgrep -f "$executable" >/dev/null; do
    if [ "$attempts" -eq 0 ]; then
      echo "Timed out waiting for $executable to stop." >&2
      exit 1
    fi
    attempts=$((attempts - 1))
    sleep 1
  done
}

mkdir -p "$install_dir"
if [ -e "$installed_bundle" ] || [ -L "$installed_bundle" ]; then
  existing_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$installed_bundle/Contents/Info.plist" 2>/dev/null || true)
  # The Kit-signed com.kitlangton.hex2 build is the same Rust app under its old identity.
  case "$existing_id" in
    anomaly.hex | anomaly.hex.test | com.kitlangton.hex2) ;;
    *)
      echo "Refusing to replace $installed_bundle: it is not the Rust Hex app. Install the new app manually." >&2
      exit 1
      ;;
  esac
fi
stop_bundle "$installed_bundle"
rm -rf "$installed_bundle"
/usr/bin/ditto "$source_bundle" "$installed_bundle"
/usr/bin/open "$installed_bundle"
echo "$installed_bundle"
