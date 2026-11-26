// Sound for the show: recorded clips only. Every voice, every effect and the music is a file
// named in audio/manifest.json. Nothing is synthesized: there is no oscillator, no noise bed and
// no drone anywhere in this file, so between clips the page is silent, and a clip that is not
// there (no file in the manifest, a file that fails to load or decode) is simply not heard:
// the captions still carry the line.
//
// The clock is the sound card's. While the AudioContext is running, show time is read off the
// audio that is being heard (advance(), below), and everything in the schedule (show.js:
// S.events, S.auto, the cues for Play, Pause and Stop) is handed to the audio thread ahead of
// time, at the sample it is due. Nothing is ever started "now" from a frame: a frame that takes
// too long cannot open a gap between two voices, or leave the music running after Pause.
// Without a running context (muted benchmarks, a browser that refuses) show time is the frame
// clock and nothing is played.
//
// The mix, by where a sound is coming from:
//
//   call   what comes out of the meeting: the four coworkers, each panned to where their tile
//          sits, the one host (a stereo file, down the middle), and everything wet (through fx,
//          one fader for all of it: S.auto.fx). All of it goes through one chain, so that it
//          sounds like one call: high-pass, low-pass (S.auto.lp), a 3:1 compressor that holds
//          the pile-ups, then the call's own fader (S.auto.call).
//   me     his own voice. Dry, dead centre, outside the call, and nothing ever ducks it.
//   ui     his own Mac: clicks, keys, the transport's clack, the drip. Dry, centre.
//   music  what his headphones are playing: three stems of one track (S.music), each started
//          at its own beginning on a kick of the one before, which fades out under it. Its
//          fader is S.auto.music; a peaking filter takes the band a voice lives in down while
//          anyone speaks (S.auto.mid). It starts on the sample Play falls on and stops dead on
//          the sample Pause falls on: a 2 ms ramp that ends there, and nothing after.
//
//   master → gate → limiter → trim → ceiling → out
//          gate shuts over the 2 ms that end at Stop and stays shut for the dead air; out is
//          set to exactly zero for the same stretch, so what reaches the speakers there is
//          digital silence. The limiter is for the scream and the hit and should otherwise be
//          idle (it holds a peak at its threshold, which trim puts at 0.81 of full scale); the
//          ceiling is a guard, a straight line under CEIL_KNEE, that can never put out more
//          than CEIL (perf/bench.mjs --live measures all of it).
import { inSilence, CAST, STEMS } from './show.js'

const MUTED_KEY = 'hex-meetings-muted'
const readMuted = () => {
  try {
    return localStorage.getItem(MUTED_KEY) === '1'
  } catch {
    return false
  }
}

// ——— the numbers (perf/audible-gaps.mjs rebuilds the mix offline from these same ones) ———
const db = (d) => Math.pow(10, d / 20)
export const LEVEL = 0.9 // master
export const VOICE_LEVEL = 1.15 // a coworker, into the call
export const ME_LEVEL = 1.0 // his own voice (a mono file, both sides at full level)
export const FX_LEVEL = 0.7 // everything wet, into the call
export const UI_LEVEL = 0.7 // his Mac
export const MUSIC_LEVEL = 1.0 // the track when S.auto.music says 0 dB
export const MUSIC = STEMS // the clips it is made of
// The call's chain.
export const CALL = { hp: 110, lp: 7200, threshold: -20, ratio: 3, knee: 0, attack: 0.005, release: 0.12, makeup: 7 }
// (A Web Audio low-pass or high-pass takes its Q in dB: this one has no bump at the corner.)
export const FLAT_Q = 20 * Math.log10(Math.SQRT1_2)
// The band of the music a voice lives in.
export const MID = { hz: 1800, q: 0.9 }
// The limiter, and what follows it.
export const LIMIT = { threshold: -3, ratio: 20, knee: 0, attack: 0.002, release: 0.12 }
// After the limiter (and net of the make-up gain the limiter adds by itself, below).
export const TRIM = 1.15
export const CEIL = 0.9
export const CEIL_KNEE = 0.82
// A DynamicsCompressorNode looks this far ahead: what goes in comes out 6 ms later (measured in
// Chrome: 288 samples at 48 kHz). There are two in the path (the call's, the limiter), so what
// feeds one is scheduled 6 ms early.
export const LOOK = 0.006
// Overlaps: the line going out gives way, the one coming in is up on its first syllable.
export const YIELD_DB = -4
export const YIELD_S = 0.15
export const BOOST_DB = 2
export const BOOST_S = 0.25
export const BOOST_OUT_S = 0.1
export const POCKET_OUT_S = 0.12
// The gates at Pause and at Stop close over this long, ending on the cue.
export const GATE_S = 0.002
export const JOIN_S = 0.008
// How far ahead of show time sources are handed to the audio thread.
const AHEAD = 1.5

// A compressor adds a fixed make-up gain of its own (Web Audio: the gain that would bring a
// full-scale input back to full scale, to the power 0.6). With no knee that is exact.
export const autoMakeupDb = (c) => -0.6 * c.threshold * (1 - 1 / c.ratio)
// The call's fader at rest: the make-up we want, less the one the node adds by itself.
export const CALL_GAIN = db(CALL.makeup - autoMakeupDb(CALL))
// The same for the limiter: trim, less the limiter's own make-up.
export const TRIM_GAIN = TRIM * db(-autoMakeupDb(LIMIT))

// The ceiling's curve: y = x under the knee, then bending over so that it never passes CEIL.
export const ceiling = (x) => {
  const a = Math.abs(x)
  if (a <= CEIL_KNEE) return x
  const span = CEIL - CEIL_KNEE
  return Math.sign(x) * (CEIL_KNEE + span * (1 - Math.exp(-(a - CEIL_KNEE) / span)))
}

// A fader's moves as [seconds, gain] points with straight lines between them (held before the
// first and after the last), and where it is at u.
export const curveAt = (pts, u) => {
  if (u <= pts[0][0]) return pts[0][1]
  for (let i = 1; i < pts.length; i++) {
    if (u < pts[i][0]) {
      const [u0, v0] = pts[i - 1], [u1, v1] = pts[i]
      return v0 + ((v1 - v0) * (u - u0)) / (u1 - u0)
    }
  }
  return pts[pts.length - 1][1]
}
// A line's own fader, over the line's own time (0: the start of its file): its gain; +2 dB on
// its first quarter second if it comes in over someone (boost); -4 dB over 150 ms from the
// moment someone comes in over it (yieldAt); up or down for a stretch that show.js gives it
// (pocket: "OH CRUMPETS!" is up, and the screaming round it is down); and, for a line that is
// cut off mid-word, 6 ms to get to nothing at its end.
export const lineCurve = (l) => {
  const a0 = l.s0 - l.t0
  const parts = []
  if (l.boost) parts.push([[0, db(BOOST_DB)], [a0 + BOOST_S, db(BOOST_DB)], [a0 + BOOST_S + BOOST_OUT_S, 1]])
  if (l.yieldAt != null) parts.push([[0, 1], [Math.max(0, l.yieldAt), 1], [Math.max(0, l.yieldAt) + YIELD_S, db(YIELD_DB)]])
  if (l.pocket) {
    // (40 ms into it, 120 ms out of it; a line that starts inside it starts there.)
    const [u0, u1, d] = l.pocket
    parts.push(u0 <= 0.04 ? [[0, db(d)], [u1, db(d)], [u1 + POCKET_OUT_S, 1]] : [[0, 1], [u0 - 0.04, 1], [u0, db(d)], [u1, db(d)], [u1 + POCKET_OUT_S, 1]])
  }
  if (l.cutoff) parts.push([[0, 1], [Math.max(0, l.dur - 0.006), 1], [l.dur, 0]])
  // (A line that something stops: 12 ms to nothing, ending on that moment.)
  if (l.cutAt != null) parts.push([[0, 1], [Math.max(0, l.cutAt - 0.012), 1], [Math.max(0.001, l.cutAt), 0]])
  const g = l.gain ?? 1
  if (!parts.length) return [[0, g]]
  const at = [...new Set(parts.flat().map((p) => p[0]))].sort((a, b) => a - b)
  return at.map((u) => [u, parts.reduce((v, p) => v * curveAt(p, u), g)])
}
// An effect's fader: its gain, and 15 ms to nothing at the end if it is cut short (dur).
export const fxCurve = (e) => (e.dur > 0 ? [[0, e.gain ?? 1], [Math.max(0, e.dur - 0.015), e.gain ?? 1], [e.dur, 0]] : [[0, e.gain ?? 1]])

// The faders that follow the schedule's automation (S.auto), in the units the audio thread
// moves them in: a gain as a gain, the filter in Hz, the band's cut in dB.
export const autoSpec = (S) => {
  const A = S.auto ?? {}
  return {
    music: { list: A.music ?? [], rest: A.music?.[0]?.[1] ?? 0, to: (d) => MUSIC_LEVEL * db(d) },
    mid: { list: A.mid ?? [], rest: 0, to: (d) => d },
    fx: { list: A.fx ?? [], rest: 0, to: (d) => db(d) },
    call: { list: A.call ?? [], rest: 0, to: (d) => CALL_GAIN * db(d) },
    lp: { list: A.lp ?? [], rest: CALL.lp, to: (hz) => hz },
  }
}
// Where such a fader is at show time t: each step is an exponential approach from wherever the
// one before had got to (which is what setTargetAtTime does, so this is the mixer's own curve).
export const autoAt = (spec, t) => {
  let v = spec.to(spec.rest)
  for (const [at, to, tau] of spec.list) {
    if (t < at) break
    const target = spec.to(to)
    v = target + (v - target) * Math.exp(-(t - at) / Math.max(tau, 1e-4))
  }
  return v
}

export class AudioEngine {
  constructor() {
    this.ctx = null
    this.S = null
    this.buffers = new Map() // clip id → AudioBuffer
    this.bytes = new Map() // clip id → Promise of the file's bytes, fetched before any click
    this.loading = new Map() // clip id → Promise of the decoded AudioBuffer
    this.live = new Set() // every source that is sounding or due, but the music's
    this.music = new Set() // the stems' sources: at most two overlap
    this.muted = readMuted()
    this.dead = false
    this.generation = 0
    this.anchor = null // { a: audio clock, t: show time } at one moment: the two run together from there
    this.next = 0 // the first event of the schedule not yet handed to the audio thread
    this.pass = 0 // the first stem (of S.music) not yet handed over
    this.waited = 0
    this.lookahead = LOOK
  }

  // Fetch every clip the manifest names, as raw bytes. Nothing is decoded here: decoding needs a
  // live AudioContext, and a browser only gives one after a click. init() decodes them all the
  // moment the context exists. A clip with no file is skipped.
  preload(S) {
    this.S = S
    for (const [id, clip] of Object.entries(S.clips)) {
      if (!clip || !clip.file || this.bytes.has(id)) continue
      const url = clip.file.startsWith('audio/') ? clip.file : 'audio/' + clip.file
      this.bytes.set(
        id,
        fetch(url)
          .then((r) => (r.ok ? r.arrayBuffer() : Promise.reject(new Error(String(r.status)))))
          .catch(() => null), // a missing file is silence
      )
    }
    if (this.ctx) this.decodeAll()
    return Promise.all(this.bytes.values())
  }

  // Decode with the live context. slice(0) hands the decoder its own copy, because decoding
  // detaches the buffer it is given and a replay must be able to decode again.
  decodeAll() {
    const ctx = this.ctx
    for (const [id, bytes] of this.bytes) {
      if (this.loading.has(id)) continue
      this.loading.set(
        id,
        bytes
          .then((buf) => (buf ? this.decode(ctx, buf.slice(0)) : null))
          .then((audio) => {
            if (audio) this.buffers.set(id, audio)
            return audio
          })
          .catch(() => null), // a broken file is silence
      )
    }
  }

  // decodeAudioData, in both its shapes: a promise, or callbacks in browsers that predate it.
  decode(ctx, bytes) {
    return new Promise((resolve, reject) => {
      const p = ctx.decodeAudioData(bytes, resolve, reject)
      if (p && typeof p.then === 'function') p.then(resolve, reject)
    })
  }

  // Must run inside the visitor's click or tap, and all of it does, synchronously: the context
  // is made, resumed and given one sample of nothing to play before this returns, which is what
  // Safari on a phone wants before it will let a page be heard.
  init() {
    this.want = true
    if (this.ctx) {
      this.wake()
      return
    }
    const Ctx = window.AudioContext || window.webkitAudioContext
    if (!Ctx) return
    try {
      this.ctx = new Ctx({ latencyHint: 'interactive' })
    } catch {
      try {
        this.ctx = new Ctx()
      } catch {
        this.ctx = null
        return
      }
    }
    const ctx = this.ctx
    // (He pressed Start call: this is playback, and a phone's ringer switch does not silence it.)
    try {
      if (navigator.audioSession) navigator.audioSession.type = 'playback'
    } catch {}
    this.wake()
    try {
      const prime = ctx.createBufferSource()
      prime.buffer = ctx.createBuffer(1, 1, ctx.sampleRate)
      prime.connect(ctx.destination)
      prime.start(0)
    } catch {}
    // A phone call, Siri, another app: the context is interrupted and does not come back by
    // itself. It is asked to whenever it stops while the show wants it running.
    ctx.onstatechange = () => { if (this.want && ctx.state !== 'running' && ctx.state !== 'closed') this.wake() }
    const gain = (v) => {
      const g = ctx.createGain()
      g.gain.value = v
      return g
    }
    const squash = (c) => {
      const n = ctx.createDynamicsCompressor()
      n.threshold.value = c.threshold
      n.knee.value = c.knee
      n.ratio.value = c.ratio
      n.attack.value = c.attack
      n.release.value = c.release
      return n
    }
    // The way out. out is what reaches the speakers (perf/bench.mjs --live listens here, and
    // on trim for what the ceiling was given).
    this.limiter = squash(LIMIT)
    this.trim = gain(TRIM_GAIN)
    this.ceiling = ctx.createWaveShaper()
    const curve = new Float32Array(4097)
    for (let i = 0; i < curve.length; i++) curve[i] = ceiling(((i - 2048) / 2048) * 1.5)
    // (The shaper reads its curve across -1..1; 1.5 of headroom either side of that is folded
    // in by the two gains around it.)
    this.ceilIn = gain(1 / 1.5)
    this.ceiling.curve = curve
    this.out = gain(1)
    this.limiter.connect(this.trim).connect(this.ceilIn).connect(this.ceiling).connect(this.out).connect(ctx.destination)
    // master: everything. It follows the mute button.
    this.master = gain(this.muted ? 0 : LEVEL)
    // gate: shut for the dead air, on the audio clock, over the 2 ms that end at Stop.
    this.gate = gain(1)
    this.master.connect(this.gate).connect(this.limiter)
    // The call.
    this.callIn = gain(1)
    this.hp = ctx.createBiquadFilter()
    this.hp.type = 'highpass'
    this.hp.frequency.value = CALL.hp
    this.hp.Q.value = FLAT_Q
    this.lp = ctx.createBiquadFilter()
    this.lp.type = 'lowpass'
    this.lp.frequency.value = CALL.lp
    this.lp.Q.value = FLAT_Q
    this.comp = squash(CALL)
    this.callBus = gain(CALL_GAIN)
    this.callIn.connect(this.hp).connect(this.lp).connect(this.comp).connect(this.callBus).connect(this.master)
    this.callVoice = gain(VOICE_LEVEL)
    this.callVoice.connect(this.callIn)
    this.fx = gain(1) // the fader: S.auto.fx
    this.callFx = gain(FX_LEVEL)
    this.callFx.connect(this.fx).connect(this.callIn)
    // His own voice and his own Mac: straight through.
    this.meBus = gain(ME_LEVEL)
    this.meBus.connect(this.master)
    this.uiBus = gain(UI_LEVEL)
    this.uiBus.connect(this.master)
    // The music: the band a voice lives in, the fader, and last the transport (musicGate:
    // shut dead at Pause; last, so that not even the filter's ringing gets past it).
    this.mid = ctx.createBiquadFilter()
    this.mid.type = 'peaking'
    this.mid.frequency.value = MID.hz
    this.mid.Q.value = MID.q
    this.mid.gain.value = 0
    this.musicBus = gain(MUSIC_LEVEL)
    this.musicGate = gain(1)
    this.mid.connect(this.musicBus).connect(this.musicGate).connect(this.master)
    // A compressor node spends the first 80 ms it ever runs coming up from nothing (measured in
    // Chrome: 12 dB down 10 ms in), and a node with nothing playing into it may not be run at
    // all, in which case it would be the first voice that paid. So both are run from the moment
    // there is a context, on half a second of a level far too low to be anything (-140 dBFS),
    // and that 80 ms is over before anyone speaks.
    const quiet = ctx.createBuffer(1, Math.ceil(ctx.sampleRate * 0.5), ctx.sampleRate)
    quiet.getChannelData(0).fill(1e-7)
    const warm = ctx.createBufferSource()
    warm.buffer = quiet
    warm.connect(this.comp)
    warm.connect(this.limiter)
    warm.onended = () => warm.disconnect()
    warm.start()
    this.decodeAll()
  }

  get clipCount() {
    return this.buffers.size
  }

  setMuted(m) {
    this.muted = m
    try {
      localStorage.setItem(MUTED_KEY, m ? '1' : '0')
    } catch {}
    this.applyMaster()
  }

  applyMaster() {
    if (!this.ctx) return
    const now = this.ctx.currentTime
    const g = this.master.gain
    g.cancelScheduledValues(now)
    if (this.muted) g.setValueAtTime(0, now)
    else g.setTargetAtTime(LEVEL, now, 0.012)
  }

  pause() {
    this.want = false
    this.ctx?.suspend().catch(() => {})
  }
  resume() {
    this.want = true
    this.wake()
  }
  // resume(), never throwing and never rejecting out loud (an interrupted context refuses).
  wake() {
    try {
      this.ctx?.resume()?.catch?.(() => {})
    } catch {}
  }

  // The schedule's clock, as it is being heard now (seconds), or null if no context is
  // running. getOutputTimestamp pairs a moment of audio with the moment it left the speakers,
  // so this is smooth from frame to frame and already allows for the output's latency; the
  // limiter's 6 ms is taken off as well, so a sound handed over for when(t) is heard at t.
  heard() {
    const ctx = this.ctx
    if (!ctx || ctx.state !== 'running') return null
    const ts = ctx.getOutputTimestamp?.()
    if (ts && ts.performanceTime > 0 && ts.contextTime > 0) {
      const at = ts.contextTime + (performance.now() - ts.performanceTime) / 1000
      // (Never ahead of what has been rendered, never unreasonably far behind it.)
      if (at <= ctx.currentTime + 0.01 && at > ctx.currentTime - 0.5) return at - LOOK
    }
    return ctx.currentTime - Math.min(0.25, ctx.outputLatency || ctx.baseLatency || 0) - LOOK
  }

  // Show time, one frame on: T follows the audio clock from wherever the two were last pinned
  // together. dt (the frame clock) is only used when there is no audio clock to follow.
  advance(T, dt) {
    const at = this.heard()
    if (at === null) {
      // A context that is on its way up (the first click, a resume) is waited for, briefly, so
      // the picture does not run on ahead of it; one that never comes up is done without.
      if (this.ctx && this.ctx.state !== 'closed' && (this.waited += dt) < 0.4) return T
      this.anchor = null
      this.unpinned = true
      return T + dt
    }
    this.waited = 0
    // (The frame clock ran the show for a while, or the context has only just come up: pin
    // the two clocks together here and start what should be sounding.)
    if (!this.anchor) this.seek(T)
    return this.anchor ? Math.max(T, this.anchor.t + (at - this.anchor.a)) : T + dt
  }

  // The context time at which the sample for show time t goes into the limiter.
  when(t) {
    return this.anchor ? this.anchor.a + (t - this.anchor.t) : this.ctx.currentTime
  }
  // The show time of the sample going into the limiter now.
  showNow() {
    return this.anchor ? this.anchor.t + (this.ctx.currentTime - this.anchor.a) : 0
  }

  // Called once per frame with show time T. All it does is keep the audio thread supplied:
  // every sound due in the next AHEAD seconds is handed over, each at its own sample. The
  // faders, Pause and Stop were handed over when the clocks were pinned (pin()).
  frame(T, playing) {
    if (!this.ctx || !this.S) return
    this.dead = playing && inSilence(this.S, T)
    if (playing && this.anchor && this.ctx.state === 'running') this.schedule(T)
  }

  schedule(T) {
    const S = this.S, ev = S.events, dead = S.cue.dead
    const upTo = Math.max(T, this.showNow()) + AHEAD
    while (this.next < ev.length && ev[this.next].t <= upTo) {
      const e = ev[this.next++]
      // Nothing is due in the dead air.
      if (e.t >= dead[0] && e.t < dead[1]) continue
      this.start(e)
    }
    this.scheduleMusic(upTo)
  }

  // Stop everything that is sounding or due (a seek, Restart, Replay): nothing that was handed
  // to the audio thread is left to come out later. What is sounding is taken to nothing over a
  // hundredth of a second first, so that a voice or the music cut in the middle of a wave is
  // not a click.
  stopAll() {
    this.generation++
    const now = this.ctx?.currentTime ?? 0
    for (const set of [this.live, this.music]) {
      for (const n of set) {
        const off = () => {
          for (const x of [n, n.fade]) {
            try {
              x?.disconnect()
            } catch {}
          }
        }
        n.onended = off
        try {
          const g = n.fade.gain
          if (g.cancelAndHoldAtTime) g.cancelAndHoldAtTime(now)
          else g.cancelScheduledValues(now)
          g.setTargetAtTime(0, now, 0.003)
          n.stop(now + 0.02)
        } catch {
          try {
            n.stop()
          } catch {}
          off()
        }
      }
      set.clear()
    }
  }

  // fade: the source's own fader (the node it plays into).
  keep(node, fade, set = this.live) {
    node.fade = fade
    set.add(node)
    node.onended = () => {
      set.delete(node)
      for (const x of [node, fade]) {
        try {
          x.disconnect()
        } catch {}
      }
    }
  }

  // Put show time T on the audio clock, and rebuild the mixer for it: whatever was scheduled is
  // cancelled, every fader is put where the schedule has it at T and given the rest of its
  // moves, the music is started where the loop has got to, and every voice and effect that is
  // in the middle of sounding is started from there. (A seek, a scrub, a replay, the first
  // click, a context that comes up late.)
  seek(T) {
    const ev = this.S.events
    this.next = ev.findIndex((e) => e.t > T)
    if (this.next < 0) this.next = ev.length
    if (!this.ctx) return
    this.stopAll()
    const at = this.heard()
    this.anchor = at === null ? null : { a: at, t: T }
    this.unpinned = at === null
    // (No clock yet: advance() comes back here when there is one.)
    if (!this.anchor) return
    this.pin()
    // What is already sounding at T: start() joins each where it has got to.
    const Tn = this.showNow()
    for (let i = 0; i < this.next; i++) if (ev[i].t + this.lasts(ev[i]) > Tn + 0.05) this.start(ev[i])
  }

  // How long an event sounds for, as far as the manifest knows.
  lasts(e) {
    if (e.name === 'voice') return e.line.dur
    return e.dur > 0 ? e.dur : (this.S.clips[e.name]?.dur ?? 0) / (e.rate || 1)
  }

  // Every fader and gate, from now to the end of the show, on the audio clock.
  pin() {
    const S = this.S, ctx = this.ctx, cue = S.cue
    const now = ctx.currentTime, Tn = this.showNow()
    // A fader that follows S.auto. early: it sits before the call's compressor, so its moves
    // are made 6 ms early, with the sound they are for.
    const fader = (param, spec, early = 0) => {
      param.cancelScheduledValues(0)
      param.setValueAtTime(autoAt(spec, Tn + early), now)
      let last = null
      for (const step of spec.list) {
        if (step[0] <= Tn + early) last = step
        else param.setTargetAtTime(spec.to(step[1]), this.when(step[0]) - early, Math.max(step[2], 1e-4))
      }
      // (In the middle of a move: carry on with it.)
      if (last) param.setTargetAtTime(spec.to(last[1]), now, Math.max(last[2], 1e-4))
    }
    const A = autoSpec(S)
    fader(this.musicBus.gain, A.music)
    fader(this.mid.gain, A.mid)
    fader(this.callBus.gain, A.call)
    fader(this.fx.gain, A.fx, LOOK)
    fader(this.lp.frequency, A.lp, LOOK)
    // A gate: open until GATE_S before 'shut', closed from 'shut' (a straight line between),
    // open again at 'open'.
    const gate = (param, shut, open) => {
      param.cancelScheduledValues(0)
      if (now >= open) return void param.setValueAtTime(1, now)
      if (now >= shut) param.setValueAtTime(0, now)
      else {
        const from = Math.max(now, shut - GATE_S)
        param.setValueAtTime(now > shut - GATE_S ? (shut - now) / GATE_S : 1, now)
        param.setValueAtTime(now > shut - GATE_S ? (shut - now) / GATE_S : 1, from)
        param.linearRampToValueAtTime(0, shut)
      }
      if (open < Infinity) param.setValueAtTime(1, open)
    }
    // Pause: the music stops dead, and does not come back.
    gate(this.musicGate.gain, this.when(cue.pause), Infinity)
    // Stop: everything does, for the dead air. The limiter hands that 2 ms ramp on 6 ms late,
    // which is when Stop is heard; from that sample out is set to nothing, so that what
    // reaches the speakers until the dead air is over is exactly zero whatever the limiter
    // does. (It opens again 6 ms before anything can come through it.)
    gate(this.gate.gain, this.when(cue.dead[0]), this.when(cue.dead[1]))
    const seal = this.when(cue.dead[0]) + LOOK, unseal = this.when(cue.dead[1])
    const o = this.out.gain
    o.cancelScheduledValues(0)
    o.setValueAtTime(now >= seal && now < unseal ? 0 : 1, now)
    if (now < seal) o.setValueAtTime(0, seal)
    if (now < unseal) o.setValueAtTime(1, unseal)
    // The music: the stem that is playing at Tn, and the one before it if its tail still is.
    const M = S.music
    this.pass = 0
    while (this.pass < M.length && M[this.pass].t + M[this.pass].lasts <= Tn) this.pass++
  }

  // Hand a param the rest of a curve (points over a sound's own time u), from u0 at context
  // time at0. A sound that is joined part-way comes up from nothing over 8 ms.
  ramp(param, pts, u0, at0) {
    let from = u0
    if (u0 > 0.02) {
      from = u0 + JOIN_S
      param.setValueAtTime(0, at0)
      param.linearRampToValueAtTime(curveAt(pts, from), at0 + JOIN_S)
    } else param.setValueAtTime(curveAt(pts, u0), at0)
    for (const [u, v] of pts) if (u > from) param.linearRampToValueAtTime(v, at0 + (u - u0))
  }

  // Start one event of the schedule at its sample. If that sample has already gone by (a seek
  // into the middle of it, a file that was still decoding, the output's latency on the first
  // frame), it is joined where it has got to.
  start(e) {
    const ctx = this.ctx, S = this.S, gen = this.generation
    const voice = e.name === 'voice'
    const id = voice ? e.line.clip : e.name
    // Into the call: 6 ms early, because the call's compressor hands it on 6 ms late.
    const due = this.when(e.t) - (e.bus === 'call' ? LOOK : 0)
    // Everything is cut off by Stop.
    const cut = e.t < S.cue.dead[0] ? this.when(S.cue.dead[0]) + 0.001 : Infinity
    const play = (buf) => {
      if (!buf || gen !== this.generation) return
      const now = ctx.currentTime
      const at = Math.max(due, now)
      if (at >= cut) return
      const rate = voice ? 1 : e.rate || 1
      // u: seconds of the sound's own time already gone by.
      const u = at - due
      // dur: an effect that is held for that long, round and round if the file is shorter
      // (typing lasts as long as the sentence does), and faded out at the end of it.
      const whole = buf.duration / rate
      const lasts = voice ? Math.min(e.line.dur, buf.duration) : e.dur > 0 ? e.dur : whole
      if (u >= lasts - 0.01) return
      // (The tail end of a click is not worth joining.)
      if (!voice && u > 0.03 && lasts - u < 0.2) return
      const src = ctx.createBufferSource()
      src.buffer = buf
      src.playbackRate.value = rate
      if (!voice && e.dur > whole + 0.02) src.loop = true
      const g = ctx.createGain()
      this.ramp(g.gain, voice ? lineCurve(e.line) : fxCurve(e), u, at)
      let out = src.connect(g)
      if (voice) {
        // A coworker is panned to their tile. The one host is a stereo file already; he is
        // dead centre.
        const pan = e.bus === 'call' && e.line.who !== 'host' ? CAST[e.line.who]?.pan ?? 0 : 0
        if (pan && ctx.createStereoPanner) {
          const p = ctx.createStereoPanner()
          p.pan.value = Math.max(-1, Math.min(1, pan))
          out = out.connect(p)
        }
        out.connect(e.bus === 'me' ? this.meBus : this.callVoice)
      } else out.connect(e.bus === 'call' ? this.callFx : this.uiBus)
      src.start(at, src.loop ? (u * rate) % buf.duration : u * rate)
      src.stop(Math.min(cut, at + (lasts - u) + 0.005))
      this.keep(src, g)
    }
    const buf = this.buffers.get(id)
    if (buf) play(buf)
    else if (this.loading.has(id)) void this.loading.get(id).then(play)
  }

  // The music: every stem that starts before show time upTo, each at its sample. The first
  // begins on the sample Play falls on; each of the others begins on a kick of the one before
  // it, at the top of its own file, and the one before fades out under it (its pts, from
  // show.js). All of them are stopped at Pause.
  scheduleMusic(upTo) {
    const S = this.S, cue = S.cue, M = S.music
    for (; this.pass < M.length && M[this.pass].t < Math.min(upTo, cue.pause); this.pass++) this.startStem(M[this.pass])
  }

  startStem(g) {
    const ctx = this.ctx, cue = this.S.cue, gen = this.generation
    const due = this.when(g.t)
    const stop = this.when(cue.pause)
    const play = (buf) => {
      if (!buf || gen !== this.generation) return
      const at = Math.max(due, ctx.currentTime)
      const u = at - due
      const lasts = Math.min(g.lasts, buf.duration)
      if (at >= stop || u >= lasts - 0.01) return
      const src = ctx.createBufferSource()
      src.buffer = buf
      const gn = ctx.createGain()
      this.ramp(gn.gain, g.pts, u, at)
      src.connect(gn).connect(this.mid)
      src.start(at, u)
      // (Pause: the gate is at nothing from that sample on; the source goes a moment later.)
      src.stop(Math.min(stop + 0.001, at + (lasts - u) + 0.005))
      this.keep(src, gn, this.music)
    }
    const buf = this.buffers.get(g.clip)
    if (buf) play(buf)
    else void this.loading.get(g.clip)?.then(play)
  }
}
