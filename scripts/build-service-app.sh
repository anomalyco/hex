#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
team_id=${VOICE_CONTROL_TEAM_ID:?Set VOICE_CONTROL_TEAM_ID to the Apple Developer signing team}
. "$root/scripts/macos-signing.sh"
identity=$(hex_codesign_identity "$team_id")
bundle="$root/target/HEX Service.app"
executable="$bundle/Contents/MacOS/hex-service"
version=${HEX_VERSION:-$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" | jq -r '.packages[0].version')}
build_number=${HEX_BUILD_NUMBER:-$(printf '%s\n' "$version" | awk -F. '{ print ($1 * 10000) + ($2 * 100) + $3 }')}

cargo build --release --manifest-path "$root/Cargo.toml"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS"
cp "$root/target/release/voice-control" "$executable"
cp "$root/app/ServiceInfo.plist" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $build_number" "$bundle/Contents/Info.plist"

codesign --force --timestamp --options runtime --sign "$identity" "$bundle"
codesign --verify --deep --strict --verbose=2 "$bundle"

echo "$bundle"
