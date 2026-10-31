# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "scipy"]
# ///
"""The onboarding comedy's keyboard, heartbeat and triumph candidates, written to
`resources/audio/sounds/onboarding/` as 48 kHz mono 16-bit WAVs:

    uv run scripts/sounds/comedy.py

- `clack_1`..`clack_8`: real key presses on a 1986 IBM Model M, cut from a
  CC0 recording kept in `scripts/sounds/comedy/` (see `SOURCE.md`): eight
  isolated presses of different weights, each from just before its strike
  to just before the next key.
- `clack_space`: the recording's space bar, a semitone and a half lower with
  its body brought up.
- `clack_backspace`: a heavier press from the same recording.
- `heartbeat`: one lub-dub under the panic, felt more than heard: each beat
  is a thud of low-passed noise over a short falling tone (60-40 Hz), the
  "lub" two valve closures 22 ms apart and the "dub" 200 ms later, softly
  saturated so small speakers still carry its body.
- `heartbeat_slow`: the same, stretched and lowered for slow motion.
- `triumph_1`: a brass stinger, "da-da-DAAA": detuned saws through a
  brightness-following low-pass, a scoop into each note, a timpani hit and
  a light cymbal under the held C major chord.
- `triumph_2`: a glockenspiel run up a C major arpeggio (E5 to G6) into a
  shimmering chord, inharmonic bar partials and a short room.
- `triumph_3`: a party popper's pop, then the room bursting into applause
  (fourteen people clapping at their own pace) and a whistle.

Levels match the cues they sit with: keys peak near where the Taptic tick did,
the heartbeat low under them, and each triumph at the old sting's loudness."""

from pathlib import Path
import subprocess
import wave

import numpy as np
from scipy.signal import butter, fftconvolve, sosfilt

SR = 48_000
OUT = Path(__file__).resolve().parents[2] / "resources/audio/sounds/onboarding"
rng = np.random.default_rng(11)


def axis(secs):
    return np.arange(int(secs * SR)) / SR


def filt(x, kind, f, order=2):
    return sosfilt(butter(order, f, btype=kind, fs=SR, output="sos"), x)


def place(out, at, x, gain=1.0):
    i = int(at * SR)
    n = min(len(x), len(out) - i)
    out[i:i + n] += gain * x[:n]


def edges(x, fade_ms=12.0, attack_ms=0.2):
    y = x.copy()
    a = max(1, int(attack_ms / 1000 * SR))
    y[:a] *= np.linspace(0.0, 1.0, a)
    f = min(len(y), int(fade_ms / 1000 * SR))
    y[-f:] *= 0.5 + 0.5 * np.cos(np.pi * np.arange(f) / f)
    return y - np.mean(y) * np.hanning(len(y)) / np.mean(np.hanning(len(y)))


def ring(spec, secs):
    """Damped modes, (hz, tau, amp), struck at once."""
    t = axis(secs)
    return sum(a * np.sin(2 * np.pi * hz * t) * np.exp(-t / tau) for hz, tau, a in spec)


def burst(secs, lo, hi, tau):
    t = axis(secs)
    return filt(rng.standard_normal(len(t)), "band", [lo, hi]) * np.exp(-t / tau)


def room(x, secs=0.5, mix=0.25, lo=300.0):
    t = axis(secs)
    ir = filt(rng.standard_normal(len(t)), "high", lo) * np.exp(-t / (secs / 5))
    wet = fftconvolve(x, ir)[: len(x)]
    wet *= np.max(np.abs(x)) / max(np.max(np.abs(wet)), 1e-9)
    return (1 - mix) * x + mix * wet


# ---------------------------------------------------------------- keys

SOURCE = Path(__file__).resolve().parent / "comedy" / "model-m-1986.ogg"

# Where each key's press begins in the recording, in seconds: eight clean,
# isolated presses with different weights, then the space bar and a heavier
# key for backspace.
KEYS = [4.390, 11.1535, 4.9223, 2.723, 0.1955, 6.2957, 6.8355, 11.2592]
SPACE = 7.7103
BACKSPACE = 8.094


def recording():
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", str(SOURCE), "-ac", "1", "-ar", str(SR),
                          "-f", "f32le", "-"], capture_output=True, check=True).stdout
    # The room's hum sits under 100 Hz; the keys don't.
    return filt(np.frombuffer(raw, dtype=np.float32).astype(np.float64), "high", 110)


def cut(x, at, length_ms, lead_ms=3.0):
    """One press: from just before its loudest strike to `length_ms` later or
    the next key, whichever comes first, faded out."""
    ms = lambda v: int(v * SR / 1000)  # noqa: E731
    a = int(at * SR)
    peak = a + int(np.argmax(np.abs(x[a:a + ms(45)])))
    env = np.sqrt(np.convolve(filt(x, "high", 1000)[peak:peak + ms(length_ms)] ** 2,
                              np.ones(ms(1)) / ms(1), "same"))
    loud = env[: ms(6)].max()
    # Its own bottoming-out follows within 30 ms; anything as loud later is
    # the next key.
    after = np.nonzero(env[ms(30):] > loud * 10 ** (-8 / 20))[0]
    stop = ms(30) + after[0] - ms(2) if len(after) else len(env)
    y = x[peak - ms(lead_ms):peak + stop].copy()
    # Past the strike and its spring, let the recording's hiss die away.
    t = np.maximum(0, np.arange(len(y)) - ms(lead_ms + 12)) / SR
    y *= np.exp(-t / 0.02)
    return edges(y, fade_ms=min(25.0, stop / SR * 1000 * 0.4), attack_ms=0.5)


def lower(x, semis):
    """Down `semis` semitones by reading the samples slower."""
    ratio = 2 ** (-semis / 12)
    return np.interp(np.arange(0, len(x) - 1, ratio), np.arange(len(x)), x)


def keys():
    x = recording()
    out = {f"clack_{i + 1}": cut(x, at, 85) for i, at in enumerate(KEYS)}
    # A deeper, hollower bar: the recording's space a little lower with its
    # body brought up.
    space = lower(cut(x, SPACE, 150), 1.5)
    out["clack_space"] = edges(space + 0.8 * filt(space, "band", [140, 420]), fade_ms=30)
    out["clack_backspace"] = cut(x, BACKSPACE, 100)
    return out


# ---------------------------------------------------------------- heartbeat


def thud(secs, f0, f1, tau, cutoff):
    t = axis(secs)
    f = f1 + (f0 - f1) * np.exp(-t / 0.02)
    env = (1 - np.exp(-t / 0.006)) * np.exp(-t / tau)
    tone = np.sin(2 * np.pi * np.cumsum(f) / SR) * env
    noise = filt(rng.standard_normal(len(t)), "low", cutoff, 4) * env
    return 0.5 * tone + 0.8 * noise / np.max(np.abs(noise))


def heartbeat(stretch=1.0, drop=1.0, secs=0.36):
    x = np.zeros(int(secs * SR))
    place(x, 0, thud(0.2 * stretch, 62 * drop, 40 * drop, 0.045 * stretch, 110 * drop))
    place(x, 0.022 * stretch, thud(0.16 * stretch, 70 * drop, 44 * drop, 0.035 * stretch, 140 * drop), 0.6)
    place(x, 0.2 * stretch, thud(0.15 * stretch, 76 * drop, 50 * drop, 0.03 * stretch, 150 * drop), 0.72)
    # A soft skin of presence, so a laptop speaker hears where the beat is.
    skin = filt(x, "band", [110, 320])
    x = np.tanh(1.6 * (x + 0.6 * skin)) / np.tanh(1.6)
    return edges(filt(x, "low", 280), fade_ms=30, attack_ms=1.0)


# ---------------------------------------------------------------- triumphs


def svf_lowpass(x, cutoff, q=0.9):
    y = np.zeros_like(x)
    low = band = 0.0
    for i, v in enumerate(x):
        f = 2 * np.sin(np.pi * min(cutoff[i], SR / 6) / SR)
        low += f * band
        high = v - low - band / q
        band += f * high
        y[i] = low
    return y


def brass(hz, secs, scoop=True):
    t = axis(secs)
    cents = (-45 * np.exp(-t / 0.025) if scoop else 0) + 9 * np.sin(2 * np.pi * 5.5 * t) * np.clip((t - 0.3) / 0.2, 0, 1)
    f = hz * 2 ** (cents / 1200)
    x = sum(2 * ((np.cumsum(f * 2 ** (d / 1200)) / SR + rng.uniform()) % 1) - 1 for d in (-7, 0, 6))
    rise = 1 - np.exp(-t / 0.018)
    env = rise * (0.8 + 0.35 * np.exp(-t / 0.06)) * np.clip((secs - t) / 0.08, 0, 1)
    y = svf_lowpass(x, 350 + 3800 * env * (0.6 + 0.4 * np.exp(-t / 0.12)))
    return np.tanh(1.3 * y * env)


def triumph_brass():
    secs = 1.35
    x = np.zeros(int(secs * SR))
    hz = lambda n: 440 * 2 ** ((n - 69) / 12)  # noqa: E731
    place(x, 0.0, brass(hz(67), 0.1), 0.5)
    place(x, 0.085, brass(hz(72), 0.1), 0.55)
    for n, g in [(48, 0.5), (60, 0.55), (64, 0.45), (67, 0.5), (72, 0.6), (76, 0.4)]:
        place(x, 0.17, brass(hz(n), secs - 0.17 - 0.02), g)
    t = axis(0.6)
    timp = np.sin(2 * np.pi * np.cumsum(65 + 25 * np.exp(-t / 0.02)) / SR) * np.exp(-t / 0.18)
    place(x, 0.17, timp + 0.3 * burst(0.6, 80, 400, 0.05), 1.2)
    cym = filt(burst(1.1, 4000, 14000, 0.35), "high", 5000)
    place(x, 0.17, cym / np.max(np.abs(cym)), 0.12)
    return edges(room(filt(x, "high", 50), 0.6, 0.18), fade_ms=60, attack_ms=2)


def glock(hz, secs, amp=1.0):
    t = axis(secs)
    parts = [(1.0, 1.0, 0.9), (2.756, 0.45, 0.35), (5.404, 0.25, 0.15), (8.933, 0.12, 0.08)]
    x = sum(a * np.sin(2 * np.pi * hz * r * t) * np.exp(-t / tau) for r, a, tau in parts if hz * r < SR / 2.2)
    click = burst(secs, 2000, 12000, 0.0012)
    return amp * (x + 0.3 * click / np.max(np.abs(click)))


def triumph_chime():
    secs = 1.45
    x = np.zeros(int(secs * SR))
    run = [659.3, 784.0, 1046.5, 1318.5, 1568.0]
    for k, hz in enumerate(run):
        place(x, 0.06 * k, glock(hz, 0.9), 0.55 + 0.08 * k)
    t = axis(secs - 0.32)
    trem = 1 + 0.15 * np.sin(2 * np.pi * 7 * t)
    for hz, g in [(1046.5, 0.7), (1318.5, 0.6), (1568.0, 0.6), (2093.0, 0.4)]:
        place(x, 0.32, glock(hz * (1 + rng.normal(0, 0.001)), secs - 0.32) * trem, g)
    for _ in range(18):
        at = 0.34 + rng.exponential(0.25)
        if at < secs - 0.2:
            place(x, at, glock(rng.uniform(4000, 7000), 0.2), rng.uniform(0.05, 0.12))
    return edges(room(x, 0.7, 0.3, 800), fade_ms=120, attack_ms=0.5)


def clap(lo):
    """One pair of hands: a crack of band-passed noise over the palms' hollow."""
    x = burst(0.06, lo, lo * 3.2, 0.006)
    x = x / np.max(np.abs(x))
    x += 0.6 * ring([(lo * rng.uniform(0.7, 0.9), 0.012, 1.0)], 0.06)
    return x * (1 - np.exp(-axis(0.06) / 0.0004))


def whistle(secs, f0, f1):
    """A fingers-in-mouth whistle: a breathy sine sliding up, then down."""
    t = axis(secs)
    u = t / secs
    f = f0 + (f1 - f0) * np.sin(np.pi * np.clip(u * 1.4, 0, 1) / 2) - (f1 - f0) * 0.35 * np.clip((u - 0.6) / 0.4, 0, 1)
    f *= 1 + 0.004 * np.sin(2 * np.pi * 6 * t)
    tone = np.sin(2 * np.pi * np.cumsum(f) / SR)
    air = filt(rng.standard_normal(len(t)), "band", [2000, 6000]) * 0.08
    env = (1 - np.exp(-t / 0.03)) * np.clip((secs - t) / 0.12, 0, 1)
    return (tone + air) * env


def triumph_cheer():
    """A party popper, then the room bursting into applause and a whistle."""
    secs = 1.5
    x = np.zeros(int(secs * SR))
    t = axis(0.4)
    pop = burst(0.4, 1500, 12000, 0.002) * 3 + np.sin(2 * np.pi * np.cumsum(60 + 70 * np.exp(-t / 0.01)) / SR) * np.exp(-t / 0.03)
    place(x, 0, pop / np.max(np.abs(pop)))
    for _ in range(40):
        place(x, 0.05 + rng.exponential(0.12), burst(0.01, 3000, 10000, 0.0007), rng.uniform(0.03, 0.08))
    # Fourteen people clapping, each at their own pace, swelling then easing.
    for _ in range(14):
        at, period = rng.uniform(0.06, 0.22), rng.uniform(0.11, 0.17)
        lo, g = rng.uniform(700, 1500), rng.uniform(0.18, 0.32)
        while at < secs - 0.08:
            swell = min(1.0, (at - 0.04) / 0.15) * (1 - max(0.0, at - 0.7) / 0.9)
            place(x, at, clap(lo), g * max(0.0, swell) * rng.uniform(0.8, 1.2))
            at += period * rng.uniform(0.9, 1.1)
    place(x, 0.2, whistle(0.55, 2100, 3300), 0.22)
    place(x, 0.62, whistle(0.45, 2500, 3600), 0.16)
    return edges(room(x, 0.5, 0.3), fade_ms=220, attack_ms=0.3)


# ---------------------------------------------------------------- output


def loudest(x, secs=0.4):
    n = int(secs * SR)
    if len(x) <= n:
        return float(np.sqrt(np.mean(x ** 2)))
    c = np.cumsum(np.concatenate([[0.0], x ** 2]))
    return float(np.sqrt(np.max(c[n:] - c[:-n]) / n))


def write(name, x):
    with wave.open(str(OUT / f"{name}.wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes((np.clip(x, -1, 1) * 32767).round().astype("<i2").tobytes())


def at_peak(x, db):
    return x * 10 ** (db / 20) / np.max(np.abs(x))


def main():
    for name, x in keys().items():
        write(name, at_peak(x, -8.5 if name.startswith("clack_") and name[-1].isdigit() else -8.0))
    write("heartbeat", at_peak(heartbeat(), -11.0))
    write("heartbeat_slow", at_peak(heartbeat(stretch=1.7, drop=0.8, secs=0.9), -11.0))
    # Each sting as loud as the old one was (its loudest 0.4 s at -18.4 dBFS
    # RMS), never past -6 dBFS.
    old = 10 ** (-18.4 / 20)
    for i, make in enumerate([triumph_brass, triumph_chime, triumph_cheer], 1):
        x = make()
        x *= old / loudest(x)
        if np.max(np.abs(x)) > 10 ** (-6 / 20):
            x = at_peak(x, -6.0)
        write(f"triumph_{i}", x)


if __name__ == "__main__":
    main()
