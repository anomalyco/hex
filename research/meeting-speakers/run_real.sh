#!/bin/bash
# Diarize private meeting audio already copied to $SPIKE/private/*.wav.
# Results stay in $SPIKE/private/results (never $LEASE_OUT): RTTMs, timings
# and voiceprints are pulled back to the owner's Mac by hand, then deleted.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
SPIKE=${SPIKE:-$HOME/scratch/meeting-speakers}
export HF_HOME=$SPIKE/hf MODELS=$SPIKE/models
NS=$SPIKE/nemo-speech-0.3.0-macos-aarch64-metal/bin/nemo-speech
DIAR=$SPIKE/diar/bin/python
R=$SPIKE/private/results
FILES=("$SPIKE"/private/m?.wav)
timed() { local dir=$1; shift; mkdir -p "$dir"; /usr/bin/time -l -o "$dir/time.txt" "$@"; }

cd "$HERE/diarize"
$DIAR run_nemo.py --bin "$NS" --name nemotron3-stream --out "$R/nemotron3-stream" --args="--model nemotron-3-diarization --force" "${FILES[@]}"
$DIAR run_nemo.py --bin "$NS" --name nemotron3-offline-preset --out "$R/nemotron3-offline-preset" --args="--model nemotron-3-diarization --preset v3-offline --force" "${FILES[@]}"
timed "$R/pyannote-community1-mps" $DIAR run_pyannote.py --device mps --out "$R/pyannote-community1-mps" "${FILES[@]}"
timed "$R/sherpa-seg3-campplus" $DIAR run_sherpa.py --embedder campplus --out "$R/sherpa-seg3-campplus" "${FILES[@]}"
cd "$HERE"
$DIAR real_voiceprints.py "$SPIKE/private" "$R"
ls -R "$R" | head -60
