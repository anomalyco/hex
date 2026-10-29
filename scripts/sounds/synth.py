# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "scipy"]
# ///
"""Hex's feedback sounds, synthesized reproducibly.

Run: uv run scripts/sounds/synth.py   (rewrites resources/audio/*.wav)

One voice, a kalimba-like FM tine, and one motif, D - B - A - D: up a major
sixth, a step down, home ("hex" is six, so the sixth is Hex's interval).
Start, stop and done play the motif in order across a dictation; cancel
collapses the sixth; error turns it minor; onboarding success plays the whole
motif and lands on a D sixth chord.

Every sound is 48 kHz mono 16-bit, starts on its first sample (no leading
silence), begins and ends at zero with raised-cosine edges, and is normalized
so its loudest 400 ms window (BS.1770 K-weighted, EBU momentary loudness)
reads its target in TARGETS. The targets match the sounds they replaced, so a
volume setting sounds as loud as it did: start -25.5 LUFS like the old start
clicks, stop like the old swish, error like macOS Basso.
"""
from pathlib import Path

import numpy as np
from scipy.io import wavfile
from scipy.signal import butter, lfilter, sosfilt

SR = 48000
CEILING = 10 ** (-1.5 / 20)  # sample peak
OUT = Path(__file__).resolve().parents[2] / "resources" / "audio"
rng = np.random.default_rng(6)  # fixed seed: same noise every run

# file name -> target LUFS (max momentary)
TARGETS = {
    "dictationStart": -25.5,
    "dictationStop": -27.0,
    "dictationDone": -27.0,
    "dictationCancel": -26.5,
    "error": -26.5,
    "onboardingSuccess": -25.0,
}


def hz(note: str) -> float:
    names = {"C": -9, "C#": -8, "D": -7, "Eb": -6, "E": -5, "F": -4, "F#": -3,
             "G": -2, "G#": -1, "A": 0, "Bb": 1, "B": 2}
    name, octave = note[:-1], int(note[-1])
    return 440.0 * 2 ** ((names[name] + 12 * (octave - 4)) / 12)


def t_axis(dur):
    return np.arange(int(dur * SR)) / SR


def attack(n_samples, ms):
    a = np.ones(n_samples)
    k = min(n_samples, max(1, int(ms / 1000 * SR)))
    a[:k] = 0.5 - 0.5 * np.cos(np.pi * np.arange(k) / k)
    return a


def mix(events, dur):
    """Sum (time, signal) events into one buffer of `dur` seconds."""
    out = np.zeros(int(dur * SR))
    for at, sig in events:
        sig = sig.copy()
        f = min(len(sig) // 3, int(0.008 * SR))  # release every event: no truncation clicks
        sig[-f:] *= 0.5 + 0.5 * np.cos(np.pi * np.arange(f) / f)
        i = int(at * SR)
        n = min(len(sig), len(out) - i)
        out[i:i + n] += sig[:n]
    return out


def band(x, lo, hi, order=2):
    return sosfilt(butter(order, [lo, hi], btype="band", fs=SR, output="sos"), x)


def tine(freq, dur, tau, amp=1.0, index=2.2, index_tau=0.014, ratio=4.0, tick_amt=0.25):
    """A kalimba-like FM tine (bright 1:4 attack that settles to a pure sine)
    with a tiny high noise tick on the onset."""
    t = t_axis(dur)
    mod = index * np.exp(-t / index_tau) * np.sin(2 * np.pi * freq * ratio * t)
    x = np.sin(2 * np.pi * freq * t + mod) * np.exp(-t / tau)
    if tick_amt:
        n = band(rng.standard_normal(len(t)), 5000, 14000) * np.exp(-t / 0.0007)
        x += tick_amt * n / (np.max(np.abs(n)) + 1e-12)
    return amp * x * attack(len(t), 1.5)


def sounds():
    D4, Bb4, D5, Fs5, A5, B5, D6 = map(hz, ["D4", "Bb4", "D5", "F#5", "A5", "B5", "D6"])
    s = {}
    s["dictationStart"] = mix([(0, 0.75 * tine(D5, 0.05, 0.02)), (0.04, tine(B5, 0.076, 0.026))], 0.116)
    s["dictationStop"] = mix([(0, 0.8 * tine(B5, 0.07, 0.026)), (0.06, tine(A5, 0.17, 0.05))], 0.23)
    s["dictationDone"] = mix([(0, tine(D6, 0.32, 0.08)),
                              (0.006, 0.45 * tine(A5, 0.32, 0.075, tick_amt=0))], 0.33)
    s["dictationCancel"] = mix([(0, 0.8 * tine(B5, 0.07, 0.025, index=1.4)),
                                (0.055, tine(D5, 0.17, 0.045, index=1.2))], 0.225)
    s["error"] = mix([(0, 0.8 * tine(D4, 0.09, 0.03, index=1.0, tick_amt=0.15)),
                      (0.09, tine(D4, 0.19, 0.05, index=1.0, tick_amt=0.15)),
                      (0.09, 0.7 * tine(Bb4, 0.19, 0.05, index=1.0, tick_amt=0))], 0.28)
    ev = [(0.00, 0.7 * tine(D5, 0.2, 0.05)),
          (0.08, 0.8 * tine(B5, 0.2, 0.05)),
          (0.16, 0.75 * tine(A5, 0.2, 0.06))]
    land = 0.26
    ev += [(land, tine(D6, 0.85, 0.28))]
    for i, (f, a) in enumerate([(D5, 0.35), (Fs5, 0.3), (A5, 0.3), (B5, 0.32)]):
        ev.append((land + 0.012 * i, a * tine(f, 0.85, 0.3, index=1.2, tick_amt=0)))
    s["onboardingSuccess"] = mix(ev, land + 0.86)
    return s


_K1 = ([1.53512485958697, -2.69169618940638, 1.19839281085285], [1.0, -1.69065929318241, 0.73248077421585])
_K2 = ([1.0, -2.0, 1.0], [1.0, -1.99004745483398, 0.99007225036621])


def max_momentary_lufs(x):
    pad = np.zeros(int(0.4 * SR))
    y = np.concatenate([pad, x, pad])
    y = lfilter(*_K2, lfilter(*_K1, y))
    w = int(0.4 * SR)
    c = np.concatenate([[0], np.cumsum(y ** 2)])
    ms = (c[w:] - c[:-w])[:: SR // 100] / w  # 10 ms hop
    return -0.691 + 10 * np.log10(ms.max() + 1e-20)


def finish(x, target):
    # trim the tail below -60 dB of peak (+10 ms), then fade the last 6 ms
    thr = np.max(np.abs(x)) * 10 ** (-60 / 20)
    last = np.where(np.abs(x) > thr)[0][-1]
    x = x[: min(len(x), last + int(0.010 * SR))]
    f = min(len(x) // 4, int(0.006 * SR))
    x[-f:] *= 0.5 + 0.5 * np.cos(np.pi * np.arange(f) / f)
    x[0] = 0.0
    x[-1] = 0.0
    x *= 10 ** ((target - max_momentary_lufs(x)) / 20)
    assert np.max(np.abs(x)) <= CEILING, "a sound would clip at its target"
    return x


def main():
    for name, x in sounds().items():
        x = finish(x.astype(np.float64), TARGETS[name])
        pcm = np.round(np.clip(x, -1, 1) * 32767).astype(np.int16)
        wavfile.write(OUT / f"{name}.wav", SR, pcm)
        first = int(np.argmax(np.abs(pcm) > 0.01 * 32767))
        print(f"{name:18s} {len(pcm) / SR * 1000:5.0f} ms  "
              f"{max_momentary_lufs(pcm / 32767):6.2f} LUFS  "
              f"peak {20 * np.log10(np.max(np.abs(pcm)) / 32767):6.2f} dBFS  "
              f"onset {first / SR * 1000:.2f} ms")


if __name__ == "__main__":
    main()
