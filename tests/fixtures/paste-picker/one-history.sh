#!/bin/bash
# Run on a disposable Mac render worker. No application launch, microphone,
# screen recording or pasteboard. Only the rendered PNGs leave the lease.
set -euo pipefail
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export HEX_APPLICATION_SUPPORT_DIR="$scratch/hex" HEX_DISABLE_UPDATES=1
export HEX_PROOF_MODEL="$scratch/parakeet.gguf" HEX_PROOF_WAV="$scratch/fixture.wav"
export HEX_UNIFIED_HISTORY_RENDER_DIR="${PWD}/captures/one-history"
mkdir -p "$HEX_APPLICATION_SUPPORT_DIR/models" "$HEX_UNIFIED_HISTORY_RENDER_DIR"
pin() {
  curl --fail --silent --show-error --location --retry 2 --max-time 240 "$1" -o "$2"
  test "$(shasum -a 256 "$2" | cut -d' ' -f1)" = "$3"
}
pin https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v2-gguf/resolve/07cee0616125a08ef619729bb47f40ef747e4bc4/parakeet-tdt-0.6b-v2-Q8_0.gguf "$HEX_PROOF_MODEL" f0d0e99cebb6d3b83f1f7069b82b5d3c2e39a54545b0da039cb4bafd9c4e5caa
pin https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/a9b2d61c20ae4ddf3644edb34969e8185a485448/silero-vad-v6.2-F32.gguf "$HEX_APPLICATION_SUPPORT_DIR/models/silero-vad-v6.2-F32.gguf" 0d871a800c420fdd5d1b17a3aea4296d1c39608f0ff4a9295eb76457eb1c88e6
# -o writes the synthetic voice directly to a file, never to speakers.
/usr/bin/say -v Samantha -o "$scratch/fixture.aiff" 'Remember the purple notebook.'
/usr/bin/afconvert -f WAVE -d LEI16@48000 -c 1 "$scratch/fixture.aiff" "$HEX_PROOF_WAV"
cargo nextest run --locked --bin hex --run-ignored only --success-output immediate actual_cancel_to_mixed_history
