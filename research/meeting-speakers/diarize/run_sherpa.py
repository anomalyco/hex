"""pyannote segmentation-3.0 + an embedding model + clustering, all through
sherpa-onnx (the shape Hex could ship on ONNX Runtime, with no Python).

usage: run_sherpa.py --embedder campplus|wespeaker-r34 [--threshold 0.5] --out DIR audio.wav...
"""

import argparse
import json
import time
from pathlib import Path

import sherpa_onnx
import soundfile as sf

from onnx_models import EMBEDDERS, SEGMENTATION


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--embedder", default="campplus")
    ap.add_argument("--threshold", type=float, default=0.5)
    ap.add_argument("--threads", type=int, default=4)
    ap.add_argument("--out", required=True)
    ap.add_argument("files", nargs="+")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    config = sherpa_onnx.OfflineSpeakerDiarizationConfig(
        segmentation=sherpa_onnx.OfflineSpeakerSegmentationModelConfig(
            pyannote=sherpa_onnx.OfflineSpeakerSegmentationPyannoteModelConfig(model=str(SEGMENTATION)),
            num_threads=args.threads,
        ),
        embedding=sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(EMBEDDERS[args.embedder]), num_threads=args.threads),
        clustering=sherpa_onnx.FastClusteringConfig(num_clusters=-1, threshold=args.threshold),
        min_duration_on=0.3,
        min_duration_off=0.5,
    )
    assert config.validate()
    t = time.time()
    sd = sherpa_onnx.OfflineSpeakerDiarization(config)
    load = time.time() - t

    for f in map(Path, args.files):
        audio, sr = sf.read(f, dtype="float32")
        assert sr == sd.sample_rate
        t = time.time()
        segments = sd.process(audio).sort_by_start_time()
        wall = time.time() - t
        labels = set()
        with open(out / f"{f.stem}.rttm", "w") as w:
            for s in segments:
                labels.add(s.speaker)
                w.write(f"SPEAKER {f.stem} 1 {s.start:.3f} {s.end - s.start:.3f} <NA> <NA> spk{s.speaker:02d} <NA> <NA>\n")
        meta = dict(system=f"sherpa-onnx-seg3-{args.embedder}", file=f.name, audio_seconds=len(audio) / sr,
                    wall_seconds=wall, load_seconds=load, speakers=len(labels))
        (out / f"{f.stem}.json").write_text(json.dumps(meta))
        print(f"{f.stem}: {len(audio) / sr:.0f}s audio in {wall:.1f}s ({len(audio) / sr / wall:.1f}x RT), {len(labels)} speakers", flush=True)


if __name__ == "__main__":
    main()
