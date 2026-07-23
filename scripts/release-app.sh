#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mode=${1:-prepare}
version=${HEX_VERSION:-$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" | jq -r '.packages[0].version')}
build_number=${HEX_BUILD_NUMBER:-$(printf '%s\n' "$version" | awk -F. '{ print ($1 * 10000) + ($2 * 100) + $3 }')}
notary_profile=${HEX_NOTARY_PROFILE:-AC_PASSWORD}
bucket=${HEX_RELEASE_BUCKET:-hex-releases}
base_url=${HEX_RELEASE_BASE_URL:-https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev}
release_notes=${HEX_RELEASE_NOTES:-$root/docs/releases/$version.md}
dist="$root/dist"
updates="$dist/updates"
artifact="HEX-$version-arm64.dmg"
latest_artifact="HEX-latest-arm64.dmg"
team_id=${VOICE_CONTROL_TEAM_ID:-QC99C9JE59}
identity=${VOICE_CONTROL_CODESIGN_IDENTITY:-}

if [ "$mode" != "prepare" ] && [ "$mode" != "publish" ]; then
  echo "Usage: $0 [prepare|publish]" >&2
  exit 1
fi
if [ -n "$(git -C "$root" status --porcelain)" ]; then
  echo "Commit or remove all working-tree changes before releasing." >&2
  exit 1
fi
upstream=$(git -C "$root" rev-parse --abbrev-ref '@{upstream}' 2>/dev/null || true)
if [ -z "$upstream" ] || [ "$(git -C "$root" rev-parse HEAD)" != "$(git -C "$root" rev-parse "$upstream")" ]; then
  echo "Push the release commit to its upstream branch before releasing." >&2
  exit 1
fi
case "$build_number" in
  ''|*[!0-9]*)
    echo "HEX_BUILD_NUMBER must be a positive integer." >&2
    exit 1
    ;;
esac
published_build=$(curl --fail --silent --show-error "$base_url/appcast.xml" \
  | sed -n 's:.*<sparkle:version>\([0-9][0-9]*\)</sparkle:version>.*:\1:p' \
  | head -1)
if [ -z "$published_build" ] || [ "$build_number" -le "$published_build" ]; then
  echo "Build $build_number must be newer than published build ${published_build:-unknown}." >&2
  exit 1
fi

if [ "$mode" = "publish" ]; then
  if [ ! -f "$dist/$artifact" ] || [ ! -f "$updates/appcast.xml" ]; then
    echo "Prepare and validate version $version before publishing it." >&2
    exit 1
  fi
  if ! grep -Fq "$base_url/releases/$artifact" "$updates/appcast.xml" \
    || ! grep -Fq 'sparkle:edSignature=' "$updates/appcast.xml"; then
    echo "The prepared appcast does not contain a signed $artifact update." >&2
    exit 1
  fi
  xcrun stapler validate "$dist/$artifact"
  spctl --assess --type open --context context:primary-signature --verbose=2 "$dist/$artifact"
  wrangler r2 object put "$bucket/releases/$artifact" --remote \
    --file "$dist/$artifact" \
    --content-type application/x-apple-diskimage
  wrangler r2 object put "$bucket/releases/$latest_artifact" --remote \
    --file "$dist/$artifact" \
    --content-type application/x-apple-diskimage
  wrangler r2 object put "$bucket/appcast.xml" --remote \
    --file "$updates/appcast.xml" \
    --content-type application/xml
  curl --fail --silent --show-error "$base_url/releases/$artifact" -o /dev/null
  curl --fail --silent --show-error "$base_url/releases/$latest_artifact" -o /dev/null
  curl --fail --silent --show-error "$base_url/appcast.xml" -o /dev/null
  echo "$base_url/releases/$artifact"
  exit 0
fi
if [ ! -f "$release_notes" ]; then
  echo "Add release notes at $release_notes or set HEX_RELEASE_NOTES." >&2
  exit 1
fi

export HEX_VERSION="$version" HEX_BUILD_NUMBER="$build_number"
bundle=$("$root/scripts/build-app.sh")
if [ -z "$identity" ]; then
  identity=$(security find-identity -v -p codesigning | sed -n "s/.*\"\(Developer ID Application:.*($team_id)\)\"/\1/p" | head -1)
fi
if [ -z "$identity" ]; then
  echo "No Developer ID signing identity found for team $team_id." >&2
  exit 1
fi
rm -rf "$dist/staging" "$updates"
rm -f "$dist/HEX-$version.zip" "$dist/$artifact"
mkdir -p "$dist/staging" "$updates"

# Notarize the app first so its ticket can be stapled inside the distributable DMG.
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$bundle" "$dist/HEX-$version.zip"
xcrun notarytool submit "$dist/HEX-$version.zip" --keychain-profile "$notary_profile" --wait
xcrun stapler staple "$bundle"
xcrun stapler validate "$bundle"

/usr/bin/ditto "$bundle" "$dist/staging/HEX.app"
ln -s /Applications "$dist/staging/Applications"
hdiutil create -quiet -fs APFS -format ULFO -volname HEX -srcfolder "$dist/staging" "$dist/$artifact"
codesign --force --timestamp --sign "$identity" "$dist/$artifact"
xcrun notarytool submit "$dist/$artifact" --keychain-profile "$notary_profile" --wait
xcrun stapler staple "$dist/$artifact"
xcrun stapler validate "$dist/$artifact"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dist/$artifact"

cp "$dist/$artifact" "$updates/$artifact"
cp "$release_notes" "$updates/HEX-$version-arm64.md"
curl --fail --silent --show-error "$base_url/appcast.xml" -o "$updates/appcast.xml" || rm -f "$updates/appcast.xml"
sparkle_dir=$("$root/scripts/setup-sparkle.sh")
security find-generic-password \
  -s 'https://sparkle-project.org' \
  -a ed25519 \
  -w | "$sparkle_dir/bin/generate_appcast" \
  --ed-key-file - \
  --download-url-prefix "$base_url/releases/" \
  --embed-release-notes \
  --maximum-deltas 0 \
  --maximum-versions 5 \
  "$updates"
grep -Fq "$base_url/releases/$artifact" "$updates/appcast.xml"
grep -Fq 'sparkle:edSignature=' "$updates/appcast.xml"

echo "$dist/$artifact"
echo "Prepared but not published. Run $0 publish after validation." >&2
