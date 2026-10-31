// Original synthesized arrival sounds, one voice per app family. Nothing here is sampled from any
// app. Every voice is built from sine and filtered-noise pieces on whatever context it is given, so
// the same code plays live and renders offline (OfflineAudioContext) for level checks.

const VOICES = {
  // Messages-style: a soft two-note rising ping.
  ping: { notes: [{ f: 988, at: 0, len: 0.2 }, { f: 1319, at: 0.085, len: 0.34 }], level: 0.34 },
  // Slack-style: two quick wooden knocks, the second a little higher.
  knock: { notes: [{ f: 520, to: 340, at: 0, len: 0.09 }, { f: 700, to: 450, at: 0.1, len: 0.11 }], level: 0.3 },
  // GitHub, X, calendar, mail: one soft tick.
  tick: { notes: [{ f: 1500, to: 1050, at: 0, len: 0.05 }], click: true, level: 0.3 },
}

const KIND_VOICE = {
  messages: "ping",
  chat: "knock",
  dm: "knock",
  discord: "knock",
  mail: "tick",
  calendar: "tick",
  review: "tick",
  issue: "tick",
  tweet: "tick",
  opencode: "tick",
}

export const arrivalVoice = (kind) => KIND_VOICE[kind] ?? "tick"

// Schedules one arrival on `ctx` at time `when`, into `destination`, and returns its end time.
export function scheduleArrival(ctx, destination, voice, when, { gain = 1, rate = 1, pan = 0 } = {}) {
  const v = VOICES[voice] ?? VOICES.tick
  const out = ctx.createGain()
  out.gain.value = v.level * gain
  let tail = out
  if (ctx.createStereoPanner) {
    const p = ctx.createStereoPanner()
    p.pan.value = Math.max(-1, Math.min(1, pan))
    out.connect(p)
    tail = p
  }
  tail.connect(destination)
  let end = when
  for (const n of v.notes) {
    const t = when + n.at
    const osc = ctx.createOscillator()
    const env = ctx.createGain()
    osc.type = "sine"
    osc.frequency.setValueAtTime(n.f * rate, t)
    if (n.to) osc.frequency.exponentialRampToValueAtTime(n.to * rate, t + n.len)
    env.gain.setValueAtTime(0.0001, t)
    env.gain.linearRampToValueAtTime(1, t + 0.004)
    env.gain.exponentialRampToValueAtTime(0.0001, t + n.len)
    osc.connect(env).connect(out)
    osc.start(t)
    osc.stop(t + n.len + 0.02)
    end = Math.max(end, t + n.len + 0.02)
  }
  if (v.click) {
    const len = Math.max(1, Math.floor(ctx.sampleRate * 0.012))
    const buf = ctx.createBuffer(1, len, ctx.sampleRate)
    const d = buf.getChannelData(0)
    // Deterministic noise so renders match.
    let seed = 12345
    for (let i = 0; i < len; i++) {
      seed = (seed * 1664525 + 1013904223) >>> 0
      d[i] = ((seed / 4294967296) * 2 - 1) * Math.exp(-i / (len * 0.28))
    }
    const src = ctx.createBufferSource()
    src.buffer = buf
    const bp = ctx.createBiquadFilter()
    bp.type = "bandpass"
    bp.frequency.value = 3200 * rate
    bp.Q.value = 0.9
    const g = ctx.createGain()
    g.gain.value = 0.5
    src.connect(bp).connect(g).connect(out)
    src.start(when)
  }
  return end
}

// Keeps a burst of cards from stacking into noise: at most one arrival per `gap` seconds, later ones in
// the same burst get quieter, and after `max` in the window the rest stay silent.
export class ArrivalGate {
  constructor({ gap = 0.09, window = 0.7, max = 4 } = {}) {
    Object.assign(this, { gap, window, max, times: [] })
  }

  // Returns a gain (0 = skip) for an arrival at time `t` (seconds).
  admit(t) {
    this.times = this.times.filter((x) => t - x < this.window)
    const last = this.times[this.times.length - 1]
    if (last !== undefined && t - last < this.gap) return 0
    if (this.times.length >= this.max) return 0
    const gain = 1 / (1 + 0.55 * this.times.length)
    this.times.push(t)
    return gain
  }
}

// A short wet "pfft-blip" for a nonsense word leaving the mouth: a pitched sine chirp with a puff of
// band-passed noise. Pitch varies a little per word through `rate`.
export function scheduleSpit(ctx, destination, when, { gain = 1, rate = 1, pan = 0 } = {}) {
  const out = ctx.createGain()
  out.gain.value = 0.32 * gain
  let tail = out
  if (ctx.createStereoPanner) {
    const p = ctx.createStereoPanner()
    p.pan.value = Math.max(-1, Math.min(1, pan))
    out.connect(p)
    tail = p
  }
  tail.connect(destination)
  const osc = ctx.createOscillator()
  const env = ctx.createGain()
  osc.type = "sine"
  osc.frequency.setValueAtTime(260 * rate, when)
  osc.frequency.exponentialRampToValueAtTime(720 * rate, when + 0.05)
  env.gain.setValueAtTime(0.0001, when)
  env.gain.linearRampToValueAtTime(1, when + 0.006)
  env.gain.exponentialRampToValueAtTime(0.0001, when + 0.075)
  osc.connect(env).connect(out)
  osc.start(when)
  osc.stop(when + 0.09)
  const len = Math.max(1, Math.floor(ctx.sampleRate * 0.04))
  const buf = ctx.createBuffer(1, len, ctx.sampleRate)
  const d = buf.getChannelData(0)
  let seed = 777
  for (let i = 0; i < len; i++) {
    seed = (seed * 1664525 + 1013904223) >>> 0
    d[i] = ((seed / 4294967296) * 2 - 1) * Math.exp(-i / (len * 0.3))
  }
  const src = ctx.createBufferSource()
  src.buffer = buf
  const bp = ctx.createBiquadFilter()
  bp.type = "bandpass"
  bp.frequency.value = 1800 * rate
  bp.Q.value = 0.7
  const g = ctx.createGain()
  g.gain.value = 0.35
  src.connect(bp).connect(g).connect(out)
  src.start(when)
  return when + 0.09
}
