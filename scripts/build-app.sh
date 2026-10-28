#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$root/scripts/macos-signing.sh"
# HEX_TEST_BUILD=1 builds the CI test app: ad-hoc signed, under its own
# name and bundle ID so it installs beside the released Hex and never
# touches its permissions; it never checks for updates.
# HEX_RELEASE_UNSIGNED=1 builds the release app (Hex, anomaly.hex) ad-hoc
# signed: the Release workflow's build job, which holds no credentials. Its
# sign-macos job signs it with the Developer ID (.github/release/sign-macos.sh).
if [ "${HEX_RELEASE_UNSIGNED:-}" = 1 ]; then
  team_id=
  identity=-
  app_name=Hex
  bundle_id=anomaly.hex
elif [ "${HEX_TEST_BUILD:-}" = 1 ]; then
  team_id=
  identity=-
  app_name="Hex Test"
  bundle_id=anomaly.hex.test
else
  team_id=${VOICE_CONTROL_TEAM_ID:?Set VOICE_CONTROL_TEAM_ID to the Apple Developer signing team}
  identity=$(hex_codesign_identity "$team_id")
  app_name=Hex
  bundle_id=anomaly.hex
fi
# HEX_CAPTURE=1 builds the `capture` experiment in (`--features capture`)
# with Scrawl's `scrawl-helper` beside `hex`, where the engine looks for
# it. crates/capture/build.rs compiles the helper from crates/capture/helper
# into target/release. Never for a public build: release-app.sh and
# validate-app.sh refuse it.
capture=${HEX_CAPTURE:-}
case "$capture" in
  '' | 0) capture= ;;
  1) ;;
  *)
    echo "HEX_CAPTURE must be 1 or unset" >&2
    exit 1
    ;;
esac
icon_output="$root/target/AppIcon.assets"
icon_info="$root/target/AppIcon-info.plist"
version=$(hex_version "$root")
build_number=$(hex_build_number "$version")
bundle_name="$app_name.app"
executable_name=hex

bundle="$root/target/app/$bundle_name"
executable="$bundle/Contents/MacOS/$executable_name"
frameworks="$bundle/Contents/Frameworks"

sign() {
  if [ "$identity" = - ]; then
    codesign --force --sign - "$@"
  else
    codesign --force --timestamp --options runtime --sign "$identity" "$@"
  fi
}

export MACOSX_DEPLOYMENT_TARGET=15.0
if [ -n "$capture" ]; then
  # The helper's embedded Info.plist names Hex, not Scrawl.
  SCRAWL_HELPER_PLIST="$root/packaging/macos/scrawl-helper-Info.plist" \
    cargo build --locked --release --features capture --manifest-path "$root/Cargo.toml"
else
  cargo build --locked --release --manifest-path "$root/Cargo.toml"
fi
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$bundle/Contents/Library" "$frameworks"
cp "$root/target/release/voice-control" "$executable"
if [ -n "$capture" ]; then
  cp "$root/target/release/scrawl-helper" "$bundle/Contents/MacOS/scrawl-helper"
fi
cp "$root/app/Info.plist" "$bundle/Contents/Info.plist"
cp "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$bundle/Contents/Resources/"
HEX_VERSION="$version" HEX_BUILD_NUMBER="$build_number" VOICE_CONTROL_TEAM_ID="$team_id" VOICE_CONTROL_CODESIGN_IDENTITY="${identity#-}" HEX_SKIP_INSTALL_SCRIPT=1 \
  "$root/scripts/build-hex-microphone-driver.sh" "$bundle/Contents/Library" >&2
(cd "$root/sdk/commands" && bun run build)
mkdir -p "$bundle/Contents/Resources/commands-sdk"
cp "$root/sdk/commands/package.json" "$bundle/Contents/Resources/commands-sdk/"
/usr/bin/ditto "$root/sdk/commands/dist" "$bundle/Contents/Resources/commands-sdk/dist"
/usr/bin/ditto "$root/sdk/commands/workspace-template" "$bundle/Contents/Resources/commands-sdk/workspace-template"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $build_number" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $bundle_id" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleExecutable $executable_name" "$bundle/Contents/Info.plist"
plutil -replace CFBundleName -string "$app_name" "$bundle/Contents/Info.plist"
plutil -replace CFBundleDisplayName -string "$app_name" "$bundle/Contents/Info.plist"
if [ -n "$capture" ]; then
  # The helper mirrors a cabled iPhone as a camera; macOS asks on Hex's behalf.
  plutil -replace NSCameraUsageDescription -string "Hex mirrors and records a cabled iPhone's screen for a capture." "$bundle/Contents/Info.plist"
fi
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
sign "$frameworks/libonnxruntime.1.23.2.dylib"
sign "$frameworks/libmoonshine.dylib"
sign "$bundle/Contents/Library/HexMicrophone.driver"
if [ -n "$capture" ]; then
  # Hex runs it only to decode the capture's movie, which needs no
  # entitlement; anything it touches is attributed to Hex, its responsible
  # process.
  sign --identifier "$bundle_id.scrawl-helper" "$bundle/Contents/MacOS/scrawl-helper"
fi
sign --entitlements "$root/.github/release/hex.entitlements" "$bundle"
codesign --verify --deep --strict --verbose=2 "$bundle"
echo "$bundle"
