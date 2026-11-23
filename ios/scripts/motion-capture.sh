#!/bin/zsh
# Records the keyboard strip's motion scenes on a simulator: a whole dictation, a cancel, and the
# bar's own transitions, at 1x and 0.25x and with Reduce Motion, in light and dark, each with a
# per-frame trace (motion-trace.ndjson) beside its .mov.
# Usage: SIM_UDID=<udid> [LEASE_OUT=<dir>] scripts/motion-capture.sh [name-prefix]
set -uo pipefail
cd "${0:A:h}/.."
: "${SIM_UDID:?}"
tag=${1:-after}
out=${LEASE_OUT:-${TMPDIR:-/tmp}/hex-motion}
dd=${SIM_DD:-${TMPDIR:-/tmp}/hex-motion-dd}
mkdir -p "$out"
command -v xcodegen >/dev/null || brew install xcodegen
scripts/fetch-cleanup-model.sh || exit 66
xcodegen generate >/dev/null || exit 67
xcodebuild build -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=$SIM_UDID" \
  -derivedDataPath "$dd" -quiet > "$out/build.log" 2>&1 || { grep -E "error:" "$out/build.log" | head -20; exit 65; }
app=$(find "$dd/Build/Products" -maxdepth 2 -name 'HEX.app' -path '*iphonesimulator*' | head -1)
[[ -d $app ]] || { echo "no app"; exit 68; }
xcrun simctl boot "$SIM_UDID" 2>/dev/null
xcrun simctl bootstatus "$SIM_UDID" -b >/dev/null
xcrun simctl status_bar "$SIM_UDID" override --time 9:41 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3 2>/dev/null
xcrun simctl install "$SIM_UDID" "$app" || exit 69
bundle=anomaly.hex.mobile
docs="$(xcrun simctl get_app_container "$SIM_UDID" $bundle data)/Documents"

run() { # name seconds args...
  local name=$1 secs=$2; shift 2
  xcrun simctl terminate "$SIM_UDID" $bundle 2>/dev/null
  rm -f "$docs/motion-trace.ndjson"
  xcrun simctl launch "$SIM_UDID" $bundle --motion-trace "$@" >/dev/null
  sleep 2.6
  xcrun simctl io "$SIM_UDID" recordVideo --codec=h264 --force "$out/$name.mov" 2>/dev/null &
  local rec=$!
  sleep "$secs"
  kill -INT $rec
  wait $rec 2>/dev/null
  sleep 1.2
  cp "$docs/motion-trace.ndjson" "$out/$name.ndjson" 2>/dev/null || echo "no trace for $name"
}

for look in light dark; do
  sfx=${look:/light/}; sfx=${sfx:/dark/-dark}
  run $tag-full-$look 8.6 --snap-state full-keyboard-sequence$sfx
  run $tag-cancel-$look 5.8 --snap-state full-keyboard-cancel-sequence$sfx
  run $tag-strip-$look 9.4 --snap-state full-keyboard-strip-sequence$sfx
  run $tag-full-slow-$look 30.5 --snap-state full-keyboard-sequence$sfx --motion-slow 4
  run $tag-cancel-slow-$look 19.5 --snap-state full-keyboard-cancel-sequence$sfx --motion-slow 4
  run $tag-strip-slow-$look 33.5 --snap-state full-keyboard-strip-sequence$sfx --motion-slow 4
  run $tag-full-rm-$look 8.6 --snap-state full-keyboard-sequence$sfx --reduce-motion
  run $tag-strip-rm-$look 9.4 --snap-state full-keyboard-strip-sequence$sfx --reduce-motion
done
xcrun simctl terminate "$SIM_UDID" $bundle 2>/dev/null
ls -la "$out"
