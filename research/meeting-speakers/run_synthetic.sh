#!/bin/bash
# Run the synthetic benchmark on the worker set up by setup.sh.
#   run_synthetic.sh tts      synthesize voices and lines (slow, resumable)
#   run_synthetic.sh eval     mix + degrade, Parakeet words, every diarizer, score
#   run_synthetic.sh score    score only
# Results (synthetic audio, RTTM, scores) are copied to $LEASE_OUT when set.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
SPIKE=${SPIKE:-$HOME/scratch/meeting-speakers}
export HF_HOME=$SPIKE/hf OUT=$SPIKE/out MODELS=$SPIKE/models RESULTS=$SPIKE/out/results
export PATH=$SPIKE/bin:/opt/homebrew/bin:$PATH
NS=$SPIKE/nemo-speech-0.3.0-macos-aarch64-metal/bin/nemo-speech
TTS=$SPIKE/tts/bin/python
DIAR=$SPIKE/diar/bin/python
timed() { local dir=$1; shift; mkdir -p "$dir"; /usr/bin/time -l -o "$dir/time.txt" "$@"; }

if [ "${1:-}" = all ]; then
  "$0" tts && exec "$0" eval
fi

case "${1:-eval}" in
tts)
  $TTS "$HERE/synthetic/tts.py" "${@:2}"
  ;;
eval)
  $TTS "$HERE/synthetic/mix.py"
  FILES=("$OUT"/calls/call?.wav)
  $TTS "$HERE/synthetic/asr.py" --out "$OUT/asr" "${FILES[@]}"
  cd "$HERE/diarize"
  timed "$RESULTS/pyannote-community1-mps" $DIAR run_pyannote.py --device mps --out "$RESULTS/pyannote-community1-mps" "${FILES[@]}"
  timed "$RESULTS/pyannote-community1-cpu" $DIAR run_pyannote.py --device cpu --out "$RESULTS/pyannote-community1-cpu" "${FILES[@]}"
  timed "$RESULTS/sherpa-seg3-campplus" $DIAR run_sherpa.py --embedder campplus --out "$RESULTS/sherpa-seg3-campplus" "${FILES[@]}"
  timed "$RESULTS/sherpa-seg3-campplus-t0.8" $DIAR run_sherpa.py --embedder campplus --threshold 0.8 --out "$RESULTS/sherpa-seg3-campplus-t0.8" "${FILES[@]}"
  timed "$RESULTS/sherpa-seg3-wespeaker" $DIAR run_sherpa.py --embedder wespeaker-r34 --out "$RESULTS/sherpa-seg3-wespeaker" "${FILES[@]}"
  timed "$RESULTS/baseline-vad-campplus-ahc" $DIAR run_baseline.py --embedder campplus --out "$RESULTS/baseline-vad-campplus-ahc" "${FILES[@]}"
  $DIAR run_nemo.py --bin "$NS" --name sortformer-v2-stream --out "$RESULTS/sortformer-v2-stream" --args="--model sortformer --force" "${FILES[@]}"
  $DIAR run_nemo.py --bin "$NS" --name sortformer-v2-offline-preset --out "$RESULTS/sortformer-v2-offline-preset" --args="--model sortformer --preset offline --force" "${FILES[@]}"
  $DIAR run_nemo.py --bin "$NS" --name nemotron3-stream --out "$RESULTS/nemotron3-stream" --args="--model nemotron-3-diarization --force" "${FILES[@]}"
  $DIAR run_nemo.py --bin "$NS" --name nemotron3-offline-preset --out "$RESULTS/nemotron3-offline-preset" --args="--model nemotron-3-diarization --preset v3-offline --force" "${FILES[@]}"
  $DIAR run_nemo.py --bin "$NS" --name nemotron3-full-attention --out "$RESULTS/nemotron3-full-attention" --args="--model nemotron-3-diarization --offline --force" "${FILES[@]}"
  "$HERE/run_fluidaudio.sh" || echo "FluidAudio failed; scoring without it"
  cd "$HERE"
  $DIAR score.py
  ;;
score)
  cd "$HERE" && $DIAR score.py "${@:2}"
  ;;
esac

if [ -n "${LEASE_OUT:-}" ]; then
  mkdir -p "$LEASE_OUT/synthetic"
  rsync -a --exclude turns "$OUT/" "$LEASE_OUT/synthetic/"
fi
