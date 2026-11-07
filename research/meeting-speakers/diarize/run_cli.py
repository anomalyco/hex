"""Run a diarization CLI once per file under /usr/bin/time -l.

usage: run_cli.py --name NAME --out DIR -- CMD ARGS... -- audio.wav...
ARGS may hold {in}, {out} (RTTM path), {json} (a JSON result with segments,
converted to RTTM) or {txt} (transcribe-cli's "[t0 -> t1] S1" lines). A 2 s clip is run twice first; its second wall time is
subtracted as model-load cost.
"""

import json
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import numpy as np
import soundfile as sf


def segments_from_json(data) -> list[tuple[float, float, str]]:
    """Find a list of {speaker, start, end}-like dicts anywhere in the JSON."""
    found = []

    def walk(x):
        if isinstance(x, list) and x and all(isinstance(i, dict) for i in x):
            keys = set(x[0])
            s = next((k for k in ("startTimeSeconds", "start", "startTime", "start_time") if k in keys), None)
            e = next((k for k in ("endTimeSeconds", "end", "endTime", "end_time") if k in keys), None)
            p = next((k for k in ("speakerId", "speaker", "speaker_id", "label") if k in keys), None)
            if s and e and p:
                found.extend((float(i[s]), float(i[e]), str(i[p])) for i in x)
                return
        if isinstance(x, dict):
            for v in x.values():
                walk(v)
        elif isinstance(x, list):
            for v in x:
                walk(v)

    walk(data)
    return found


def run_one(cmd: list[str], wav: Path, rttm: Path) -> tuple[float, int, str]:
    js = rttm.with_suffix(".out.json")
    args = [a.replace("{in}", str(wav)).replace("{out}", str(rttm)).replace("{json}", str(js))
            .replace("{txt}", str(rttm.with_suffix(".out.txt"))) for a in cmd]
    with tempfile.NamedTemporaryFile(suffix=".time") as tf:
        t = time.time()
        run = subprocess.run(["/usr/bin/time", "-l", "-o", tf.name, *args], capture_output=True, text=True)
        wall = time.time() - t
        timing = Path(tf.name).read_text()
    if run.returncode:
        raise RuntimeError(run.stderr[-2000:] + run.stdout[-1000:])
    txt = rttm.with_suffix(".out.txt")
    if txt.exists():  # transcribe-cli: "[   0.32 ->    2.40] S1"
        segs = [(float(a), float(b), s) for a, b, s in re.findall(r"\[\s*([\d.]+)\s*->\s*([\d.]+)\]\s*S(\d+)", txt.read_text())]
        rttm.write_text("".join(f"SPEAKER {wav.stem} 1 {s:.3f} {e - s:.3f} <NA> <NA> spk{p} <NA> <NA>\n" for s, e, p in segs))
        txt.unlink()
    elif js.exists():
        segs = segments_from_json(json.loads(js.read_text()))
        rttm.write_text("".join(f"SPEAKER {wav.stem} 1 {s:.3f} {e - s:.3f} <NA> <NA> spk{p} <NA> <NA>\n" for s, e, p in segs))
        js.unlink()
    elif rttm.exists():
        lines = []
        for line in rttm.read_text().splitlines():
            p = line.split()
            if p and p[0] == "SPEAKER":
                p[1] = wav.stem
                lines.append(" ".join(p))
        rttm.write_text("\n".join(lines) + "\n")
    m = re.search(r"(\d+)\s+peak memory footprint", timing)
    return wall, int(m.group(1)) if m else 0, run.stdout


def main() -> None:
    argv = sys.argv[1:]
    name = argv[argv.index("--name") + 1]
    out = Path(argv[argv.index("--out") + 1])
    first = argv.index("--")
    second = argv.index("--", first + 1)
    cmd, files = argv[first + 1 : second], [Path(f) for f in argv[second + 1 :]]
    out.mkdir(parents=True, exist_ok=True)

    tiny = out / "_load.wav"
    sf.write(tiny, (np.random.default_rng(0).standard_normal(32000) * 0.01).astype("float32"), 16000)
    load = 0.0
    for _ in range(2):
        try:
            load = run_one(cmd, tiny, out / "_load.rttm")[0]
        except RuntimeError as e:
            print(f"{name}: warm-up failed: {e}", flush=True)
    tiny.unlink()
    (out / "_load.rttm").unlink(missing_ok=True)

    peak = 0
    for f in files:
        try:
            wall, mem, _ = run_one(cmd, f, out / f"{f.stem}.rttm")
        except RuntimeError as e:
            print(f"{name} {f.stem}: failed: {e}", flush=True)
            continue
        peak = max(peak, mem)
        dur = sf.info(f).duration
        speakers = len({l.split()[7] for l in (out / f"{f.stem}.rttm").read_text().splitlines() if l.strip()})
        (out / f"{f.stem}.json").write_text(json.dumps(dict(
            system=name, file=f.name, audio_seconds=dur, wall_seconds=max(wall - load, 1e-3),
            raw_wall_seconds=wall, load_seconds=load, speakers=speakers)))
        print(f"{name} {f.stem}: {dur:.0f}s audio, {wall:.1f}s wall incl. {load:.1f}s load, {speakers} speakers", flush=True)
    (out / "time.txt").write_text(f"{peak} peak memory footprint\n")


if __name__ == "__main__":
    main()
