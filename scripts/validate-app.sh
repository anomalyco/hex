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
team_id=${VOICE_CONTROL_TEAM_ID:?Set VOICE_CONTROL_TEAM_ID to the Apple Developer signing team}
feed_url=${HEX_EXPECTED_APP_FEED_URL:-https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/appcast.xml}
sparkle_key=mIek27lttJe8cIBqVZFhh6reRKjpTx1h9ZY9OKWPtuM=
plist="$bundle/Contents/Info.plist"

fail() {
  echo "Rust HEX bundle validation failed: $*" >&2
  exit 1
}

plist_value() {
  /usr/libexec/PlistBuddy -c "Print :$1" "$plist"
}

[ -d "$bundle" ] || fail "$bundle does not exist"
[ "$(basename "$bundle")" = "$expected_name" ] || fail "expected $expected_name, found $(basename "$bundle")"
[ "$(plist_value CFBundleIdentifier)" = com.kitlangton.hex2 ] || fail "wrong bundle identifier"
[ "$(plist_value CFBundleExecutable)" = hex ] || fail "wrong executable name"
[ "$(plist_value CFBundleDisplayName)" = Hex ] || fail "wrong display name"
[ "$(plist_value CFBundleName)" = Hex ] || fail "wrong bundle name"
[ "$(plist_value CFBundleShortVersionString)" = "$version" ] || fail "wrong version"
[ "$(plist_value CFBundleVersion)" = "$build_number" ] || fail "wrong build number"
[ "$(plist_value LSMinimumSystemVersion)" = 15.0 ] || fail "wrong minimum macOS"
[ "$(plist_value SUFeedURL)" = "$feed_url" ] || fail "wrong Sparkle feed"
[ "$(plist_value SUPublicEDKey)" = "$sparkle_key" ] || fail "wrong Sparkle public key"
[ "$(plist_value SUVerifyUpdateBeforeExtraction)" = true ] || fail "update verification is disabled"
[ "$(plist_value LSUIElement)" = true ] || fail "menu-bar application policy is disabled"
[ -x "$bundle/Contents/MacOS/hex" ] || fail "packaged executable is missing"
[ "$(lipo -archs "$bundle/Contents/MacOS/hex")" = arm64 ] || fail "packaged executable is not arm64-only"

codesign --verify --deep --strict --verbose=2 "$bundle"
signature=$(codesign -d --verbose=4 "$bundle" 2>&1)
printf '%s\n' "$signature" | grep -Fq 'Identifier=com.kitlangton.hex2' || fail "code signature has the wrong identifier"
printf '%s\n' "$signature" | grep -Fq "TeamIdentifier=$team_id" || fail "code signature has the wrong team"
printf '%s\n' "$signature" | grep -Fq 'Authority=Developer ID Application:' || fail "not signed for Developer ID distribution"
entitlements=$(codesign -d --entitlements - "$bundle" 2>/dev/null)
printf '%s\n' "$entitlements" | grep -Fq 'com.apple.security.device.audio-input' || fail "microphone entitlement is missing"
printf '%s\n' "$entitlements" | grep -Fq 'com.apple.security.automation.apple-events' || fail "automation entitlement is missing"
if printf '%s\n' "$entitlements" | grep -Fq 'com.apple.security.app-sandbox'; then
  fail "Rust application must not be sandboxed"
fi

echo "$bundle"
