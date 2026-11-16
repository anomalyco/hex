#!/bin/bash
# Run under a render lease, in a synced checkout. No native windows or audio.
set -eu
case "${1:-}" in before|fitted|reduced) ;; *) echo 'usage: preview-offer-motion.sh before|fitted|reduced' >&2; exit 2;; esac
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    # scripts/check's code-shape step enumerates tracked sources; a render
    # worker sync may be a disposable source-only copy, without an index.
    git init -q
    git add .
fi
out=captures/offer-preview/$1
mkdir -p "$out"
frames=$(mktemp -d)
trap 'find "$frames" -type f -delete; rmdir "$frames"' EXIT
export HEX_OFFER_PREVIEW="$frames"
export HEX_OFFER_VARIANT="$1"
scripts/check offer_navigation_preview --bin hex
ffmpeg -loglevel error -y -framerate 60 -i "$frames/%03d.png" -an -c:v libx264 -crf 17 -pix_fmt yuv420p -movflags +faststart "$out/navigation.mp4"
cp "$frames/trace.csv" "$out/"
for f in 023 024 028 036 080 084 088 100; do cp "$frames/$f.png" "$out/"; done
