#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bundle="$root/target/HEX.app"
executable="$bundle/Contents/MacOS/voice-control-watch"
frameworks="$bundle/Contents/Frameworks"
icon_output="$root/target/AppIcon.assets"
icon_info="$root/target/AppIcon-info.plist"
sparkle_dir=$("$root/scripts/setup-sparkle.sh")
version=${HEX_VERSION:-$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" | jq -r '.packages[0].version')}
build_number=${HEX_BUILD_NUMBER:-$(printf '%s\n' "$version" | awk -F. '{ print ($1 * 10000) + ($2 * 100) + $3 }')}

cargo build --release --manifest-path "$root/Cargo.toml"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$frameworks"
cp "$root/target/release/voice-control" "$executable"
cp "$root/app/Info.plist" "$bundle/Contents/Info.plist"
(cd "$root/sdk/commands" && bun run build)
mkdir -p "$bundle/Contents/Resources/commands-sdk"
cp "$root/sdk/commands/package.json" "$bundle/Contents/Resources/commands-sdk/"
/usr/bin/ditto "$root/sdk/commands/dist" "$bundle/Contents/Resources/commands-sdk/dist"
/usr/bin/ditto "$root/sdk/commands/workspace-template" "$bundle/Contents/Resources/commands-sdk/workspace-template"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $build_number" "$bundle/Contents/Info.plist"
rm -rf "$icon_output"
mkdir -p "$icon_output"
xcrun actool "$root/app/AppIcon.icon" \
  --compile "$icon_output" \
  --platform macosx \
  --minimum-deployment-target 15.0 \
  --app-icon AppIcon \
  --standalone-icon-behavior all \
  --output-partial-info-plist "$icon_info" \
  --output-format human-readable-text \
  --warnings \
  --notices >&2
cp "$icon_output/AppIcon.icns" "$icon_output/Assets.car" "$bundle/Contents/Resources/"

set -- "$root"/.venv/lib/python*/site-packages/moonshine_voice
moonshine_dir=$1
if [ ! -d "$moonshine_dir" ]; then
  echo "Moonshine is not installed. Run ./scripts/setup.sh." >&2
  exit 1
fi
cp "$moonshine_dir/libmoonshine.dylib" "$frameworks/"
cp "$moonshine_dir/libonnxruntime.1.23.2.dylib" "$frameworks/"
/usr/bin/ditto "$sparkle_dir/Sparkle.framework" "$frameworks/Sparkle.framework"

identity=${VOICE_CONTROL_CODESIGN_IDENTITY:-}
team_id=${VOICE_CONTROL_TEAM_ID:-QC99C9JE59}
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

sparkle="$frameworks/Sparkle.framework/Versions/B"
codesign --force --timestamp --options runtime --sign "$identity" "$sparkle/XPCServices/Installer.xpc"
codesign --force --timestamp --options runtime --preserve-metadata=entitlements --sign "$identity" "$sparkle/XPCServices/Downloader.xpc"
codesign --force --timestamp --options runtime --sign "$identity" "$sparkle/Autoupdate"
codesign --force --timestamp --options runtime --sign "$identity" "$sparkle/Updater.app"
codesign --force --timestamp --options runtime --sign "$identity" "$frameworks/Sparkle.framework"
codesign --force --timestamp --options runtime --sign "$identity" "$frameworks/libonnxruntime.1.23.2.dylib"
codesign --force --timestamp --options runtime --sign "$identity" "$frameworks/libmoonshine.dylib"
codesign --force --timestamp --options runtime --entitlements "$root/app/VoiceControl.entitlements" --sign "$identity" "$bundle"
codesign --verify --deep --strict --verbose=2 "$bundle"
echo "$bundle"
