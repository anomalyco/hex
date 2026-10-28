#!/usr/bin/env bash
# macos-credentials.sh <dir>: imports the Developer ID certificate into a new
# keychain in <dir> and writes the App Store Connect API key for notarytool
# there. Prints nothing secret. Appends HEX_KEYCHAIN, HEX_SIGNING_IDENTITY,
# and HEX_NOTARY_KEY to $GITHUB_ENV.
#
# Environment: APPLE_SIGNING_CERTIFICATE (a base64 .p12),
# APPLE_SIGNING_CERTIFICATE_PASSWORD (optional; the .p12 may have none),
# APPLE_NOTARY_KEY (the .p8, as PEM or base64), HEX_TEAM_ID.
. "$(dirname "$0")/lib.sh"

dir=$1
mkdir -p "$dir"
chmod 700 "$dir"
certificate="$dir/certificate.p12"
keychain="$dir/signing.keychain-db"
key="$dir/notary-key.p8"

(umask 077 && printf '%s' "$APPLE_SIGNING_CERTIFICATE" | tr -d '\r\n ' | base64 -D > "$certificate") ||
  die "APPLE_SIGNING_CERTIFICATE is not base64."
[ -s "$certificate" ] || die "APPLE_SIGNING_CERTIFICATE is empty."

password=$(uuidgen)
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
if ! security import "$certificate" -k "$keychain" -f pkcs12 -P "${APPLE_SIGNING_CERTIFICATE_PASSWORD:-}" \
  -T /usr/bin/codesign -T /usr/bin/security > /dev/null 2>&1; then
  if [ -z "${APPLE_SIGNING_CERTIFICATE_PASSWORD:-}" ]; then
    die "APPLE_SIGNING_CERTIFICATE did not import without a password; it needs APPLE_SIGNING_CERTIFICATE_PASSWORD."
  fi
  die "APPLE_SIGNING_CERTIFICATE did not import with APPLE_SIGNING_CERTIFICATE_PASSWORD."
fi
rm -f "$certificate"
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" > /dev/null
# codesign finds identities only in the search list.
# shellcheck disable=SC2046
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
identity=$(security find-identity -v -p codesigning "$keychain" |
  sed -n "s/.*\"\(Developer ID Application:.*($HEX_TEAM_ID)\)\"/\1/p" | head -1)
[ -n "$identity" ] || die "APPLE_SIGNING_CERTIFICATE has no Developer ID Application identity for team $HEX_TEAM_ID."
echo "Signing as $identity."

case "$APPLE_NOTARY_KEY" in
  -----BEGIN*) (umask 077 && printf '%s\n' "$APPLE_NOTARY_KEY" > "$key") ;;
  *) (umask 077 && printf '%s' "$APPLE_NOTARY_KEY" | tr -d '\r\n ' | base64 -D > "$key") || die "APPLE_NOTARY_KEY is neither PEM nor base64." ;;
esac
grep -q -- '-----BEGIN PRIVATE KEY-----' "$key" || die "APPLE_NOTARY_KEY is not an App Store Connect API key (.p8)."

{
  echo "HEX_KEYCHAIN=$keychain"
  echo "HEX_SIGNING_IDENTITY=$identity"
  echo "HEX_NOTARY_KEY=$key"
} >> "$GITHUB_ENV"
