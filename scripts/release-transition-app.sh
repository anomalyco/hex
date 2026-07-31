#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mode=${1:-prepare}
version=${HEX_VERSION:-2.1.0}
build_number=${HEX_BUILD_NUMBER:-20100}
previous_rust_build=${HEX_TRANSITION_RUST_BUILD:-20023}
swift_build=${HEX_TRANSITION_SWIFT_BUILD:-91}
phase_interval=${HEX_PHASED_ROLLOUT_INTERVAL:-172800}
notary_profile=${HEX_NOTARY_PROFILE:-AC_PASSWORD}
bucket=${HEX_RELEASE_BUCKET:-hex-releases}
base_url=${HEX_RELEASE_BASE_URL:-https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev}
legacy_bucket=${HEX_LEGACY_RELEASE_BUCKET:-hex-updates}
legacy_feed_url=${HEX_LEGACY_FEED_URL:-https://hex-updates.s3.amazonaws.com/appcast.xml}
release_notes=${HEX_RELEASE_NOTES:-$root/docs/releases/$version.md}
team_id=${VOICE_CONTROL_TEAM_ID:-QC99C9JE59}
identity=${VOICE_CONTROL_CODESIGN_IDENTITY:-}
dist="$root/dist/transition-$version"
r2_updates="$dist/r2"
s3_updates="$dist/s3"
rust_artifact="HEX-$version-rust-transition-arm64.dmg"
swift_artifact="Hex-$version-swift-transition-arm64.dmg"
rust_dmg="$dist/$rust_artifact"
swift_dmg="$dist/$swift_artifact"
source_commit_file="$dist/source-commit"

case "$mode" in
  prepare|validate|publish|verify-public) ;;
  *)
    echo "Usage: $0 [prepare|validate|publish|verify-public]" >&2
    exit 1
    ;;
esac

fail() {
  echo "HEX transition release failed: $*" >&2
  exit 1
}

[ "$version" = 2.1.0 ] || fail "transition version must be 2.1.0"
[ "$build_number" = 20100 ] || fail "transition build must be 20100"
[ "$phase_interval" = 172800 ] || fail "transition rollout interval must be 172800 seconds"

sparkle_dir=$("$root/scripts/setup-sparkle.sh")
generate_appcast="$sparkle_dir/bin/generate_appcast"
sign_update="$sparkle_dir/bin/sign_update"
[ -x "$sign_update" ] || fail "Sparkle sign_update is unavailable"

require_release_source() {
  [ -z "$(git -C "$root" status --porcelain)" ] || fail "commit all working-tree changes first"
  upstream=$(git -C "$root" rev-parse --abbrev-ref '@{upstream}' 2>/dev/null || true)
  [ -n "$upstream" ] || fail "the release branch has no upstream"
  [ "$(git -C "$root" rev-parse HEAD)" = "$(git -C "$root" rev-parse "$upstream")" ] \
    || fail "push the release commit before preparing the transition"
}

item_xpath() {
  printf "/*[local-name()='rss']/*[local-name()='channel']/*[local-name()='item'][*[local-name()='version' and text()='%s']]" "$build_number"
}

xml_value() {
  feed=$1
  element=$2
  xmllint --xpath "string(($(item_xpath)/*[local-name()='$element'])[1])" "$feed"
}

xml_enclosure_attribute() {
  feed=$1
  attribute=$2
  xmllint --xpath "string(($(item_xpath)/*[local-name()='enclosure']/@*[local-name()='$attribute'])[1])" "$feed"
}

validate_feed() {
  feed=$1
  artifact=$2
  artifact_url=$3
  minimum_update=$4
  [ -f "$feed" ] || fail "missing feed $feed"
  [ "$(xml_value "$feed" version)" = "$build_number" ] || fail "$feed has the wrong transition build"
  [ "$(xml_value "$feed" shortVersionString)" = "$version" ] || fail "$feed has the wrong transition version"
  [ "$(xml_value "$feed" minimumSystemVersion)" = 15.0 ] || fail "$feed has the wrong minimum macOS"
  [ "$(xml_value "$feed" hardwareRequirements)" = arm64 ] || fail "$feed does not require Apple silicon"
  [ "$(xml_value "$feed" minimumUpdateVersion)" = "$minimum_update" ] || fail "$feed has the wrong source-version gate"
  [ "$(xml_value "$feed" minimumAutoupdateVersion)" = "$build_number" ] || fail "$feed is not an explicit major upgrade"
  [ "$(xml_value "$feed" phasedRolloutInterval)" = "$phase_interval" ] || fail "$feed has the wrong phased rollout"
  [ "$(xml_enclosure_attribute "$feed" url)" = "$artifact_url" ] || fail "$feed has the wrong artifact URL"
  [ "$(xml_enclosure_attribute "$feed" length)" = "$(wc -c < "$artifact" | tr -d ' ')" ] || fail "$feed has the wrong artifact length"
  signature=$(xml_enclosure_attribute "$feed" edSignature)
  [ -n "$signature" ] || fail "$feed has no EdDSA signature"
  "$sign_update" --verify "$artifact" "$signature"
  item=$(xmllint --xpath "$(item_xpath)" "$feed")
  printf '%s\n' "$item" | grep -Fq '<description' || fail "$feed has no embedded release notes"
  if printf '%s\n' "$item" | grep -Fq '<sparkle:deltas'; then
    fail "$feed contains a forbidden transition delta"
  fi
}

extract_dmg_app() {
  dmg=$1
  expected_name=$2
  destination=$3
  mountpoint=$(mktemp -d "${TMPDIR:-/tmp}/hex-transition-mount.XXXXXX")
  hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mountpoint" "$dmg"
  if [ ! -d "$mountpoint/$expected_name" ]; then
    hdiutil detach -quiet "$mountpoint" || true
    rm -rf "$mountpoint"
    fail "$(basename "$dmg") does not contain $expected_name"
  fi
  found_apps=$(find "$mountpoint" -maxdepth 1 -type d -name '*.app' -print | wc -l | tr -d ' ')
  if [ "$found_apps" != 1 ]; then
    hdiutil detach -quiet "$mountpoint" || true
    rm -rf "$mountpoint"
    fail "$(basename "$dmg") contains unexpected application bundles"
  fi
  /usr/bin/ditto "$mountpoint/$expected_name" "$destination"
  hdiutil detach -quiet "$mountpoint"
  rm -rf "$mountpoint"
}

validate_prepared() {
  [ -f "$source_commit_file" ] || fail "missing prepared source commit"
  [ "$(cat "$source_commit_file")" = "$(git -C "$root" rev-parse HEAD)" ] \
    || fail "prepared artifacts do not belong to the current commit"
  [ -f "$rust_dmg" ] || fail "missing $rust_dmg"
  [ -f "$swift_dmg" ] || fail "missing $swift_dmg"
  [ -f "$r2_updates/appcast.xml" ] || fail "missing the R2 transition feed"
  [ -f "$s3_updates/appcast.xml" ] || fail "missing the legacy transition feed"
  validation=$(mktemp -d "${TMPDIR:-/tmp}/hex-transition-validation.XXXXXX")
  extract_dmg_app "$rust_dmg" HEX.app "$validation/rust.app"
  extract_dmg_app "$swift_dmg" Hex.app "$validation/swift.app"
  "$root/scripts/validate-permanent-app.sh" "$validation/rust.app" rust.app "$version" "$build_number" >/dev/null
  "$root/scripts/validate-permanent-app.sh" "$validation/swift.app" swift.app "$version" "$build_number" >/dev/null
  rust_hash=$(codesign -d --verbose=4 "$validation/rust.app" 2>&1 | sed -n 's/^CDHash=//p')
  swift_hash=$(codesign -d --verbose=4 "$validation/swift.app" 2>&1 | sed -n 's/^CDHash=//p')
  [ -n "$rust_hash" ] || fail "the Rust-host archive has no code-directory hash"
  [ "$rust_hash" = "$swift_hash" ] \
    || fail "the two transition archives do not contain the same signed payload"
  rm -rf "$validation"
  validate_feed "$r2_updates/appcast.xml" "$rust_dmg" "$base_url/releases/$rust_artifact" "$previous_rust_build"
  validate_feed "$s3_updates/appcast.xml" "$swift_dmg" "$base_url/releases/$swift_artifact" "$swift_build"
  xmllint --xpath "boolean(//*[local-name()='version' and text()='$previous_rust_build'])" "$r2_updates/appcast.xml" \
    | grep -Fq true || fail "R2 feed lost the prior Rust release"
  xmllint --xpath "boolean(//*[local-name()='version' and text()='$swift_build'])" "$s3_updates/appcast.xml" \
    | grep -Fq true || fail "legacy feed lost Swift 0.8.4"
}

verify_public() {
  download=$(mktemp -d "${TMPDIR:-/tmp}/hex-transition-public.XXXXXX")
  cache_buster=$(date +%s)
  curl --fail --silent --show-error "$base_url/appcast.xml?transition=$cache_buster" -o "$download/r2-appcast.xml"
  curl --fail --silent --show-error "$legacy_feed_url?transition=$cache_buster" -o "$download/s3-appcast.xml"
  curl --fail --silent --show-error "$base_url/releases/$rust_artifact" -o "$download/$rust_artifact"
  curl --fail --silent --show-error "$base_url/releases/$swift_artifact" -o "$download/$swift_artifact"
  validate_feed "$download/r2-appcast.xml" "$download/$rust_artifact" "$base_url/releases/$rust_artifact" "$previous_rust_build"
  validate_feed "$download/s3-appcast.xml" "$download/$swift_artifact" "$base_url/releases/$swift_artifact" "$swift_build"
  extract_dmg_app "$download/$rust_artifact" HEX.app "$download/rust.app"
  extract_dmg_app "$download/$swift_artifact" Hex.app "$download/swift.app"
  "$root/scripts/validate-permanent-app.sh" "$download/rust.app" rust.app "$version" "$build_number" >/dev/null
  "$root/scripts/validate-permanent-app.sh" "$download/swift.app" swift.app "$version" "$build_number" >/dev/null
  rm -rf "$download"
}

if [ "$mode" = verify-public ]; then
  verify_public
  echo "Public transition feeds and artifacts are valid."
  exit 0
fi

if [ "$mode" = validate ]; then
  validate_prepared
  echo "Prepared transition feeds and artifacts are valid."
  exit 0
fi

require_release_source

if [ "$mode" = publish ]; then
  validate_prepared
  wrangler r2 object put "$bucket/releases/$rust_artifact" --remote --file "$rust_dmg" --content-type application/x-apple-diskimage
  wrangler r2 object put "$bucket/releases/$swift_artifact" --remote --file "$swift_dmg" --content-type application/x-apple-diskimage
  wrangler r2 object put "$bucket/releases/HEX-latest-arm64.dmg" --remote --file "$swift_dmg" --content-type application/x-apple-diskimage
  curl --fail --silent --show-error "$base_url/releases/$rust_artifact" -o /dev/null
  curl --fail --silent --show-error "$base_url/releases/$swift_artifact" -o /dev/null
  wrangler r2 object put "$bucket/appcast.xml" --remote --file "$r2_updates/appcast.xml" --content-type application/xml
  aws s3 cp "$s3_updates/appcast.xml" "s3://$legacy_bucket/appcast.xml" --content-type application/xml --cache-control no-cache
  verify_public
  echo "$base_url/releases/$swift_artifact"
  exit 0
fi

[ -f "$release_notes" ] || fail "add release notes at $release_notes"
published_build=$(curl --fail --silent --show-error "$base_url/appcast.xml" \
  | sed -n 's|.*<sparkle:version>\([0-9][0-9]*\)</sparkle:version>.*|\1|p' \
  | head -1)
[ -n "$published_build" ] || fail "could not determine the current R2 build"
[ "$build_number" -gt "$published_build" ] || fail "build $build_number is not newer than R2 build $published_build"
curl --fail --silent --show-error "$legacy_feed_url" \
  | xmllint --xpath "boolean(//*[local-name()='version' and text()='$swift_build'])" - \
  | grep -Fq true || fail "legacy feed does not contain Swift build $swift_build"

export HEX_VERSION="$version" HEX_BUILD_NUMBER="$build_number" HEX_APP_IDENTITY=permanent
bundle=$("$root/scripts/build-app.sh")
"$root/scripts/validate-permanent-app.sh" "$bundle" Hex.app "$version" "$build_number" >/dev/null

if [ -z "$identity" ]; then
  identity=$(security find-identity -v -p codesigning | sed -n "s/.*\"\(Developer ID Application:.*($team_id)\)\"/\1/p" | head -1)
fi
[ -n "$identity" ] || fail "no Developer ID signing identity found for team $team_id"

rm -rf "$dist"
mkdir -p "$dist" "$r2_updates" "$s3_updates"
git -C "$root" rev-parse HEAD > "$source_commit_file"
zip="$dist/Hex-$version.zip"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$bundle" "$zip"
xcrun notarytool submit "$zip" --keychain-profile "$notary_profile" --wait
xcrun stapler staple "$bundle"
xcrun stapler validate "$bundle"

package_dmg() {
  app_name=$1
  output=$2
  staging=$(mktemp -d "${TMPDIR:-/tmp}/hex-transition-staging.XXXXXX")
  /usr/bin/ditto "$bundle" "$staging/$app_name"
  ln -s /Applications "$staging/Applications"
  hdiutil create -quiet -fs APFS -format ULFO -volname HEX -srcfolder "$staging" "$output"
  rm -rf "$staging"
  codesign --force --timestamp --sign "$identity" "$output"
  xcrun notarytool submit "$output" --keychain-profile "$notary_profile" --wait
  xcrun stapler staple "$output"
  xcrun stapler validate "$output"
  spctl --assess --type open --context context:primary-signature --verbose=2 "$output"
}

package_dmg HEX.app "$rust_dmg"
package_dmg Hex.app "$swift_dmg"

cp "$rust_dmg" "$r2_updates/$rust_artifact"
cp "$swift_dmg" "$s3_updates/$swift_artifact"
cp "$release_notes" "$r2_updates/${rust_artifact%.dmg}.md"
cp "$release_notes" "$s3_updates/${swift_artifact%.dmg}.md"
curl --fail --silent --show-error "$base_url/appcast.xml" -o "$r2_updates/appcast.xml"
curl --fail --silent --show-error "$legacy_feed_url" -o "$s3_updates/appcast.xml"

security find-generic-password -s 'https://sparkle-project.org' -a ed25519 -w \
  | "$generate_appcast" --ed-key-file - --download-url-prefix "$base_url/releases/" \
      --embed-release-notes --versions "$build_number" --minimum-update-version "$previous_rust_build" \
      --major-version "$build_number" --phased-rollout-interval "$phase_interval" \
      --maximum-deltas 0 --maximum-versions 5 "$r2_updates"
security find-generic-password -s 'https://sparkle-project.org' -a ed25519 -w \
  | "$generate_appcast" --ed-key-file - --download-url-prefix "$base_url/releases/" \
      --embed-release-notes --versions "$build_number" --minimum-update-version "$swift_build" \
      --major-version "$build_number" --phased-rollout-interval "$phase_interval" \
      --maximum-deltas 0 --maximum-versions 0 "$s3_updates"

validate_prepared
echo "$rust_dmg"
echo "$swift_dmg"
echo "Prepared but not published. Validate signed upgrades, then run $0 publish." >&2
