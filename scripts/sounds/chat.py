#!/usr/bin/env python3
"""The onboarding comedy's chat sounds, written to
`resources/audio/sounds/onboarding/` as 48 kHz mono 16-bit WAVs. Standard
library only, so it runs anywhere:

    python3 scripts/sounds/chat.py

- `chat_incoming`: a message arriving, two quick bright pips (G5 -> B5).
- `chat_sent`: a message leaving, a breathy upward swish (500 Hz -> 2.4 kHz).
- `chat_read`: the deadpan `"Read"`, one dry low pip (about 360 Hz).
- `chat_rewind`: the story winding back, a soft downward swish that swells
  and stops short, like tape pulled back (2.6 kHz -> 420 Hz).
- `chat_tapback`: a tapback landing, one tiny bright tick (about 1.9 kHz).
- `heartbeat`: one low lub-dub under panicked typing, falling thumps
  (78 -> 46 Hz) with a quiet second harmonic so small speakers carry it.
- `heartbeat_slow`: the same lub-dub stretched and lowered for slow motion.
- `slow_down`: time slowing, the clicks smearing down like a tape stop
  (a band of noise 1.8 kHz -> 160 Hz over a falling tone).
- `slam`: a comic title hitting the stage, a short crack over a saturated
  bass drop (120 -> 36 Hz).
- `confetti`: a party popper, a crack and a shower of tiny bright ticks
  that thin out as the paper falls.
- `fireworks`: a quiet whistle rising (900 Hz -> 2.2 kHz), a soft boom and
  the crackle after it.
- `triumph`: a short brass-ish sting, a quick C major arpeggio up into a
  held chord.

Peaks are set to sit with the other onboarding cues (about -14 dBFS)."""

import math
import random
import struct
import wave
from pathlib import Path

SR = 48_000
OUT = Path(__file__).resolve().parents[2] / "resources/audio/sounds/onboarding"
rng = random.Random(7)


def silence(secs):
    return [0.0] * int(secs * SR)


def add(into, at, x):
    i = int(at * SR)
    for j, v in enumerate(x):
        if i + j < len(into):
            into[i + j] += v


def pip(hz, secs, tau, attack=0.002, h2=0.0):
    out = []
    for i in range(int(secs * SR)):
        t = i / SR
        env = min(1.0, t / attack) * math.exp(-t / tau)
        out.append(env * (math.sin(2 * math.pi * hz * t) + h2 * math.sin(4 * math.pi * hz * t)))
    return out


def swish(secs, lo, hi, backwards=False):
    """White noise through a resonant band-pass sweeping `lo` -> `hi`.
    `backwards` swells toward the end instead and stops short."""
    n = int(secs * SR)
    low = band = 0.0
    out = []
    for i in range(n):
        u = i / n
        f = lo * (hi / lo) ** u
        g = 2 * math.sin(math.pi * f / SR)
        q = 0.9
        x = rng.uniform(-1.0, 1.0)
        low += g * band
        high = x - low - q * band
        band += g * high
        # Swells in quickly and trails off, like breath.
        env = math.sin(math.pi * (1 - u if backwards else u) ** 0.7) ** 1.5
        out.append(env * band)
    return out


def thump(secs, hi, lo, tau, h2=0.25, attack=0.004):
    """A falling sine thump, `hi` -> `lo` Hz, decaying over `tau`."""
    out = []
    phase = 0.0
    for i in range(int(secs * SR)):
        t = i / SR
        f = lo + (hi - lo) * math.exp(-t / (tau * 0.6))
        phase += 2 * math.pi * f / SR
        env = min(1.0, t / attack) * math.exp(-t / tau)
        out.append(env * (math.sin(phase) + h2 * math.sin(2 * phase)))
    return out


def lub_dub(stretch, pitch):
    x = silence(0.36 * stretch)
    add(x, 0.0, thump(0.2 * stretch, 78.0 * pitch, 46.0 * pitch, 0.055 * stretch))
    add(x, 0.15 * stretch, [0.7 * v for v in thump(0.2 * stretch, 70.0 * pitch, 44.0 * pitch, 0.05 * stretch)])
    return x


def noise_burst(secs, tau, hp=0.85):
    """A short crack: white noise, high-passed by a one-pole difference."""
    out = []
    prev = 0.0
    for i in range(int(secs * SR)):
        t = i / SR
        x = rng.uniform(-1.0, 1.0)
        out.append((x - hp * prev) * math.exp(-t / tau))
        prev = x
    return out


def finish(x, peak_dbfs, fade=0.005):
    n = int(fade * SR)
    for i in range(min(n, len(x))):
        x[-1 - i] *= i / n
    top = max(abs(v) for v in x) or 1.0
    gain = 10 ** (peak_dbfs / 20) / top
    return [v * gain for v in x]


def write(name, x):
    with wave.open(str(OUT / f"{name}.wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", round(max(-1.0, min(1.0, v)) * 32767)) for v in x))


def main():
    incoming = silence(0.09)
    add(incoming, 0.0, [0.8 * v for v in pip(783.99, 0.05, 0.016, h2=0.12)])
    add(incoming, 0.032, pip(987.77, 0.058, 0.018, h2=0.10))
    write("chat_incoming", finish(incoming, -15.0))

    sent = silence(0.105)
    add(sent, 0.0, swish(0.105, 500.0, 2400.0))
    write("chat_sent", finish(sent, -14.0))

    read = silence(0.055)
    add(read, 0.0, pip(360.0, 0.055, 0.014, attack=0.0008, h2=0.25))
    write("chat_read", finish(read, -14.5, fade=0.006))

    rewind = silence(0.18)
    add(rewind, 0.0, swish(0.18, 2600.0, 420.0, backwards=True))
    write("chat_rewind", finish(rewind, -16.5, fade=0.008))

    tapback = silence(0.04)
    add(tapback, 0.0, pip(1900.0, 0.04, 0.007, attack=0.0006, h2=0.15))
    write("chat_tapback", finish(tapback, -19.0, fade=0.004))

    write("heartbeat", finish(lub_dub(1.0, 1.0), -13.0, fade=0.02))
    write("heartbeat_slow", finish(lub_dub(2.5, 0.62), -13.0, fade=0.05))

    slow = silence(0.7)
    add(slow, 0.0, swish(0.7, 1800.0, 160.0, backwards=False))
    add(slow, 0.0, [0.35 * v for v in thump(0.7, 220.0, 60.0, 0.3, h2=0.4, attack=0.03)])
    write("slow_down", finish(slow, -18.0, fade=0.06))

    slam = silence(1.0)
    add(slam, 0.0, [0.5 * v for v in noise_burst(0.06, 0.012)])
    add(slam, 0.0, thump(1.0, 120.0, 36.0, 0.32, h2=0.5, attack=0.002))
    slam = [math.tanh(2.2 * v) for v in slam]
    write("slam", finish(slam, -11.0, fade=0.08))

    confetti = silence(0.95)
    add(confetti, 0.0, [0.9 * v for v in noise_burst(0.05, 0.01)])
    add(confetti, 0.0, [0.5 * v for v in thump(0.12, 260.0, 120.0, 0.03, h2=0.3)])
    for i in range(34):
        at = 0.03 + 0.85 * (i / 34) ** 1.6 + rng.uniform(-0.01, 0.01)
        hz = rng.uniform(2600.0, 5200.0)
        loud = 0.55 * (1.0 - i / 40)
        add(confetti, at, [loud * v for v in pip(hz, 0.03, 0.005, attack=0.0005)])
    write("confetti", finish(confetti, -15.0, fade=0.02))

    fireworks = silence(1.5)
    whistle = []
    phase = 0.0
    for i in range(int(0.38 * SR)):
        t = i / SR
        f = 900.0 + 1300.0 * (t / 0.38) ** 1.4
        phase += 2 * math.pi * f / SR
        whistle.append(0.18 * min(1.0, t / 0.05) * math.sin(phase))
    add(fireworks, 0.0, whistle)
    add(fireworks, 0.38, [0.6 * v for v in noise_burst(0.08, 0.02, hp=0.5)])
    add(fireworks, 0.38, thump(0.8, 95.0, 40.0, 0.22, h2=0.35, attack=0.003))
    for i in range(40):
        at = 0.45 + rng.uniform(0.0, 0.95) * (i / 40) ** 0.7
        add(fireworks, at, [0.35 * (1.0 - i / 48) * v for v in noise_burst(0.006, 0.0015, hp=0.95)])
    write("fireworks", finish([math.tanh(1.6 * v) for v in fireworks], -14.0, fade=0.1))

    triumph = silence(1.2)
    def horn(hz, secs, start_amp=1.0):
        out = []
        for i in range(int(secs * SR)):
            t = i / SR
            env = min(1.0, t / 0.012) * math.exp(-t / (secs * 0.55))
            vib = 1.0 + 0.004 * math.sin(2 * math.pi * 5.5 * t) * min(1.0, t / 0.2)
            x = sum(math.sin(2 * math.pi * hz * vib * k * t) / k ** 1.3 for k in range(1, 7))
            out.append(start_amp * env * x)
        return out
    for at, hz in [(0.0, 523.25), (0.07, 659.25), (0.14, 783.99)]:
        add(triumph, at, [0.5 * v for v in horn(hz, 0.16)])
    for hz in (523.25, 659.25, 783.99, 1046.5):
        add(triumph, 0.21, [0.45 * v for v in horn(hz, 0.98)])
    write("triumph", finish([math.tanh(1.3 * v) for v in triumph], -13.0, fade=0.12))


if __name__ == "__main__":
    main()
