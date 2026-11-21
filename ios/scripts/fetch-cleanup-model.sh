#!/bin/sh
set -eu

# Downloads the cleanup tagger's Core ML model (25 MB), kept out of git, into the keyboard
# extension's sources and checks its SHA-256. Run before `xcodegen generate`.
tag=ios-model-cleanup-tagger-1
asset=HexCleanupTagger-1.mlpackage.zip
sha256=dd681dfddaa3902a5f359056a60f94917651cc10134e465cc3063847002d52fc

ios=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
destination="$ios/HEXKeyboard/CleanupTagger"
stamp="$destination/.HexCleanupTagger.sha256"
if [ -d "$destination/HexCleanupTagger.mlpackage" ] && [ "$(cat "$stamp" 2>/dev/null)" = "$sha256" ]; then
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl -fsSL --retry 3 -o "$work/$asset" "https://github.com/anomalyco/hex/releases/download/$tag/$asset"
echo "$sha256  $work/$asset" | shasum -a 256 -c - >/dev/null
rm -rf "$destination/HexCleanupTagger.mlpackage"
unzip -q "$work/$asset" -d "$destination"
echo "$sha256" > "$stamp"
echo "Cleanup tagger model $tag ready"
