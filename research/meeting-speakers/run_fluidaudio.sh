#!/bin/bash
# FluidAudio (Swift, CoreML) on the synthetic calls: Nemotron-3 CoreML presets
# and the offline pyannote community-1 pipeline (VBx). Called by
# run_synthetic.sh eval after the calls are mixed.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
SPIKE=${SPIKE:-$HOME/scratch/meeting-speakers}
export OUT=$SPIKE/out MODELS=$SPIKE/models RESULTS=$SPIKE/out/results HF_HOME=$SPIKE/hf
export PATH=/opt/homebrew/bin:$PATH
FA=$SPIKE/FluidAudio
TAG=${FLUIDAUDIO_TAG:-v0.18.2}
[ -d "$FA" ] || git clone -q --depth 1 --branch "$TAG" https://github.com/FluidInference/FluidAudio "$FA"
(cd "$FA" && swift build -c release --product fluidaudiocli 2>&1 | tail -3)
CLI=$(cd "$FA" && swift build -c release --show-bin-path)/fluidaudiocli
DIAR=$SPIKE/diar/bin/python
FILES=("$OUT"/calls/call?.wav)

run() {  # name, then CLI args with {in} and {out} placeholders
  local name=$1; shift
  $DIAR "$HERE/diarize/run_cli.py" --name "$name" --out "$RESULTS/$name" -- "$CLI" "$@" -- "${FILES[@]}"
}
run fluidaudio-nemotron3-offline nemotron3-diarize {in} --variant offline --output {out}
run fluidaudio-nemotron3-fast128 nemotron3-diarize {in} --variant fast128 --output {out}
run fluidaudio-nemotron3-fast32 nemotron3-diarize {in} --variant fast32 --output {out}
run fluidaudio-community1-offline process {in} --mode offline --output {json}
