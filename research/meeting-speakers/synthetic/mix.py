"""Lay the synthesized turns out as calls, write RTTM truth, and degrade them.

Reads $OUT/turns/<call>/<NNN>.wav (from tts.py) and writes
$OUT/calls/<call>.wav (degraded, 16 kHz mono), <call>.clean.wav, <call>.rttm,
and <call>.json (turn times and text).
"""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import butter, correlate, resample_poly, sosfilt

sys.path.insert(0, str(Path(__file__).resolve().parent))
from script import load_calls  # noqa: E402

OUT = Path(os.environ.get("OUT", "out")).resolve()
SR = 16000
FRAME = 320  # 20 ms
SPLIT_PAUSE = 0.5  # a pause this long inside a turn splits its truth segment

# Each call gets a different line: codec, bitrate and noise.
CHANNEL = {
    "call1": dict(codec="opus", bitrate="20k", snr=28, hum=False),
    "call2": dict(codec="aac", bitrate="32k", snr=24, hum=False),
    "call3": dict(codec="opus", bitrate="16k", snr=22, hum=True),
    "call4": dict(codec="opus", bitrate="24k", snr=26, hum=False),
    "call5": dict(codec="aac", bitrate="24k", snr=20, hum=True),
}


def frame_rms(x: np.ndarray) -> np.ndarray:
    n = len(x) // FRAME
    return np.sqrt(np.mean(x[: n * FRAME].reshape(n, FRAME) ** 2, axis=1) + 1e-12)


def active_frames(x: np.ndarray) -> np.ndarray:
    rms = frame_rms(x)
    return rms > max(rms.max() * 10 ** (-38 / 20), 1e-4)


def trim(x: np.ndarray) -> np.ndarray:
    act = np.flatnonzero(active_frames(x))
    if len(act) == 0:
        return x
    margin = 2  # 40 ms
    a = max(act[0] - margin, 0) * FRAME
    b = min((act[-1] + 1 + margin) * FRAME, len(x))
    return x[a:b]


def speech_runs(x: np.ndarray) -> list[tuple[float, float]]:
    """Active spans of a trimmed clip in seconds, split at long pauses."""
    act = active_frames(x)
    runs, start, silent = [], None, 0
    for i, a in enumerate(act):
        if a:
            if start is None:
                start = i
            silent = 0
            last = i
        elif start is not None:
            silent += 1
            if silent * FRAME / SR >= SPLIT_PAUSE:
                runs.append((start, last + 1))
                start, silent = None, 0
    if start is not None:
        runs.append((start, last + 1))
    return [(a * FRAME / SR, b * FRAME / SR) for a, b in runs if (b - a) * FRAME / SR >= 0.1]


def pink(n: int, rng) -> np.ndarray:
    white = rng.standard_normal(n)
    spec = np.fft.rfft(white)
    spec /= np.sqrt(np.arange(1, len(spec) + 1))
    p = np.fft.irfft(spec, n)
    return p / (np.std(p) + 1e-12)


def codec_roundtrip(x: np.ndarray, codec: str, bitrate: str) -> np.ndarray:
    with tempfile.TemporaryDirectory() as tmp:
        src, enc, dst = (Path(tmp) / n for n in ("in.wav", "enc." + ("ogg" if codec == "opus" else "m4a"), "out.wav"))
        sf.write(src, x, SR, subtype="FLOAT")
        args = ["-c:a", "libopus", "-application", "voip"] if codec == "opus" else ["-c:a", "aac"]
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", src, *args, "-b:a", bitrate, enc], check=True)
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", enc, "-ar", str(SR), "-ac", "1", "-c:a", "pcm_f32le", dst], check=True)
        y, _ = sf.read(dst, dtype="float32")
    # Undo any codec delay so the RTTM still lines up.
    n = min(len(x), len(y), SR * 30)
    lag = int(np.argmax(correlate(y[:n], x[:n], mode="full", method="fft"))) - (n - 1)
    if lag > 0:
        y = y[lag:]
    elif lag < 0:
        y = np.concatenate([np.zeros(-lag, np.float32), y])
    y = np.pad(y, (0, max(0, len(x) - len(y))))[: len(x)]
    print(f"  codec {codec} {bitrate}: realigned by {lag} samples")
    return y


def build(call: str, turns, rng) -> None:
    calls_dir = OUT / "calls"
    calls_dir.mkdir(parents=True, exist_ok=True)
    speakers = sorted({t.speaker for t in turns})
    level_db = {s: rng.uniform(-9, 0) for s in speakers}

    placed = []
    main_start = main_end = 0.0
    clock = 0.4
    for turn in turns:
        clip, sr = sf.read(OUT / "turns" / call / f"{turn.index:03d}.wav", dtype="float32")
        clip = trim(resample_poly(clip, SR, sr).astype(np.float32))
        dur = len(clip) / SR
        if turn.kind == "back":
            start = main_start + rng.uniform(0.35, 0.7) * (main_end - main_start)
        elif turn.kind == "barge":
            start = main_end - min(rng.uniform(0.5, 1.2), 0.5 * (main_end - main_start))
        else:
            gap = -rng.uniform(0.2, 0.6) if rng.random() < 0.3 else rng.uniform(0.2, 0.7)
            start = max(clock + gap, 0.0)
        if turn.kind != "back":
            main_start, main_end = start, start + dur
        clock = max(clock, start + dur)
        mask = np.repeat(active_frames(clip), FRAME)
        active = clip[: len(mask)][mask] if len(mask) and mask.any() else clip
        gain = 10 ** ((-20 - 20 * np.log10(np.sqrt(np.mean(active**2)) + 1e-9) + level_db[turn.speaker] + rng.uniform(-1.5, 1.5)) / 20)
        placed.append((turn, start, clip * gain))

    total = int((clock + 0.6) * SR)
    mix = np.zeros(total, np.float32)
    segments = []
    for turn, start, clip in placed:
        a = int(start * SR)
        mix[a : a + len(clip)] += clip
        for s, e in speech_runs(clip):
            segments.append((start + s, start + e, turn.speaker))
    segments.sort()

    clean = mix / max(1.0, np.abs(mix).max() / 0.9)
    sf.write(calls_dir / f"{call}.clean.wav", clean, SR, subtype="PCM_16")

    ch = CHANNEL[call]
    t = np.arange(total) / SR
    speech_rms = np.sqrt(np.mean(clean[np.abs(clean) > 1e-3] ** 2))
    noise = pink(total, rng) * speech_rms * 10 ** (-ch["snr"] / 20)
    if ch["hum"]:
        noise += speech_rms * 10 ** (-(ch["snr"] + 8) / 20) * np.sin(2 * np.pi * 50 * t) * np.sqrt(2)
    line = sosfilt(butter(4, [100, 7000], btype="band", fs=SR, output="sos"), clean + noise).astype(np.float32)
    line = codec_roundtrip(line, ch["codec"], ch["bitrate"])
    line = line / max(1.0, np.abs(line).max() / 0.95)
    sf.write(calls_dir / f"{call}.wav", line, SR, subtype="PCM_16")

    with open(calls_dir / f"{call}.rttm", "w") as f:
        for s, e, spk in segments:
            f.write(f"SPEAKER {call} 1 {s:.3f} {e - s:.3f} <NA> <NA> {spk} <NA> <NA>\n")

    grid = np.zeros((len(speakers), total // FRAME + 1), bool)
    for s, e, spk in segments:
        grid[speakers.index(spk), int(s * SR / FRAME) : int(e * SR / FRAME)] = True
    voiced = grid.any(0).sum()
    overlap = (grid.sum(0) > 1).sum()
    meta = dict(
        call=call,
        duration=total / SR,
        speakers=speakers,
        levels_db={s: round(v, 1) for s, v in level_db.items()},
        channel=ch,
        overlap_fraction=round(float(overlap / voiced), 4),
        turns=[dict(index=t.index, kind=t.kind, speaker=t.speaker, text=t.text,
                    start=round(s, 3), end=round(s + len(c) / SR, 3)) for t, s, c in placed],
    )
    (calls_dir / f"{call}.json").write_text(json.dumps(meta, indent=1))
    print(f"{call}: {total / SR / 60:.1f} min, {len(speakers)} speakers, overlap {100 * overlap / voiced:.1f}% of speech")


def main() -> None:
    calls = load_calls()
    for i, (call, turns) in enumerate(calls.items()):
        if len(sys.argv) > 1 and call not in sys.argv[1:]:
            continue
        build(call, turns, np.random.default_rng(42 + i))


if __name__ == "__main__":
    main()
