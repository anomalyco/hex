#!/usr/bin/env bash
# Renders each named scene in dark and light into $OUT; TREE=1 also dumps the dark element tree,
# SIZE=WxH renders at that window size (files get a -tall suffix).
set -euo pipefail
out="${OUT:?}"
mkdir -p "$out"
for scene in "$@"; do
  for theme in dark light; do
    extra=()
    suffix=""
    if [ -n "${TREE:-}" ] && [ "$theme" = dark ]; then extra+=(--tree "$out/$scene.json"); fi
    if [ -n "${SIZE:-}" ]; then extra+=(--size "$SIZE"); suffix="-tall"; fi
    cargo run -q ${FEATURES:+--features "$FEATURES"} -- app -- --snap "$out/$scene$suffix-$theme.png" --scene "$scene" --theme "$theme" "${extra[@]+"${extra[@]}"}" 2>/dev/null
  done
done
ls "$out"
