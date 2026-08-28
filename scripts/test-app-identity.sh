#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
export VOICE_CONTROL_TEAM_ID=TESTTEAM00
temporary=$(mktemp -d "${TMPDIR:-/tmp}/hex-app-identity.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
bundle="$temporary/Hex.app"
mkdir -p "$bundle/Contents"
plist="$bundle/Contents/Info.plist"
cp "$root/app/Info.plist" "$plist"

[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist")" = com.kitlangton.hex2 ]
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$plist")" = hex ]
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleDisplayName' "$plist")" = Hex ]
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleName' "$plist")" = Hex ]

expect_rejection() {
  if output=$(sh "$root/scripts/validate-app.sh" "$bundle" Hex.app 2.1.0 20100 2>&1); then
    echo "Unexpectedly accepted an incompatible app." >&2
    exit 1
  fi
  printf '%s\n' "$output" | grep -Fq "$1"
}

/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier com.kitlangton.Hex' "$plist"
expect_rejection 'wrong bundle identifier'
/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier com.kitlangton.voice-control.agent' "$plist"
expect_rejection 'wrong bundle identifier'
/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier ly.anoma.Hex' "$plist"
expect_rejection 'wrong bundle identifier'
/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier com.kitlangton.hex2' "$plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleExecutable voice-control-watch' "$plist"
expect_rejection 'wrong executable name'

for script in build-app.sh release-app.sh; do
  if output=$(env -u VOICE_CONTROL_TEAM_ID sh "$root/scripts/$script" 2>&1); then
    echo "Unexpectedly allowed $script without an explicit signing team." >&2
    exit 1
  fi
  printf '%s\n' "$output" | grep -Fq 'Set VOICE_CONTROL_TEAM_ID to the Apple Developer signing team'
done

for identity in 'Developer ID Application: Fixture (WRONGTEAM0)' 'Apple Development: Fixture (TESTTEAM00)'; do
  if output=$(VOICE_CONTROL_CODESIGN_IDENTITY="$identity" sh "$root/scripts/build-app.sh" 2>&1); then
    echo "Unexpectedly allowed an incompatible signing identity." >&2
    exit 1
  fi
  printf '%s\n' "$output" | grep -Fq 'Signing identity is not a Developer ID Application identity for team TESTTEAM00'
done

echo "Rust app identity guards passed."
