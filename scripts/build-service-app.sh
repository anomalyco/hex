#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bundle="$root/target/HEX Service.app"
executable="$bundle/Contents/MacOS/hex-service"
version=${HEX_VERSION:-$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" | jq -r '.packages[0].version')}
build_number=${HEX_BUILD_NUMBER:-$(printf '%s\n' "$version" | awk -F. '{ print ($1 * 10000) + ($2 * 100) + $3 }')}
team_id=${VOICE_CONTROL_TEAM_ID:-QC99C9JE59}
identity=${VOICE_CONTROL_CODESIGN_IDENTITY:-}

cargo build --release --manifest-path "$root/Cargo.toml"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS"
cp "$root/target/release/voice-control" "$executable"
cp "$root/app/ServiceInfo.plist" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $build_number" "$bundle/Contents/Info.plist"

if [ -z "$identity" ]; then
  identity=$(security find-identity -v -p codesigning | sed -n "s/.*\"\(Developer ID Application:.*($team_id)\)\"/\1/p" | head -1)
fi
if [ -z "$identity" ]; then
  echo "No Developer ID signing identity found for team $team_id. Set VOICE_CONTROL_CODESIGN_IDENTITY." >&2
  exit 1
fi
case "$identity" in
  *"($team_id)"*) ;;
  *)
    echo "Signing identity does not belong to expected team $team_id: $identity" >&2
    exit 1
    ;;
esac

codesign --force --timestamp --options runtime --sign "$identity" "$bundle"
codesign --verify --deep --strict --verbose=2 "$bundle"

echo "$bundle"
