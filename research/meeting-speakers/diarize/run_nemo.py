"""NVIDIA NeMo-Speech.cpp (C++/ggml, Metal): Streaming Sortformer v2 and
Nemotron-3-Diarization, one CLI call per file under /usr/bin/time -l.

usage: run_nemo.py --bin nemo-speech --name NAME --out DIR [--args "..."] audio.wav...
"""

import argparse
import json
import re
import shlex
import subprocess
import tempfile
import time
from pathlib import Path

import soundfile as sf


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--name", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--args", default="")
    ap.add_argument("files", nargs="+")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    peak = 0
    # Each call loads the model; time a 2 s clip (after one warm-up) as load cost.
    import numpy as np
    tiny = out / "_load.wav"
    sf.write(tiny, (np.random.default_rng(0).standard_normal(32000) * 0.01).astype("float32"), 16000)
    load = 0.0
    for _ in range(2):
        t = time.time()
        subprocess.run([args.bin, "diarize", str(tiny), "--format", "rttm", "--output", str(out / "_load.rttm"),
                        *shlex.split(args.args)], capture_output=True)
        load = time.time() - t
    tiny.unlink()
    (out / "_load.rttm").unlink(missing_ok=True)
    for f in map(Path, args.files):
        info = sf.info(f)
        rttm = out / f"{f.stem}.rttm"
        with tempfile.NamedTemporaryFile(suffix=".time") as tf:
            cmd = ["/usr/bin/time", "-l", "-o", tf.name, args.bin, "diarize", str(f),
                   "--format", "rttm", "--output", str(rttm), *shlex.split(args.args)]
            t = time.time()
            run = subprocess.run(cmd, capture_output=True, text=True)
            wall = time.time() - t
            timing = Path(tf.name).read_text()
        if run.returncode:
            print(f"{f.stem}: failed\n{run.stderr[-2000:]}", flush=True)
            continue
        m = re.search(r"(\d+)\s+peak memory footprint", timing)
        peak = max(peak, int(m.group(1)) if m else 0)
        # The CLI names the file by its own uri; rewrite it to the stem.
        lines = []
        for line in rttm.read_text().splitlines():
            p = line.split()
            if p and p[0] == "SPEAKER":
                p[1] = f.stem
                lines.append(" ".join(p))
        rttm.write_text("\n".join(lines) + "\n")
        speakers = len({l.split()[7] for l in lines})
        meta = dict(system=args.name, file=f.name, audio_seconds=info.duration, wall_seconds=max(wall - load, 1e-3),
                    raw_wall_seconds=wall, load_seconds=load, speakers=speakers)
        (out / f"{f.stem}.json").write_text(json.dumps(meta))
        print(f"{f.stem}: {info.duration:.0f}s audio in {wall:.1f}s incl. {load:.1f}s load "
              f"({info.duration / max(wall - load, 1e-3):.1f}x RT after load), {speakers} speakers", flush=True)
    (out / "time.txt").write_text(f"{peak} peak memory footprint\n")


if __name__ == "__main__":
    main()
