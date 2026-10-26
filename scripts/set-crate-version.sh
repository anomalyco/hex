#!/bin/sh
# Stamps a prerelease of the checked-out version into Cargo.toml and
# Cargo.lock, so a staging build reports the version the release service
# publishes (X.Y.Z-dev.N or X.Y.Z-beta.N). Release builds only; the change is
# never committed.
#
#   set-crate-version.sh <source root> <version>
set -eu

root=$1
version=$2
current=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$root/Cargo.toml" | head -1)
[ "$version" = "$current" ] && exit 0
case "$version" in
  "$current"-dev.[0-9]* | "$current"-beta.[0-9]*) ;;
  *)
    echo "Can only stamp a prerelease of $current, not $version." >&2
    exit 1
    ;;
esac
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+-(dev|beta)\.[0-9]+$'
# awk, not sed's GNU-only 0,/re/ address: this also runs on the macOS runner.
awk -v current="$current" -v version="$version" '
  !done && $0 == "version = \"" current "\"" { $0 = "version = \"" version "\""; done = 1 }
  { print }
' "$root/Cargo.toml" > "$root/Cargo.toml.next"
mv "$root/Cargo.toml.next" "$root/Cargo.toml"
awk -v current="$current" -v version="$version" '
  /^name = "voice-control"$/ { package = 1 }
  package && $0 == "version = \"" current "\"" { $0 = "version = \"" version "\""; package = 0 }
  { print }
' "$root/Cargo.lock" > "$root/Cargo.lock.next"
mv "$root/Cargo.lock.next" "$root/Cargo.lock"
grep -q "^version = \"$version\"" "$root/Cargo.toml"
echo "Building Hex as $version." >&2
