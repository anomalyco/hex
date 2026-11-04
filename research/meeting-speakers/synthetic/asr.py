"""Parakeet TDT (MLX) words and sentences with times, for word-level vs
segment-level speaker attribution.

usage: asr.py --out DIR audio.wav...   ->  DIR/<stem>.json
"""

import argparse
import json
import time
from pathlib import Path

from parakeet_mlx import from_pretrained

MODEL = "mlx-community/parakeet-tdt-0.6b-v3"


def words_of(sentence) -> list[dict]:
    words = []
    for tok in sentence.tokens:
        if not words or tok.text.startswith(" "):
            words.append(dict(text=tok.text.strip(), start=tok.start, end=tok.end))
        else:
            words[-1]["text"] += tok.text
            words[-1]["end"] = tok.end
    return [w for w in words if w["text"]]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("files", nargs="+")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    model = from_pretrained(MODEL)
    for f in map(Path, args.files):
        t = time.time()
        result = model.transcribe(str(f))
        sentences = [dict(text=s.text.strip(), start=s.start, end=s.end, words=words_of(s)) for s in result.sentences]
        (out / f"{f.stem}.json").write_text(json.dumps(dict(file=f.name, sentences=sentences)))
        print(f"{f.stem}: {len(sentences)} sentences in {time.time() - t:.1f}s", flush=True)


if __name__ == "__main__":
    main()
