#!/bin/sh
set -eu

if [ "$#" -lt 2 ]; then
  echo "usage: $0 OUTPUT TARGET [PREVIEW OPTIONS...]" >&2
  exit 2
fi

output=$1
shift
target=$1
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

cd "$root"
if [ -n "${HEX_PREVIEW_BINARY:-}" ]; then
  binary=$HEX_PREVIEW_BINARY
else
  binary=$root/target/release/voice-control
fi
if [ -z "${HEX_PREVIEW_BINARY:-}" ] && [ "${HEX_PREVIEW_SKIP_BUILD:-0}" != "1" ]; then
  CMAKE=${CMAKE:-/opt/homebrew/lib/python3.10/site-packages/cmake/data/bin/cmake} \
    cargo build --release --bin voice-control
elif [ ! -x "$binary" ]; then
  echo "preview binary does not exist: $binary" >&2
  exit 1
fi

"$binary" preview "$@" &
pid=$!
cleanup() {
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

window_id=$(swift - "$pid" "$target" <<'SWIFT'
import CoreGraphics
import Foundation

let pid = Int(CommandLine.arguments[1])!
let target = CommandLine.arguments[2]
for _ in 0..<200 {
    let windows = CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly, .excludeDesktopElements],
        kCGNullWindowID
    ) as! [[String: Any]]
    let candidates = windows.filter { window in
        guard (window[kCGWindowOwnerPID as String] as? Int) == pid else {
            return false
        }
        if target == "dictation-hud" {
            return true
        }
        let title = window[kCGWindowName as String] as? String
        let bounds = window[kCGWindowBounds as String] as? [String: CGFloat]
        return (title == nil || title == "" || title == "HEX")
            && (bounds?["Width"] ?? 0) >= 800
    }
    if let window = candidates.max(by: {
        let lhs = $0[kCGWindowBounds as String] as! [String: CGFloat]
        let rhs = $1[kCGWindowBounds as String] as! [String: CGFloat]
        return lhs["Width"]! * lhs["Height"]! < rhs["Width"]! * rhs["Height"]!
    }) {
        print(window[kCGWindowNumber as String] as! Int)
        exit(0)
    }
    Thread.sleep(forTimeInterval: 0.05)
}
exit(1)
SWIFT
)

mkdir -p "$(dirname -- "$output")"
screencapture -x -l "$window_id" "$output"
echo "$output"
