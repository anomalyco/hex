#!/bin/bash
# Sortformer v2.1 through transcribe.cpp, the runtime Hex already links, at
# Hex's pinned rev. Builds transcribe-cli with Metal, then diarizes
#   run_transcribecpp.sh synthetic   $SPIKE/out/calls/call?.wav, then re-scores
#   run_transcribecpp.sh real        $SPIKE/private/m?.wav (results stay private)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
SPIKE=${SPIKE:-$HOME/scratch/meeting-speakers}
export HF_HOME=$SPIKE/hf OUT=$SPIKE/out MODELS=$SPIKE/models RESULTS=$SPIKE/out/results
export PATH=$SPIKE/bin:/opt/homebrew/bin:$PATH
REV=${TRANSCRIBE_CPP_REV:-2778ae4bf5ca5f394d129a39467f4ad9552dc34f}
TC=$SPIKE/transcribe.cpp
DIAR=$SPIKE/diar/bin/python
if [ ! -d "$TC" ]; then
  git init -q "$TC"
  git -C "$TC" fetch -q --depth 1 https://github.com/anomalyco/transcribe.cpp "$REV"
  git -C "$TC" checkout -q FETCH_HEAD
fi
cmake -S "$TC" -B "$TC/build" -DCMAKE_BUILD_TYPE=Release >/dev/null
cmake --build "$TC/build" --target transcribe-cli -j 8 2>&1 | tail -2
CLI=$(find "$TC/build" -name transcribe-cli -type f -perm -u+x | head -1)
GGUF=$MODELS/diar_streaming_sortformer_4spk-v2.1-Q8_0.gguf
[ -f "$GGUF" ] || curl -sSfL -o "$GGUF" \
  https://huggingface.co/handy-computer/diar_streaming_sortformer_4spk-v2.1-gguf/resolve/main/diar_streaming_sortformer_4spk-v2.1-Q8_0.gguf

case "${1:-synthetic}" in
synthetic)
  $DIAR "$HERE/diarize/run_cli.py" --name transcribecpp-sortformer-v21 --out "$RESULTS/transcribecpp-sortformer-v21" \
    -- "$CLI" -q -m "$GGUF" -o {txt} {in} -- "$OUT"/calls/call?.wav
  cd "$HERE" && $DIAR score.py
  ;;
real)
  R=$SPIKE/private/results
  $DIAR "$HERE/diarize/run_cli.py" --name transcribecpp-sortformer-v21 --out "$R/transcribecpp-sortformer-v21" \
    -- "$CLI" -q -m "$GGUF" -o {txt} {in} -- "$SPIKE"/private/m?.wav
  cd "$HERE" && $DIAR real_voiceprints.py "$SPIKE/private" "$R"
  ;;
esac
