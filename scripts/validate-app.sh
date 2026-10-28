#!/bin/sh
set -eu

if [ "$#" -ne 4 ]; then
  echo "Usage: $0 <Hex.app path> <expected bundle name> <version> <build>" >&2
  exit 1
fi

bundle=$1
expected_name=$2
version=$3
build_number=$4
team_id=${HEX_TEAM_ID:?Set HEX_TEAM_ID to the Apple Developer signing team}
plist="$bundle/Contents/Info.plist"
# Logged only by a build with the `capture` feature (src/capture_host.rs).
capture_marker='capture experiment on: `hex capture` is available'

fail() {
  echo "Rust Hex bundle validation failed: $*" >&2
  exit 1
}

plist_value() {
  /usr/libexec/PlistBuddy -c "Print :$1" "$plist"
}

[ -d "$bundle" ] || fail "$bundle does not exist"
[ "$(basename "$bundle")" = "$expected_name" ] || fail "expected $expected_name, found $(basename "$bundle")"
[ "$(plist_value CFBundleIdentifier)" = anomaly.hex ] || fail "wrong bundle identifier"
[ "$(plist_value CFBundleExecutable)" = hex ] || fail "wrong executable name"
[ "$(plist_value CFBundleDisplayName)" = Hex ] || fail "wrong display name"
[ "$(plist_value CFBundleName)" = Hex ] || fail "wrong bundle name"
[ "$(plist_value CFBundleShortVersionString)" = "$version" ] || fail "wrong version"
[ "$(plist_value CFBundleVersion)" = "$build_number" ] || fail "wrong build number"
[ "$(plist_value LSMinimumSystemVersion)" = 15.0 ] || fail "wrong minimum macOS"
# Hex updates itself through the Anomaly release service; Sparkle is gone.
! plist_value SUFeedURL >/dev/null 2>&1 || fail "a Sparkle feed is still configured"
[ ! -e "$bundle/Contents/Frameworks/Sparkle.framework" ] || fail "Sparkle.framework is still bundled"
[ "$(plist_value LSUIElement)" = true ] || fail "menu-bar application policy is disabled"
# The capture build option is never released (scripts/build-app.sh HEX_CAPTURE).
[ ! -e "$bundle/Contents/MacOS/scrawl-helper" ] || fail "built with the capture build option: scrawl-helper is bundled"
! plist_value NSCameraUsageDescription >/dev/null 2>&1 || fail "built with the capture build option: Info.plist asks for the camera"
[ -x "$bundle/Contents/MacOS/hex" ] || fail "packaged executable is missing"
! LC_ALL=C grep -aqF "$capture_marker" "$bundle/Contents/MacOS/hex" || fail "built with the capture build option: hex has the capture engine"
[ "$(lipo -archs "$bundle/Contents/MacOS/hex")" = arm64 ] || fail "packaged executable is not arm64-only"
[ -x "$bundle/Contents/Library/HexMicrophone.driver/Contents/MacOS/HexMicrophoneDriver" ] || fail "bundled HexMicrophone.driver is missing"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle/Contents/Library/HexMicrophone.driver/Contents/Info.plist")" = anomaly.hex.microphone.driver ] || fail "bundled driver has wrong bundle identifier"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$bundle/Contents/Library/HexMicrophone.driver/Contents/Info.plist")" = "$version" ] || fail "bundled driver has wrong version"

codesign --verify --deep --strict --verbose=2 "$bundle"
signature=$(codesign -d --verbose=4 "$bundle" 2>&1)
printf '%s\n' "$signature" | grep -Fq 'Identifier=anomaly.hex' || fail "code signature has the wrong identifier"
printf '%s\n' "$signature" | grep -Fq "TeamIdentifier=$team_id" || fail "code signature has the wrong team"
printf '%s\n' "$signature" | grep -Fq 'Authority=Developer ID Application:' || fail "not signed for Developer ID distribution"
entitlements=$(codesign -d --entitlements - "$bundle" 2>/dev/null)
printf '%s\n' "$entitlements" | grep -Fq 'com.apple.security.device.audio-input' || fail "microphone entitlement is missing"
printf '%s\n' "$entitlements" | grep -Fq 'com.apple.security.automation.apple-events' || fail "automation entitlement is missing"
if printf '%s\n' "$entitlements" | grep -Fq 'com.apple.security.app-sandbox'; then
  fail "Rust application must not be sandboxed"
fi

echo "$bundle"
