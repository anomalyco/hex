#!/bin/sh
# Builds `Hex Playground.app`: the Scrawl capture capsule in a window of its
# own (`hex capsule-playground`), driven by a fake host with live tuning.
# It touches no Hex data, settings, or captures, and records and sends
# nothing, so it runs beside the installed Hex. Ad-hoc signed, for this Mac.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
export MACOSX_DEPLOYMENT_TARGET=15.0
cargo build --locked --release --features capture --bin voice-control --manifest-path "$root/Cargo.toml"
target=${CARGO_TARGET_DIR:-$root/target}
bundle="${HEX_PLAYGROUND_OUT:-$root/target/app}/Hex Playground.app"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$target/release/voice-control" "$bundle/Contents/MacOS/hex"
cat > "$bundle/Contents/MacOS/playground" <<'SH'
#!/bin/sh
exec "$(dirname "$0")/hex" capsule-playground
SH
chmod +x "$bundle/Contents/MacOS/playground"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>playground</string>
  <key>CFBundleIdentifier</key><string>anomaly.hex.playground</string>
  <key>CFBundleName</key><string>Hex Playground</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --sign - "$bundle/Contents/MacOS/hex"
codesign --force --sign - "$bundle"
echo "$bundle"
