#!/bin/sh
set -eu

model_dir="$HOME/Library/Application Support/voice-control/models"
model="$model_dir/parakeet-unified-en-0.6b-Q8_0.gguf"
partial="$model.partial"
revision="7e948f21b7bdbac698d3318db9d350f1096f3b6c"
sha256="4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795"

mkdir -p "$model_dir"
if [ -f "$model" ] && echo "$sha256  $model" | shasum -a 256 -c - >/dev/null 2>&1; then
  exit 0
fi
curl -fL --retry 3 --continue-at - \
  "https://huggingface.co/handy-computer/parakeet-unified-en-0.6b-gguf/resolve/$revision/parakeet-unified-en-0.6b-Q8_0.gguf" \
  -o "$partial"
echo "$sha256  $partial" | shasum -a 256 -c -
mv "$partial" "$model"
