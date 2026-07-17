#!/bin/sh
set -eu

model_dir="$HOME/Library/Application Support/voice-control/models"
model="$model_dir/parakeet-tdt-0.6b-v2-Q8_0.gguf"
partial="$model.partial"
revision="07cee0616125a08ef619729bb47f40ef747e4bc4"
sha256="f0d0e99cebb6d3b83f1f7069b82b5d3c2e39a54545b0da039cb4bafd9c4e5caa"

mkdir -p "$model_dir"
if [ -f "$model" ] && echo "$sha256  $model" | shasum -a 256 -c - >/dev/null 2>&1; then
  exit 0
fi
curl -fL --retry 3 --continue-at - \
  "https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v2-gguf/resolve/$revision/parakeet-tdt-0.6b-v2-Q8_0.gguf" \
  -o "$partial"
echo "$sha256  $partial" | shasum -a 256 -c -
mv "$partial" "$model"
