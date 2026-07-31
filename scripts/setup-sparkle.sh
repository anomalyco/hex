#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=2.9.4
sha256=ce89daf967db1e1893ed3ebd67575ed82d3902563e3191ca92aaec9164fbdef9
archive="$root/target/Sparkle-$version.tar.xz"
destination="$root/target/sparkle-$version"
url="https://github.com/sparkle-project/Sparkle/releases/download/$version/Sparkle-$version.tar.xz"

if [ -x "$destination/bin/generate_appcast" ] \
  && [ -x "$destination/bin/sign_update" ] \
  && [ -d "$destination/Sparkle.framework" ]; then
  echo "$destination"
  exit 0
fi

mkdir -p "$root/target"
curl --fail --location --retry 3 --continue-at - --output "$archive.partial" "$url"
if ! printf '%s  %s\n' "$sha256" "$archive.partial" | shasum -a 256 -c - >&2; then
  rm -f "$archive.partial"
  exit 1
fi
mv "$archive.partial" "$archive"
rm -rf "$destination.partial"
mkdir -p "$destination.partial"
tar -xJf "$archive" -C "$destination.partial"
mv "$destination.partial" "$destination"
echo "$destination"
