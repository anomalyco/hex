// Web Audio mixer for the hero. Nothing is fetched until the show starts; then the clips the
// opening needs are decoded first and the rest of the bank streams in behind them. In capture
// builds every play() is logged (time, file, gain, rate, pan, bus) so a recording can be mixed
// offline without playing anything aloud.

import { ArrivalGate, arrivalVoice, scheduleArrival, scheduleSpit } from "./arrival.js"

const URLS = import.meta.glob("./audio/*.mp3", { query: "?url", import: "default", eager: true })
const urlOf = (file) => URLS[`./audio/${file}`]

export class AudioEngine {
  constructor({ clock }) {
    this.clock = clock
    this.log = import.meta.env.VITE_SHOUT_CAPTURE === "1" ? [] : null
    this.ctx = null
    this.manifest = null
    this.buffers = new Map()
    this.muted = false
    this.hidden = false
    this.duck = 0
    this.arrivals = new ArrivalGate()
    this.spits = new ArrivalGate({ gap: 0.07, window: 0.5, max: 6 })
  }

  // The context is created by the caller inside the start gesture (or later, on unmute).
  attach(context, { muted }) {
    this.muted = muted
    if (context) this.build(context)
  }

  // Resolves once the first replies and the interface sounds are decoded.
  async load(manifest) {
    this.manifest = manifest
    const first = new Set(Object.values(manifest.sfx))
    const rest = new Set()
    for (const line of Object.values(manifest.voice)) for (const t of line.takes) (line.stage === "1" ? first : rest).add(t.file)
    const queue = [...first, ...rest]
    const firstDone = Promise.all([...first].map((f) => this.fetchOne(f)))
    queue.splice(0, first.size)
    const worker = async () => {
      while (queue.length) await this.fetchOne(queue.shift())
    }
    void firstDone.then(() => Promise.all([worker(), worker(), worker()]))
    // Never hold the show hostage to a slow network.
    await Promise.race([firstDone, new Promise((r) => setTimeout(r, 2500))])
  }

  async fetchOne(file) {
    if (this.buffers.has(file)) return
    try {
      const bytes = await (await fetch(urlOf(file))).arrayBuffer()
      this.buffers.set(file, bytes)
      if (this.ctx) await this.decode(file)
    } catch {
      this.buffers.delete(file)
    }
  }

  async decode(file) {
    const v = this.buffers.get(file)
    if (!(v instanceof ArrayBuffer)) return
    this.buffers.set(file, null)
    try {
      this.buffers.set(file, await this.ctx.decodeAudioData(v))
    } catch {
      this.buffers.delete(file)
    }
  }

  get on() {
    return !this.muted && !this.hidden && this.ctx?.state === "running"
  }

  setMuted(muted, context) {
    this.muted = muted
    if (!muted && !this.ctx && context) this.build(context)
    this.sync()
  }

  setHidden(hidden) {
    this.hidden = hidden
    this.sync()
  }

  sync() {
    if (!this.ctx) return
    if (this.muted || this.hidden) this.ctx.suspend().catch(() => {})
    else this.ctx.resume().catch(() => {})
  }

  close() {
    this.ctx?.close().catch(() => {})
    this.ctx = null
  }

  build(ctx) {
    this.ctx = ctx
    const limiter = ctx.createDynamicsCompressor()
    limiter.threshold.value = -3
    limiter.knee.value = 0
    limiter.ratio.value = 20
    limiter.attack.value = 0.002
    limiter.release.value = 0.12
    const comp = ctx.createDynamicsCompressor()
    comp.threshold.value = -18
    comp.ratio.value = 3
    comp.attack.value = 0.01
    comp.release.value = 0.2
    this.master = ctx.createGain()
    this.master.gain.value = 0.8
    this.master.connect(comp).connect(limiter).connect(ctx.destination)
    this.buses = {}
    for (const [name, g] of Object.entries(BUS)) {
      const bus = ctx.createGain()
      bus.gain.value = g
      this.buses[name] = bus
    }
    this.buses.ui.connect(this.master)
    this.buses.voice.connect(this.master)
    // Impacts get brighter as intensity rises.
    this.tone = ctx.createBiquadFilter()
    this.tone.type = "lowpass"
    this.tone.frequency.value = 6000
    this.buses.impacts.connect(this.tone).connect(this.master)
    for (const f of this.buffers.keys()) void this.decode(f)
    this.sync()
  }

  setIntensity(i) {
    this.i = i
    this.applyDuck(0.25)
  }

  applyDuck(timeConstant = 0.015) {
    if (!this.ctx || !this.buses) return
    const t = this.ctx.currentTime
    const i = this.i ?? 0
    const d = Math.max(0, Math.min(1, this.duck))
    this.tone.frequency.setTargetAtTime(3500 + 16000 * i * i, t, 0.3)
    this.buses.ui.gain.setTargetAtTime(BUS.ui * (1 - 0.35 * i) * (1 - 0.86 * d), t, timeConstant)
    this.buses.impacts.gain.setTargetAtTime(BUS.impacts * (1 - 0.88 * d), t, timeConstant)
  }

  setDuck(level, dur = 0) {
    this.duck = Math.max(0, Math.min(1, level))
    this.applyDuck(this.duck > 0 ? 0.01 : 0.14)
    clearTimeout(this.duckTimer)
    if (dur > 0) {
      this.duckTimer = setTimeout(() => {
        this.duck = 0
        this.applyDuck(0.16)
      }, dur * 1000)
    }
  }

  sfx(name, opts = {}) {
    const file = this.manifest?.sfx[name]
    if (file) this.play(file, { bus: "ui", ...opts })
  }

  // A soft synthesized arrival sound for a notification card, matched to its app. Rides the ui bus,
  // so it ducks under the explosion and voice like every other interface sound.
  arrive(kind, { gain = 1, rate = 1, pan = 0 } = {}) {
    const voice = arrivalVoice(kind)
    const g = this.arrivals.admit(this.clock()) * gain
    if (g <= 0) return
    this.log?.push({ t: +this.clock().toFixed(4), file: `synth-arrive-${voice}`, kind, bus: "ui", gain: +g.toFixed(3), rate: +rate.toFixed(3), pan: +pan.toFixed(3), i: +(this.i ?? 0).toFixed(3), duck: +this.duck.toFixed(3) })
    if (!this.on || !this.buses?.ui) return
    try {
      scheduleArrival(this.ctx, this.buses.ui, voice, this.ctx.currentTime, { gain: g, rate, pan })
    } catch {}
  }

  // The small pfft a nonsense word makes leaving the mouth. Gated so a fast stream stays a patter.
  spit({ gain = 1, rate = 1, pan = 0 } = {}) {
    const g = this.spits.admit(this.clock()) * gain
    if (g <= 0) return
    this.log?.push({ t: +this.clock().toFixed(4), file: "synth-spit", bus: "ui", gain: +g.toFixed(3), rate: +rate.toFixed(3), pan: +pan.toFixed(3), i: +(this.i ?? 0).toFixed(3), duck: +this.duck.toFixed(3) })
    if (!this.on || !this.buses?.ui) return
    try {
      scheduleSpit(this.ctx, this.buses.ui, this.ctx.currentTime, { gain: g, rate, pan })
    } catch {}
  }

  // Immediate synthesized mechanical key bottom-out + sub-bass detonation on click (zero network wait).
  keyBoom() {
    this.log?.push({ t: +this.clock().toFixed(4), file: "synth-key-boom", bus: "impacts", gain: 0.9, rate: 1, pan: 0, i: +(this.i ?? 0).toFixed(3), duck: this.duck })
    if (!this.on || !this.ctx || !this.buses?.impacts) return
    try {
      const ctx = this.ctx
      const now = ctx.currentTime
      const bus = this.buses.impacts

      // 1. Sub-bass detonation thump (135 Hz -> 34 Hz)
      const osc = ctx.createOscillator()
      const og = ctx.createGain()
      osc.type = "sine"
      osc.frequency.setValueAtTime(135, now)
      osc.frequency.exponentialRampToValueAtTime(34, now + 0.26)
      og.gain.setValueAtTime(0.65, now)
      og.gain.exponentialRampToValueAtTime(0.001, now + 0.3)
      osc.connect(og).connect(bus)
      osc.start(now)
      osc.stop(now + 0.32)

      // 2. Crisp switch clack + plastic shatter noise burst
      const len = Math.floor(ctx.sampleRate * 0.18)
      const buf = ctx.createBuffer(1, len, ctx.sampleRate)
      const data = buf.getChannelData(0)
      for (let i = 0; i < len; i++) {
        const env = i < ctx.sampleRate * 0.006 ? 1 : Math.exp(-i / (ctx.sampleRate * 0.038))
        data[i] = (Math.random() * 2 - 1) * env
      }
      const nSrc = ctx.createBufferSource()
      nSrc.buffer = buf
      const bp = ctx.createBiquadFilter()
      bp.type = "bandpass"
      bp.frequency.setValueAtTime(2200, now)
      bp.frequency.exponentialRampToValueAtTime(520, now + 0.16)
      bp.Q.value = 1.4
      const ng = ctx.createGain()
      ng.gain.setValueAtTime(0.55, now)
      ng.gain.exponentialRampToValueAtTime(0.001, now + 0.18)
      nSrc.connect(bp).connect(ng).connect(bus)
      nSrc.start(now)
    } catch {}
  }

  voice(file, opts = {}) {
    if (opts.duck) {
      const d = typeof opts.duck === "number" ? opts.duck : 0.88
      this.setDuck(d, (opts.dur ?? 1) + 0.18)
    }
    this.play(file, { bus: "voice", ...opts })
  }

  play(file, { bus = "ui", gain = 1, rate = 1, pan = 0, dur = 0, duck = 0 } = {}) {
    const effDuck = bus === "voice" ? (typeof duck === "number" ? duck : duck ? 0.88 : 0) : this.duck
    this.log?.push({
      t: +this.clock().toFixed(4),
      file,
      bus,
      gain: +gain.toFixed(3),
      rate: +rate.toFixed(3),
      pan: +pan.toFixed(3),
      dur: +dur.toFixed(3),
      i: +(this.i ?? 0).toFixed(3),
      duck: +effDuck.toFixed(3),
    })
    if (!this.on) return
    const buf = this.buffers.get(file)
    if (!(buf instanceof AudioBuffer)) return
    const ctx = this.ctx
    const src = ctx.createBufferSource()
    src.buffer = buf
    src.playbackRate.value = rate
    const g = ctx.createGain()
    g.gain.value = gain
    const p = ctx.createStereoPanner()
    p.pan.value = Math.max(-1, Math.min(1, pan))
    src.connect(g).connect(p).connect(this.buses[bus])
    src.start()
  }
}

const BUS = { ui: 0.4, voice: 1.25, impacts: 0.56 }
