#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bundle="$root/target/HEX.app"
executable="$bundle/Contents/MacOS/voice-control-watch"
frameworks="$bundle/Contents/Frameworks"
iconset="$root/target/HEX.iconset"

cargo build --release --manifest-path "$root/Cargo.toml"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$frameworks"
cp "$root/target/release/voice-control" "$executable"
cp "$root/app/Info.plist" "$bundle/Contents/Info.plist"
rm -rf "$iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$root/app/AppIcon.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$root/app/AppIcon.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/HEX.icns"

set -- "$root"/.venv/lib/python*/site-packages/moonshine_voice
moonshine_dir=$1
if [ ! -d "$moonshine_dir" ]; then
  echo "Moonshine is not installed. Run ./scripts/setup.sh." >&2
  exit 1
fi
cp "$moonshine_dir/libmoonshine.dylib" "$frameworks/"
cp "$moonshine_dir/libonnxruntime.1.23.2.dylib" "$frameworks/"

identity=${VOICE_CONTROL_CODESIGN_IDENTITY:-}
if [ -z "$identity" ]; then
  identity=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application:.*\)"/\1/p' | head -1)
fi
if [ -z "$identity" ]; then
  echo "No Developer ID signing identity found. Set VOICE_CONTROL_CODESIGN_IDENTITY." >&2
  exit 1
fi

codesign --force --options runtime --sign "$identity" "$frameworks/libonnxruntime.1.23.2.dylib"
codesign --force --options runtime --sign "$identity" "$frameworks/libmoonshine.dylib"
codesign --force --options runtime --entitlements "$root/app/VoiceControl.entitlements" --sign "$identity" "$bundle"
codesign --verify --deep --strict --verbose=2 "$bundle"
echo "$bundle"
