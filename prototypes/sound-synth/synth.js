// Hex's procedural feedback tones. This is the reference for src/sound_synth.rs:
// keep the two implementations step for step identical (same operations, same
// order, f64 throughout) so tests/fixtures/sound_synth renders match in Rust.

export const SAMPLE_RATE = 48000
export const CUES = ['start', 'stop', 'cancel']

const TAU = 2 * Math.PI
const semis = (s) => Math.pow(2, s / 12)

function biquad(x, b0, b1, b2, a0, a1, a2) {
  const y = new Float64Array(x.length)
  let x1 = 0, x2 = 0, y1 = 0, y2 = 0
  for (let i = 0; i < x.length; i++) {
    const v = (b0 * x[i] + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) / a0
    x2 = x1; x1 = x[i]; y2 = y1; y1 = v; y[i] = v
  }
  return y
}

function lowPass(x, cutoff, sr) {
  const w = (TAU * Math.min(cutoff, sr * 0.45)) / sr, c = Math.cos(w), alpha = Math.sin(w) / (2 * 0.7071067811865476)
  return biquad(x, (1 - c) / 2, 1 - c, (1 - c) / 2, 1 + alpha, -2 * c, 1 - alpha)
}

/** Raised-cosine attack over `attack` ms, then an exponential decay with time constant `decay` ms. */
function envelope(i, attackFrames, decay, sr) {
  if (i < attackFrames) return 0.5 - 0.5 * Math.cos((Math.PI * i) / attackFrames)
  return Math.exp(-((i - attackFrames) / sr) / (decay / 1000))
}

function tone(out, offset, voice, preset, sr) {
  const attackFrames = Math.max(1, Math.round((preset.attack * sr) / 1000))
  const f0 = voice.f0, ratio = semis(voice.glide), glide = Math.max(1, preset.glideMs) / 1000
  const partials = [[1, 1, 0], ...preset.partials]
  const phase = new Float64Array(partials.length)
  for (let i = 0; offset + i < out.length; i++) {
    const t = i / sr, k = Math.min(1, t / glide), f = f0 * Math.pow(ratio, k * (2 - k))
    let v = 0
    for (let j = 0; j < partials.length; j++) {
      const [r, amp, extra] = partials[j]
      phase[j] += (TAU * f * r) / sr
      const w = j === 0 && preset.wave === 'triangle' ? (2 / Math.PI) * Math.asin(Math.sin(phase[j])) : Math.sin(phase[j])
      v += amp * w * Math.exp(-t * extra)
    }
    out[offset + i] += voice.gain * v * envelope(i, attackFrames, voice.decay, sr)
  }
}

/** Band-passed noise whose centre glides from fc0 to fc1, from a fixed LCG seed. */
function breath(n, fc0, fc1, q, attack, decay, seed, sr) {
  const out = new Float64Array(n), attackFrames = Math.max(1, Math.round((attack * sr) / 1000))
  let state = seed >>> 0, x1 = 0, x2 = 0, y1 = 0, y2 = 0
  for (let i = 0; i < n; i++) {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0
    const x = ((state >>> 8) / 16777216) * 2 - 1
    const fc = fc0 * Math.pow(fc1 / fc0, i / n), w = (TAU * Math.min(fc, sr * 0.45)) / sr
    const alpha = Math.sin(w) / (2 * q), a0 = 1 + alpha, a1 = -2 * Math.cos(w), a2 = 1 - alpha
    const y = (alpha * x + 0 * x1 - alpha * x2 - a1 * y1 - a2 * y2) / a0
    x2 = x1; x1 = x; y2 = y1; y1 = y
    out[i] = y * envelope(i, attackFrames, decay, sr)
  }
  return out
}

const rms = (x) => { let s = 0; for (const v of x) s += v * v; return Math.sqrt(s / Math.max(1, x.length)) }

/** BS.1770 K-weighted loudness of the clip in a 400 ms window (zero-padded), in LUFS. */
export function loudness(x, sr = SAMPLE_RATE) {
  const n = Math.max(x.length, Math.round(0.4 * sr))
  const padded = new Float64Array(n); padded.set(x)
  let w = (TAU * 1681.9744509555319) / sr, c = Math.cos(w)
  const A = Math.pow(10, 3.99984385397 / 40), sq = Math.sqrt(A)
  let alpha = Math.sin(w) / (2 * 0.7071752369554193)
  const shelf = biquad(padded,
    A * (A + 1 + (A - 1) * c + 2 * sq * alpha), -2 * A * (A - 1 + (A + 1) * c), A * (A + 1 + (A - 1) * c - 2 * sq * alpha),
    A + 1 - (A - 1) * c + 2 * sq * alpha, 2 * (A - 1 - (A + 1) * c), A + 1 - (A - 1) * c - 2 * sq * alpha)
  w = (TAU * 38.13547087613982) / sr; c = Math.cos(w); alpha = Math.sin(w) / (2 * 0.5003270373253953)
  const k = biquad(shelf, (1 + c) / 2, -(1 + c), (1 + c) / 2, 1 + alpha, -2 * c, 1 - alpha)
  let s = 0; for (const v of k) s += v * v
  return -0.691 + 10 * Math.log10(Math.max(s / n, 1e-20))
}

/** The voices a cue plays: start pings, stop pongs `interval` semitones lower, cancel falls further. */
export function voices(preset, cue) {
  const stop = preset.pitch * semis(-preset.interval)
  if (cue === 'start') return [{ at: 0, f0: preset.pitch, glide: preset.startGlide, gain: 1, decay: preset.decay }]
  if (cue === 'stop') return [{ at: 0, f0: stop, glide: preset.stopGlide, gain: 1, decay: preset.decay }]
  const f0 = stop * semis(-preset.cancelDrop)
  if (preset.cancelTaps < 2) return [{ at: 0, f0, glide: preset.cancelGlide, gain: 1, decay: preset.decay * 0.8 }]
  return [
    { at: 0, f0, glide: preset.cancelGlide, gain: 0.7, decay: preset.decay * 0.66 },
    { at: preset.tapGap, f0: f0 * semis(-1), glide: preset.cancelGlide, gain: 0.5, decay: preset.decay * 0.66 },
  ]
}

/** Renders one cue of a preset as mono f32 samples at `sr`. Deterministic. */
export function render(preset, cue, sr = SAMPLE_RATE) {
  const n = Math.round((preset.length * sr) / 1000), index = CUES.indexOf(cue)
  const vs = voices(preset, cue)
  const mix = new Float64Array(n)
  for (const v of vs) tone(mix, Math.round((v.at * sr) / 1000), v, preset, sr)
  const fc0 = vs[0].f0 * preset.breathCenter
  const air = breath(n, fc0, fc0 * semis(preset.breathSweep), preset.breathQ, preset.attack * 0.75, preset.decay * preset.breathDecay, 0x2545f491 + index * 7919, sr)
  const level = preset.breath * rms(mix) / Math.max(rms(air), 1e-12)
  for (let i = 0; i < n; i++) mix[i] += air[i] * level
  let out = lowPass(lowPass(mix, preset.brightness, sr), preset.brightness, sr)
  let peak = 0; for (const v of out) peak = Math.max(peak, Math.abs(v))
  const drive = preset.drive, norm = drive > 1 ? Math.tanh(drive) : 1
  for (let i = 0; i < n; i++) { const v = out[i] / Math.max(peak, 1e-12); out[i] = drive > 1 ? Math.tanh(drive * v) / norm : v }
  const fade = Math.round(0.01 * sr)
  for (let i = 0; i < fade && i < n; i++) out[n - 1 - i] *= i / fade
  const gain = Math.pow(10, (preset.volume - loudness(out, sr)) / 20)
  const result = new Float32Array(n)
  for (let i = 0; i < n; i++) result[i] = out[i] * gain
  return result
}
