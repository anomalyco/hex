// Hex's recording field: Dot scope, as the meeting capsule draws it. A line-for-line port of
//   hex src/rune_hud/dot_scope.rs        Tuning, AutoLevel, loudness
//   hex src/rune_hud/dot_scope/scope.rs  the 240 Hz simulation: Voice, Trace, the braid, the phosphor
//   hex src/rune_hud/spring.rs           the analytic spring
//   hex src/rune_ui/meeting_capsules/waveform.rs  Listening: max(mic, system) RMS, presence, settle_field, cooling
//   hex src/rune_hud.wgsl                dot_scope_fragment with dots_only (the capsule's painter)
// Only the Recording stage: a meeting field never processes; after Stop it releases and fades.
//
// It is driven by the call itself: the decoded PCM of every voice line that is sounding, as two
// tracks (his own voice is the microphone, everyone else the system audio), each line through the
// same fader the mixer gives it. The simulation steps on a fixed grid from the moment Record is
// pressed (240 Hz inside, a meter read every 1/60 s, as Hex's capsule polls), and keeps a
// checkpoint every half second, so a frame at show time t is the same picture whether the show
// played up to it live or a script jumped there with ?t= / renderAt(t).

import { lineCurve, curveAt, VOICE_LEVEL, ME_LEVEL } from './audio.js'

const TAU = Math.PI * 2, PI = Math.PI
const STEP = 1 / 240
const HISTORY = 512
const SUB = 3 // SAMPLES_PER_COLUMN
const SPEEDS = [0.8, 1.25, 1.85, 2.6]
const BASE = [0.55, 0.42, 0.3, 0.2]
const SPATIAL = [1.4, 2.4, 3.8, 5.6]
const START_WIDTH = 30, ENTER = 0.42, PHOSPHOR_HEAD = 0.15, BREATH_PERIOD = 5.5
const HIDDEN_SCALE = 0.9, HIDDEN_SOFTNESS = 3, CARRIER_REST = 0.14
const MAX_COLUMNS = 96, MAX_ROWS = 15, MAX_DOTS = 1024

// Tuning::default(), with the meeting field's 78 pt / 31 columns (waveform.rs Listening::new).
export const TUNING = {
  columns: 31, rows: 9, spacing: 2.6, width: 78, height: 26,
  braid_speed: 1, helix_cycles: 1.6, gain: 1, response: 6.5, release: 0.15, history_speed: 150,
  phosphor: 0.6, transition: 0.3, bounce: 0.15, auto_release: 3, noise_floor: -60,
}
// waveform.rs
const ATTACK = [0.16, 0], RELEASE = [0.55, 0]
const COOL = 0.45
const FLOOR = 0.001 // -60 dBFS in the meter's RMS units
// What the microphone hears when nobody in the room speaks: a quiet laptop room, as
// AutoLevel's own default assumes (-55 dB). Hex reads the real one; the film has none.
export const ROOM_DB = -56

const clamp = (v, a, b) => (v < a ? a : v > b ? b : v)
const smoothstep = (a, b, x) => { const t = clamp((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t) }
const bump = (d, w) => Math.exp(-(d / w) * (d / w))
const passPosition = (p) => 0.5 - 0.5 * Math.cos(PI * clamp(p, 0, 1))
const loudness = (rms) => 20 * Math.log10(Math.max(rms, 0) + 1e-6)

// ——— spring.rs: exact damped harmonic oscillator, SwiftUI's (duration, bounce) ———
class Spring {
  constructor(x) { this.x = x; this.v = 0; this.target = x }
  jump(x) { this.x = x; this.v = 0; this.target = x }
  stepTo(target, [duration, bounce], dt) {
    this.target = target
    if (dt <= 0) return
    const omega = TAU / Math.max(duration, 0.01)
    const zeta = bounce >= 0 ? Math.max(1 - bounce, 0) : 1 / Math.max(1 + bounce, 0.01)
    const x0 = this.x - target, v0 = this.v
    if (zeta < 1 - 1e-4) {
      const decay = zeta * omega, od = omega * Math.sqrt(1 - zeta * zeta)
      const e = Math.exp(-decay * dt), s = Math.sin(od * dt), c = Math.cos(od * dt), so = s / od
      this.x = target + e * (x0 * (c + decay * so) + v0 * so)
      this.v = e * (-x0 * omega * omega * so + v0 * (c - decay * so))
    } else if (zeta > 1 + 1e-4) {
      const root = Math.sqrt(zeta * zeta - 1), sum = zeta + root
      const sr = -omega / sum, fr = -omega * sum, den = sr - fr
      const se = Math.exp(sr * dt), fe = Math.exp(fr * dt)
      this.x = target + ((-fr * se + sr * fe) / den) * x0 + ((se - fe) / den) * v0
      this.v = ((sr * fr * (fe - se)) / den) * x0 + ((sr * se - fr * fe) / den) * v0
    } else {
      const e = Math.exp(-omega * dt), c = v0 + omega * x0
      this.x = target + (x0 + c * dt) * e
      this.v = (v0 - omega * c * dt) * e
    }
  }
  copy() { const s = new Spring(0); s.x = this.x; s.v = this.v; s.target = this.target; return s }
}

// ——— dot_scope.rs: AutoLevel ———
const AUTO_MIN_SPAN = 16, AUTO_RANGE = 24, AUTO_ROOM_MARGIN = 5, AUTO_ATTACK = 0.04, ROOM_FALL = 0.3, ROOM_RISE = 6
function autoStep(a, db, h, release, floor) {
  db = Math.max(db, -120)
  const follow = (v, target, time) => v + (target - v) * (1 - Math.exp(-h / Math.max(time, 1e-3)))
  a.room = follow(a.room, db, db < a.room ? ROOM_FALL : ROOM_RISE)
  const lowest = Math.max(a.room, floor) + AUTO_MIN_SPAN
  a.peak = db > a.peak ? follow(a.peak, db, AUTO_ATTACK) : follow(a.peak, lowest, release)
  a.peak = Math.max(a.peak, lowest)
  const low = Math.max(Math.max(a.room, floor) + AUTO_ROOM_MARGIN, a.peak - AUTO_RANGE)
  return clamp((db - low) / Math.max(a.peak - low, 1), 0, 1)
}

// ——— scope.rs: Voice ———
const voice = () => ({ level: 0, reservoir: 0, energy: 0, kick: 0, kickV: 0, band: 0 })
function voiceStep(v, raw, h) {
  const prev = v.level
  v.level += (raw - v.level) * (1 - Math.exp(-h / (raw > v.level ? 0.035 : 0.095)))
  const rise = Math.max((v.level - prev) / h, 0)
  v.reservoir += (1.75 * v.level * v.level - v.reservoir / 1.8) * h
  v.energy = 1 - Math.exp(-v.reservoir * 1.45)
  const w = TAU * 1.3, z = 0.22
  v.kickV += (-w * w * v.kick - 2 * z * w * v.kickV + 15 * rise + 2.2 * v.level) * h
  v.kick += v.kickV * h
  v.band += (raw - v.band) * (1 - Math.exp(-h / (raw > v.band ? 0.025 : 0.14)))
}

// ——— scope.rs: Trace ———
const trace = (phases) => ({ envelope: 0, amplitude: 0, amplitudeV: 0, glowFast: 0, glowSpring: 0, glowSpringV: 0, glowSlow: 0, glow: 0, phases: phases.slice(), history: new Float32Array(HISTORY * 5), head: 0 })
function traceTick(tr, v, live, speed, T, h) {
  const follow = (value, target, rise, fall) => value + (target - value) * (1 - Math.exp(-h / (target > value ? rise : fall)))
  tr.envelope = follow(tr.envelope, v.level, 0.022, T.release)
  const w = TAU * T.response, z = 0.52
  tr.amplitudeV += (-w * w * (tr.amplitude - tr.envelope) - 2 * z * w * tr.amplitudeV) * h
  tr.amplitude += tr.amplitudeV * h
  tr.glowFast = follow(tr.glowFast, v.level, 0.045, 0.13)
  const gw = TAU * 3.6, gz = 0.5
  tr.glowSpringV += (-gw * gw * (tr.glowSpring - tr.glowFast) - 2 * gz * gw * tr.glowSpringV) * h
  tr.glowSpring += tr.glowSpringV * h
  tr.glowSlow = follow(tr.glowSlow, v.level, 0.15, 0.7)
  const swell = 0.8 * Math.max(tr.glowSpring - 0.25, 0) + 0.04 * Math.max(v.kick, -0.5)
  tr.glow = Math.min(Math.max(swell, 0) + 0.42 * tr.glowSlow + 0.2 * v.energy, 1.15) * live
  const go = 0.6 + 1.1 * Math.min(tr.envelope * 1.6, 1)
  for (let c = 0; c < 4; c++) tr.phases[c] = (tr.phases[c] + h * TAU * SPEEDS[c] * go * speed) % TAU
  const emphasis = 0.35 + 0.65 * Math.min(v.band / (v.band + 0.06), 2)
  const o = tr.head * 5, amp = Math.max(tr.amplitude, 0) * live
  tr.history[o] = amp
  for (let c = 0; c < 4; c++) tr.history[o + 1 + c] = amp * BASE[c] * emphasis
  tr.head = (tr.head + 1) % HISTORY
}
function past(tr, delay, ch) {
  const d = Math.min(delay / STEP, HISTORY - 2), i = Math.floor(d), k = d - i
  const a = (tr.head + 2 * HISTORY - 1 - i) % HISTORY, b = (a + HISTORY - 1) % HISTORY
  return tr.history[a * 5 + ch] * (1 - k) + tr.history[b * 5 + ch] * k
}
const copyTrace = (t) => ({ ...t, phases: t.phases.slice(), history: t.history.slice() })

// ——— scope.rs: Scope, Recording only ———
class Scope {
  constructor(tuning) {
    this.T = { ...tuning }
    this.elapsed = 0
    this.auto = { peak: -30, room: -55 }
    this.voices = [voice(), voice()]
    this.traces = [trace([0.1, 1.7, 3.3, 4.6]), trace([2.4, 0.6, 5.1, 1.9])]
    this.live = 0; this.carrier = 0; this.time = 0; this.steps = 0
    this.columns = 0; this.rows = 0; this.shift = 0
    this.head = [new Float32Array(0), new Float32Array(0)]
    this.tail = [new Float32Array(0), new Float32Array(0)]
    this.power = new Float32Array(0); this.fresh = new Float32Array(0)
    this.ys = [null, null]; this.bright = [null, null]
    this.width = new Spring(START_WIDTH); this.opacity = new Spring(0)
    this.scale = new Spring(HIDDEN_SCALE); this.softness = new Spring(HIDDEN_SOFTNESS)
    this.resize()
  }
  copy() {
    const s = Object.create(Scope.prototype)
    Object.assign(s, this)
    s.T = { ...this.T }; s.auto = { ...this.auto }
    s.voices = this.voices.map((v) => ({ ...v })); s.traces = this.traces.map(copyTrace)
    s.head = this.head.map((a) => a.slice()); s.tail = this.tail.map((a) => a.slice())
    s.power = this.power.slice(); s.fresh = this.fresh.slice()
    s.ys = this.ys.map((a) => a.slice()); s.bright = this.bright.map((a) => a.slice())
    for (const k of ['width', 'opacity', 'scale', 'softness']) s[k] = this[k].copy()
    return s
  }
  // Scope::span with Hold::Right: the field room pt wide at once, its right end held.
  span(room) {
    const T = this.T
    room = clamp(room, 40, 280)
    const columns = clamp(Math.max(Math.floor(room / T.spacing + 1e-3) + 1, 1), 11, MAX_COLUMNS)
    const rows = Math.min(TUNING.rows, Math.floor(MAX_DOTS / columns), MAX_ROWS)
    const shift = (room - (columns - 1) * T.spacing) / 2
    if (columns === T.columns && room === T.width && rows === T.rows && shift === this.shift) return
    T.columns = columns; T.width = room; T.rows = rows
    this.shift = shift
    this.width.jump(room)
    this.resize()
    this.beam(0)
  }
  resize() {
    const columns = this.T.columns, rows = this.T.rows
    if (columns === this.columns && rows === this.rows) return
    const wc = this.columns, wr = this.rows
    const across = columns - wc // Hold::Right
    const down = Math.floor((rows - wr) / 2)
    const kept = (old) => {
      const n = new Float32Array(columns * rows)
      for (let k = 0; k < n.length; k++) {
        const i = (k % columns) - across, j = Math.floor(k / columns) - down
        if (i >= 0 && i < wc && j >= 0 && j < wr) n[k] = old[j * wc + i]
      }
      return n
    }
    this.head = this.head.map(kept); this.tail = this.tail.map(kept)
    this.power = kept(this.power); this.fresh = kept(this.fresh)
    this.columns = columns; this.rows = rows
    const samples = (columns - 1) * SUB + 1
    this.ys = [new Float32Array(samples), new Float32Array(samples)]
    this.bright = [new Float32Array(samples), new Float32Array(samples)]
  }
  step(db) {
    const h = STEP, T = this.T
    const level = clamp(autoStep(this.auto, db, h, T.auto_release, T.noise_floor) * T.gain, 0, 1)
    voiceStep(this.voices[0], level, h)
    voiceStep(this.voices[1], level * 0.85, h)
    this.live += (1 - this.live) * (1 - Math.exp(-h / 0.55))
    const speed = T.braid_speed
    traceTick(this.traces[0], this.voices[0], this.live, speed, T, h)
    traceTick(this.traces[1], this.voices[1], this.live, speed, T, h)
    const energy = Math.max(this.voices[0].energy, this.voices[1].energy)
    this.carrier = (this.carrier + h * TAU * (CARRIER_REST + 0.22 * energy) * speed) % TAU
    const fade = [T.transition, 0], bouncy = [T.transition, T.bounce]
    this.width.stepTo(T.width, bouncy, h)
    this.opacity.stepTo(1, fade, h)
    this.scale.stepTo(1, bouncy, h)
    this.softness.stepTo(0, fade, h)
    this.elapsed += h; this.time += h; this.steps++
    if (this.steps % 2 === 0) this.beam(2 * h)
  }
  beam(hb) {
    const { columns, rows } = this, T = this.T
    const samples = (columns - 1) * SUB + 1, pitch = T.spacing
    const width = Math.max(this.width.x, T.height), e = this.elapsed
    // Scan for Recording: Envelope::Enter, the scanner that unfolds the trace.
    const reach = (width / 2 + 12) * passPosition(e / ENTER)
    const headK = 2.8 * (1 - smoothstep(0.08, ENTER, e))
    const unfold = smoothstep(0, 0.1, e)
    const mid = (rows - 1) / 2, half = mid - 0.55
    const breath = 0.5 + 0.5 * Math.sin((this.time * TAU) / BREATH_PERIOD)
    const rest = (0.55 + 0.35 * breath) * (0.4 + 0.6 * this.live)
    const hw = width / 2
    for (let k = 0; k < 2; k++) {
      const tr = this.traces[k], sign = k === 0 ? 1 : -1, ys = this.ys[k], br = this.bright[k]
      for (let s = 0; s < samples; s++) {
        const x = (s / SUB - (columns - 1) / 2) * pitch + this.shift
        if (Math.abs(x) > hw) { ys[s] = mid; br[s] = 0; continue }
        const d = Math.abs(x)
        const drawn = unfold * (1 - smoothstep(Math.max(reach - 14, 0), Math.max(reach, 0.001), d))
        const amplitude = drawn * (1 - smoothstep(Math.max(reach - 22, 0), Math.max(reach - 3, 0.001), d))
        const bright = amplitude + headK * bump(d - reach, 6)
        const swell = 1.8 * unfold * (headK / 2.8) * bump(d - Math.max(reach - 14, 0), 14)
        const xn = (x + hw) / width
        const ends = Math.min(smoothstep(-hw + 3, -hw + 17, x), smoothstep(-hw + 3, -hw + 17, -x))
        const rch = 1 - 0.8 * smoothstep(14, hw, d)
        const delay = d / T.history_speed
        const a = past(tr, delay, 0)
        let spread = 0
        for (let c = 0; c < 4; c++) spread += past(tr, delay, 1 + c) * Math.sin(TAU * SPATIAL[c] * xn - tr.phases[c])
        const helix = (rest + swell + half * 0.55 * a * rch) * Math.sin(TAU * T.helix_cycles * xn - this.carrier)
        const y = half * Math.tanh((helix + half * 0.7 * spread * rch) / half) * ends * amplitude
        ys[s] = mid - sign * y
        const spatial = (0.3 + 0.7 * Math.min(a * 1.8, 1)) * (0.45 + 0.55 * ends)
        br[s] = spatial * amplitude + 0.36 * Math.max(bright - amplitude, 0)
      }
      for (let s = 0; s < samples; s++) {
        const slope = ((ys[Math.min(s + 1, samples - 1)] - ys[Math.max(s - 1, 0)]) * SUB) / 2
        br[s] *= 1 - 0.55 + 0.55 / Math.sqrt(1 + 0.22 * slope * slope)
      }
    }
    const headKeep = Math.exp(-hb / PHOSPHOR_HEAD), tailKeep = Math.exp(-hb / T.phosphor)
    const dx = 1 / SUB
    for (let i = 0; i < columns; i++) {
      const s0 = Math.max(i - 3, 0) * SUB, s1 = Math.min((i + 3) * SUB, samples - 1)
      for (let j = 0; j < rows; j++) {
        const n = j * columns + i
        let now = 0, total = 0, l0 = 0, l1 = 0
        for (let k = 0; k < 2; k++) {
          const ys = this.ys[k]
          let best = Infinity, nearest = i * SUB
          for (let s = s0; s < s1; s++) {
            const ay = ys[s], dy = ys[s + 1] - ay, px = i - s * dx, py = j - ay
            const t = clamp((px * dx + py * dy) / (dx * dx + dy * dy), 0, 1)
            const ex = px - t * dx, ey = py - t * dy, d2 = ex * ex + ey * ey
            if (d2 < best) { best = d2; nearest = s }
          }
          const b = this.bright[k][nearest]
          const onBeam = Math.exp(-best / 0.25)
          const instant = Math.min(onBeam + 0.5 * Math.exp(-Math.sqrt(best) / 0.8), 1.2) * b
          const core = onBeam * b
          const hd = this.head[k], tl = this.tail[k]
          hd[n] = Math.max(hd[n] * headKeep, core)
          tl[n] = Math.max(tl[n] * tailKeep, core)
          const light = Math.max(instant, hd[n], 0.55 * tl[n])
          if (k === 0) l0 = light; else l1 = light
          now += instant; total += light
        }
        this.power[n] = clamp(1 - (1 - Math.min(l0, 1)) * (1 - Math.min(l1, 1)), 0, 1)
        this.fresh[n] = Math.min(now / Math.max(total, 1e-4), 1)
      }
    }
  }
}

// ——— waveform.rs: Listening, the meeting capsule's field ———
class Listening {
  constructor() {
    this.scope = new Scope(TUNING)
    this.presence = new Spring(0)
    this.drawn = false
    this.cooling = null
    this.fade = 1
    this.gone = false
  }
  copy() {
    const l = Object.create(Listening.prototype)
    Object.assign(l, this)
    l.scope = this.scope.copy(); l.presence = this.presence.copy()
    return l
  }
  // One poll of the capsule (feed, fit, sample), dt = 1/60.
  sample(now, levels, recording, room) {
    if (this.gone) return
    this.scope.span(room)
    const [mic, system] = recording ? levels : [0, 0]
    const dt = 1 / 60
    const rms = Math.max(mic, system)
    if (!(rms < FLOOR && this.presence.x === 0 && this.drawn)) {
      const target = rms >= FLOOR ? 1 : 0
      // Never travel away from newly heard speech, nor swell on a pause.
      if (target !== this.presence.target) this.presence.v = 0
      this.presence.stepTo(target, target ? ATTACK : RELEASE, dt)
      if (Math.abs(target - this.presence.x) < 0.001) this.presence.jump(target)
      const db = loudness(rms)
      for (let i = 0; i < 4; i++) this.scope.step(db) // IndicatorModel::step → Scope::advance(1/60) at 240 Hz
      this.drawn = true
    }
    if (!recording) {
      if (this.cooling == null) this.cooling = now
      const t = (now - this.cooling) / COOL
      if (t >= 1) this.gone = true
      else this.fade = 1 - t * t * (3 - 2 * t)
    }
  }
}

// ——— the call's two tracks, from the decoded voice PCM ———
// RMS over the last 10 ms (one capture buffer), every 1/240 s from each clip's first sample.
const RATE = 24000, HOP = 240, WIN = Math.round(RATE * 0.01)
function rmsTrack(buf) {
  const n = buf.length, chans = buf.numberOfChannels
  const data = []
  for (let c = 0; c < chans; c++) data.push(buf.getChannelData(c))
  const sq = new Float64Array(n + 1)
  for (let i = 0; i < n; i++) {
    let v = 0
    for (let c = 0; c < chans; c++) v += data[c][i]
    v /= chans
    sq[i + 1] = sq[i] + v * v
  }
  const frames = Math.ceil((n / RATE) * HOP) + 1
  const out = new Float32Array(frames)
  for (let f = 0; f < frames; f++) {
    const end = Math.min(Math.round((f / HOP) * RATE), n), start = Math.max(end - WIN, 0)
    out[f] = end > start ? Math.sqrt(Math.max(sq[end] - sq[start], 0) / (end - start)) : 0
  }
  return { rms: out, dur: n / RATE }
}

export class CallMeter {
  // bytes: clip id → Promise of the file's ArrayBuffer (AudioEngine.bytes)
  constructor(S, bytes) {
    this.lines = S.lines.map((l) => ({ l, me: l.who === 'me', curve: lineCurve(l), track: null }))
    this.room = Math.pow(10, ROOM_DB / 20)
    this.version = 0
    const Ctx = window.OfflineAudioContext || window.webkitOfflineAudioContext
    const ids = [...new Set(S.lines.map((l) => l.clip))]
    const tracks = new Map()
    this.ready = !Ctx
      ? Promise.resolve(false)
      : Promise.all(ids.map(async (id) => {
          const b = await bytes.get(id)
          if (!b) return
          const ctx = new Ctx(1, 1, RATE)
          const buf = await new Promise((res, rej) => { const p = ctx.decodeAudioData(b.slice(0), res, rej); if (p && p.then) p.then(res, rej) })
          tracks.set(id, rmsTrack(buf))
        })).then(() => {
          for (const x of this.lines) x.track = tracks.get(x.l.clip) ?? null
          this.version++
          return true
        }).catch(() => false)
  }
  // [microphone, system] RMS at show time t.
  at(t) {
    let mic = this.room * this.room, sys = 0
    for (const x of this.lines) {
      const l = x.l, u = t - l.t0
      if (u < 0) continue
      let r
      if (x.track) {
        if (u >= Math.min(l.dur, x.track.dur)) continue
        const f = u * HOP, i = Math.floor(f), k = f - i, a = x.track.rms
        r = i + 1 < a.length ? a[i] * (1 - k) + a[i + 1] * k : 0
      } else {
        // Before the PCM is decoded: the manifest's 16 ms envelope.
        const e = l.env, g = u / 0.016
        if (g >= e.length - 1) continue
        const i = Math.floor(g)
        r = ((e[i] * (1 - (g - i)) + e[i + 1] * (g - i)) / 100) * 0.1
      }
      r *= curveAt(x.curve, u) * (x.me ? ME_LEVEL : VOICE_LEVEL)
      if (x.me) mic += r * r
      else sys += r * r
    }
    return [Math.sqrt(mic), Math.sqrt(sys)]
  }
}

// ——— the timeline: the field at any show time, the same however it got there ———
const EVERY = 30 // frames between checkpoints
export class FieldTimeline {
  // meter: CallMeter. t0: Record. stop: Stop. roomAt(t): the field's width then, pt.
  constructor(meter, t0, stop, roomAt) {
    Object.assign(this, { meter, t0, stop, roomAt })
    this.reset()
  }
  reset() {
    this.version = this.meter.version
    this.marks = new Map()
    this.cur = new Listening()
    this.k = 0
    this.marks.set(0, this.cur.copy())
  }
  frameOf(t) { return Math.floor((t - this.t0) * 60 + 1e-6) }
  poll(l, k) {
    const now = this.t0 + k / 60
    l.sample(now, this.meter.at(now), now < this.stop, this.roomAt(now))
    if (k % EVERY === 0 && !this.marks.has(k)) this.marks.set(k, l.copy())
  }
  // The Listening state at show time t, or null before Record.
  at(t) {
    if (this.meter.version !== this.version) this.reset()
    const k = this.frameOf(t)
    if (k < 1) return null
    if (k < this.k || k - this.k > EVERY) {
      let m = Math.floor(k / EVERY) * EVERY
      while (m > 0 && !this.marks.has(m)) m -= EVERY
      if (k < this.k || m > this.k) { this.cur = this.marks.get(m).copy(); this.k = m }
    }
    while (this.k < k) this.poll(this.cur, ++this.k)
    return this.cur
  }
  // Fills the checkpoints up to until, a few ms at a time, off the frame loop.
  warm(until, ms = 6) {
    const version = this.meter.version
    const w = { l: new Listening(), k: 0 }
    const last = this.frameOf(until)
    return new Promise((res) => {
      const slice = () => {
        if (version !== this.meter.version || version !== this.version) return res(false)
        const end = performance.now() + ms
        while (w.k < last && performance.now() < end) this.poll(w.l, ++w.k)
        if (w.k >= last) res(true)
        else setTimeout(slice, 0)
      }
      slice()
    })
  }
}

// ——— rune_hud.wgsl dot_scope_fragment, dots_only, as Canvas2D ———
// Dark palette, recording (cool 0, fail 0, saturation 1).
const BASE_C = [0.86, 0.83, 0.77], CORE_C = [0.98, 0.96, 0.92]
const FULL = 0.96, CURVE = 0.9 // dark
function roundedBox(px, py, hx, hy, r) {
  const qx = Math.abs(px) - hx + r, qy = Math.abs(py) - hy + r
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r
}
// ctx is in points; (cx, cy) is the field's middle; px is one device pixel, in points.
export function drawField(ctx, f, cx, cy, px) {
  const sc = f.scope, T = sc.T
  const { columns, rows } = sc
  const pitch = T.spacing, scale = Math.max(sc.scale.x, 0.01)
  const W = Math.max(sc.width.x, T.height), H = T.height, hx = W / 2, hy = H / 2, radius = H / 2
  const softness = clamp(sc.softness.x, 0, HIDDEN_SOFTNESS)
  const aa = (px / scale) * 0.75 + softness * 0.72
  const opacity = f.fade
  const presence = f.presence.x
  const midRow = Math.floor(rows / 2)
  const ox = (columns - 1) / 2, oy = (rows - 1) / 2
  const at = (r, i) => (r >= 0 && r < rows ? r * columns + i : -1)
  for (let j = 0; j < rows; j++) {
    for (let i = 0; i < columns; i++) {
      const x = (i - ox) * pitch + sc.shift, y = (j - oy) * pitch
      const mask = smoothstep(1.4, 4.6, -roundedBox(x, y, hx, hy, radius))
      if (mask <= 0) continue
      // settle_field: as presence falls the light gathers to the middle row and dims.
      let p = 0, fr = 0
      if (presence === 1) { p = sc.power[j * columns + i]; fr = sc.fresh[j * columns + i] }
      else if (presence > 0) {
        const yy = midRow + (j - midRow) / presence, lo = Math.floor(yy), t = yy - lo
        const a = at(lo, i), b = at(lo + 1, i)
        const pa = a < 0 ? 0 : sc.power[a], pb = b < 0 ? 0 : sc.power[b]
        const fa = a < 0 ? 0 : sc.fresh[a], fb = b < 0 ? 0 : sc.fresh[b]
        p = (pa + (pb - pa) * t) * presence
        fr = fa + (fb - fa) * t
      }
      const power = Math.max(p, 1e-5)
      // A quiet centreline, not a plate-shaped bed of inactive dots.
      const floor = j === midRow ? 0.26 : 0
      const alpha = (floor + (FULL - floor) * Math.pow(power, CURVE * (1.4 - 0.4 * fr))) * mask * opacity
      if (alpha < 0.002) continue
      const r = (0.25 + 0.17 * Math.pow(power, 0.6)) * (1 + (0.82 + 0.18 * fr - 1) * smoothstep(0.03, 0.2, power)) * pitch
      const core = clamp(power * power * 1.1, 0, 0.9) * (0.3 + 0.7 * fr)
      const R = Math.round((BASE_C[0] + (CORE_C[0] - BASE_C[0]) * core) * 255)
      const G = Math.round((BASE_C[1] + (CORE_C[1] - BASE_C[1]) * core) * 255)
      const B = Math.round((BASE_C[2] + (CORE_C[2] - BASE_C[2]) * core) * 255)
      const dx = cx + x * scale, dy = cy + y * scale, rr = r * scale, ar = aa * scale
      const rgb = 'rgba(' + R + ',' + G + ',' + B + ','
      ctx.beginPath()
      if (ar > 0.6) {
        // coverage(d - r, aa): soft while the field unblurs as it arrives.
        const outer = rr + ar, g = ctx.createRadialGradient(dx, dy, 0, dx, dy, outer)
        for (let s = 0; s <= 4; s++) g.addColorStop(s / 4, rgb + (alpha * smoothstep(ar, -ar, (outer * s) / 4 - rr)).toFixed(3) + ')')
        ctx.fillStyle = g
        ctx.arc(dx, dy, outer, 0, TAU)
      } else {
        ctx.fillStyle = rgb + alpha.toFixed(3) + ')'
        ctx.arc(dx, dy, rr, 0, TAU)
      }
      ctx.fill()
    }
  }
}
