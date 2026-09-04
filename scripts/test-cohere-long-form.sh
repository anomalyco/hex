#!/bin/sh
set -eu

if [ "$#" -ne 1 ] || [ "$(uname -s)" != Darwin ]; then
  echo "Usage (macOS): $0 /path/to/cohere-transcribe-03-2026-Q8_0.gguf" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
model=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$1")
test -f "$model"
fixtures=$(mktemp -d "${TMPDIR:-/tmp}/hex-cohere-fixtures.XXXXXX")
cleanup() {
  result=$?
  if [ "$result" -eq 0 ] && [ "${HEX_KEEP_COHERE_FIXTURES:-0}" != 1 ]; then
    rm -rf "$fixtures"
  else
    echo "Synthetic fixture evidence retained at $fixtures" >&2
  fi
}
trap cleanup EXIT

python3 - "$root/tests/fixtures/cohere-long-form.txt" "$fixtures" <<'PY'
from pathlib import Path
import sys
paragraphs = Path(sys.argv[1]).read_text().strip().split("\n\n")
assert len(paragraphs) == 3
for name, text in zip(("opening", "middle", "ending"), paragraphs):
    (Path(sys.argv[2]) / f"{name}.txt").write_text(text)
PY

for part in opening middle ending; do
  /usr/bin/say -v Samantha -r 165 -f "$fixtures/$part.txt" \
    -o "$fixtures/$part.wav" --file-format=WAVE --data-format=LEI16@16000
done

python3 - "$fixtures" <<'PY'
from pathlib import Path
import sys, wave
root = Path(sys.argv[1])
parts = []
for name in ("opening", "middle", "ending"):
    with wave.open(str(root / f"{name}.wav"), "rb") as audio:
        assert (audio.getnchannels(), audio.getsampwidth(), audio.getframerate()) == (1, 2, 16000)
        parts.append(audio.readframes(audio.getnframes()))
for name, pause, repeats in (("long-continuous", 0, 1), ("long-pauses", 8, 1), ("long-repeated", 8, 3)):
    with wave.open(str(root / f"{name}.wav"), "wb") as audio:
        audio.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
        audio.writeframes((b"\0\0" * (16000 * pause)).join(parts * repeats))
PY

# This runs inference on generated speech only, without a listener or microphone.
cd "$root"
HEX_APPLICATION_SUPPORT_DIR="$fixtures/support" \
HEX_COHERE_MODEL="$model" HEX_COHERE_FIXTURES="$fixtures" \
  cargo test --locked --release --bin voice-control \
    cohere_long_form_keeps_all_sections_with_and_without_pauses -- --ignored --nocapture
