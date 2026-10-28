#!/bin/sh
set -eu

# Regenerates the installer window from background.html: Hex's keycap
# (src/rune_ui/dmg_keycap.rs, drawn with Rune), background.tiff (1× and 2×),
# and DS_Store (the Finder layout). Needs macOS, a Chromium-family browser
# (CHROME, else Google Chrome or Brave), and uv.
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../../.." && pwd)
chrome=${CHROME:-}
for candidate in "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"; do
  [ -n "$chrome" ] || { [ -x "$candidate" ] && chrome=$candidate; }
done
[ -n "$chrome" ] || { echo "Set CHROME to a Chromium-family browser." >&2; exit 1; }
work=$(mktemp -d "${TMPDIR:-/tmp}/hex-dmg.XXXXXX")
trap 'hdiutil detach -quiet "$work/mnt" 2>/dev/null || true; rm -rf "$work"' EXIT HUP INT TERM

(cd "$root" && cargo test --locked --bin hex \
  rune_ui::dmg_keycap::render -- --ignored --exact)

for scale in 1 2; do
  "$chrome" --headless --disable-gpu --hide-scrollbars --user-data-dir="$work/profile" \
    --force-device-scale-factor="$scale" --window-size=660,400 \
    --screenshot="$work/background@${scale}x.png" "file://$here/background.html" 2>/dev/null
done
tiffutil -cathidpicheck "$work/background@1x.png" "$work/background@2x.png" \
  -out "$here/background.tiff" >/dev/null 2>&1

mkdir "$work/Hex.app"
uv run --quiet --with dmgbuild==1.6.7 dmgbuild -s "$here/layout.py" \
  -D app="$work/Hex.app" -D background="$here/background.tiff" Hex "$work/layout.dmg"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$work/mnt" "$work/layout.dmg"
cp "$work/mnt/.DS_Store" "$here/DS_Store"
