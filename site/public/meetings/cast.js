// The performance. perform() writes one record per face into the frame for show time t: where
// its tile is, how its head sits, its eyes, lids, brows and mouth, what it wears, the
// headphones on Me's head, and (for the four on the call, once it starts) how far the change
// has gone in them. Everything is a function of t (and, before and after the show, of the
// page's own clock and the pointer), so any moment can be drawn cold. The only state is
// smoothing of the gaze and where things were last frame (for the shutter).
import { CAST, WHO, ME, lineAt, speakerAt, level, beatAt, crit } from './show.js'

// Offsets into the frame, in floats. Mirrors struct U in wgsl/common.wgsl.
export const G = { VIEW: 0, MISC: 4, POST: 8, PULSE: 12, BLUR: 16, BLUR2: 20, GRADE: 24, SEAM: 28, META: 32, GRID: 36, HOST2: 40, WINS: 44, LIGHTS: 60, RINGS: 92, BURSTS: 156, HEADS: 204, TENDRILS: 684, FLOATS: 1196 }
export const STRIDE = 96
export const MAX = 5
export const TENDRIL = 16
export const TENDRILS = 32
export const SHUTTER = 0.75
// Where each of the four sits in the one mass, from its middle, in head radii.
export const NEST = [0.80, 0.92]
export const F = {
  CX: 0, CY: 1, R: 2, ROLL: 3, TX: 4, TY: 5, THW: 6, THH: 7, TA: 8, TRAD: 9, RING: 10, DIM: 11, LEANX: 12, LEANY: 13, FACEX: 14, FACEY: 15,
  GAZEX: 16, GAZEY: 17, LIDL: 18, LIDR: 19, LOWL: 20, LOWR: 21, PUPIL: 22, EYE: 23, UPL: 24, UPR: 25, ANGL: 26, ANGR: 27, ARCHL: 28, ARCHR: 29, WEIGHT: 30, SPACING: 31,
  OPEN: 32, WIDE: 33, ROUND: 34, SMILE: 35, SMIRK: 36, TONGUE: 37, JAW: 38, RIMR: 40, RIMG: 41, RIMB: 42, RIMS: 43, GLASSES: 44, GLOSS: 45, EMERALD: 46, ALPHA: 47,
  RX: 48, RY: 49, STRETCH: 50, RIMANG: 51, VX: 52, VY: 53, VGROW: 54, TVX: 56, TVY: 57, TVW: 58, TVH: 59, SEED: 60, TIME: 61, KEY: 62, WARM: 63,
  FLESH: 64, BREATH: 65, BEAT: 66, LIQUID: 67, THIRD: 69, SURGE: 71, ENTRYX: 72, ENTRYY: 73, FUSE: 74, SEAMSIDE: 75, SEAMY: 76, HAT: 77, TAPER: 78, INMASS: 79,
  META: 80, CHIMERA: 81, PANIC: 82, DRIP: 83, XEYES: 84, BROWS: 85, REAL: 86, CROOK: 87, PODS: 88, PODA: 89, SUNK: 90, UNEVEN: 92, CRACK: 93, HUE: 94,
}

const clamp = (v, a = 0, b = 1) => (v < a ? a : v > b ? b : v)
const lerp = (a, b, k) => a + (b - a) * k
const smooth = (k) => { k = clamp(k); return k * k * (3 - 2 * k) }
const hash = (n) => { const x = Math.sin(n * 127.1 + 311.7) * 43758.5453; return x - Math.floor(x) }
const bell = (x) => { const k = clamp(1 - Math.abs(x)); return k * k * (3 - 2 * k) }
// A blink: shut in 60 ms, open again in 130. u is seconds since it began, w how slow this one is.
const blinkAt = (u, w = 1) => (u <= 0 ? 0 : u < 0.06 * w ? smooth(u / (0.06 * w)) : 1 - smooth((u - 0.06 * w) / (0.13 * w)))
// Slow noise with holds: a value that sits, then moves to the next, about every 'every' seconds.
const drift = (t, every, seed) => {
  const f = t / every + seed, i = Math.floor(f), k = smooth((f - i - 0.55) / 0.45)
  return lerp(hash(i * 1.37 + seed), hash((i + 1) * 1.37 + seed), k) * 2 - 1
}
// Overshoots a little and settles: something pushed home.
const seat = (k) => { k = clamp(k); const c = 1.9; return 1 + (c + 1) * Math.pow(k - 1, 3) + c * Math.pow(k - 1, 2) }
const CRIMSON_RIM = [1.0, 0.24, 0.12]

// The shape a word puts the mouth in, from its first vowel: round (O), wide (EE), or open (AH).
// Lips meet on M, B and P; the lower one tucks under the teeth on F and V.
function visemeOf(word) {
  const w = word.toLowerCase().replace(/[^a-z0-9]/g, '')
  const v = (w.match(/[aeiouy]+/) ?? ['a'])[0]
  let o = 0.9, wide = 0.55, round = 0
  if (/^(o|u)/.test(v) || /^w/.test(w)) { round = 0.85; wide = 0.1; o = 0.8 }
  else if (/^(e|i|y)/.test(v)) { wide = 1; o = 0.62 }
  else { wide = 0.75; o = 1 }
  return { o, wide, round, shut: /^[mbp]/.test(w), bite: /^[fv]/.test(w), tip: /^(l|th)/.test(w) }
}

// Emotion: a few poses per person, each eased into when its moment comes.
const POSE = ['bl', 'br', 'al', 'ar', 'arch', 'lu', 'll', 'pd', 'sm', 'sk', 'lean', 'rest', 'th', 'ey']
// The brows get there first, the lids two frames later, the mouth last.
const LAG = { bl: 0, br: 0, al: 0, ar: 0, arch: 0, lu: 0.035, ll: 0.035, pd: 0.035, ey: 0.035, sm: 0.075, sk: 0.075, rest: 0.075, lean: 0.05, th: 0 }
function poses(base, list) {
  const keys = [{ t: -1e9, ease: 1, ...base }]
  list.sort((a, b) => a[0] - b[0])
  for (const [t, v, ease = 0.3] of list) keys.push({ ...keys[keys.length - 1], ...v, t, ease })
  return keys
}
function poseAt(keys, t, out) {
  for (const p of POSE) {
    const tp = t - LAG[p]
    let i = keys.length - 1
    while (i > 0 && keys[i].t > tp) i--
    const b = keys[i]
    if (i === 0) { out[p] = b[p]; continue }
    // Ease from wherever the pose before this one had got to.
    const a = keys[i - 1]
    const k = smooth((tp - b.t) / b.ease)
    const ka = i > 1 ? smooth((b.t - a.t) / a.ease) : 1
    const z = i > 1 ? keys[i - 2] : a
    out[p] = lerp(lerp(z[p], a[p], ka), b[p], k)
  }
  return out
}

export class Stage {
  constructor(S) {
    this.S = S
    this.geo = null
    this.rect = new Float32Array(MAX * 4) // each tile: cx, cy, half w, half h
    this.headAt = new Float32Array(MAX * 3) // each head: cx, cy, R
    this.was = new Float32Array(MAX * 8)
    this.gaze = new Float32Array(MAX * 2)
    this.face = new Float32Array(MAX * 2)
    this.call = { x: 0, y: 0, w: 1, h: 1, a: 0, s: 1 } // the call's window, this frame (s: its scale about its middle, as it opens)
    this.oc = { x: 0, y: 0, w: 1, h: 1, a: 0 } // OpenCode's
    this.host = { x: 0, y: 0, R: 1, k: 0 } // the one mass: its middle, a head's radius, how far it has gone
    this.dock = 0
    this.step = 0
    this.lastT = NaN
    this.pose = {}
    this.flesh = 0
    this.meta = 0
    this.panic = 0
    this.hoverSpr = { x: 0, v: 0 }
    for (const l of S.lines) l.vis = l.words.map((w) => visemeOf(w[0]))
    // When each turn starts: a listener's eyes move then, and most blinks happen on a move.
    this.turns = S.lines.filter((l) => !l.under).sort((a, b) => a.s0 - b.s0)
    const L = S.byId, cue = S.cue, m = cue.merge
    const V = cue.tabs
    // What each of them is like once they are one thing. No two alike.
    const calm = {
      lead: { bl: 0.34, br: 0.3, al: 0.02, ar: 0.02, arch: 0.3, lu: 0.05, ll: 0.16, pd: 0.74, ey: 1.16, sm: 1, sk: 0, rest: 0.04, lean: 1.0, th: 1 },
      eng: { bl: 0.1, br: 0.26, al: 0.0, ar: 0.0, arch: 0.22, lu: 0.38, ll: 0.12, pd: 0.6, ey: 1.04, sm: 0.62, sk: 0.2, rest: 0, lean: 1.0, th: 1 },
      ops: { bl: 0.5, br: 0.44, al: -0.12, ar: -0.12, arch: 0.34, lu: 0.78, ll: 0.3, pd: 0.5, ey: 1.0, sm: 0.9, sk: 0, rest: 0, lean: 1.0, th: 0 },
      dev: { bl: 0.6, br: 0.5, al: -0.06, ar: -0.02, arch: 0.3, lu: 0.0, ll: 0.06, pd: 0.86, ey: 1.24, sm: 0.78, sk: 0, rest: 0.16, lean: 1.0, th: 1 },
    }
    const still = { bl: 1, br: 1, al: -0.3, ar: -0.3, arch: 0.36, lu: 0, ll: 0, pd: 0.1, ey: 1.3, sm: -0.2, rest: 0.22 }
    this.keys = {
      me: poses({ bl: -0.28, br: -0.16, al: 0.24, ar: 0.14, arch: 0.08, lu: 0.48, ll: 0.10, pd: 0.25, sm: 0, sk: 0.32, lean: 1, rest: 0, th: 0, ey: 1 }, [
        // A notification: a meeting. He joins it, and watches the others turn up.
        [cue.invite + 0.2, { bl: 0.4, br: 0.62, al: 0.08, ar: 0.0, lu: 0.3, sk: 0.2 }, 0.2],
        [cue.join + 0.2, { bl: -0.1, br: 0.0, al: 0.22, ar: 0.14, lu: 0.46, sk: 0.3 }, 0.4],
        // Hex has noticed the call. Good: Record. Then the prompt, one sentence, one switch.
        [cue.offer + 0.12, { bl: 0.3, br: 0.55, al: 0.1, ar: 0.0, lu: 0.34, sk: 0.2 }, 0.22],
        [cue.record + 0.12, { bl: 0.05, br: 0.15, al: 0.12, ar: 0.06, lu: 0.44, sm: 0.22, sk: 0.4 }, 0.3],
        [cue.hover - 0.1, { bl: -0.22, br: -0.12, al: 0.24, ar: 0.14, lu: 0.5, sm: 0, sk: 0.3 }, 0.4],
        [cue.auto - 0.1, { bl: 0.1, br: 0.42, sk: 0.5, sm: 0.1 }, 0.25],
        [cue.send + 0.3, { bl: -0.2, br: -0.12, sk: 0.3, sm: 0 }, 0.5],
        // Headphones, Play: he is somewhere nicer now.
        [cue.phones[0] - 0.1, { br: 0.5, bl: 0.2, sk: 0.2, lu: 0.6 }, 0.3],
        [cue.play + 0.25, { bl: 0.05, br: 0.05, al: 0.06, ar: 0.06, arch: 0.16, lu: 0.56, ll: 0.2, sm: 0.34, sk: 0.14, pd: 0.4 }, 0.7],
        // Slack: his rocket goes on the pile.
        [V[0].acts[0].t + 0.15, { bl: 0.3, br: 0.45, sm: 0.55 }, 0.35], [V[0].acts[0].t + 1.7, { bl: 0.05, br: 0.05, sm: 0.34 }, 0.7],
        // Spaceman: workspace 2. Nice.
        [V[1].acts[0].t + 0.15, { bl: 0.45, br: 0.5, lu: 0.4, sm: 0.62 }, 0.3], [V[1].acts[0].t + 1.9, { bl: 0.05, br: 0.05, lu: 0.56, sm: 0.34 }, 0.8],
        // The duck. "He is not technically doing anything wrong."
        [V[2].stops[2][0] + 0.5, { bl: 0.4, br: 0.55, lu: 0.5, sm: 0.62 }, 0.5], [V[2].stops[2][0] + 2.2, { bl: 0.05, br: 0.05, lu: 0.56, sm: 0.38 }, 0.5],
        // X. He reads it sideways, for as long as they scream.
        [V[3].t + 1.3, { bl: -0.15, br: 0.42, al: 0.2, ar: 0.05, lu: 0.6, sm: 0.05, sk: 0.55 }, 0.4],
        // "do not turn auto-approve on." Liked.
        [cue.like + 0.12, { bl: 0.45, br: 0.5, al: 0.04, ar: 0.04, lu: 0.4, sm: 0.75, sk: 0.1 }, 0.22],
        [cue.closeTab - 0.2, { sm: 0.3, bl: 0.0, br: 0.0, sk: 0.14 }, 0.4],
        [cue.pause + 0.25, { bl: 0.3, br: 0.42, al: 0.1, ar: 0.02, lu: 0.34, ll: 0.1, sm: 0.1, sk: 0.2, pd: 0.3 }, 0.3],
        [L.me_bye.t0 - 0.25, { bl: 0.5, br: 0.5, al: 0.0, ar: 0.0, arch: 0.2, lu: 0.3, sm: 0.6, sk: 0.1 }, 0.25],
        [L.me_bye.t1 + 0.3, { bl: 0.0, br: 0.1, lu: 0.46, sm: 0.3, sk: 0.2 }, 0.6],
        [cue.cut, { bl: -0.28, br: -0.16, al: 0.24, ar: 0.14, arch: 0.08, lu: 0.48, ll: 0.1, pd: 0.25, sm: 0, sk: 0.32 }, 0.04],
      ]),
      // Marcus: pleased with every sentence he says.
      lead: poses({ bl: 0.12, br: 0.12, al: 0.12, ar: 0.12, arch: 0.1, lu: 0.14, ll: 0.12, pd: 0.4, sm: 0.3, sk: 0, lean: 1, rest: 0, th: 0, ey: 1 }, [
        [L.lead_wish.s0 - 0.12, { bl: 0.5, br: 0.5, al: 0.02, ar: 0.02, arch: 0.2, lu: 0.06, sm: 0.5, lean: 1.03 }, 0.25],
        [L.lead_wish.s1 + 0.1, { bl: 0.3, br: 0.3, sm: 0.6, lean: 1 }, 0.4],
        [L.lead_ci.s0 - 0.1, { bl: 0.1, br: 0.3, al: 0.3, ar: 0.3, sm: -0.1, lu: 0.2 }, 0.25],
        // The number lands: delight.
        [cue.fix1 + 0.12, { bl: 0.8, br: 0.8, al: -0.1, ar: -0.1, arch: 0.26, lu: 0, pd: 0.55, sm: 0.3, rest: 0.12 }, 0.16],
        [L.lead_thats.s0 - 0.1, { bl: 0.6, br: 0.55, al: 0.04, ar: 0.04, arch: 0.22, lu: 0.04, sm: 0.9, rest: 0, lean: 1.04 }, 0.25],
        [L.lead_meet.s1 + 0.2, { bl: 0.4, br: 0.4, sm: 0.7, lean: 1 }, 0.4],
        // "...Did it delete Jira?"
        [L.eng_404.s0 + 0.5, { bl: 0.85, br: 0.3, al: -0.2, ar: 0.1, arch: 0.3, lu: 0.02, pd: 0.3, sm: -0.3, rest: 0.1, ey: 1.08 }, 0.3],
        [L.lead_leave.s0 - 0.15, { bl: 0.3, br: 0.3, al: 0.0, ar: 0.0, arch: 0.14, lu: 0.2, sm: 0.25, rest: 0, ey: 1 }, 0.35],
        [L.lead_walls.s0 - 0.12, { bl: 0.5, br: 0.5, lu: 0.08, sm: 0.6, lean: 1.03 }, 0.25],
        // He has seen where his tile ends, and that it does not. He decides it is fine.
        [L.eng_window.s0 + 0.4, { bl: 0.2, br: 0.75, al: -0.2, ar: -0.1, sm: -0.1, lean: 1 }, 0.4],
        [L.lead_voice.s0 - 0.12, { bl: 0.7, br: 0.7, al: 0.05, ar: 0.05, arch: 0.26, lu: 0.02, sm: 0.85, lean: 1.05, ey: 1.04 }, 0.25],
        // The first of them lands.
        [m + 0.5, { bl: 1, br: 1, al: -0.4, ar: -0.4, arch: 0.4, lu: 0, ll: 0, pd: 0.05, ey: 1.34, sm: -0.75, sk: 0, rest: 0.95, lean: 0.97 }, 0.16],
        [m + 1.7, { th: 1 }, 0.4],
        [cue.calm, still, 0.06],
        [cue.blink + 0.12, calm.lead, 1.3],
        [cue.cut, { bl: 0.15, br: 0.15, al: 0.1, ar: 0.1, arch: 0.12, lu: 0.14, ll: 0.12, pd: 0.4, sm: 0.2, rest: 0, lean: 1, th: 0, ey: 1 }, 0.04],
      ]),
      // Maya: deadpan, head tipped, one lid lower. She notices things first.
      eng: poses({ bl: -0.05, br: 0.08, al: 0.2, ar: 0.14, arch: 0.26, lu: 0.26, ll: 0.12, pd: 0.4, sm: -0.05, sk: 0.12, lean: 1, rest: 0, th: 0, ey: 1 }, [
        [L.eng_love.s0 - 0.1, { bl: 0.25, br: 0.3, sm: 0.35 }, 0.2], [L.eng_love.s1 + 0.3, { bl: -0.05, br: 0.08, sm: -0.05 }, 0.5],
        [L.lead_ci.s1 - 0.3, { bl: -0.1, br: 0.4, lu: 0.34, sk: 0.3 }, 0.2],
        [cue.fix1 + 0.1, { bl: 1, br: 1, al: -0.22, ar: -0.22, arch: 0.36, lu: 0, ll: 0, pd: 0.7, rest: 0.25, ey: 1.08, sm: 0.4, sk: 0 }, 0.14],
        [L.eng_whoa.s1 + 0.1, { bl: 0.3, br: 0.3, al: 0.1, ar: 0.1, arch: 0.2, lu: 0.1, ll: 0.12, sm: 0.4, pd: 0.5, ey: 1, rest: 0 }, 0.5],
        [L.eng_jira.s0 - 0.12, { bl: -0.1, br: 0.1, al: 0.24, ar: 0.2, lu: 0.4, sm: -0.2, sk: 0.2 }, 0.25],
        [cue.del + 0.25, { bl: 0.2, br: 0.75, al: 0.1, ar: -0.1, lu: 0.08, ll: 0.12, pd: 0.35, sm: -0.1, sk: 0 }, 0.3],
        // "...Don't fix it."
        [L.eng_leave.s0 - 0.3, { bl: 0.0, br: 0.0, al: 0.1, ar: 0.1, lu: 0.5, sm: 0.3, sk: 0.3 }, 0.3],
        [L.eng_leave.s1 + 0.4, { bl: -0.05, br: 0.08, lu: 0.26, sm: 0.1, sk: 0.12 }, 0.5],
        // The wall.
        [cue.wall + 0.15, { bl: 0.85, br: 0.85, al: -0.3, ar: -0.3, arch: 0.3, lu: 0, ll: 0.04, pd: 0.25, sm: -0.4, sk: 0, ey: 1.1 }, 0.3],
        [L.eng_nono.s0 - 0.1, { bl: 0.6, br: 0.6, al: 0.3, ar: 0.3, lu: 0, sm: -0.6, ey: 1.14, lean: 0.98 }, 0.2],
        [m + 0.35, { bl: 1, br: 1, al: -0.45, ar: -0.45, arch: 0.4, lu: 0, ll: 0, pd: 0.05, ey: 1.36, sm: -0.75, rest: 0.9, lean: 0.96 }, 0.16],
        [m + 1.4, { th: 1 }, 0.4],
        [cue.calm, { ...still, bl: 0.9, br: 1 }, 0.06],
        [cue.blink + 0.2, calm.eng, 1.5],
        [cue.cut, { bl: 0.1, br: 0.1, al: 0.2, ar: 0.2, arch: 0.1, lu: 0.12, ll: 0.12, pd: 0.4, sm: 0.15, rest: 0, lean: 1, th: 0, ey: 1 }, 0.04],
      ]),
      // Priya: sunny, heavy-lidded, one brow always up, and a beat behind everything.
      ops: poses({ bl: 0.5, br: 0.14, al: -0.08, ar: -0.02, arch: 0.28, lu: 0.3, ll: 0.16, pd: 0.45, sm: 0.4, sk: 0, lean: 1, rest: 0, th: 0, ey: 1 }, [
        [L.ops_hotel.s0 - 0.1, { bl: 0.7, br: 0.6, al: -0.3, ar: -0.3, sm: 0.1, lu: 0.2 }, 0.2], [L.ops_hotel.s1 + 0.3, { bl: 0.5, br: 0.14, al: -0.08, ar: -0.02, sm: 0.4, lu: 0.3 }, 0.5],
        [L.ops_love.s0 - 0.1, { bl: 0.7, br: 0.5, sm: 0.8, lu: 0.2 }, 0.2], [L.ops_love.s1 + 0.5, { bl: 0.5, br: 0.14, sm: 0.4, lu: 0.3 }, 0.6],
        // A second after everyone else.
        [cue.fix1 + 1.1, { bl: 0.8, br: 0.6, sm: 0.7, lu: 0.1 }, 0.3], [cue.fix1 + 3.2, { bl: 0.5, br: 0.14, sm: 0.4, lu: 0.3 }, 0.6],
        // "Slack!": pleased with herself. Then she agrees with everyone, and then she looks.
        [L.ops_pick.s0 - 0.1, { bl: 0.85, br: 0.7, sm: 0.85, lu: 0.1 }, 0.15], [L.ops_pick.s1 + 0.7, { bl: 0.5, br: 0.14, sm: 0.4, lu: 0.3 }, 0.5],
        [L.ops_slack.s0 - 0.1, { bl: 0.8, br: 0.6, sm: 0.9, lu: 0.15 }, 0.2],
        [(S.clips.ops_slack.words?.find((w) => /^wait/i.test(w[0]))?.[1] ?? 1.9) + L.ops_slack.t0 - 0.1, { bl: 0.95, br: 0.9, al: -0.34, ar: -0.3, arch: 0.36, lu: 0, pd: 0.3, sm: -0.4, ey: 1.12 }, 0.2],
        [L.ops_slack.s1 + 0.6, { bl: 0.6, br: 0.3, al: -0.2, ar: -0.1, lu: 0.2, sm: 0.0, ey: 1 }, 0.6],
        [L.ops_beam.s0 - 0.12, { bl: 0.8, br: 0.6, al: -0.1, ar: -0.05, arch: 0.3, lu: 0.1, sm: 0.9, pd: 0.55, lean: 1.03 }, 0.25],
        [L.ops_beam.s1 + 0.4, { bl: 0.5, br: 0.2, sm: 0.6, lean: 1 }, 0.5],
        // Still smiling for a second after the others have started screaming.
        [m + 1.15, { bl: 1, br: 1, al: -0.4, ar: -0.4, arch: 0.4, lu: 0, ll: 0, pd: 0.08, ey: 1.3, sm: -0.6, rest: 0.6, lean: 0.97 }, 0.16],
        [cue.late + 0.05, { ...still, sm: 0.1 }, 0.06],
        [cue.late + 0.5, calm.ops, 1.2],
        [cue.cut, { bl: 0.3, br: 0.15, al: 0.0, ar: 0.0, arch: 0.2, lu: 0.2, ll: 0.12, pd: 0.4, sm: 0.3, rest: 0, lean: 1, th: 0, ey: 1 }, 0.04],
      ]),
      // David: big eyes, a small worried mouth, and a question nobody lets him finish. The only one who is worried.
      dev: poses({ bl: 0.3, br: 0.3, al: -0.14, ar: -0.14, arch: 0.24, lu: 0.04, ll: 0.1, pd: 0.5, sm: 0.1, sk: 0, lean: 1, rest: 0, th: 0, ey: 1 }, [
        [L.dev_q1.s0 - 0.14, { bl: 0.7, br: 0.7, al: -0.1, ar: -0.1, sm: 0.3, lean: 1.04 }, 0.18],
        [L.dev_q1.s1 + 0.05, { bl: 0.1, br: 0.1, al: -0.3, ar: -0.3, sm: -0.2, lean: 0.99, lu: 0.2 }, 0.25],
        [L.dev_q2.s0 - 0.14, { bl: 0.6, br: 0.6, al: -0.2, ar: -0.2, sm: 0.2, lean: 1.03, lu: 0.04 }, 0.18],
        [L.dev_q2.s1 + 0.05, { bl: 0.05, br: 0.05, al: -0.32, ar: -0.32, sm: -0.25, lean: 0.99, lu: 0.24 }, 0.25],
        [L.dev_love.s0 - 0.1, { bl: 0.4, br: 0.4, al: -0.1, ar: -0.1, sm: 0.5, lu: 0.04, lean: 1 }, 0.2],
        [L.dev_love.s1 + 0.4, { bl: 0.3, br: 0.3, al: -0.14, ar: -0.14, sm: 0.1 }, 0.5],
        // "I think it… deleted all the tests."
        [cue.fix1 + 0.3, { bl: 0.75, br: 0.75, al: -0.3, ar: -0.3, lu: 0, sm: -0.2, pd: 0.4, ey: 1.06 }, 0.3],
        [L.dev_skip.s1 + 0.4, { bl: 0.2, br: 0.2, al: -0.28, ar: -0.28, sm: -0.2, lu: 0.12, ey: 1 }, 0.5],
        [cue.del + 0.4, { bl: 0.8, br: 0.8, al: -0.2, ar: -0.2, lu: 0, sm: -0.2, ey: 1.08 }, 0.3],
        [cue.wall + 0.6, { bl: 0.9, br: 0.9, al: -0.34, ar: -0.34, arch: 0.3, lu: 0, pd: 0.3, sm: -0.4, ey: 1.12 }, 0.3],
        // He works it out a moment before it gets to him.
        [m - 0.2, { bl: 0.6, br: 0.8, al: -0.36, ar: -0.3, arch: 0.3, lu: 0, pd: 0.3, sm: -0.5, ey: 1.12, lean: 0.99 }, 0.2],
        [m + 0.7, { bl: 1, br: 0.6, al: -0.4, ar: -0.2, arch: 0.4, lu: 0, ll: 0.3, pd: 0.05, ey: 1.3, sm: -0.75, rest: 0.75, lean: 0.96 }, 0.16],
        [m + 1.9, { th: 1 }, 0.4],
        [cue.calm, still, 0.06],
        [cue.blink + 0.3, calm.dev, 1.4],
        [cue.cut, { bl: 0.3, br: 0.3, al: -0.1, ar: -0.1, arch: 0.2, lu: 0.06, ll: 0.1, pd: 0.5, sm: 0.2, rest: 0, lean: 1, th: 0, ey: 1 }, 0.04],
      ]),
    }
  }

  // Where everything sits on a page of W × H. box is the hero's frame.
  layout(W, H, box, safe = 0) {
    const narrow = W < 820 || W / H < 0.95
    const edge = narrow ? 8 : clamp(W * 0.022, 14, 36)
    // (safe: what the phone keeps for its status bar; everything starts under it.)
    const top = (narrow ? 58 : clamp(H * 0.082, 62, 86)) + safe
    // (Under the windows: the three buttons, in their pill.)
    const bottom = narrow ? 66 : clamp(H * 0.066, 58, 66)
    const gap = narrow ? 8 : 14
    const aw = W - 2 * edge, ah = H - top - bottom
    let call, oc
    if (narrow) {
      const ch = Math.round(ah * 0.54)
      call = { x: edge, y: top, w: aw, h: ch }
      oc = { x: edge, y: top + ch + gap, w: aw, h: ah - ch - gap }
    } else {
      const ow = Math.round(clamp(aw * 0.37, 400, 620))
      call = { x: edge, y: top, w: aw - ow - gap, h: ah }
      oc = { x: W - edge - ow, y: top, w: ow, h: ah }
    }
    // Before OpenCode comes forward the call has the desktop to itself; OpenCode slides in.
    const solo = narrow ? { ...call, y: top + (ah - call.h) / 2 } : { ...call, x: (W - call.w) / 2 }
    const off = narrow ? { ...oc, y: oc.y + 70 } : { ...oc, x: oc.x + 90 }
    const bar = narrow ? 30 : 38, pad = narrow ? 8 : 12
    // Four equal tiles with a narrow gutter, and Me in a short wide one under them.
    const gut = narrow ? 6 : 8
    const room = call.h - bar - pad - pad - pad
    const th = (room - gut) / 2.6
    const meH = Math.round(th * 0.6)
    const tw = (call.w - 2 * pad - gut) / 2
    const mw = Math.min(meH * 2.2, call.w * 0.46)
    const R = Math.min(th * 0.36, tw * 0.3)
    const gx = [pad + tw / 2, call.w - pad - tw / 2], gy = [bar + pad + th / 2, bar + pad + th + gut + th / 2]
    // Each tile, relative to the call window's corner: cx, cy, half w, half h, head radius.
    const tiles = [0, 1, 2, 3].map((i) => ({ x: gx[i % 2], y: gy[i >> 1], hw: tw / 2, hh: th / 2, R }))
    tiles.push({ x: call.w / 2, y: bar + pad + 2 * th + gut + pad + meH / 2, hw: mw / 2, hh: meH / 2, R: Math.min(mw * 0.26, meH * 0.385) })
    const grid = { x: call.w / 2, y: bar + pad + th + gut / 2, hw: call.w / 2 - pad, hh: th + gut / 2, gut }
    // The browser: a window of its own over the lower 55% of OpenCode, hanging a little past
    // its left, right and bottom edges, so none of OpenCode shows down its side.
    const bh = Math.round(oc.h * 0.55)
    const browser = narrow ? { x: oc.x + 4, y: oc.y + oc.h - bh + 4, w: oc.w - 8, h: bh } : { x: oc.x - 20, y: oc.y + oc.h - bh + 10, w: oc.w + 30, h: bh }
    this.geo = {
      W, H, narrow, call, oc, solo, off, bar, pad, gut, tiles, grid, gap, top, browser, hero: { x: box.x, y: box.y, R: box.R },
      pill: { x: W / 2, y: safe + (narrow ? 30 : Math.round((top - safe) / 2) + 2) },
      // The calendar's notification: top right, as macOS puts it (its right edge, its top, its width).
      notif: { right: W - (narrow ? 8 : edge), y: safe + (narrow ? 10 : 18), w: Math.min(348, W - 16) },
      notes: { x: W / 2, y: Math.max(top + 30, H * 0.2), R: Math.min(72, H * 0.085) },
    }
    return this.geo
  }

  // Write every face for show time t. Returns how many records were written.
  perform(t, live, frame) {
    const S = this.S, geo = this.geo, cue = S.cue, L = S.byId
    const idle = live.phase === 'idle'
    const gone = !idle && t >= cue.cut // Stop: the call is over, and only Me's bean is left
    const clk = idle || gone ? live.pageTime : t
    // The call opens when he clicks Join: its window on a spring, and Me, who had the desktop
    // to himself, into his tile on a softer one.
    const open = idle ? 0 : S.openAt(t)
    const callIn = idle ? 0 : S.seatAt(t)
    const inCall = gone || idle ? 0 : smooth((t - cue.open) / 0.2)
    const step = Number.isFinite(this.lastT) ? Math.abs(t - this.lastT) : 0
    const fresh = !(step > 0 && step < 0.1) || idle !== this.wasIdle
    this.step = fresh ? 0 : step
    this.lastT = t
    this.wasIdle = idle
    frame.fill(0, G.HEADS, G.HEADS + MAX * STRIDE)

    // The windows: one glide when OpenCode comes forward.
    const dk = idle ? 0 : 1 - Math.pow(1 - clamp((t - cue.dock) / 0.85), 3)
    this.dock = dk
    const C = this.call, O = this.oc
    C.x = lerp(geo.solo.x, geo.call.x, dk)
    C.y = lerp(geo.solo.y, geo.call.y, dk)
    C.w = geo.call.w
    C.h = geo.call.h
    C.a = inCall
    C.s = gone || idle ? 1 : 0.94 + 0.06 * open
    const ccx = C.x + C.w / 2, ccy = C.y + C.h / 2
    O.x = lerp(geo.off.x, geo.oc.x, dk)
    O.y = lerp(geo.off.y, geo.oc.y, dk)
    O.w = geo.oc.w
    O.h = geo.oc.h
    O.a = idle || gone ? 0 : smooth((t - cue.dock) / 0.45)

    const live2 = !idle && !gone
    const Fl = live2 ? S.fleshAt(t) : 0
    this.flesh = Fl
    const beat = live2 ? beatAt(S, t) : 0
    const fuse = live2 ? S.fuseAt(t) : 0
    const panic = live2 ? S.panicAt(t) : 0
    const meta = live2 ? S.metaAt(t) : 0
    // How far into it they are: it gets worse all the way to the hit.
    const worse = live2 && panic > 0 ? smooth((t - cue.merge - 0.4) / Math.max(1, cue.hit - cue.merge - 0.5)) : 0
    const wobble = live2 ? S.wobbleAt(t) : 0
    const m = cue.merge
    // The walls between the four tiles: gone, from the middle of the grid outward.
    const seamTiles = live2 ? smooth((t - cue.wall) / 1.3) : 0
    // The calm that comes with being one thing: dark, wet, smiling. It waits for the blink.
    const chimera = live2 ? S.calmAt(t) : 0
    // Eyes that push out through the brow while it is at its worst, and close again after.
    const xeyes = live2 ? smooth((t - m - 1.2) / 0.9) * (1 - smooth((t - cue.hit - 0.1) / 1.0)) : 0
    const brows = live2 ? clamp(1 - smooth((t - m - 0.9) / 0.9)) : 1
    // A read of the transcript: the agent's colour runs through whatever it has hold of.
    let surge = 0
    if (live2 && t > m) for (const r of S.reads) { const u = t - r.run; if (u >= 0 && u < 2) surge += Math.exp(-u * 2.6) }
    // The headphones (from just before Play, and still on for the notes) and the music in them.
    const phones = idle ? 0 : gone ? 1 : S.phonesAt(t)
    const wearing = live2 && S.playing(t)
    const groove = wearing ? smooth((t - cue.music - 0.1) / 0.9) : 0
    const beats = wearing ? S.beatsAt(t) : 0
    const dim = live2 ? S.dimAt(t) : 0
    this.meta = meta
    this.panic = panic
    this.chimera = chimera
    this.beat = beat
    // The one mass: where its middle is (the middle of the grid) and how big a head is.
    const HR = geo.tiles[0].R
    const H0 = this.host
    H0.x = C.x + geo.grid.x
    H0.y = C.y + geo.grid.y + HR * 0.06
    H0.R = HR * (1 + 0.05 * wobble)
    H0.k = meta
    // Whoever has the floor, and whether somebody has just asked the one question that matters.
    const floor = live2 ? speakerAt(S, t) : null
    // The last turn to begin, and how long ago: eyes move then.
    let turn = null
    if (live2) for (const l of this.turns) { if (l.s0 > t) break; turn = l }

    for (let i = 0; i < MAX; i++) {
      const who = WHO[i], Cst = CAST[who], o = G.HEADS + i * STRIDE
      const isMe = i === ME
      // Which way the middle of the grid lies from this tile: +1 to its right / below it.
      const sx = isMe ? 0 : i % 2 === 0 ? 1 : -1
      const sy = isMe ? 0 : i < 2 ? 1 : -1
      const line = idle ? null : lineAt(S, who, t)
      const P = poseAt(this.keys[who], idle ? -1 : t, this.pose)
      if (isMe) {
        const target = (idle || gone) && live.hoverJoin ? 1 : 0
        const dt = Math.min(live.dt || 1 / 60, 0.05)
        if (live.reduced) {
          this.hoverSpr.x = lerp(this.hoverSpr.x, target, 1 - Math.exp(-dt * 18))
          this.hoverSpr.v = 0
        } else {
          // Damped spring (k=260, c=23): snappy curious brow lift with soft organic settle.
          const a = (target - this.hoverSpr.x) * 260 - this.hoverSpr.v * 23
          this.hoverSpr.v += a * dt
          this.hoverSpr.x += this.hoverSpr.v * dt
        }
        const h = this.hoverSpr.x
        if (Math.abs(h) > 0.0005) {
          P.br = lerp(P.br, 0.8, h)
          P.sk = lerp(P.sk, 0.6, h)
          P.ar = lerp(P.ar, -0.1, h)
          P.lu = lerp(P.lu, 0.34, h)
        }
      }
      // How far it has gone in this one. Maya, nearest OpenCode, first; Priya, as ever, last.
      const late = isMe ? 0 : [0.3, 0, 0.55, 0.15][i]
      const fl = isMe || !live2 ? 0 : S.fleshAt(t - late)
      const liquid = smooth((fl - 0.1) / 0.55)
      // Terror. Priya's starts a second after everyone else's.
      const pn = isMe ? 0 : who === 'ops' ? panic * smooth((t - m - 1.15) / 0.3) : panic
      // Into the mass. Priya gets there a beat after the others, and not the right way up.
      const mt = isMe || !live2 ? 0 : who === 'ops' ? S.metaAt(t - (cue.late - cue.hit)) : meta
      const cm = isMe ? 0 : who === 'ops' && live2 ? smooth((t - cue.late - 0.5) / 1.2) : chimera
      // Its own heart: the top row on the lub, the bottom row on the dub.
      const bt = isMe || !live2 ? 0 : i < 2 ? beat : beatAt(S, t - 0.17)
      // Each tendril that lands in it goes through it.
      let jolt = 0
      if (!isMe && live2 && t > m && t < cue.hit) { const u = t - S.jolts[[2, 0, 3, 1][i]]; if (u >= 0) jolt = Math.exp(-u * 7) }

      // Speech: how open from the envelope, what shape from the word.
      let open = P.rest, wide = 0.3 + 0.5 * pn, round = 0, env = 0, shut = 1, tip = 0
      // The breath before a line: brows up a little, head back.
      let intake = 0
      if (line) {
        const tau = t - line.t0
        intake = line.who === 'host' ? 0 : bell((t - line.s0 + 0.07) / 0.1)
        env = Math.max(level(line, t), 0.7 * level(line, t - 0.02), 0.45 * level(line, t - 0.045))
        let n = 0
        while (n < line.words.length - 1 && tau > line.words[n][2]) n++
        const a = line.vis[Math.max(0, n - 1)], b = line.vis[n] ?? a
        const k = line.words.length ? smooth((tau - (line.words[n][1] - 0.07)) / 0.1) : 1
        if (a) {
          wide = lerp(a.wide, b.wide, k)
          round = lerp(a.round, b.round, k)
          const shape = lerp(a.o, b.o, k)
          if (b.shut) shut = clamp(Math.abs(tau - line.words[n][1]) / 0.06, 0.12, 1)
          if (b.bite) shut = clamp(Math.abs(tau - line.words[n][1]) / 0.05, 0.3, 1)
          if (b.tip) tip = bell((tau - line.words[n][1]) / 0.09)
          // Me barely opens his mouth; terror opens it all the way; nobody shouts in bliss.
          // (And a hum is said with it shut.)
          const hum = /^[\s"“]*(m+[- ]?h*m+|h+m+)[\s.!?,…"”]*$/i.test(line.text ?? '')
          const loud = hum ? 0.05 : isMe ? 0.62 : 1.0 + 0.3 * pn - 0.45 * cm
          open = Math.max(open, clamp(Math.pow(env, 0.72) * shape * loud * shut, 0, 1.3))
          // The feeling pulls at the mouth while it talks: wider for a smile, narrower for alarm.
          wide = clamp(wide + 0.25 * P.sm)
        }
      }
      // Terror: each of them has their own. Marcus roars, Maya screams, Priya is late, Dev winces.
      const gape = [0.95, 1.0, 0.55, 0.7][i] ?? 0
      wide = lerp(wide, [0.45, 0.2, 0.3, 0.6][i] ?? 0.2, 0.75 * pn)
      round = lerp(round, [0.2, 0.6, 0.7, 0.1][i] ?? 0.5, 0.8 * pn)
      open = Math.max(open, gape * pn)
      // And it does not hold still: the jaw shudders, faster than anyone talks.
      open *= 1 + pn * (0.16 + 0.14 * worse) * Math.sin(t * 43 + i * 1.7)
      const speak = line ? clamp((t - line.t0 + 0.05) / 0.1) * clamp((line.t1 + 0.12 - t) / 0.12) : 0

      // Blinks. Most happen when the eyes move (a new speaker); the rest by a slow clock. Shut
      // fast, open slower; the two lids are never quite together. Nobody blinks while it is
      // happening to them. The moment they are one thing, every eye blinks once, together.
      const every = isMe ? 4.6 : who === 'eng' ? 3.1 : who === 'ops' ? 5.2 : 3.7
      const slot = Math.floor(clk / every)
      const tb = (slot + 0.2 + 0.6 * hash(slot * 3.7 + Cst.seed)) * every
      const slow = isMe ? 1.7 : who === 'ops' ? 1.6 : 1
      let blinkL = blinkAt(clk - tb, slow), blinkR = blinkAt(clk - tb - 0.016, slow)
      if (who === 'eng' && hash(slot + 9.1) > 0.55) { blinkL = Math.max(blinkL, blinkAt(clk - tb - 0.26)); blinkR = Math.max(blinkR, blinkAt(clk - tb - 0.27)) }
      if (!isMe && turn && turn.who !== who && hash(turn.s0 * 7.3 + Cst.seed) > 0.35) {
        const u = t - turn.s0 - 0.06 - 0.05 * i - (who === 'ops' ? 0.25 : 0)
        blinkL = Math.max(blinkL, blinkAt(u, slow))
        blinkR = Math.max(blinkR, blinkAt(u - 0.014, slow))
      }
      if (!isMe && live2) {
        const hush = 1 - smooth(pn * 3)
        blinkL *= hush
        blinkR *= hush
        if (t > cue.calm - 0.05) {
          // (All of them at once, and then slowly, a little apart, for good.)
          const one = blinkAt(t - (who === 'ops' ? cue.late + 0.3 : cue.blink), 1.5)
          const s2 = Math.floor((t + i * 0.37) / 3.9)
          const again = t > cue.blink + 1.5 ? blinkAt(t + i * 0.37 - (s2 + 0.5) * 3.9, 2.0) : 0
          blinkL = Math.max(one, again)
          blinkR = Math.max(one, again * 0.9)
        }
      }
      if (isMe && live2) { const b = bell((t - cue.phones[1] + 0.05) / 0.3); blinkL = Math.max(blinkL, b); blinkR = Math.max(blinkR, b) }

      // The tile, and the head in it. The four breathe a quarter of a cycle apart.
      const T = geo.tiles[i]
      const breath = !live2 || t < cue.breathe ? 0 : Math.sin(S.breathPhase(t) - [0, 1.57, 3.14, 4.71][i % 4])
      const breathe = fl > 0 ? breath * smooth(fl / 0.14) : 0
      // They join one at a time (cue.joins: Marcus, Maya, Dev, and then Priya), and each pops
      // into place: a tile that grows the last tenth of its size, on a spring with no bounce.
      const joined = isMe ? cue.open : cue.joins[who]
      const pop = isMe || idle ? 1 : crit(t - joined, 16)
      const grow = C.s * (0.9 + 0.1 * pop)
      let hw = T.hw * grow * (1 + 0.013 * breathe), hh = T.hh * grow * (1 + 0.011 * breathe)
      // The tile itself rattles: something has hold of the window.
      const rattle = isMe ? 0 : pn * (0.7 + 2.6 * worse)
      let tx = ccx + (T.x - C.w / 2) * C.s + rattle * (0.6 * Math.sin(t * 39 + i * 1.9) + 0.4 * Math.sin(t * 67 + i * 3.1))
      let ty = ccy + (T.y - C.h / 2) * C.s + rattle * 0.8 * (0.6 * Math.cos(t * 35 + i * 2.7) + 0.4 * Math.sin(t * 59 + i * 1.3))
      const tileA = gone || idle ? 0 : isMe ? inCall : clamp((t - joined) / 0.16)
      const lean = P.lean * (1 + 0.1 * mt) * (1 + 0.025 * intake)
      let R = T.R * grow * lean * (1 + 0.02 * breathe) * (1 + 0.05 * wobble * mt)
      // Home is the middle of its tile. Idle life: slow drift with holds, and now and then a
      // change of posture. Not three pendulums.
      const px = R * (0.03 * drift(clk, 5.3 + i * 0.7, Cst.seed) + 0.012 * Math.sin(clk * 0.9 + Cst.seed))
      const py = R * (0.02 * drift(clk, 6.1 + i * 0.5, Cst.seed + 4) + 0.01 * Math.sin(clk * 1.3 + Cst.seed * 2))
      let hx = tx + px, hy = ty + hh * 0.04 - R * 0.02 * env + (lean - 1) * R * 0.25 - R * 0.03 * intake + py
      // Once the walls are gone they lean a little toward the middle. Then they are drawn to
      // it by the cords, strain against them, and the last of the distance goes all at once.
      const strain = pn * (0.10 + 0.03 * Math.sin(t * 9 + i))
      hx += sx * (hw * 0.07 * seamTiles - R * strain)
      hy += sy * (hh * 0.05 * seamTiles - R * strain * 0.8)
      const nx = H0.x - sx * NEST[0] * HR, ny = H0.y - sy * NEST[1] * HR
      hx = lerp(hx, nx, mt)
      hy = lerp(hy, ny, mt)
      // What the shutter sees: where it is without the shaking.
      const sxm = hx, sym = hy
      if (!isMe) {
        const shiver = pn * (1.5 + 3.4 * worse)
        hx += 2.2 * jolt * Math.sin(t * 90 + i * 2) + shiver * (0.6 * Math.sin(t * 43 + i * 4) + 0.4 * Math.sin(t * 71 + i * 2.3))
        hy += -1.2 * jolt + shiver * 0.8 * (0.6 * Math.sin(t * 37 + i) + 0.4 * Math.cos(t * 61 + i * 3.7))
      }
      // Maya's head is always tipped a little. Priya arrives in the mass rolled right over.
      let roll = 0.03 * drift(clk, 4.7 + i, Cst.seed + 9) + (who === 'eng' ? 0.07 : 0)
      if (who === 'ops' && live2) roll += 0.62 * mt * (1 - 0.72 * smooth((t - cue.late - 0.1) / 0.9))
      // Their crowns splay out from the middle of it.
      roll += -sx * sy * 0.16 * mt
      if (isMe) {
        // The track: a dip of the head on every kick, a sway every bar, and more of both with
        // every stem. One real nod for the meeting, the one time somebody asks him something.
        const dip = groove * (0.5 - 0.5 * Math.cos(beats * Math.PI * 2))
        const nod = idle ? 0 : 0.6 * bell((t - L.me_gotit.s0 - 0.25) / 0.3)
        const into = 1 + 0.35 * Math.max(0, S.stageAt(t))
        hy += R * (0.05 * into * dip + 0.12 * nod)
        hx += R * 0.03 * into * groove * Math.sin(beats * Math.PI * 0.5)
        roll += 0.07 * into * groove * Math.sin(beats * Math.PI * 0.5)
        // Me's bean is the hero before the call and the host of the notes after it.
        const k = Math.max(1 - callIn, gone ? 1 : 0)
        const home = gone ? geo.notes : geo.hero
        hx = lerp(hx, home.x, k)
        hy = lerp(hy, home.y + home.R * 0.012 * Math.sin(clk * 1.1), k)
        R = lerp(R, home.R, k)
        // His tile comes with him: alone in the call it is most of the window, and it shrinks
        // to his seat as he does (the head is clipped to its tile, so the tile has to hold it).
        if (!gone && k > 0) {
          const big = (home.R / T.R) * 0.9
          tx = lerp(tx, home.x, k)
          ty = lerp(ty, home.y, k)
          hw = lerp(hw, Math.min(T.hw * big, C.w * C.s / 2 - 14), k)
          hh = lerp(hh, T.hh * big, k)
        }
      }
      this.rect[i * 4] = tx
      this.rect[i * 4 + 1] = ty
      this.rect[i * 4 + 2] = hw
      this.rect[i * 4 + 3] = hh
      this.headAt[i * 3] = hx
      this.headAt[i * 3 + 1] = hy
      this.headAt[i * 3 + 2] = R

      // Where they look. Eyes jump; they do not pan.
      const toward = (x, y, k = 1) => {
        const dx = (x - hx) / (R * 4.5), dy = (y - hy) / (R * 4.5)
        const d = Math.hypot(dx, dy)
        const s = d > 1 ? 1 / d : 1
        return [dx * s * k, dy * s * k]
      }
      // A small dart every 0.6 to 1.2 s, wherever they are looking.
      const beatN = Math.floor((clk + Cst.seed * 3) / (0.75 + 0.3 * hash(i * 5.1)))
      const dartX = (hash(beatN * 1.31 + Cst.seed) - 0.5) * 0.2, dartY = (hash(beatN * 2.17 + Cst.seed) - 0.5) * 0.12
      let gx = 0, gy = 0
      if (idle || gone) {
        if (live.pointer.on) [gx, gy] = toward(live.pointer.x, live.pointer.y)
        else { gx = 0.25 * Math.sin(clk * 0.5); gy = 0.1 * Math.sin(clk * 0.37 + 1) }
      } else if (isMe) {
        // Me watches his own screen: the offer, the pointer, and then nothing but his four tabs:
        // the pointer while it is going somewhere, the page while it is not. He never looks up
        // at the call. He does, once, for goodbye.
        const cur = live.cursor
        const N = geo.notif
        if (t < cue.invite + 0.22) { gx = dartX; gy = dartY } // alone, with nothing to look at
        else if (t < cue.cursor + 0.3) [gx, gy] = toward(N.right - N.w / 2, N.y + 30, 0.95) // the notification, top right
        else if (t < cue.join + 0.3) [gx, gy] = toward(cur.x, cur.y, 0.95) // the pointer, to Join
        else if (t < cue.offer + 0.1) { [gx, gy] = toward(C.x + geo.grid.x, C.y + geo.grid.y, 0.7); gx += dartX * 0.5; gy += dartY * 0.5 } // the others, arriving
        else if (t < cue.toRecord + 0.2) [gx, gy] = toward(geo.pill.x, geo.pill.y + 6, 0.95) // Hex, at the top of his screen
        else if (t < cue.browser + 0.9 || t > cue.reach) [gx, gy] = toward(cur.x, cur.y, 0.95)
        else if (t < cue.closeTab - 0.5) {
          // Reading: left to right, and back, three lines every two seconds. A click takes the eyes.
          const B = geo.browser
          const scan = ((t * 1.5) % 1) * 2 - 1
          ;[gx, gy] = toward(B.x + B.w * (0.5 + 0.22 * scan), B.y + B.h * 0.52, 0.95)
          if ((cur.moving || 0) > 30) [gx, gy] = toward(cur.x, cur.y, 0.95)
        } else if (t < cue.pause + 0.3) [gx, gy] = toward(cur.x, cur.y, 0.95) // over to the tab, and to Pause
        else if (t < L.me_bye.s0 - 0.35) [gx, gy] = toward(H0.x, H0.y, 0.7) // up, at whatever that is
        else if (t < L.me_bye.s1) { gx = 0; gy = -0.05 } // the lens: goodbye
        else [gx, gy] = toward(cur.x, cur.y, 0.95)
      } else {
        if (line && pn < 0.2) {
          // Speaking: the lens, with a glance down and aside in the gaps to think.
          const tau = t - line.t0
          let gapK = 0
          for (let n = 1; n < line.words.length; n++) if (tau > line.words[n - 1][2] + 0.03 && tau < line.words[n][1] - 0.02 && line.words[n][1] - line.words[n - 1][2] > 0.16) gapK = 1
          gx = dartX * 0.5 + gapK * (i % 2 ? -0.4 : 0.4)
          gy = dartY * 0.5 + gapK * 0.3
        } else if (floor && floor.who !== who) {
          // Listening: whoever has the floor. Priya gets there a quarter of a second late.
          const j = WHO.indexOf(floor.who)
          if (j >= 0 && j !== ME && t > floor.s0 + (who === 'ops' ? 0.3 : 0.08 + 0.04 * i)) [gx, gy] = toward(C.x + geo.tiles[j].x, C.y + geo.tiles[j].y, 0.8)
          gx += dartX * 0.5
          gy += dartY * 0.5
        } else { gx = dartX; gy = dartY }
        // The number landing, the workspaces going: they look at their own screens.
        for (const tt of [cue.fix1, cue.del]) if (t > tt - 0.05 && t < tt + 0.8 + (who === 'ops' ? 0.9 : 0) && (who !== 'ops' || t > tt + 0.9)) { gx = sx * 0.45; gy = 0.5 }
        if (pn > 0.02) {
          // Eyes everywhere at once, held for uneven moments: at what is coming in, at each
          // other, at the lens, for help.
          const n = Math.floor(t * 3.1 + i * 0.63 + 0.8 * Math.sin(t * 1.7 + i))
          gx = lerp(gx, (hash(n * 3.3 + i) - 0.5) * 1.8, pn)
          gy = lerp(gy, (hash(n * 4.9 + i * 2) - 0.5) * 1.2, pn)
        }
      }
      // One organism looks one way: out. Each eye gets there a little after the one before.
      const lock = isMe ? 0 : smooth((t - cue.calm - 0.03 * i) / 0.06)
      gx *= 1 - lock
      gy *= 1 - lock
      // (And once it is calm, all of it watches him.)
      if (!isMe && live2 && t > cue.blink + 1.0) {
        const [wx, wy] = toward(C.x + geo.tiles[ME].x, C.y + geo.tiles[ME].y, 0.6)
        const k = smooth((t - cue.pause - 0.2 - 0.06 * i) / 0.1)
        gx = lerp(gx, wx, k)
        gy = lerp(gy, wy, k)
      }
      const dt = live.dt || 1 / 60
      const gk = fresh ? 1 : 1 - Math.exp(-dt * (idle ? 12 : 55))
      const gzx = (this.gaze[i * 2] += (clamp(gx, -1, 1) - this.gaze[i * 2]) * gk)
      const gzy = (this.gaze[i * 2 + 1] += (clamp(gy, -1, 1) - this.gaze[i * 2 + 1]) * gk)
      // The face turns after the eyes have landed.
      const fk = fresh ? 1 : 1 - Math.exp(-dt * 7)
      const fcx = (this.face[i * 2] += (gzx - this.face[i * 2]) * fk)
      const fcy = (this.face[i * 2 + 1] += (gzy - this.face[i * 2 + 1]) * fk)

      frame[o + F.CX] = hx
      frame[o + F.CY] = hy
      frame[o + F.R] = R
      frame[o + F.ROLL] = roll + 0.05 * fcx * (1 - speak) + 0.02 * speak * Math.sin(t * 5.2 + i) + (isMe ? 0 : 0.02 * jolt * Math.sin(t * 70) + (0.025 + 0.045 * worse) * pn * Math.sin(t * 27 + i))
      frame[o + F.TX] = tx
      frame[o + F.TY] = ty
      frame[o + F.THW] = hw
      frame[o + F.THH] = hh
      frame[o + F.TA] = tileA
      frame[o + F.TRAD] = (geo.narrow ? 9 : 13) + 22 * smooth(fl * 1.2)
      frame[o + F.RING] = speak * (1 - cm) * (line && line.under ? 0.4 : 1) * (1 - smooth(mt * 4))
      frame[o + F.DIM] = 0.6 * dim
      frame[o + F.FACEX] = 0.07 * fcx
      frame[o + F.FACEY] = 0.045 * fcy
      frame[o + F.GAZEX] = gzx
      frame[o + F.GAZEY] = gzy
      // One lid sits a little lower than the other (whose, and by how much, is theirs).
      frame[o + F.LIDL] = lerp(clamp(P.lu + Cst.lid), 1, blinkL)
      frame[o + F.LIDR] = lerp(P.lu, 1, blinkR)
      frame[o + F.LOWL] = clamp(P.ll + 0.3 * blinkL + 0.16 * Math.max(P.sm, 0) * clamp(open * 2))
      frame[o + F.LOWR] = clamp(P.ll + 0.3 * blinkR + 0.16 * Math.max(P.sm, 0) * clamp(open * 2))
      frame[o + F.PUPIL] = clamp(P.pd + (isMe ? 0 : 0.12 * bt * smooth(fl * 2)))
      // Terror: the eyes are as wide as they go, and wider as it gets worse.
      frame[o + F.EYE] = Cst.eye * P.ey * (1 + pn * (0.07 + 0.09 * worse))
      const jit = isMe ? 0 : 0.25 * jolt + 0.1 * pn * Math.sin(t * 31 + i * 2.1)
      frame[o + F.UPL] = P.bl + 0.18 * env * (isMe ? 0.3 : 1) + jit + 0.2 * intake
      frame[o + F.UPR] = P.br + 0.18 * env * (isMe ? 0.3 : 1) + jit + 0.2 * intake
      frame[o + F.ANGL] = P.al
      frame[o + F.ANGR] = P.ar
      frame[o + F.ARCHL] = P.arch
      frame[o + F.ARCHR] = P.arch
      frame[o + F.WEIGHT] = Cst.weight
      frame[o + F.SPACING] = Cst.spacing
      frame[o + F.OPEN] = open
      frame[o + F.WIDE] = wide
      frame[o + F.ROUND] = round * clamp(1 - P.sm * 0.5)
      frame[o + F.SMILE] = P.sm
      frame[o + F.SMIRK] = P.sk * (1 - 0.6 * clamp(open * 1.5))
      // The tongue stays down and dark, except on an L or a TH, and in a scream.
      frame[o + F.TONGUE] = clamp(0.12 + 0.5 * tip + 0.7 * pn)
      frame[o + F.JAW] = clamp(open - 0.9, 0, 0.4) + 0.25 * pn
      // A quarter of who they were stays in their part of it.
      const rk = 0.75 * smooth(fl * 1.3)
      frame[o + F.RIMR] = lerp(Cst.rim[0], CRIMSON_RIM[0], rk)
      frame[o + F.RIMG] = lerp(Cst.rim[1], CRIMSON_RIM[1], rk)
      frame[o + F.RIMB] = lerp(Cst.rim[2], CRIMSON_RIM[2], rk)
      frame[o + F.RIMS] = 1 + 0.5 * fl * bt
      frame[o + F.GLASSES] = Cst.glasses
      frame[o + F.GLOSS] = 1
      frame[o + F.EMERALD] = 0
      frame[o + F.ALPHA] = isMe ? 1 : tileA
      frame[o + F.RX] = Cst.shape[0]
      frame[o + F.RY] = Cst.shape[1]
      frame[o + F.STRETCH] = 1 + 0.05 * Math.max(0, open - 0.3) + 0.012 * Math.sin(clk * 1.6 + Cst.seed) + 0.02 * breathe + 0.03 * intake - 0.07 * wobble * mt
      frame[o + F.RIMANG] = Cst.rimAngle
      frame[o + F.SEED] = Cst.seed
      frame[o + F.TIME] = clk
      frame[o + F.KEY] = 1
      frame[o + F.WARM] = gone ? 1 : 0
      frame[o + F.FLESH] = fl
      frame[o + F.BREATH] = breathe
      frame[o + F.BEAT] = bt * smooth(fl * 3)
      frame[o + F.LIQUID] = liquid
      frame[o + F.THIRD] = who === 'ops' ? 0 : P.th * (1 - 0.95 * Math.max(blinkL, blinkR))
      frame[o + F.SURGE] = isMe ? 0 : clamp(surge) * fuse
      // Where the tendrils go into it: from OpenCode's side for the two on the right; out of
      // the gutter between the rows, from below or above, for the two on the left.
      frame[o + F.ENTRYX] = sx < 0 ? 0.8 : 0.3
      frame[o + F.ENTRYY] = sx < 0 ? -0.15 : 0.85 * sy
      frame[o + F.FUSE] = isMe ? 0 : seamTiles
      frame[o + F.SEAMSIDE] = sx
      frame[o + F.SEAMY] = sy
      frame[o + F.HAT] = Cst.hat
      frame[o + F.TAPER] = Cst.taper
      // Part of the one mass: the mass draws it, in one piece, and its tile only draws the room.
      frame[o + F.INMASS] = !isMe && meta > 0.002 ? 1 : 0
      frame[o + F.META] = mt
      frame[o + F.CHIMERA] = cm
      frame[o + F.PANIC] = pn
      frame[o + F.DRIP] = clk
      frame[o + F.XEYES] = isMe ? 0 : xeyes * (1 - 0.9 * Math.max(blinkL, blinkR)) * (who === 'ops' ? 0.5 : 1)
      frame[o + F.BROWS] = isMe ? 1 : brows
      frame[o + F.REAL] = isMe ? 0 : smooth((fl - 0.12) / 0.5)
      // Her glasses: knocked crooked, a lens cracked, and then the flesh has them.
      frame[o + F.CROOK] = Cst.glasses && live2 ? smooth((t - cue.crack + 0.25) / 0.3) : 0
      frame[o + F.CRACK] = Cst.glasses && live2 && t >= cue.crack ? 1 : 0
      frame[o + F.SUNK] = Cst.glasses ? cm : 0
      // No two eyes in it are the same size.
      frame[o + F.UNEVEN] = isMe ? 0 : [0.16, -0.2, 0.1, -0.13][i] * smooth(mt * 1.4) + (Cst.lid ? 0.03 : 0)
      frame[o + F.HUE] = i
      // The headphones: lowered onto his head, settling with a little overshoot.
      frame[o + F.PODS] = isMe ? seat(phones) : 0
      frame[o + F.PODA] = isMe ? clamp(phones * 3) : 0

      // For the shutter: how fast the face and its tile moved since the last frame drawn.
      const w = i * 8, was = this.was
      if (this.step > 0) {
        frame[o + F.VX] = (sxm - was[w]) / this.step
        frame[o + F.VY] = (sym - was[w + 1]) / this.step
        frame[o + F.VGROW] = was[w + 2] > 0 ? (R / was[w + 2] - 1) / this.step : 0
        frame[o + F.TVX] = (tx - was[w + 3]) / this.step
        frame[o + F.TVY] = (ty - was[w + 4]) / this.step
        frame[o + F.TVW] = (hw - was[w + 5]) / this.step
        frame[o + F.TVH] = (hh - was[w + 6]) / this.step
      }
      was[w] = sxm
      was[w + 1] = sym
      was[w + 2] = R
      was[w + 3] = tx
      was[w + 4] = ty
      was[w + 5] = hw
      was[w + 6] = hh
    }
    this.breath = !live2 || t < cue.breathe ? 0 : Math.sin(S.breathPhase(t))
    this.fuse = fuse
    this.seamTiles = seamTiles
    this.wobble = wobble
    return MAX
  }
}
