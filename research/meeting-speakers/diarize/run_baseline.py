"""Hand-built baseline: Silero VAD -> sliding-window embeddings -> agglomerative
clustering. Every step is small enough to write in Rust on ONNX Runtime.

usage: run_baseline.py --embedder campplus [--threshold 0.6] --out DIR audio.wav...
"""

import argparse
import json
import time
from pathlib import Path

import numpy as np
import sherpa_onnx
import soundfile as sf
from scipy.cluster.hierarchy import fcluster, linkage

from onnx_models import SILERO, embed, extractor

SR = 16000
WIN, HOP = 1.5, 0.75
STEP = 0.01


def speech_regions(audio: np.ndarray) -> list[tuple[float, float]]:
    config = sherpa_onnx.VadModelConfig()
    config.silero_vad.model = str(SILERO)
    config.silero_vad.min_silence_duration = 0.3
    config.silero_vad.min_speech_duration = 0.2
    config.silero_vad.max_speech_duration = 60
    config.sample_rate = SR
    vad = sherpa_onnx.VoiceActivityDetector(config, buffer_size_in_seconds=120)
    regions = []
    window = config.silero_vad.window_size

    def drain():
        while not vad.empty():
            seg = vad.front
            regions.append((seg.start / SR, (seg.start + len(seg.samples)) / SR))
            vad.pop()

    for i in range(0, len(audio), window):
        vad.accept_waveform(audio[i : i + window])
        drain()
    vad.flush()
    drain()
    return regions


def diarize(audio: np.ndarray, ex, threshold: float):
    regions = speech_regions(audio)
    windows = []
    for a, b in regions:
        if b - a <= WIN:
            windows.append((a, b))
            continue
        t = a
        while t + WIN < b:
            windows.append((t, t + WIN))
            t += HOP
        windows.append((b - WIN, b))
    windows = [(a, b) for a, b in windows if b - a >= 0.4]
    if not windows:
        return [], {}
    embs = np.stack([embed(ex, audio[int(a * SR) : int(b * SR)]) for a, b in windows])
    if len(windows) == 1:
        labels = np.array([1])
    else:
        labels = fcluster(linkage(embs, method="average", metric="cosine"), t=threshold, criterion="distance")
    # Fold clusters holding under 2% of windows into their nearest big cluster.
    centroids = {c: embs[labels == c].mean(0) for c in set(labels)}
    big = [c for c in centroids if (labels == c).mean() >= 0.02] or list(centroids)
    for c in set(labels) - set(big):
        target = max(big, key=lambda k: float(centroids[c] @ centroids[k]))
        labels[labels == c] = target
    # Per 10 ms step inside speech, the label of the window whose centre is closest.
    centres = np.array([(a + b) / 2 for a, b in windows])
    segments = []
    for a, b in regions:
        steps = np.arange(a, b, STEP)
        lab = labels[np.abs(centres[None, :] - steps[:, None]).argmin(1)]
        start = a
        for i in range(1, len(steps) + 1):
            if i == len(steps) or lab[i] != lab[i - 1]:
                end = steps[i] if i < len(steps) else b
                segments.append((start, end, int(lab[i - 1])))
                start = end
    names = {c: f"spk{i:02d}" for i, c in enumerate(sorted(set(labels)))}
    voiceprints = {names[c]: embs[labels == c].mean(0) for c in set(labels)}
    return [(a, b, names[c]) for a, b, c in segments], voiceprints


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--embedder", default="campplus")
    ap.add_argument("--threshold", type=float, default=0.6)
    ap.add_argument("--out", required=True)
    ap.add_argument("files", nargs="+")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    t = time.time()
    ex = extractor(args.embedder)
    load = time.time() - t
    for f in map(Path, args.files):
        audio, sr = sf.read(f, dtype="float32")
        assert sr == SR
        t = time.time()
        segments, voiceprints = diarize(audio, ex, args.threshold)
        wall = time.time() - t
        with open(out / f"{f.stem}.rttm", "w") as w:
            for a, b, s in segments:
                w.write(f"SPEAKER {f.stem} 1 {a:.3f} {b - a:.3f} <NA> <NA> {s} <NA> <NA>\n")
        meta = dict(system=f"baseline-vad-{args.embedder}-ahc", file=f.name, audio_seconds=len(audio) / sr,
                    wall_seconds=wall, load_seconds=load, speakers=len(voiceprints),
                    embeddings={k: [round(float(x), 5) for x in v] for k, v in voiceprints.items()})
        (out / f"{f.stem}.json").write_text(json.dumps(meta))
        print(f"{f.stem}: {len(audio) / sr:.0f}s audio in {wall:.1f}s ({len(audio) / sr / wall:.1f}x RT), {len(voiceprints)} speakers", flush=True)


if __name__ == "__main__":
    main()
