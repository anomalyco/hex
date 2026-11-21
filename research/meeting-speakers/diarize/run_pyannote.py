"""pyannote community-1 (pyannote.audio 4): segmentation-3.0 + WeSpeaker ResNet34 + VBx.

usage: run_pyannote.py --device cpu|mps --out DIR audio.wav...
Writes DIR/<stem>.rttm (overlap-aware), DIR/<stem>.exclusive.rttm, DIR/<stem>.json.
"""

import argparse
import json
import time
from pathlib import Path

import numpy as np
import soundfile as sf
import torch
from pyannote.audio import Pipeline

REPO = "pyannote-community/speaker-diarization-community-1"


def write_rttm(path: Path, uri: str, annotation) -> None:
    with open(path, "w") as f:
        for seg, _, label in annotation.itertracks(yield_label=True):
            f.write(f"SPEAKER {uri} 1 {seg.start:.3f} {seg.duration:.3f} <NA> <NA> {label} <NA> <NA>\n")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--device", default="cpu")
    ap.add_argument("--out", required=True)
    ap.add_argument("files", nargs="+")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    t = time.time()
    pipe = Pipeline.from_pretrained(REPO)
    pipe.to(torch.device(args.device))
    pipe({"waveform": torch.randn(1, 16000 * 10) * 0.01, "sample_rate": 16000})  # warm-up
    load = time.time() - t

    for f in map(Path, args.files):
        audio, sr = sf.read(f, dtype="float32")
        wave = torch.from_numpy(audio).unsqueeze(0)
        t = time.time()
        result = pipe({"waveform": wave, "sample_rate": sr})
        wall = time.time() - t
        diar = getattr(result, "speaker_diarization", result)
        excl = getattr(result, "exclusive_speaker_diarization", diar)
        write_rttm(out / f"{f.stem}.rttm", f.stem, diar)
        write_rttm(out / f"{f.stem}.exclusive.rttm", f.stem, excl)
        emb = getattr(result, "speaker_embeddings", None)
        embeddings = {}
        if emb is not None:
            for label, vec in zip(diar.labels(), np.asarray(emb)):
                embeddings[label] = [round(float(v), 5) for v in vec]
        meta = dict(system=f"pyannote-community-1-{args.device}", file=f.name, audio_seconds=len(audio) / sr,
                    wall_seconds=wall, load_seconds=load, speakers=len(diar.labels()), embeddings=embeddings)
        (out / f"{f.stem}.json").write_text(json.dumps(meta))
        print(f"{f.stem}: {len(audio) / sr:.0f}s audio in {wall:.1f}s ({len(audio) / sr / wall:.1f}x RT), {len(diar.labels())} speakers", flush=True)


if __name__ == "__main__":
    main()
