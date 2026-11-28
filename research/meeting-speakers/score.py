"""Score every system in $RESULTS against the synthetic truth in $OUT/calls.

- DER (0.25 s collar, overlap scored) and its parts, speaker-count error
- speed (x realtime) and peak memory (from <system>/time.txt, /usr/bin/time -l)
- voiceprints: one CAM++ embedding per found speaker, matched across calls
  (leave one call out) and against the clean reference clips
- word-level vs segment-level attribution of Parakeet words

usage: score.py [system ...]   ->  $RESULTS/summary.json
"""

import json
import os
import re
import sys
from pathlib import Path

import numpy as np
import soundfile as sf
from pyannote.core import Annotation, Segment
from pyannote.metrics.diarization import DiarizationErrorRate
from scipy.signal import resample_poly

sys.path.insert(0, str(Path(__file__).resolve().parent / "diarize"))
from onnx_models import embed, extractor  # noqa: E402

OUT = Path(os.environ.get("OUT", "out")).resolve()
RESULTS = Path(os.environ.get("RESULTS", OUT / "results")).resolve()
CALLS = OUT / "calls"
ASR = OUT / "asr"
SR = 16000
MATCH_THRESHOLD = 0.5  # cosine; fixed before looking at results


def read_rttm(path: Path) -> Annotation:
    ann = Annotation(uri=path.stem.split(".")[0])
    for line in path.read_text().splitlines():
        p = line.split()
        if len(p) >= 8 and p[0] == "SPEAKER":
            s, d = float(p[3]), float(p[4])
            if d > 0:
                ann[Segment(s, s + d), len(ann)] = p[7]
    return ann


def exclusive_spans(ann: Annotation, label: str) -> list[tuple[float, float]]:
    mine = ann.label_timeline(label).support()
    others = Annotation()
    for seg, _, lab in ann.itertracks(yield_label=True):
        if lab != label:
            others[seg, len(others)] = lab
    other = others.get_timeline().support()
    return [(s.start, s.end) for s in mine.extrude(other).support() if s.duration >= 0.5]


def voiceprints(ann: Annotation, audio: np.ndarray, ex) -> dict[str, np.ndarray]:
    prints = {}
    for label in ann.labels():
        vecs, weights = [], []
        for a, b in exclusive_spans(ann, label):
            t = a
            while t < b - 0.4:
                e = min(t + 10.0, b)
                vecs.append(embed(ex, audio[int(t * SR) : int(e * SR)]))
                weights.append(e - t)
                t = e
        if vecs:
            v = np.average(np.stack(vecs), axis=0, weights=weights)
            prints[label] = v / np.linalg.norm(v)
    return prints


def speaker_at(ann: Annotation, a: float, b: float) -> str | None:
    """Label with the most time inside [a, b]; the nearest segment if none."""
    best, best_t = None, 0.0
    for seg, _, lab in ann.crop(Segment(a, max(b, a + 0.01))).itertracks(yield_label=True):
        if seg.duration > best_t:
            best, best_t = lab, seg.duration
    if best is None:
        mid = (a + b) / 2
        dist = [(min(abs(s.start - mid), abs(s.end - mid)), l) for s, _, l in ann.itertracks(yield_label=True)]
        best = min(dist)[1] if dist else None
    return best


def truth_at(ann: Annotation, t: float) -> set[str]:
    labs = {l for s, _, l in ann.itertracks(yield_label=True) if s.start - 0.1 <= t <= s.end + 0.1}
    if labs:
        return labs
    return {speaker_at(ann, t, t)}


def attribution(ref: Annotation, hyp: Annotation, mapping: dict, words_json: dict) -> dict:
    word_ok = seg_ok = n = 0
    for sentence in words_json["sentences"]:
        seg_label = mapping.get(speaker_at(hyp, sentence["start"], sentence["end"]))
        for w in sentence["words"]:
            truth = truth_at(ref, (w["start"] + w["end"]) / 2)
            word_label = mapping.get(speaker_at(hyp, w["start"], w["end"]))
            word_ok += word_label in truth
            seg_ok += seg_label in truth
            n += 1
    return dict(words=n, word_level=word_ok / max(n, 1), segment_level=seg_ok / max(n, 1))


def peak_memory(system_dir: Path) -> float | None:
    f = system_dir / "time.txt"
    if not f.exists():
        return None
    text = f.read_text()
    m = re.search(r"(\d+)\s+peak memory footprint", text) or re.search(r"(\d+)\s+maximum resident set size", text)
    return int(m.group(1)) / 2**20 if m else None


def score_system(system: str, ex, refs, audios, reference_prints) -> dict:
    sdir = RESULTS / system
    der = DiarizationErrorRate(collar=0.25, skip_overlap=False)
    per_call, prints_by_call = {}, {}
    audio_s = wall_s = 0.0
    for call, ref in refs.items():
        rttm = sdir / f"{call}.rttm"
        if not rttm.exists():
            continue
        hyp = read_rttm(rttm)
        parts = der(ref, hyp, detailed=True)
        mapping = der.optimal_mapping(ref, hyp)
        meta = json.loads((sdir / f"{call}.json").read_text()) if (sdir / f"{call}.json").exists() else {}
        audio_s += meta.get("audio_seconds", 0)
        wall_s += meta.get("wall_seconds", 0)
        total = parts["total"]
        entry = dict(
            der=parts["diarization error rate"],
            miss=parts["missed detection"] / total,
            false_alarm=parts["false alarm"] / total,
            confusion=parts["confusion"] / total,
            speakers_true=len(ref.labels()),
            speakers_found=len(hyp.labels()),
        )
        words = ASR / f"{call}.json"
        if words.exists():
            entry["attribution"] = attribution(ref, hyp, mapping, json.loads(words.read_text()))
        prints = voiceprints(hyp, audios[call], ex)
        prints_by_call[call] = {mapping[l]: v for l, v in prints.items() if l in mapping}
        per_call[call] = entry

    # Cross-call: enroll each voice from the other calls, then name this call's speakers.
    cross = []
    for call, prints in prints_by_call.items():
        library: dict[str, list] = {}
        for other, op in prints_by_call.items():
            if other != call:
                for name, v in op.items():
                    library.setdefault(name, []).append(v)
        library = {n: np.mean(v, 0) / np.linalg.norm(np.mean(v, 0)) for n, v in library.items()}
        for name, v in prints.items():
            scores = {n: float(v @ lv) for n, lv in library.items()}
            best = max(scores, key=scores.get) if scores else None
            guess = best if best and scores[best] >= MATCH_THRESHOLD else "unknown"
            expected = name if name in library else "unknown"
            same = scores.get(name)
            diff = max([s for n, s in scores.items() if n != name], default=None)
            cross.append(dict(call=call, speaker=name, guess=guess, ok=guess == expected, same=same, best_other=diff))
    # Enrollment from one clean 10 s reference clip per voice.
    enrolled = []
    for call, prints in prints_by_call.items():
        for name, v in prints.items():
            scores = {n: float(v @ rv) for n, rv in reference_prints.items()}
            best = max(scores, key=scores.get)
            enrolled.append(dict(call=call, speaker=name, guess=best if scores[best] >= MATCH_THRESHOLD else "unknown",
                                 ok=best == name and scores[best] >= MATCH_THRESHOLD, same=scores[name]))

    calls = list(per_call.values())
    attr = [c["attribution"] for c in calls if "attribution" in c]
    total_words = sum(a["words"] for a in attr)
    return dict(
        system=system,
        calls=per_call,
        mean_der=float(np.mean([c["der"] for c in calls])) if calls else None,
        mean_confusion=float(np.mean([c["confusion"] for c in calls])) if calls else None,
        count_exact=sum(c["speakers_true"] == c["speakers_found"] for c in calls),
        count_abs_error=float(np.mean([abs(c["speakers_true"] - c["speakers_found"]) for c in calls])) if calls else None,
        rtfx=audio_s / wall_s if wall_s else None,
        peak_mb=peak_memory(sdir),
        cross_call_accuracy=float(np.mean([c["ok"] for c in cross])) if cross else None,
        cross_call=cross,
        reference_accuracy=float(np.mean([e["ok"] for e in enrolled])) if enrolled else None,
        reference=enrolled,
        word_level=sum(a["word_level"] * a["words"] for a in attr) / total_words if total_words else None,
        segment_level=sum(a["segment_level"] * a["words"] for a in attr) / total_words if total_words else None,
    )


def main() -> None:
    refs = {p.stem: read_rttm(p) for p in sorted(CALLS.glob("call*.rttm"))}
    audios = {c: sf.read(CALLS / f"{c}.wav", dtype="float32")[0] for c in refs}
    ex = extractor("campplus")
    reference_prints = {}
    for p in sorted((OUT / "voices").glob("*.wav")):
        a, sr = sf.read(p, dtype="float32")
        reference_prints[p.stem] = embed(ex, resample_poly(a, SR, sr).astype(np.float32))
    systems = sys.argv[1:] or sorted(d.name for d in RESULTS.iterdir() if d.is_dir() and any(d.glob("call*.rttm")))
    summary = [score_system(s, ex, refs, audios, reference_prints) for s in systems]
    (RESULTS / "summary.json").write_text(json.dumps(summary, indent=1))
    print(f"{'system':40} {'DER':>6} {'conf':>6} {'count':>6} {'RTFx':>7} {'MB':>6} {'xcall':>6} {'ref':>6} {'word':>6} {'seg':>6}")
    fmt = lambda v, p=1, m=100: "-" if v is None else f"{v * m:.{p}f}"  # noqa: E731
    for s in summary:
        print(f"{s['system']:40} {fmt(s['mean_der'])} {fmt(s['mean_confusion'])} {s['count_exact']:>3}/{len(s['calls'])} "
              f"{fmt(s['rtfx'], 0, 1):>7} {fmt(s['peak_mb'], 0, 1):>6} {fmt(s['cross_call_accuracy'], 0):>6} "
              f"{fmt(s['reference_accuracy'], 0):>6} {fmt(s['word_level'])} {fmt(s['segment_level'])}")


if __name__ == "__main__":
    main()
