#!/bin/bash
# Set up the spike on a macOS (Apple Silicon) worker. Everything lands in
# $SPIKE (default ~/scratch/meeting-speakers), outside the synced worktree:
# two uv venvs (tts: MLX; diar: torch/pyannote/sherpa-onnx), NeMo-Speech.cpp,
# and the ONNX models. Safe to re-run.
set -euo pipefail
SPIKE=${SPIKE:-$HOME/scratch/meeting-speakers}
mkdir -p "$SPIKE"/{models,bin,hf}
cd "$SPIKE"
export HF_HOME=$SPIKE/hf UV_CACHE_DIR=$SPIKE/uv-cache

if ! command -v uv >/dev/null; then
  curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR="$SPIKE/bin" UV_NO_MODIFY_PATH=1 sh
  export PATH=$SPIKE/bin:$PATH
fi
if ! command -v ffmpeg >/dev/null && [ ! -x "$SPIKE/bin/ffmpeg" ]; then
  curl -LsSf -o ffmpeg.zip https://evermeet.cx/ffmpeg/getrelease/zip && unzip -o -q ffmpeg.zip -d bin && rm ffmpeg.zip
fi

[ -d tts ] || uv venv -q --python 3.12 tts
VIRTUAL_ENV=$SPIKE/tts uv pip install -q mlx-audio parakeet-mlx soundfile scipy numpy

[ -d diar ] || uv venv -q --python 3.12 diar
VIRTUAL_ENV=$SPIKE/diar uv pip install -q "pyannote.audio>=4" pyannote.metrics sherpa-onnx soundfile scipy numpy

NS=nemo-speech-0.3.0-macos-aarch64-metal
if [ ! -d "$NS" ]; then
  curl -LsSf -O "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/download/v0.3.0/$NS.tar.gz"
  curl -LsSf -O "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/download/v0.3.0/$NS.tar.gz.sha256"
  shasum -a 256 -c "$NS.tar.gz.sha256"
  tar xzf "$NS.tar.gz" && rm "$NS.tar.gz" "$NS.tar.gz.sha256"
fi

cd models
REL=https://github.com/k2-fsa/sherpa-onnx/releases/download
[ -d sherpa-onnx-pyannote-segmentation-3-0 ] || { curl -LsSf "$REL/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2" | tar xj; }
for m in 3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx wespeaker_en_voxceleb_resnet34_LM.onnx; do
  [ -f "$m" ] || curl -LsSf -O "$REL/speaker-recongition-models/$m"
done
[ -f silero_vad.onnx ] || curl -LsSf -O "$REL/asr-models/silero_vad.onnx"
ls -la "$SPIKE/models"
find "$SPIKE/$NS" -maxdepth 2 | head -30
"$SPIKE/$NS/bin/nemo-speech" --version || true
"$SPIKE/$NS/bin/nemo-speech" help diarize || true
"$SPIKE/$NS/bin/nemo-speech" model list || true
du -sh "$SPIKE"/* 2>/dev/null
