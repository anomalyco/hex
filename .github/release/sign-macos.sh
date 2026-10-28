#!/usr/bin/env bash
# sign-macos.sh <unsigned Hex.app zip> <out dir>: signs Hex.app inside-out
# with the Developer ID identity (hardened runtime, secure timestamp, Hex's
# entitlements), notarizes and staples it, then writes the release service's
# two macOS targets from the stapled app: Hex-<version>-macos-arm64.zip (root
# Hex.app, for the updater) and Hex-<version>-macos-arm64.dmg (signed,
# notarized, stapled, with an Applications link). The app is data here: it is
# never launched.
#
# Environment: HEX_VERSION, HEX_SIGNING_IDENTITY, HEX_KEYCHAIN, HEX_NOTARY_KEY,
# HEX_NOTARY_KEY_ID, HEX_NOTARY_ISSUER (macos-credentials.sh).
. "$(dirname "$0")/lib.sh"

input=$1
out=$2
release=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"
zip="$out/Hex-$HEX_VERSION-macos-arm64.zip"
dmg="$out/Hex-$HEX_VERSION-macos-arm64.dmg"

/usr/bin/ditto -x -k "$input" "$work/unpacked"
app="$work/unpacked/Hex.app"
if [ ! -d "$app" ] || [ "$(find "$work/unpacked" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')" != 1 ]; then
  die "The macOS build must hold exactly Hex.app."
fi
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")" = anomaly.hex ] ||
  die "Hex.app has the wrong bundle identifier."
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")" = "$HEX_VERSION" ] ||
  die "Hex.app is not version $HEX_VERSION."

# Every Mach-O in the bundle must be one this script signs.
expected=$(printf '%s\n' Contents/MacOS/hex \
  Contents/Library/HexMicrophone.driver/Contents/MacOS/HexMicrophoneDriver \
  Contents/Frameworks/libmoonshine.dylib Contents/Frameworks/libonnxruntime.1.23.2.dylib | LC_ALL=C sort)
found=$(cd "$app" && find . -type f -exec sh -c 'file -b "$1" | grep -q Mach-O && printf "%s\n" "${1#./}"' _ {} \; | LC_ALL=C sort)
[ "$found" = "$expected" ] || die "Unexpected code in Hex.app: $(diff <(echo "$expected") <(echo "$found") | grep '^[<>]' | tr '\n' ' ')"

sign() {
  codesign --force --timestamp --options runtime --keychain "$HEX_KEYCHAIN" --sign "$HEX_SIGNING_IDENTITY" "$@"
}
notarize() {
  xcrun notarytool submit "$1" --key "$HEX_NOTARY_KEY" --key-id "$HEX_NOTARY_KEY_ID" --issuer "$HEX_NOTARY_ISSUER" --wait --timeout 45m
}

sign "$app/Contents/Frameworks/libonnxruntime.1.23.2.dylib"
sign "$app/Contents/Frameworks/libmoonshine.dylib"
sign "$app/Contents/Library/HexMicrophone.driver"
sign --entitlements "$release/hex.entitlements" "$app"
codesign --verify --deep --strict --verbose=2 "$app"

# Notarize the app first so its ticket is stapled inside both targets.
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$app" "$work/notarize.zip"
notarize "$work/notarize.zip"
xcrun stapler staple "$app"
xcrun stapler validate "$app"

rm -f "$zip" "$dmg"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$app" "$zip"

mkdir "$work/staging"
/usr/bin/ditto "$app" "$work/staging/Hex.app"
ln -s /Applications "$work/staging/Applications"
hdiutil create -quiet -fs APFS -format ULFO -volname Hex -srcfolder "$work/staging" "$dmg"
codesign --force --timestamp --keychain "$HEX_KEYCHAIN" --sign "$HEX_SIGNING_IDENTITY" "$dmg"
notarize "$dmg"
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
ls -l "$out"
