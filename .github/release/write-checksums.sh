#!/usr/bin/env bash
# write-checksums.sh <dir>: writes <dir>/SHA256SUMS for every file under <dir>
# and prints that manifest's digest. Every release artifact carries this
# manifest, and the job that made it reports the digest as a job output; the
# jobs that use the artifact check both (verify-artifacts.sh).
. "$(dirname "$0")/lib.sh"

cd "$(posix "$1")"
manifest=$(mktemp)
find . -type f ! -name SHA256SUMS | sed 's|^\./||' | LC_ALL=C sort | while IFS= read -r file; do sha256 "$file"; done > "$manifest"
[ -s "$manifest" ] || die "Nothing to upload in $1"
mv "$manifest" SHA256SUMS
cat SHA256SUMS >&2
sha256 SHA256SUMS | cut -c1-64
