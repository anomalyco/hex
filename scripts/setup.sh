#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
uv venv .venv
uv pip install --python .venv/bin/python moonshine-voice==0.0.68
.venv/bin/moonshine-voice download --stt --language en --model-arch 5
