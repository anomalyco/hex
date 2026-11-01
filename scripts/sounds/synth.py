# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "scipy"]
# ///
"""Hex's feedback sound sets, made reproducibly.

Run: uv run scripts/sounds/synth.py   (rewrites resources/audio/sounds/)

Each set is a folder of start, stop, done, error and success WAVs:

- signature: one voice, a kalimba-like FM tine, and one motif, D - B - A - D:
  up a major sixth, a step down, home ("hex" is six, so the sixth is Hex's
  interval). Start, stop and done play the motif in order across a
  dictation; error turns it minor; success plays
  the whole motif and lands on a D sixth chord.
- classic: the tones Hex played before the signature set, decoded from the
  original MP3s in git (start and stop only; Hex plays no done tone,
  macOS Basso for errors, and signature's success for this set).
- click: mechanical key clicks.
- soft: warm sine blips in A major.
- wood: a soft-mallet marimba and hollow wood blocks in G major pentatonic.
- analog: a warm analogue synth, two detuned saws through a closing low-pass
  filter, in F major.
- thock: one mechanical keyboard key, cut from the team vote's winning start
  sound, an ElevenLabs take kept in scripts/sounds/thock/; no done sound.

Every synthesized sound is 48 kHz mono 16-bit, starts on its first sample (no
leading silence), begins and ends at zero with raised-cosine edges, and is
normalized so its loudest 400 ms window (BS.1770 K-weighted, EBU momentary
loudness) reads its target in TARGETS, the same in every set, so a volume
setting sounds as loud whichever set is chosen. The targets match the sounds
Hex first shipped: start -25.5 LUFS like the classic start clicks, stop like
the classic swish, error like macOS Basso. A mono sound plays in both
speakers, so the stereo classic sounds are measured per channel, which puts
their start 2.5 dB and stop 3 dB under the targets their summed loudness once
set. They are raised to the targets like every other sound (the limiter
touches 0.1% of the start's samples, the peaks of its clicks).
"""
import subprocess
from fractions import Fraction
from math import gcd
from pathlib import Path

import numpy as np
from scipy.io import wavfile
from scipy.signal import butter, lfilter, resample_poly, sosfilt

SR = 48000
CEILING = 10 ** (-1.5 / 20)  # sample peak
ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "resources" / "audio" / "sounds"
# The commit before the signature set replaced the original MP3s.
CLASSIC_COMMIT = "06d60cbb90a87a293ac35bea8fa6e44bb9b3fd70^"
rng = np.random.default_rng(6)

# sound -> target LUFS (max momentary), shared by every set
TARGETS = {
    "start": -25.5,
    "stop": -27.0,
    "done": -27.0,
    "error": -26.5,
    "success": -25.0,
}


def hz(note: str) -> float:
    names = {"C": -9, "C#": -8, "D": -7, "Eb": -6, "E": -5, "F": -4, "F#": -3,
             "Gb": -3, "G": -2, "G#": -1, "A": 0, "Bb": 1, "B": 2}
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


def lowpass(x, f, order=2):
    return sosfilt(butter(order, f, btype="low", fs=SR, output="sos"), x)


# ---------------------------------------------------------------- signature

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


def signature():
    D4, Bb4, D5, Fs5, A5, B5, D6 = map(hz, ["D4", "Bb4", "D5", "F#5", "A5", "B5", "D6"])
    s = {}
    s["start"] = mix([(0, 0.75 * tine(D5, 0.05, 0.02)), (0.04, tine(B5, 0.076, 0.026))], 0.116)
    s["stop"] = mix([(0, 0.8 * tine(B5, 0.07, 0.026)), (0.06, tine(A5, 0.17, 0.05))], 0.23)
    s["done"] = mix([(0, tine(D6, 0.32, 0.08)),
                     (0.006, 0.45 * tine(A5, 0.32, 0.075, tick_amt=0))], 0.33)
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
    s["success"] = mix(ev, land + 0.86)
    return s


# ---------------------------------------------------------------- click

def tick(body=1800.0, body_tau=0.005, noise_band=(2500, 9000), noise_tau=0.0011,
         thock=0.0, thock_tau=0.016, noise_amt=0.55, body_amt=0.7, thock_amt=0.0,
         dull=None, dur=0.06):
    """One mechanical click: a filtered noise transient, a plastic body
    resonance, and an optional low thock from the key bottoming out."""
    t = t_axis(dur)
    n = band(rng.standard_normal(len(t)), *noise_band) * np.exp(-t / noise_tau)
    n /= np.max(np.abs(n)) + 1e-12
    b = np.sin(2 * np.pi * body * t) * np.exp(-t / (body_tau * 1.8))
    x = noise_amt * n + body_amt * b
    if thock:
        x += thock_amt * np.sin(2 * np.pi * thock * t) * np.exp(-t / thock_tau)
    if dull:
        x = lowpass(x, dull)
    return x * attack(len(t), 0.25)


def click():
    press = lambda **k: tick(body=1500, thock=250, thock_amt=0.75, thock_tau=0.022, body_tau=0.005, **k)
    s = {}
    # Start: a key press, tick then the keycap bottoming out.
    s["start"] = mix([(0, press(dur=0.09)), (0.016, 0.35 * tick(body=1100, noise_amt=0.5, dull=5000))], 0.1)
    # Stop: the lighter, higher key release with a small rattle.
    s["stop"] = mix([(0, tick(body=2400, body_tau=0.004, noise_band=(3500, 12000), thock=340, thock_amt=0.45, thock_tau=0.018)),
                     (0.020, 0.3 * tick(body=2900, body_tau=0.003, noise_band=(4000, 12000)))], 0.1)
    # Done: two bright ticks, the second a touch higher.
    s["done"] = mix([(0, 0.85 * tick(body=2600, body_tau=0.004)),
                     (0.055, tick(body=3100, body_tau=0.0045))], 0.12)
    # Error: three dull low taps, like a haptic "nope".
    dull_tap = lambda: tick(body=520, body_tau=0.008, thock=170, thock_amt=0.9,
                            thock_tau=0.014, noise_amt=0.5, dull=2000)
    s["error"] = mix([(0, dull_tap()), (0.07, dull_tap()), (0.14, dull_tap())], 0.22)
    # Success: a six-tick ratchet that speeds up and rises, then clicks into
    # place with a small metallic ring.
    times, at = [], 0.0
    for gap in (0.07, 0.06, 0.05, 0.042, 0.036, 0.034):
        times.append(at)
        at += gap
    ev = [(tt, (0.55 + 0.06 * i) * tick(body=1400 + 240 * i, body_tau=0.0035)) for i, tt in enumerate(times)]
    t = t_axis(0.35)
    ring = 0.12 * np.sin(2 * np.pi * 3200 * t) * np.exp(-t / 0.07) * attack(len(t), 1)
    ev += [(at, press(dur=0.08)), (at, ring), (at + 0.014, 0.3 * tick(body=1100, noise_amt=0.5, dull=5000))]
    s["success"] = mix(ev, at + 0.36)
    return s


# ---------------------------------------------------------------- soft

def warm(freq, dur, tau, amp=1.0, h2=0.12, h3=0.035, bell=0.0, atk_ms=4.0, bend_cents=0.0):
    """A warm sine blip: fundamental with soft 2nd/3rd harmonics, an optional
    glassy 2.76x bell partial, and an optional pitch glide over its length."""
    t = t_axis(dur)
    glide = 2 ** (bend_cents / 1200 * (t / dur))
    phase = 2 * np.pi * np.cumsum(freq * glide) / SR
    x = (np.sin(phase) * np.exp(-t / tau)
         + h2 * np.sin(2 * phase) * np.exp(-t / (tau * 0.6))
         + h3 * np.sin(3 * phase) * np.exp(-t / (tau * 0.4))
         + bell * np.sin(2.76 * phase) * np.exp(-t / (tau * 0.3)))
    return amp * x * attack(len(t), atk_ms)


def soft():
    E5, A5, B4, C5, Cs6, E6, A6 = map(hz, ["E5", "A5", "B4", "C5", "C#6", "E6", "A6"])
    s = {}
    # Start: a quick rising fourth, E5 to A5. A 1.5 ms attack keeps the onset
    # instant like every start sound.
    s["start"] = mix([(0, 0.8 * warm(E5, 0.05, 0.018, atk_ms=1.5)), (0.034, warm(A5, 0.081, 0.026))], 0.115)
    # Stop: the same fourth falling, A5 to E5, "got it, working".
    s["stop"] = mix([(0, 0.85 * warm(A5, 0.07, 0.025)), (0.055, warm(E5, 0.16, 0.045))], 0.215)
    # Done: a soft bell dyad, C#6 over A5.
    s["done"] = mix([(0, warm(Cs6, 0.3, 0.075, bell=0.10)), (0.008, 0.45 * warm(A5, 0.3, 0.07, bell=0.05))], 0.31)
    # Error: a soft low semitone cluster, twice.
    cluster = lambda: warm(B4, 0.11, 0.035, h2=0.05) + warm(C5, 0.11, 0.035, h2=0.05)
    s["error"] = mix([(0, 0.8 * cluster()), (0.11, cluster())], 0.23)
    # Success: an A major arpeggio landing on a ringing A6.
    ev = [(0.00, 0.7 * warm(A5, 0.25, 0.06, bell=0.05, atk_ms=2.0)),
          (0.07, 0.75 * warm(Cs6, 0.25, 0.06, bell=0.05)),
          (0.14, 0.8 * warm(E6, 0.3, 0.07, bell=0.06)),
          (0.21, warm(A6, 0.75, 0.22, bell=0.08)),
          (0.21, 0.35 * warm(A5, 0.75, 0.25)),
          (0.21, 0.25 * warm(E6, 0.75, 0.2))]
    s["success"] = mix(ev, 0.96)
    return s


# ---------------------------------------------------------------- wood

def bar(freq, dur, tau, amp=1.0, hardness=0.5):
    """A marimba bar struck by a soft mallet: tuned modes at 1x, 4x and 9.9x
    whose upper ones die quickly, plus a short felt thump."""
    t = t_axis(dur)
    x = np.sin(2 * np.pi * freq * t) * np.exp(-t / tau)
    for ratio, level, life in ((3.99, 0.32, 0.22), (9.9, 0.1, 0.08)):
        if freq * ratio < 18000:
            x += hardness * level * np.sin(2 * np.pi * freq * ratio * t) * np.exp(-t / (tau * life))
    thump = lowpass(rng.standard_normal(len(t)), 2200) * np.exp(-t / 0.0012)
    x += 0.35 * hardness * thump / (np.max(np.abs(thump)) + 1e-12)
    return amp * x * attack(len(t), 0.8)


def block(freq, dur, tau, amp=1.0, dull=None):
    """A hollow wood block: a few inharmonic modes that ring briefly over a
    knock of band-limited noise."""
    t = t_axis(dur)
    x = np.zeros(len(t))
    for ratio, level, life in ((1.0, 1.0, 1.0), (1.52, 0.45, 0.6), (2.67, 0.25, 0.35), (3.9, 0.12, 0.2)):
        x += level * np.sin(2 * np.pi * freq * ratio * t) * np.exp(-t / (tau * life))
    knock = band(rng.standard_normal(len(t)), freq * 0.8, min(freq * 5, 16000)) * np.exp(-t / 0.0015)
    x += 0.4 * knock / (np.max(np.abs(knock)) + 1e-12)
    if dull:
        x = lowpass(x, dull)
    return amp * x * attack(len(t), 0.5)


def wood():
    G5, A5, B5, D6, E6, G6 = map(hz, ["G5", "A5", "B5", "D6", "E6", "G6"])
    s = {}
    # Start: two quick mallet notes up a fifth, G5 to D6.
    s["start"] = mix([(0, 0.75 * bar(G5, 0.06, 0.022, hardness=0.6)), (0.036, bar(D6, 0.08, 0.028))], 0.116)
    # Stop: a step back down, D6 to B5, the second note left to ring.
    s["stop"] = mix([(0, 0.8 * bar(D6, 0.07, 0.024)), (0.055, bar(B5, 0.17, 0.05))], 0.225)
    # Done: a ringing dyad, G6 over D6.
    s["done"] = mix([(0, bar(G6, 0.32, 0.075, hardness=0.6)), (0.007, 0.5 * bar(D6, 0.32, 0.08, hardness=0.3))], 0.33)
    # Error: two low hollow knocks, the second lower, like a log drum's
    # "uh-uh".
    s["error"] = mix([(0, 0.85 * block(260, 0.11, 0.032, dull=2500)),
                      (0.1, block(225, 0.15, 0.036, dull=2200))], 0.25)
    # Success: a pentatonic run up the bars into a small G6 roll over a D6
    # and B5 chord.
    ev = [(0.05 * i, (0.55 + 0.05 * i) * bar(f, 0.22, 0.06, hardness=0.55))
          for i, f in enumerate([G5, A5, B5, D6, E6])]
    land = 0.27
    for i, a in enumerate([1.0, 0.55, 0.4, 0.3]):
        ev.append((land + 0.055 * i, a * bar(G6, 0.8 - 0.055 * i, 0.28, hardness=0.5)))
    ev += [(land + 0.008, 0.45 * bar(D6, 0.8, 0.3, hardness=0.3)),
           (land + 0.016, 0.35 * bar(B5, 0.8, 0.3, hardness=0.3))]
    s["success"] = mix(ev, land + 0.85)
    return s


# ---------------------------------------------------------------- analog

def svf_lowpass(x, cutoff, q):
    """A resonant state-variable low-pass (topology-preserving transform)
    whose cutoff may change every sample."""
    g = np.tan(np.pi * np.minimum(cutoff, 0.45 * SR) / SR)
    k = 1 / q
    out = np.empty_like(x)
    ic1 = ic2 = 0.0
    for i in range(len(x)):
        a1 = 1 / (1 + g[i] * (g[i] + k))
        a2 = g[i] * a1
        a3 = g[i] * a2
        v3 = x[i] - ic2
        v1 = a1 * ic1 + a2 * v3
        v2 = ic2 + a2 * ic1 + a3 * v3
        ic1 = 2 * v1 - ic1
        ic2 = 2 * v2 - ic2
        out[i] = v2
    return out


def synth(freq, dur, amp_tau, amp=1.0, open_=6.0, close=1.6, filter_tau=0.03,
          detune=7.0, sub=0.35, q=1.1, bend_cents=0.0, bend_tau=0.04, atk_ms=1.2):
    """A warm analogue voice: two saws detuned by +-`detune` cents and a sub
    sine an octave down, through a low-pass that snaps open to `open_` times
    the pitch and closes to `close` times it. `bend_cents` glides the pitch
    away from `freq` over about `bend_tau`."""
    t = t_axis(dur)
    pitch = freq * 2 ** (bend_cents * (1 - np.exp(-t / bend_tau)) / 1200)
    x = np.zeros(len(t))
    for cents in (detune, -detune):
        phase = 2 * np.pi * np.cumsum(pitch * 2 ** (cents / 1200)) / SR
        for n in range(1, int(16000 / (freq * 1.1)) + 1):  # band-limited saw
            x += np.sin(n * phase) / n / 2
    x += sub * np.sin(np.pi * np.cumsum(pitch) / SR)
    cutoff = pitch * (close + (open_ - close) * np.exp(-t / filter_tau))
    y = svf_lowpass(x, cutoff, q)
    return amp * y * np.exp(-t / amp_tau) * attack(len(t), atk_ms)


def analog():
    F3, Gb3, F5, A5, C6, E6, F6 = map(hz, ["F3", "Gb3", "F5", "A5", "C6", "E6", "F6"])
    s = {}
    # Start: a quick filter blip up a fifth, F5 to C6.
    s["start"] = mix([(0, 0.75 * synth(F5, 0.05, 0.02)), (0.04, synth(C6, 0.076, 0.026))], 0.116)
    # Stop: down a third, C6 to A5, closing a little slower.
    s["stop"] = mix([(0, 0.8 * synth(C6, 0.07, 0.025)),
                     (0.06, synth(A5, 0.17, 0.05, filter_tau=0.05))], 0.23)
    # Done: a soft F major stab, A5 C6 F6.
    s["done"] = mix([(0, 0.6 * synth(f, 0.32, 0.08, open_=5, close=1.3, filter_tau=0.06))
                     for f in (A5, C6, F6)], 0.33)
    # Error: a low, buzzy semitone (F3 and Gb3), pulsed twice.
    buzz = lambda dur: (synth(F3, dur, 0.04, open_=9, close=3, filter_tau=0.04, detune=12, sub=0.2)
                        + synth(Gb3, dur, 0.04, open_=9, close=3, filter_tau=0.04, detune=12, sub=0.2))
    s["error"] = mix([(0, 0.8 * buzz(0.1)), (0.11, buzz(0.15))], 0.26)
    # Success: an F major arpeggio of plucks that blooms into an Fmaj7 chord
    # as the filter slowly closes.
    ev = [(0.07 * i, (0.6 + 0.05 * i) * synth(f, 0.2, 0.05, filter_tau=0.04))
          for i, f in enumerate([F5, A5, C6, F6])]
    land = 0.28
    for f, a in ((F5, 0.45), (A5, 0.4), (C6, 0.4), (E6, 0.35), (F6, 0.45)):
        ev.append((land, a * synth(f, 0.85, 0.28, open_=7, close=1.4, filter_tau=0.18, detune=9)))
    s["success"] = mix(ev, land + 0.86)
    return s


# ---------------------------------------------------------------- classic

def decode_mp3(name):
    mp3 = subprocess.run(["git", "-C", str(ROOT), "show", f"{CLASSIC_COMMIT}:resources/audio/{name}.mp3"],
                         capture_output=True, check=True).stdout
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", "-", "-f", "f32le", "-"],
                         input=mp3, capture_output=True, check=True).stdout
    return np.frombuffer(raw, dtype=np.float32).reshape(-1, 2).astype(np.float64)


def classic():
    """The original stereo 44.1 kHz sounds, starting at their first audible
    sample (the files led with up to 25 ms of near-silence) and ending once
    they fall 60 dB below their peak."""
    s = {}
    for key, name in (("start", "startRecording"), ("stop", "stopRecording")):
        x = decode_mp3(name)
        first = int(np.argmax(np.max(np.abs(x), axis=1) > 0.001))
        x = x[first:].copy()
        f = int(0.0003 * 44100)
        x[:f] *= (0.5 - 0.5 * np.cos(np.pi * np.arange(f) / f))[:, None]
        s[key] = x
    return s


# ---------------------------------------------------------------- thock

THOCK = Path(__file__).resolve().parent / "thock"


def decode_take(name):
    """One of the vote's ElevenLabs takes in scripts/sounds/thock/, 48 kHz mono."""
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", str(THOCK / name), "-ac", "1",
                          "-ar", str(SR), "-f", "f32le", "-"], capture_output=True, check=True).stdout
    return np.frombuffer(raw, dtype=np.float32).astype(np.float64)


def first_hit_ms(x, from_ms, to_ms):
    """Where a hit begins: the first sample within 15 dB of the window's peak."""
    seg = x[int(from_ms / 1000 * SR):int(to_ms / 1000 * SR)]
    return from_ms + int(np.argmax(np.abs(seg) > np.max(np.abs(seg)) * 10 ** (-15 / 20))) / SR * 1000


def take_cut(x, at_ms, length_ms, fade_ms, lead_ms=0.3):
    """`length_ms` of a take from just before `at_ms`, faded out over `fade_ms`, with no DC."""
    a = max(0, int((at_ms - lead_ms) / 1000 * SR))
    y = x[a:a + int(length_ms / 1000 * SR)].copy()
    y *= attack(len(y), 0.3)
    f = min(len(y), int(fade_ms / 1000 * SR))
    y[-f:] *= 0.5 + 0.5 * np.cos(np.pi * np.arange(f) / f)
    return y - np.mean(y) * np.hanning(len(y)) / np.mean(np.hanning(len(y)))


def varispeed(x, semitones):
    """Played faster or slower, like a smaller or bigger key: lower is longer."""
    r = Fraction(2 ** (semitones / 12)).limit_denominator(200)
    return resample_poly(x, r.denominator, r.numerator)


def faded(x, ms, fade_ms):
    y = x[:int(ms / 1000 * SR)].copy()
    f = min(len(y), int(fade_ms / 1000 * SR))
    y[-f:] *= 0.5 + 0.5 * np.cos(np.pi * np.arange(f) / f)
    return y


def keys(parts):
    """(offset ms, gain, sound) on one timeline."""
    out = np.zeros(max(int(o / 1000 * SR) + len(x) for o, _, x in parts))
    for o, g, x in parts:
        i = int(o / 1000 * SR)
        out[i:i + len(x)] += g * x
    return out


def highpass(x, f, order=2):
    return sosfilt(butter(order, f, btype="high", fs=SR, output="sos"), x)


def thock():
    """One keyboard, from the team vote's winning start sound (ElevenLabs v3,
    "A single mechanical keyboard key press, deep creamy thock, tactile, dry,
    very short."). Start is its press, stop the same key's upstroke, both cut
    from the take; the rest is that press played as bigger and smaller keys:
    error two keys at once, twice, low, and
    success a run of keys up to a full press. It has no done sound: the stop
    already says it (Kit, 2026-10-30)."""
    take = decode_take("mech-key-thock.mp3")
    press = take_cut(take, first_hit_ms(take, 45, 60), 112, 40)
    release = take_cut(take, first_hit_ms(take, 145, 165), 60, 30)
    tap = lambda st, ms: faded(varispeed(press, st), ms, 25)  # noqa: E731
    return {
        "start": press,
        "stop": release,
        "error": highpass(lowpass(keys([
            (0, 1.0, faded(varispeed(press, -6), 70, 30)),
            (14, 0.7, faded(varispeed(press, -9), 70, 30)),
            (125, 0.9, faded(varispeed(press, -8), 95, 45)),
            (139, 0.6, faded(varispeed(press, -11), 95, 45)),
        ]), 1500), 70),
        "success": keys([
            (0, 0.55, tap(0, 45)), (80, 0.62, tap(2, 45)), (155, 0.7, tap(4, 45)),
            (235, 0.8, tap(7, 50)), (330, 1.0, varispeed(press, 3)), (470, 0.8, varispeed(release, 3)),
        ]),
    }


SETS = {"signature": signature, "click": click, "soft": soft, "wood": wood, "analog": analog, "thock": thock}

# ---------------------------------------------------------------- onboarding

ONBOARDING_TARGETS = {
    "hexagon": -30.5,
    "panel_escape": -30.0,
    "permission_tick": -29.5,
    "almost_ready": -30.5,
}


def onboarding():
    """Quiet harmonic motion cues in Hex's D-major sixth world. Tactile keycap
    and shortcut foley WAVs (`key_*`, `shortcut_*`) live alongside these in
    `resources/audio/sounds/onboarding/` and are kept intact."""
    D5, Fs5, A5, B5, D6 = map(hz, ["D5", "F#5", "A5", "B5", "D6"])
    s = {}

    # Hexagon mark landing: feather-light crystalline D5 + A5 glass harmonic bloom.
    s["hexagon"] = mix([
        (0.0, 0.72 * warm(D5, 0.17, 0.048, h2=0.08, bell=0.07, atk_ms=6.5)),
        (0.018, 0.58 * warm(A5, 0.16, 0.044, h2=0.05, bell=0.09, atk_ms=7.0)),
    ], 0.185)

    # Panel escaping & docking beside System Settings: soft velvet air sweep -> quiet dock click.
    t_air = t_axis(0.145)
    raw_air = rng.standard_normal(len(t_air))
    cutoff = 850.0 + 1350.0 * np.sin(np.pi * np.clip(t_air / 0.145, 0, 1))
    air = svf_lowpass(band(raw_air, 500, 4200), cutoff, 1.35)
    air /= np.max(np.abs(air)) + 1e-12
    air *= np.sin(np.pi * np.clip(t_air / 0.145, 0, 1)) ** 1.4
    dock = mix([
        (0.0, 0.65 * tick(body=1420, body_tau=0.0038, thock=235, thock_amt=0.55,
                          thock_tau=0.012, noise_amt=0.28, dull=4400, dur=0.048)),
        (0.0, 0.45 * warm(B5, 0.085, 0.024, h2=0.06, bell=0.05, atk_ms=1.8)),
    ], 0.09)
    s["panel_escape"] = mix([(0.0, 0.42 * air), (0.125, dock)], 0.22)

    # Permission row ticking to Allowed: reassuring two-grace-note warm pip (A5 -> D6).
    s["permission_tick"] = mix([
        (0.0, 0.68 * warm(A5, 0.065, 0.020, h2=0.08, bell=0.05, atk_ms=1.8)),
        (0.030, 0.92 * warm(D6, 0.105, 0.030, h2=0.06, bell=0.08, atk_ms=1.8)),
    ], 0.14)

    # "Almost ready" waiting cue: calm suspended breath (F#5 -> A5).
    s["almost_ready"] = mix([
        (0.0, 0.72 * warm(Fs5, 0.12, 0.036, h2=0.06, atk_ms=5.0)),
        (0.052, 0.85 * warm(A5, 0.15, 0.044, h2=0.06, bell=0.04, atk_ms=5.0)),
    ], 0.21)

    return s

# ---------------------------------------------------------------- loudness

_K1 = ([1.53512485958697, -2.69169618940638, 1.19839281085285], [1.0, -1.69065929318241, 0.73248077421585])
_K2 = ([1.0, -2.0, 1.0], [1.0, -1.99004745483398, 0.99007225036621])


def max_momentary_lufs(x, sr=SR):
    """Of a mono signal. A mono sound plays the same in both speakers, so a
    stereo one is measured by the mean of its channels' power to compare.
    The K-filter coefficients are for 48 kHz, so other rates are resampled
    first."""
    channels = x.T if x.ndim == 2 else x[None, :]
    if sr != SR:
        channels = resample_poly(channels, SR // gcd(SR, sr), sr // gcd(SR, sr), axis=1)
    pad = np.zeros(int(0.4 * SR))
    power = np.mean([lfilter(*_K2, lfilter(*_K1, np.concatenate([pad, c, pad]))) ** 2
                     for c in channels], axis=0)
    w = int(0.4 * SR)
    c = np.concatenate([[0], np.cumsum(power)])
    ms = (c[w:] - c[:-w])[:: SR // 100] / w  # 10 ms hop
    return -0.691 + 10 * np.log10(ms.max() + 1e-20)


def soft_limit(x, ceiling=CEILING, knee=0.7):
    k = knee * ceiling
    a = np.abs(x)
    over = a > k
    y = x.copy()
    y[over] = np.sign(x[over]) * (k + (ceiling - k) * np.tanh((a[over] - k) / (ceiling - k)))
    return y


def trim_tail(x, sr):
    """Cut once it falls 60 dB below its peak (+10 ms), fading the last 6 ms."""
    level = np.abs(x) if x.ndim == 1 else np.max(np.abs(x), axis=1)
    last = np.where(level > level.max() * 10 ** (-60 / 20))[0][-1]
    x = x[: min(len(x), last + int(0.010 * sr))].copy()
    f = min(len(x) // 4, int(0.006 * sr))
    fade = 0.5 + 0.5 * np.cos(np.pi * np.arange(f) / f)
    x[-f:] *= fade if x.ndim == 1 else fade[:, None]
    x[-1] = 0.0
    return x


def finish(x, target, sr=SR):
    x = trim_tail(x, sr)
    x[0] = 0.0
    for _ in range(6):
        x *= 10 ** ((target - max_momentary_lufs(x, sr)) / 20)
        if np.max(np.abs(x)) <= CEILING:
            return x
        x = soft_limit(x)
    raise AssertionError("a sound would clip at its target")


def write(path, sr, x):
    pcm = np.round(np.clip(x, -1, 1) * 32767).astype(np.int16)
    wavfile.write(path, sr, pcm)
    mono = pcm if pcm.ndim == 1 else np.max(np.abs(pcm), axis=1)
    first = int(np.argmax(np.abs(mono) > 0.01 * 32767))
    print(f"  {path.stem:8s} {len(pcm) / sr * 1000:5.0f} ms  "
          f"{max_momentary_lufs(pcm / 32767, sr):6.2f} LUFS  "
          f"peak {20 * np.log10(np.max(np.abs(pcm)) / 32767):6.2f} dBFS  "
          f"onset {first / sr * 1000:.2f} ms")


def main():
    global rng
    for name, build in SETS.items():
        print(name)
        rng = np.random.default_rng(6)  # fixed seed per set: same noise every run
        folder = OUT / name
        folder.mkdir(parents=True, exist_ok=True)
        for key, x in build().items():
            write(folder / f"{key}.wav", SR, finish(x.astype(np.float64), TARGETS[key]))
    print("classic")
    folder = OUT / "classic"
    folder.mkdir(parents=True, exist_ok=True)
    for key, x in classic().items():
        write(folder / f"{key}.wav", 44100, finish(x, TARGETS[key], 44100))
    print("onboarding")
    rng = np.random.default_rng(6)
    folder = OUT / "onboarding"
    folder.mkdir(parents=True, exist_ok=True)
    for key, x in onboarding().items():
        write(folder / f"{key}.wav", SR, finish(x.astype(np.float64), ONBOARDING_TARGETS[key]))


if __name__ == "__main__":
    main()
