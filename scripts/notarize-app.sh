#!/bin/sh
set -eu

# Notarize and staple a Developer ID signed app, then wrap it in a signed,
# notarized, and stapled DMG with an Applications link.
if [ "$#" -ne 3 ]; then
  echo "Usage: $0 <app path> <dmg path> <volume name>" >&2
  exit 1
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$root/scripts/macos-signing.sh"
bundle=$1
dmg=$2
volume=$3
team_id=${VOICE_CONTROL_TEAM_ID:?Set VOICE_CONTROL_TEAM_ID to the Apple Developer signing team}
identity=$(hex_codesign_identity "$team_id")
work=$(mktemp -d "${TMPDIR:-/tmp}/hex-notarize.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

# Notarize the app first so its ticket can be stapled inside the distributable DMG.
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$bundle" "$work/app.zip"
hex_notarize "$work/app.zip"
xcrun stapler staple "$bundle"
xcrun stapler validate "$bundle"

mkdir "$work/staging"
/usr/bin/ditto "$bundle" "$work/staging/$(basename "$bundle")"
ln -s /Applications "$work/staging/Applications"
rm -f "$dmg"
hdiutil create -quiet -fs APFS -format ULFO -volname "$volume" -srcfolder "$work/staging" "$dmg"
codesign --force --timestamp --sign "$identity" "$dmg"
hex_notarize "$dmg"
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
