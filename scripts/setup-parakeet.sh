#!/bin/sh
set -eu

model_dir="$HOME/Library/Application Support/voice-control/models"
archive="$(mktemp -t parakeet-v2).tar.gz"
trap 'rm -f "$archive"' EXIT

mkdir -p "$model_dir"
curl -fL --retry 3 \
  https://blob.handy.computer/parakeet-v2-int8.tar.gz \
  -o "$archive"
tar -xzf "$archive" -C "$model_dir"
