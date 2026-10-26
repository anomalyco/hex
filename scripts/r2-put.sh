#!/bin/sh
# Upload one object to a Hex files bucket (HEX_RELEASE_BUCKET:
# hex-production-files or hex-dev-files) through its S3 API, using the
# bucket-scoped CI credentials (R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY).
# HEX_RELEASE_PREFIX (e.g. staging/<run>/) is prepended to every key.
#
#   r2-put.sh <key> <file> <content-type> <cache-control> [content-disposition]
set -eu

[ $# -ge 4 ] || {
  echo "Usage: $0 <key> <file> <content-type> <cache-control> [content-disposition]" >&2
  exit 1
}
: "${R2_ACCESS_KEY_ID:?Set R2_ACCESS_KEY_ID}"
: "${R2_SECRET_ACCESS_KEY:?Set R2_SECRET_ACCESS_KEY}"
account=${R2_ACCOUNT_ID:-15d29c8639fd3733b1b5486a2acfd968}
bucket=${HEX_RELEASE_BUCKET:?Set HEX_RELEASE_BUCKET}
key="${HEX_RELEASE_PREFIX:-}$1"
test -f "$2"

# The secret travels in a curl config on stdin, never in argv.
printf 'user = "%s:%s"\n' "$R2_ACCESS_KEY_ID" "$R2_SECRET_ACCESS_KEY" |
  curl --config - --fail-with-body --silent --show-error --retry 3 \
    --aws-sigv4 aws:amz:auto:s3 \
    --header "Content-Type: $3" \
    --header "Cache-Control: $4" \
    ${5:+--header} ${5:+"Content-Disposition: $5"} \
    --upload-file "$2" \
    --output /dev/null \
    "https://$account.r2.cloudflarestorage.com/$bucket/$key"
echo "Uploaded $key" >&2
