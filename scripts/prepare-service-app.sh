#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=${HEX_VERSION:-$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" | jq -r '.packages[0].version')}
notary_profile=${HEX_NOTARY_PROFILE:-AC_PASSWORD}
arch=$(uname -m)
dist="$root/dist/service"
artifact="$dist/HEX-Service-$version-$arch.zip"
submission="$dist/HEX-Service-$version-$arch.notarization.zip"

bundle=$("$root/scripts/build-service-app.sh")
rm -rf "$dist"
mkdir -p "$dist"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$bundle" "$submission"
xcrun notarytool submit "$submission" --keychain-profile "$notary_profile" --wait
xcrun stapler staple "$bundle"
xcrun stapler validate "$bundle"
rm -f "$submission"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$bundle" "$artifact"
shasum -a 256 "$artifact" > "$artifact.sha256"

echo "$artifact"
