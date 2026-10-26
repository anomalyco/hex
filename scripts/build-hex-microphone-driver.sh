#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_DIR="$ROOT_DIR/macos/HexMicrophoneDriver"
OUT_DIR="${1:-$HOME/Library/Caches/hex-mic}"
BUNDLE_DIR="$OUT_DIR/HexMicrophone.driver"
VERSION="${HEX_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -1)}"
BUILD_NUMBER="${HEX_BUILD_NUMBER:-$(printf '%s\n' "${VERSION%%-*}" | awk -F. '{ print ($1 * 10000) + ($2 * 100) + $3 }')}"

rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/Contents/MacOS"
cp "$SRC_DIR/Info.plist" "$BUNDLE_DIR/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$BUNDLE_DIR/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD_NUMBER" "$BUNDLE_DIR/Contents/Info.plist"

clang -std=c11 -O2 -Wall -Wextra -Werror \
  -bundle \
  -arch arm64 -arch x86_64 \
  -mmacosx-version-min=13.0 \
  -framework CoreAudio \
  -framework CoreFoundation \
  "$SRC_DIR/HexMicrophoneDriver.c" \
  -o "$BUNDLE_DIR/Contents/MacOS/HexMicrophoneDriver"

SIGN_ID="${VOICE_CONTROL_CODESIGN_IDENTITY:-}"
if [[ -z "$SIGN_ID" && -n "${VOICE_CONTROL_TEAM_ID:-}" ]]; then
  SIGN_ID="$(security find-identity -v -p codesigning | awk -F'"' -v team="$VOICE_CONTROL_TEAM_ID" '$2 ~ "^Developer ID Application: .*\\(" team "\\)$" { print $2; exit }')"
fi
if [[ -n "$SIGN_ID" ]]; then
  codesign --force --options runtime --timestamp --sign "$SIGN_ID" "$BUNDLE_DIR"
else
  codesign --force --sign - "$BUNDLE_DIR"
fi

if [[ "${HEX_SKIP_INSTALL_SCRIPT:-0}" != "1" ]]; then
  cat > "$OUT_DIR/install-driver.sh" <<EOF
#!/usr/bin/env bash
set -euo pipefail
sudo cp -R "$BUNDLE_DIR" /Library/Audio/Plug-Ins/HAL/ && (sudo launchctl kickstart -k system/com.apple.audio.coreaudiod || sudo killall coreaudiod)
EOF
  chmod +x "$OUT_DIR/install-driver.sh"
  echo "Install script ready at $OUT_DIR/install-driver.sh"
fi

echo "Built $BUNDLE_DIR (v$VERSION)"
