#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$root/scripts/macos-signing.sh"
: "${HEX_NOTARY_PROFILE:?Set HEX_NOTARY_PROFILE to the matching notarization profile}"
version=$(hex_version "$root")
arch=$(uname -m)
dist="$root/dist/service"
artifact="$dist/HEX-Service-$version-$arch.zip"
submission="$dist/HEX-Service-$version-$arch.notarization.zip"

bundle=$("$root/scripts/build-service-app.sh")
rm -rf "$dist"
mkdir -p "$dist"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$bundle" "$submission"
hex_notarize "$submission"
xcrun stapler staple "$bundle"
xcrun stapler validate "$bundle"
rm -f "$submission"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$bundle" "$artifact"
shasum -a 256 "$artifact" > "$artifact.sha256"

echo "$artifact"
