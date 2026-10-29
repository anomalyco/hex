// The Hex hero: Kit's notifications (GitHub issues, X replies, Slack pings) pile up around a
// floating head; it voices replies into them and dictates prompts into an OpenCode window that
// fixes the complaints. The arc runs calm → clipped → SHOUTING → SUPER SAIYAN rapid-fire →
// gibberish → the Tommy-gun wipe → dizzy exhale, then restarts.
//
// mountShout() draws the idle head immediately (no audio, no network beyond this chunk);
// start() loads the voice bank and runs the show.
import { Canvas2DRenderer } from "./canvas2d.js"
import { Cards, FIXABLE, KINDS, PAD } from "./cards.js"
import { GpuRenderer } from "./gpu.js"
import { Words } from "./words.js"
import { AudioEngine } from "./audio.js"
import { ScreenBreak } from "./screen-break.js"

const CAPTURE = import.meta.env.VITE_SHOUT_CAPTURE === "1"
// The poster box is this many head radii wide (head, hair, and halo).
export const HEAD_BOX = 3.2

export async function mountShout({ root, canvas, head: headEl, reduced: reducedMotion, forceCanvas2d = false }) {
const params = new URLSearchParams(location.search)
const capture = CAPTURE && params.has("capture")
const reduced = reducedMotion && !capture
const coarse = matchMedia("(pointer: coarse)").matches
let started = false

let seed = Number((capture && params.get("seed")) || 11) >>> 0
const rand = () => {
  seed = (seed + 0x6d2b79f5) >>> 0
  let t = seed
  t = Math.imul(t ^ (t >>> 15), t | 1)
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296
}
const rnd = (a, b) => a + rand() * (b - a)
const pick = (arr) => arr[Math.floor(rand() * arr.length)]
const clamp = (x, a, b) => Math.max(a, Math.min(b, x))
const lerp = (a, b, t) => a + (b - a) * t
const smooth = (a, b, x) => {
  const t = clamp((x - a) / (b - a), 0, 1)
  return t * t * (3 - 2 * t)
}

// Tuned in the prototype's knobs panel.
const v = {
  intensity: -0.01, // < 0: follow the arc
  cycle: 102.5,
  curve: 1.4,
  spawn: 1,
  maxCards: 22,
  shake: 1,
  fx: 1,
  hitStop: 90,
  wordScale: 1,
  shutter: 0.75,
  rim: 0.9,
  dprCap: 2,
}

await Promise.all(['600 26px "Inter Variable"', '800 26px "Inter Variable"', '400 13px "Inter Variable"'].map((f) => document.fonts.load(f)))
let manifest = null
let rtime = 0 // real seconds since mount (voice clips and their words run on this clock)
const audio = new AudioEngine({ clock: () => rtime })
const cards = new Cards()
await cards.ready
const words = new Words()
const atlases = { cards, words }
const renderer = (!forceCanvas2d && (await GpuRenderer.create(canvas, atlases).catch(() => null))) || new Canvas2DRenderer(canvas, atlases)
const screenBreak = new ScreenBreak(root, canvas, rand)
root.dataset.renderer = renderer.kind

// ─── Layout ────────────────────────────────────────────────────────────────────
// The head sits on the prerendered poster's box (`headEl`), so swapping poster for canvas
// never moves it. Cards keep clear of everything marked [data-avoid].
let W = 0
let H = 0
let copyRects = []
let slot = { x: 0, y: 0, R: 60 }
const titleWords = [...root.querySelectorAll("[data-title-word]")].map((el, idx) => ({
  el,
  idx,
  text: (el.textContent || "").trim(),
  state: "rest", // "rest" | "jolted" | "falling" | "returning"
  x: 0,
  y: 0,
  vx: 0,
  vy: 0,
  rot: 0,
  vrot: 0,
  heat: 0,
  bounces: 0,
  targetRot: 0,
  returnAge: 0,
  returnDelay: 0,
  fromX: 0,
  fromY: 0,
  fromRot: 0,
  rest: { cx: 0, cy: 0, w: 0, h: 0, bottom: 0 },
}))

function applyTitleWordStyle(w) {
  if (w.state === "rest") {
    w.el.style.transform = ""
    w.el.style.color = ""
    w.el.style.textShadow = ""
    return
  }
  w.el.style.transform = `translate3d(${w.x.toFixed(2)}px, ${w.y.toFixed(2)}px, 0) rotate(${w.rot.toFixed(4)}rad)`
  if (w.heat > 0.02) {
    const h = clamp(w.heat, 0, 1)
    w.el.style.color = `rgb(255, ${Math.round(244 - 64 * h)}, ${Math.round(240 - 155 * h)})`
    w.el.style.textShadow = `0 0 ${Math.round(18 * h)}px rgba(255, 132, 36, ${(0.88 * h).toFixed(2)}), 0 0 ${Math.round(38 * h)}px rgba(255, 68, 16, ${(0.55 * h).toFixed(2)})`
  } else {
    w.el.style.color = ""
    w.el.style.textShadow = ""
  }
}

function measureAvoid() {
  const r = root.getBoundingClientRect()
  W = r.width
  H = r.height
  // Temporarily clear transforms on title words so resting geometry and [data-avoid] rects are exact.
  for (const w of titleWords) if (w.state !== "rest") w.el.style.transform = ""

  const rects = []
  let heroL = Infinity
  let heroT = Infinity
  let heroR = -Infinity
  let heroB = -Infinity

  for (const el of root.querySelectorAll("[data-avoid]")) {
    const e = el.getBoundingClientRect()
    if (e.width <= 0 || e.height <= 0) continue
    const inHero = !!el.closest(".hero")
    const padX = inHero ? 32 : 22
    const padY = inHero ? 26 : 16
    const l = e.left - r.left - padX
    const t = e.top - r.top - padY
    const rr = e.right - r.left + padX
    const b = e.bottom - r.top + padY
    rects.push({ l, t, r: rr, b, isHero: inHero })
    if (inHero) {
      heroL = Math.min(heroL, l)
      heroT = Math.min(heroT, t)
      heroR = Math.max(heroR, rr)
      heroB = Math.max(heroB, b)
    }
  }
  // Also measure individual hero children so wide containers never hide inner bounds, and unify the hero stack
  // from above h1 all the way to the bottom of the viewport so no card ever wedges between copy and Download.
  for (const el of root.querySelectorAll(".hero h1, .hero__copy p, .hero .download, .hero .command, .hero .requirements, .hero .download-note, .hero .platforms")) {
    const e = el.getBoundingClientRect()
    if (e.width <= 0 || e.height <= 0) continue
    heroL = Math.min(heroL, e.left - r.left - 34)
    heroT = Math.min(heroT, e.top - r.top - 30)
    heroR = Math.max(heroR, e.right - r.left + 34)
    heroB = Math.max(heroB, e.bottom - r.top + 32)
  }
  if (heroL < heroR && heroT < heroB) {
    rects.push({ l: heroL, t: heroT, r: heroR, b: Math.max(heroB, H + 40), isHero: true })
  }
  copyRects = rects

  for (const w of titleWords) {
    const e = w.el.getBoundingClientRect()
    w.rest = {
      cx: e.left - r.left + e.width / 2,
      cy: e.top - r.top + e.height / 2,
      w: e.width,
      h: e.height,
      bottom: e.bottom - r.top,
    }
    if (w.state !== "rest") applyTitleWordStyle(w)
  }
}

function resize() {
  const r = root.getBoundingClientRect()
  W = r.width
  H = r.height
  v.maxCards = W < 640 ? 9 : W < 1100 ? 15 : 22
  const dpr = Math.min(devicePixelRatio || 1, W < 640 ? 1.5 : v.dprCap)
  renderer.resize(W, H, dpr)
  screenBreak.resize(W, H, dpr)
  const b = headEl.getBoundingClientRect()
  slot = { x: b.left - r.left + b.width / 2, y: b.top - r.top + b.height / 2, R: b.width / HEAD_BOX }
  measureAvoid()
}
const cardScale = () => clamp(W / 1500, 0.56, 1)
let fourthZoom = { x: 1, v: 0 }
const headR = () => slot.R * fourthZoom.x
// Phones have no room beside the head, so once the show starts it slides right and the
// OpenCode window takes the left.
const narrow = () => W < 640
let baseShift = 0
const shiftTarget = () => (narrow() && started ? Math.max(0, W - headR() * 1.45 - 8 - slot.x) : 0)
const headBase = () => ({ x: slot.x + baseShift, y: slot.y + (fourthZoom.x - 1) * slot.R * 0.22 })

// Only during the Tommy-gun spin / screen-break finale may cards or debris overlap the hero copy;
// at all other times (and again in "after" / reset) the hero copy + download stack is a hard exclusion zone.
const allowCopyOverlap = () =>
  blastStage === "fire" ||
  blastStage === "blast" ||
  blastStage === "stare" ||
  blastStage === "cracking" ||
  blastStage === "shattered" ||
  blastStage === "rebuilding"

function effectiveAvoidRect(r, hw) {
  let l = r.l
  let rr = r.r
  if (r.isHero) {
    // If a side flank beside the hero copy is too narrow to hold a card cleanly, close that flank
    // so cards stay above the copy instead of wedging into the side of the subtitle/button.
    if (l < hw * 2 + 28) l = -200
    if (W - rr < hw * 2 + 28) rr = W + 200
  }
  return { l, t: r.t, r: rr, b: r.b }
}

function overlapsAvoid(x, y, hw, hh) {
  if (allowCopyOverlap()) return false
  for (const raw of copyRects) {
    const r = effectiveAvoidRect(raw, hw)
    if (x + hw > r.l && x - hw < r.r && y + hh > r.t && y - hh < r.b) return true
  }
  return false
}

function resolveAvoid(x, y, hw, hh) {
  let cx = clamp(x, hw + 14, Math.max(hw + 14, W - hw - 14))
  let cy = clamp(y, hh + 58, Math.max(hh + 58, H - hh - 18))
  if (allowCopyOverlap()) return { x: cx, y: cy, pushed: false }
  let pushed = false
  for (let pass = 0; pass < 2; pass++) {
    for (const raw of copyRects) {
      const r = effectiveAvoidRect(raw, hw)
      if (cx + hw <= r.l || cx - hw >= r.r || cy + hh <= r.t || cy - hh >= r.b) continue
      pushed = true
      const cands = []
      const upY = r.t - hh - 6
      if (upY >= hh + 52) cands.push({ x: cx, y: upY, cost: Math.abs(cy - upY) * 0.85 })
      const leftX = r.l - hw - 6
      if (leftX >= hw + 12) cands.push({ x: leftX, y: cy, cost: Math.abs(cx - leftX) })
      const rightX = r.r + hw + 6
      if (rightX <= W - hw - 12) cands.push({ x: rightX, y: cy, cost: Math.abs(cx - rightX) })
      const downY = r.b + hh + 6
      if (downY <= H - hh - 16) cands.push({ x: cx, y: downY, cost: Math.abs(cy - downY) * 1.15 })
      if (cands.length) {
        cands.sort((a, b) => a.cost - b.cost)
        cx = cands[0].x
        cy = cands[0].y
      } else {
        cy = Math.max(hh + 52, r.t - hh - 6)
      }
    }
  }
  return { x: cx, y: cy, pushed }
}

// ─── Springs ───────────────────────────────────────────────────────────────────
const S = (x = 0) => ({ x, v: 0 })
function spring(s, target, freq, zeta, dt) {
  const w = 2 * Math.PI * freq
  const n = Math.max(1, Math.ceil(dt / (1 / 240)))
  const h = dt / n
  for (let i = 0; i < n; i++) {
    s.v += (-w * w * (s.x - target) - 2 * zeta * w * s.v) * h
    s.x += s.v * h
  }
  return s.x
}

// ─── Arc ───────────────────────────────────────────────────────────────────────
// Times are for an 88 s cycle and scale with v.cycle.
const ARC = [
  { id: "calm", at: 0, stage: 1, i0: 0, i1: 0.22 },
  { id: "clipped", at: 16, stage: 2, i0: 0.26, i1: 0.46 },
  { id: "shout", at: 31, stage: 3, i0: 0.5, i1: 0.8 },
  { id: "powerup", at: 46, stage: 3, i0: 0.82, i1: 0.86 },
  { id: "saiyan", at: 50, stage: 4, i0: 0.86, i1: 0.94 },
  { id: "gibberish", at: 61.5, stage: 5, i0: 0.96, i1: 1 },
  { id: "blast", at: 65.5, stage: 5, i0: 1, i1: 1 },
]
// OpenCode prompts. `issue` is spawned a few seconds before so the prompt has something to fix;
// `lead` is how early the OpenCode window arrives for the first one.
const BEATS = [
  { at: 5.5, lead: 2.5, issueAt: 0.6, line: "p1-linux", agents: 1, fix: 1, version: "2.3.1", issue: ["anomalyco/hex", "#65", "Linux: listener stuck in “stopping” after suspend", "linux"] },
  { at: 13, line: "p1-clip", agents: 2, fix: 2, version: "2.3.2", issue: ["anomalyco/hex", "#168", "⌘V right after dictating pastes the transcript again", "bug"] },
  { at: 21, line: "p2-close", agents: 3, fix: 3, version: "2.3.3" },
  { at: 27, line: "p2-all", agents: 5, fix: 4, version: "2.3.4" },
  { at: 40, line: "p3-every", agents: 10, fix: 6, version: "2.4.0" },
  { at: 53.5, line: "p4-nine", agents: 9001, fix: 12, version: "9.0.0", set: true },
  { at: 57.5, line: "p4-go", agents: 999, fix: 12, version: "9.0.1" },
]
const k80 = () => 1
const arcAt = (id) => ARC.find((a) => a.id === id).at * k80()
function arcNow() {
  let cur = ARC[0]
  for (const a of ARC) if (clock >= a.at * k80()) cur = a
  return cur
}
let clock = 0 // seconds into the current cycle
let I = 0 // smoothed intensity, 0..1
let blastStage = null // null | windup | fire | blast | after (dizzy)
let saiyan = 0 // 0..1, drives renderer.setSaiyan
let powerStage = null // null | charge | on
let beatIndex = 0
function targetIntensity() {
  if (reduced) return 0.06
  if (v.intensity >= 0) return v.intensity
  if (blastStage) return blastStage === "after" ? 0 : 1
  if (!started) return 0
  const a = arcNow()
  const n = ARC[ARC.indexOf(a) + 1]
  const span = n ? (n.at - a.at) * k80() : 1
  return lerp(a.i0, a.i1, Math.pow(clamp((clock - a.at * k80()) / span, 0, 1), v.curve / 1.4))
}
function currentStage() {
  if (v.intensity >= 0) return I > 0.97 ? 5 : I > 0.85 ? 4 : clamp(Math.floor(I * 3.6) + 1, 1, 3)
  return arcNow().stage
}

// ─── Replies ───────────────────────────────────────────────────────────────────
const LINES = { 1: [], 2: [], 3: [], 4: [], 5: [] }
const recentLines = []
// Prefer a line written for this kind of message; otherwise any line from the stage.
function replyFor(stage, kind) {
  const fresh = LINES[stage].filter((l) => !recentLines.includes(l.id))
  const fit = fresh.filter((l) => l.kinds?.includes(kind))
  const general = fresh.filter((l) => !l.kinds?.length)
  const pool = fit.length && rand() < 0.8 ? fit : general.length && rand() < 0.5 ? general : fresh.length ? fresh : LINES[stage]
  const line = pick(pool)
  recentLines.unshift(line.id)
  recentLines.length = Math.min(recentLines.length, 4)
  return takeOf(line)
}
function takeOf(line) {
  line.next ??= 0
  return line.takes[line.next++ % line.takes.length]
}

// ─── State ─────────────────────────────────────────────────────────────────────
let time = 0
const live = [] // cards on screen
const flying = [] // words
const fx = []
const releaseQueue = []
const head = {
  stretch: S(0),
  lx: S(0),
  ly: S(0),
  tilt: S(0),
  mouth: S(0),
  anger: S(0),
  browUp: S(0),
  gx: S(0),
  gy: S(0),
  faceX: S(0),
  faceY: S(0),
  smile: S(1),
  lid: S(0),
  squint: S(0),
  asym: S(0),
  blink: 0,
  nextBlink: 2.2,
  phase: "idle",
  t: 0,
  queue: [],
  target: null,
  dir: { x: 0, y: 1 },
  lastWord: 0,
  mouthW: 1,
  hold: 0,
  voice: null, // { env, start, rate } of the clip driving the mouth
}
let oc = null // the OpenCode window card
let ocBusy = null // the beat being dictated or worked on
const pointer = { x: 0, y: 0, seen: false }
const hitStop = { remaining: 0, scale: 0.04 }
const cam = { trauma: 0, kx: S(0), ky: S(0) }
let spawnAcc = 0
let recentKinds = []
let userRage = false

// Plays a voice clip and lets its amplitude envelope drive the mouth.
function say(take, opts = {}) {
  const rate = opts.rate ?? 1
  audio.voice(take.file, { gain: 1, ...opts, rate, dur: take.dur / rate })
  head.voice = { env: take.env, start: rtime, rate, level: opts.level ?? 1 }
}
// Envelopes are normalized per clip; calm lines shouldn't open the mouth as wide as screams.
const LEVEL = { 1: 0.22, 2: 0.3, 3: 0.8, 4: 0.9, 5: 1 }
function mouthOpen() {
  const h = head.voice
  if (!h?.env) return 0
  const k = Math.floor((rtime - h.start) * h.rate * 50)
  if (k < 0 || k >= h.env.length) return 0
  const a = h.env[k] / 99
  const b = (h.env[k + 1] ?? 0) / 99
  return lerp(a, b, ((rtime - h.start) * h.rate * 50) % 1) * h.level
}

const headPos = () => {
  const b = headBase()
  return { x: b.x + head.lx.x, y: b.y + head.ly.x + bob() }
}
const bob = () => (reduced ? 0 : Math.sin(time * 1.25) * 5 + Math.sin(time * 0.53) * 2)
function mouthPos() {
  const c = headPos()
  const R = headR()
  const sy = 1 + head.stretch.x
  const sx = 1 / Math.pow(sy, 0.7)
  const lx = head.faceX.x * 0.9
  const ly = 0.33 + 0.24 * clamp(head.mouth.x, 0, 1.2) + head.faceY.x * 0.9
  const a = head.tilt.x
  const px = lx * R * sx
  const py = ly * R * sy
  return { x: c.x + px * Math.cos(a) - py * Math.sin(a), y: c.y + px * Math.sin(a) + py * Math.cos(a) }
}

// ─── Cards ─────────────────────────────────────────────────────────────────────
function freeSpot(c, want) {
  const s = cardScale() * (c.kind === "opencode" ? 1.12 : 1)
  const hw = (c.w / 2 + PAD * 0.75) * s
  const hh = (c.h / 2 + PAD * 0.75) * s
  const hp = headBase()
  const R = headR()
  const overlap = 0.45 * smooth(0.35, 0.85, I) // how much crowding is allowed; capped so text stays readable
  for (let i = 0; i < 90; i++) {
    let x
    let y
    if (want && i === 0) ({ x, y } = want)
    else {
      x = rnd(hw + 16, Math.max(hw + 16, W - hw - 16))
      y = rnd(hh + 62, Math.max(hh + 62, H - hh - 20))
    }
    const dx = Math.max(Math.abs(x - hp.x) - hw, 0)
    const dy = Math.max(Math.abs(y - hp.y) - hh, 0)
    const core = R * lerp(1.9, 1.15, overlap)
    if (Math.hypot(dx, dy) < core) continue
    if (overlapsAvoid(x, y, hw, hh)) continue
    let hits = 0
    for (const o of live) {
      if (o.state === "dead") continue
      const ox = Math.max(0, Math.min(x + hw, o.bx + o.hw) - Math.max(x - hw, o.bx - o.hw))
      const oy = Math.max(0, Math.min(y + hh, o.by + o.hh) - Math.max(y - hh, o.by - o.hh))
      if (ox * oy > hw * hh * 4 * overlap * 0.6 + 40) hits++
    }
    if (hits === 0 || (overlap > 0.8 && i > 30) || i > 70) return { x, y }
  }
  if (want) return resolveAvoid(want.x, want.y, hw, hh)
  return null
}

function spawnCard(at, opts = {}) {
  if (!opts.kind && live.filter((c) => c.state !== "dead").length >= v.maxCards) return null
  const kinds = KINDS.filter((k) => !recentKinds.includes(k))
  const kind = opts.kind ?? pick(kinds.length ? kinds : KINDS)
  const c = cards.make(kind, rand, opts.item)
  const s = cardScale() * (kind === "opencode" ? 1.12 : 1)
  const hw = (c.w / 2) * s
  const hh = (c.h / 2) * s
  const rawSpot = freeSpot(c, at) ?? (opts.force && at ? resolveAvoid(at.x, at.y, hw + PAD * 0.75 * s, hh + PAD * 0.75 * s) : null)
  if (!rawSpot) return null
  const spot = resolveAvoid(rawSpot.x, rawSpot.y, hw + PAD * 0.75 * s, hh + PAD * 0.75 * s)
  if (!cards.alloc(c)) return null
  if (!opts.kind) recentKinds = [kind, ...recentKinds].slice(0, 2)
  const cue = { chat: "knock", dm: "knock", mail: "swoosh", messages: "bubble", calendar: "pop", review: "pop", issue: "pop", tweet: "bubble", discord: "knock", opencode: "pop" }[kind]
  audio.sfx(cue, { gain: lerp(0.55, 0.3, I), rate: rnd(0.95, 1.06) * lerp(1, 1.12, I), pan: (spot.x / W) * 1.4 - 0.7 })
  const baseScale = cardScale()
  const fromRight = spot.x > W / 2 ? 1 : -1
  Object.assign(c, {
    x: spot.x,
    y: spot.y,
    bx: spot.x,
    by: spot.y,
    hw: (c.w / 2) * baseScale,
    hh: (c.h / 2) * baseScale,
    base: baseScale,
    pop: S(0),
    ox: S(fromRight * lerp(26, 70, I)),
    oy: S(-lerp(10, 24, I)),
    kx: S(0),
    ky: S(0),
    scale: 0,
    alpha: 0,
    rot: 0,
    flash: 0,
    vx: 0,
    vy: 0,
    state: "wait",
    born: time,
    spin: rnd(-1, 1) * 0.06 * I,
    seed: rand() * 100,
    landed: 0,
    words: 0,
    pinned: !!opts.pinned,
  })
  if (kind === "opencode") c.state = "oc"
  live.push(c)
  return c
}

function cardPoint(c, lx, ly) {
  const px = (lx + PAD - c.sw / 2) * c.scale
  const py = (ly + PAD - c.sh / 2) * c.scale
  const a = c.rot
  return { x: c.x + px * Math.cos(a) - py * Math.sin(a), y: c.y + px * Math.sin(a) + py * Math.cos(a) }
}
const caretOf = (c) => (c.caret ? cardPoint(c, c.caret.x, c.caret.y) : { x: c.x, y: c.y })

function land(c, word) {
  if (c.state !== "typing") return
  c.typed = c.typed ? `${c.typed} ${word.text}` : word.text
  if (c.reply && word.slot != null) {
    // Hot on arrival, cooling to the card's text colour; hotter and slower when furious.
    c.lw[word.slot] = { fade: 0, heat: 1, cool: lerp(0.3, 0.6, word.i), age: 0 }
    const me = c.reply.words[word.slot]
    c.caretTo = { x: me.x + me.w + 2, y: me.y }
    landing.add(c)
  }
  const pan = (word.x / W) * 1.4 - 0.7
  audio.sfx("tick", { gain: lerp(0.5, 0.35, word.i), rate: rnd(0.85, 1.2), pan })
  if (word.i > 0.4) audio.sfx("hit", { bus: "impacts", gain: smooth(0.4, 1, word.i) * 0.8, rate: rnd(0.9, 1.1), pan })
  c.landed++
  c.heat = Math.max(c.heat, smooth(0.35, 0.8, word.i))
  cards.draw(c)
  const d = Math.hypot(word.vx, word.vy) || 1
  // Calm words land softly: a nudge, no sparks.
  const push = word.i < 0.3 ? 18 : lerp(40, 520, word.i)
  c.kx.v += (word.vx / d) * push
  c.ky.v += (word.vy / d) * push
  c.flash = smooth(0.4, 0.9, word.i)
  if (word.i >= 0.3) chip(word.x, word.y, word.vx / d, word.vy / d, Math.round(lerp(3, 14, word.i) * v.fx), word.i)
  if (word.i > 0.45) {
    hitStop.remaining = Math.max(hitStop.remaining, 0.018 * word.i)
    cam.trauma = Math.min(1, cam.trauma + 0.07 * word.i * word.i * v.shake)
  }
  if (c.landed >= c.words) finish(c, word)
}

function finish(c, word) {
  c.typing = false
  if (c.kind === "opencode") return submitPrompt(c)
  if (c.hold) {
    Object.assign(c, { state: "wait", pinned: true, hold: false })
    cards.draw(c)
    return
  }
  cards.draw(c)
  const blast = rand() < smooth(0.42, 0.62, word.i)
  if (blast) {
    c.state = "dying"
    c.dieAt = time + 0.12 + lerp(0, v.hitStop / 1000, smooth(0.5, 1, word.i))
    c.hitDir = { x: word.vx, y: word.vy }
    c.flash = 1
    hitStop.remaining = Math.max(hitStop.remaining, lerp(0.02, v.hitStop / 1000, smooth(0.5, 1, word.i)))
    audio.sfx("thump", { bus: "impacts", gain: smooth(0.5, 1, word.i) * 0.9, rate: rnd(0.85, 1) })
  } else {
    c.state = "sent"
    setTimeout0(0.3, () => audio.sfx("send", { gain: 0.45, rate: rnd(0.95, 1.08), pan: (c.x / W) * 1.4 - 0.7 }))
    c.sentAt = time + 0.35
  }
}

function retire(c) {
  c.state = "dead"
  releaseQueue.push({ c, at: time + 1.6 })
}

let lastShatter = -1
function explode(c, power = 1) {
  if (c.state === "dead") return
  retire(c)
  // Round-robin-ish variation; at the wipe many cards explode at once, so thin them out.
  if (rtime - lastShatter > 0.05) {
    lastShatter = rtime
    const pan = (c.x / W) * 1.4 - 0.7
    audio.sfx("shatter", { bus: "impacts", gain: lerp(0.45, 0.85, I), rate: rnd(0.88, 1.12), pan })
    audio.sfx("boom", { bus: "impacts", gain: lerp(0.2, 0.6, I) * power, rate: rnd(0.9, 1.15), pan })
  }
  const dir0 = c.hitDir ?? { x: c.x - headPos().x, y: c.y - headPos().y }
  const dl = Math.hypot(dir0.x, dir0.y) || 1
  const dir = { x: dir0.x / dl, y: dir0.y / dl }
  const ew = c.sw
  const eh = c.sh
  const cols = Math.round(clamp(6 + 3 * I, 5, 10))
  const rows = Math.max(2, Math.round((cols * eh) / ew) + 1)
  const pts = []
  for (let j = 0; j <= rows; j++) {
    for (let i = 0; i <= cols; i++) {
      const edge = i === 0 || j === 0 || i === cols || j === rows
      pts.push([((i + (edge ? 0 : rnd(-0.38, 0.38))) / cols - 0.5) * ew, ((j + (edge ? 0 : rnd(-0.38, 0.38))) / rows - 0.5) * eh])
    }
  }
  const at = (i, j) => pts[j * (cols + 1) + i]
  const list = []
  const half = [ew / 2, eh / 2]
  const speed = lerp(380, 900, I) * power * v.fx
  const sc = c.scale
  for (let j = 0; j < rows; j++) {
    for (let i = 0; i < cols; i++) {
      const a = at(i, j)
      const b = at(i + 1, j)
      const cc = at(i + 1, j + 1)
      const d = at(i, j + 1)
      const tris = rand() < 0.5 ? [[a, b, cc], [a, cc, d]] : [[a, b, d], [b, cc, d]]
      for (const t of tris) {
        const cx = (t[0][0] + t[1][0] + t[2][0]) / 3
        const cy = (t[0][1] + t[1][1] + t[2][1]) / 3
        const rx = cx / half[0]
        const ry = cy / half[1]
        const rl = Math.hypot(rx, ry) + 0.25
        const sp = speed * rnd(0.55, 1.25)
        const ox = (rx / rl) * 0.85 + dir.x * 0.8 + rnd(-0.25, 0.25)
        const oy = (ry / rl) * 0.85 + dir.y * 0.8 + rnd(-0.25, 0.25) - 0.1
        const p = cardPoint(c, cx + c.sw / 2 - PAD, cy + c.sh / 2 - PAD)
        list.push({
          x: p.x,
          y: p.y,
          vx: ox * sp,
          vy: oy * sp,
          ang: c.rot,
          spin: rnd(-12, 12),
          life: rnd(0.55, 1.1),
          v0: [(t[0][0] - cx) * sc, (t[0][1] - cy) * sc],
          v1: [(t[1][0] - cx) * sc, (t[1][1] - cy) * sc],
          v2: [(t[2][0] - cx) * sc, (t[2][1] - cy) * sc],
          heat: 1,
          base: [cx, cy],
          half,
          uv: c.uv,
          entry: { px: c.px, py: c.py, w: c.sw, h: c.sh },
        })
      }
    }
  }
  renderer.spawnShards(list)
  const sparks = []
  const n = Math.round(lerp(18, 60, I) * v.fx * power)
  for (let i = 0; i < n; i++) {
    const a = rand() * Math.PI * 2
    const s = rnd(500, 1800)
    sparks.push({ x: c.x, y: c.y, vx: Math.cos(a) * s + dir.x * 400, vy: Math.sin(a) * s + dir.y * 400, life: rnd(0.18, 0.5), heat: 1, size: rnd(1.4, 2.6) })
  }
  for (let i = 0; i < 10; i++) {
    const a = rand() * Math.PI * 2
    const s = rnd(40, 200)
    sparks.push({ x: c.x + rnd(-40, 40), y: c.y + rnd(-14, 14), vx: Math.cos(a) * s, vy: Math.sin(a) * s - 40, life: rnd(0.8, 1.5), heat: 0, size: rnd(1.2, 2) })
  }
  const size = Math.max(c.hw, c.hh) * 2
  const pyro = smooth(0.32, 0.92, Math.max(I, blastStage ? 1 : 0)) * power
  // Buoyant rising embers at higher intensity
  if (pyro > 0.1) {
    const emberCount = Math.round(lerp(8, 28, pyro) * v.fx)
    for (let i = 0; i < emberCount; i++) {
      const a = rand() * Math.PI * 2
      const s = rnd(60, 260)
      sparks.push({
        x: c.x + rnd(-c.hw * 0.65, c.hw * 0.65),
        y: c.y + rnd(-c.hh * 0.45, c.hh * 0.45),
        vx: Math.cos(a) * s + dir.x * 110,
        vy: -rnd(120, 360) + Math.sin(a) * 60,
        life: rnd(0.65, 1.45),
        heat: rnd(0.75, 1.0),
        size: rnd(1.6, 3.0),
      })
    }
  }
  renderer.spawnSparks(sparks)

  fx.push({ kind: 0, x: c.x, y: c.y, r: 0, rMax: (size * 0.78 + 68) * v.fx * lerp(0.85, 1.4, I), width: lerp(2.2, 3.6, pyro), t: 0, k: 1.05, age: 0, life: 0.44 })
  fx.push({ kind: 1, x: c.x, y: c.y, r: (size * 0.42 + 30) * v.fx, t: 0, k: 1.2, age: 0, life: 0.1 })

  // Volumetric noise-driven fireballs (kind 8), rising smoke plumes (kind 9), scorch/pressure wave (kind 10),
  // and dynamic point lighting on nearby cards
  if (pyro > 0.08 && !reduced) {
    // Scorch heat glow & double shockwave pressure ring
    fx.push({
      kind: 10,
      x: c.x,
      y: c.y,
      r: (size * 0.85 + 75) * lerp(0.8, 1.35, pyro) * v.fx,
      t: 0,
      k: lerp(0.65, 1.25, pyro),
      seed: rnd(1, 50),
      age: 0,
      life: lerp(0.32, 0.52, pyro),
    })

    // Primary + secondary volumetric fireballs expanding and cooling into charcoal smoke
    const lobeCount = pyro > 0.55 ? 3 : 2
    for (let li = 0; li < lobeCount; li++) {
      const ox = li === 0 ? 0 : rnd(-c.hw * 0.45, c.hw * 0.45)
      const oy = li === 0 ? 0 : rnd(-c.hh * 0.35, c.hh * 0.35)
      const r0 = (li === 0 ? size * 0.32 : size * 0.24) * lerp(0.85, 1.35, pyro)
      fx.push({
        kind: 8,
        x: c.x + ox,
        y: c.y + oy,
        vx: dir.x * rnd(25, 70) + ox * 0.6,
        vy: -rnd(28, 65) + dir.y * 25,
        r: r0,
        r0,
        rMax: r0 * lerp(1.85, 2.35, pyro),
        t: 0,
        k: lerp(0.95, 1.45, pyro) * (li === 0 ? 1 : 0.85),
        seed: rnd(1, 99),
        age: -li * 0.025,
        life: lerp(0.58, 0.92, pyro),
      })
    }

    // Lingering rising smoke plumes that drift upward after the fireball
    const smokeCount = pyro > 0.6 ? 3 : 2
    for (let si = 0; si < smokeCount; si++) {
      const sx = c.x + rnd(-c.hw * 0.5, c.hw * 0.5)
      const sy = c.y + rnd(-c.hh * 0.3, c.hh * 0.3)
      const sr0 = size * rnd(0.26, 0.38) * lerp(0.85, 1.3, pyro)
      fx.push({
        kind: 9,
        x: sx,
        y: sy,
        vx: rnd(-28, 28) + dir.x * 18,
        vy: -rnd(52, 98),
        r: sr0,
        r0: sr0,
        rMax: sr0 * rnd(1.75, 2.25),
        t: 0,
        k: lerp(0.75, 1.15, pyro),
        seed: rnd(1, 99),
        age: -0.05 - si * 0.04,
        life: rnd(1.15, 1.85),
      })
    }

    addBlastLight(c.x, c.y, (size * 1.65 + 160) * lerp(0.85, 1.35, pyro), lerp(0.9, 1.55, pyro), 0.38)
  }

  cam.trauma = Math.min(1, cam.trauma + 0.32 * lerp(0.5, 1.15, I) * v.shake * power)
  cam.kx.v += dir.x * 280 * v.shake * power
  cam.ky.v += dir.y * 280 * v.shake * power
}

// Dynamic point lights cast by explosions onto nearby cards and the head
const blastLights = []
function addBlastLight(x, y, r, k, life = 0.36) {
  blastLights.push({ x, y, r, k, k0: k, age: 0, life })
}
function updateBlastLights(dt) {
  for (let i = blastLights.length - 1; i >= 0; i--) {
    const l = blastLights[i]
    l.age += dt
    if (l.age >= l.life) {
      blastLights.splice(i, 1)
      continue
    }
    const t = l.age / l.life
    l.k = l.k0 * Math.pow(1 - t, 1.85)
  }
  blastLights.sort((a, b) => b.k - a.k)
}

// ─── Keycap Press & Tactile Explosion ──────────────────────────────────────────
let keycapExploded = false
function pressKey(down, rect) {
  if (reduced || started) return
  if (down) {
    head.stretch.v -= 1.55
    head.ly.v += 28
    head.browUp.v += 2.2
    if (rect) {
      const r = root.getBoundingClientRect()
      aimAt(rect.left - r.left + rect.width / 2, rect.top - r.top + rect.height / 2)
    }
  } else {
    head.stretch.v += 1.1
  }
}

function explodeKeycap(rect) {
  if (keycapExploded || reduced) return
  keycapExploded = true
  root.setAttribute("data-key-exploded", "")

  const r = root.getBoundingClientRect()
  const hp = headPos()
  const kx = rect && rect.width > 0 ? rect.left - r.left + rect.width / 2 : hp.x
  const ky = rect && rect.height > 0 ? rect.top - r.top + rect.height / 2 : hp.y + headR() * 1.55
  const kw = rect && rect.width > 0 ? rect.width : 124
  const kh = rect && rect.height > 0 ? rect.height : 102

  // 1. Textured keycap atlas slot fractured into Voronoi chunks (cap face with ⌥ + "option", bevels, amber LED)
  const kc = cards.makeKeycap()
  if (kc) {
    const cols = 6
    const rows = 4
    const ew = kc.sw
    const eh = kc.sh
    const half = [ew / 2, eh / 2]
    const scX = kw / kc.w
    const scY = kh / kc.h
    const sc = (scX + scY) * 0.5
    const pts = []
    for (let j = 0; j <= rows; j++) {
      for (let i = 0; i <= cols; i++) {
        const edge = i === 0 || j === 0 || i === cols || j === rows
        pts.push([
          ((i + (edge ? 0 : rnd(-0.36, 0.36))) / cols - 0.5) * ew,
          ((j + (edge ? 0 : rnd(-0.36, 0.36))) / rows - 0.5) * eh,
        ])
      }
    }
    const at = (i, j) => pts[j * (cols + 1) + i]
    const shards = []
    for (let j = 0; j < rows; j++) {
      for (let i = 0; i < cols; i++) {
        const a = at(i, j)
        const b = at(i + 1, j)
        const cc = at(i + 1, j + 1)
        const d = at(i, j + 1)
        const tris = rand() < 0.5 ? [[a, b, cc], [a, cc, d]] : [[a, b, d], [b, cc, d]]
        for (const t of tris) {
          const cx = (t[0][0] + t[1][0] + t[2][0]) / 3
          const cy = (t[0][1] + t[1][1] + t[2][1]) / 3
          const rx = cx / half[0]
          const ry = cy / half[1]
          const rl = Math.hypot(rx, ry) + 0.18
          const sp = rnd(280, 660)
          const vx = (rx / rl) * sp * 0.95 + rnd(-85, 85)
          const vy = (ry / rl) * sp * 0.65 - rnd(240, 520) // pop up, then heavy gravity pulls off bottom of screen
          shards.push({
            x: kx + cx * sc,
            y: ky + cy * sc,
            vx,
            vy,
            ang: rnd(-0.18, 0.18),
            spin: (rx >= 0 ? 1 : -1) * rnd(6, 16),
            life: rnd(1.5, 2.3),
            v0: [(t[0][0] - cx) * sc * 1.08, (t[0][1] - cy) * sc * 1.08],
            v1: [(t[1][0] - cx) * sc * 1.08, (t[1][1] - cy) * sc * 1.08],
            v2: [(t[2][0] - cx) * sc * 1.08, (t[2][1] - cy) * sc * 1.08],
            heat: 1.0,
            base: [cx, cy],
            // Negative half[1] signals keycap shard in SHARD_SIM / SHARDS (amber LED rim + falls off bottom of screen)
            half: [half[0], -half[1]],
            uv: kc.uv,
            entry: { px: kc.px, py: kc.py, w: kc.sw, h: kc.sh },
          })
        }
      }
    }
    renderer.spawnShards(shards)
    releaseQueue.push({ c: kc, at: time + 2.5 })
  }

  // 2. Amber switch-LED sparks, polycarbonate chips, volumetric fireball, smoke plume, and shockwave ring
  const sparks = []
  for (let i = 0; i < 68; i++) {
    const a = rand() * Math.PI * 2
    const s = rnd(180, 1250)
    sparks.push({
      x: kx + rnd(-kw * 0.28, kw * 0.28),
      y: ky + rnd(-kh * 0.22, kh * 0.22),
      vx: Math.cos(a) * s,
      vy: Math.sin(a) * s - rnd(90, 280),
      life: rnd(0.26, 0.85),
      heat: rnd(0.75, 1.0),
      size: rnd(1.5, 3.1),
    })
  }
  renderer.spawnSparks(sparks)

  // Crisp shockwave ring + scorch wave + amber fireball + rising smoke plume
  fx.push({ kind: 0, x: kx, y: ky, r: 0, rMax: 240 * v.fx, width: 3.4, t: 0, k: 1.25, age: 0, life: 0.42 })
  fx.push({ kind: 10, x: kx, y: ky, r: 195 * v.fx, t: 0, k: 1.15, seed: 17.3, age: 0, life: 0.36 })
  fx.push({ kind: 1, x: kx, y: ky, r: 96 * v.fx, t: 0, k: 1.45, age: 0, life: 0.11 })
  fx.push({
    kind: 8,
    x: kx,
    y: ky,
    vx: 0,
    vy: -42,
    r: 54,
    r0: 54,
    rMax: 128,
    t: 0,
    k: 1.38,
    seed: 31.7,
    age: 0,
    life: 0.62,
  })
  fx.push({
    kind: 9,
    x: kx,
    y: ky - 8,
    vx: rnd(-18, 18),
    vy: -76,
    r: 48,
    r0: 48,
    rMax: 112,
    t: 0,
    k: 0.92,
    seed: 53.1,
    age: -0.04,
    life: 1.28,
  })
  addBlastLight(kx, ky, 360, 1.45, 0.42)

  // 3. Brief hit-stop, camera punch, and head flinch / perk-up reaction
  hitStop.remaining = Math.max(hitStop.remaining, 0.036)
  cam.trauma = Math.min(1, cam.trauma + 0.42 * v.shake)
  cam.ky.v -= 240 * v.shake
  head.stretch.v += 4.6
  head.ly.v -= 75
  head.browUp.v += 4.2
  head.blink = 1
}

// ─── Title ("Speak. It appears.") Physics Destruction & Recovery ───────────────
function joltTitleWords() {
  if (reduced) return
  measureAvoid()
  for (const w of titleWords) {
    if (w.state !== "rest") continue
    w.state = "jolted"
    const side = w.idx === 0 ? -1 : w.idx === 2 ? 1 : 0.35
    w.vx = side * rnd(24, 52)
    w.vy = -rnd(42, 78)
    w.vrot = side * rnd(0.28, 0.55)
    w.targetRot = side * rnd(0.028, 0.052)
    w.heat = 0.65
    chip(w.rest.cx, w.rest.cy, side * 0.5, -1, 14, 0.9)
  }
}

function knockTitleWord(idx) {
  if (reduced) return
  const w = titleWords[idx]
  if (!w || w.state === "falling") return
  measureAvoid()
  const cx = w.rest.cx + w.x
  const cy = w.rest.cy + w.y
  w.state = "falling"
  w.bounces = 0
  w.heat = 1.0
  const dirX = idx === 0 ? -1 : idx === 2 ? 1 : rnd(-0.45, 0.45)
  w.vx = dirX * rnd(105, 220)
  w.vy = -rnd(260, 410)
  w.vrot = (dirX >= 0 ? 1 : -1) * rnd(2.2, 4.6)

  // Volumetric fireball + rising smoke plume + shockwave at the word detachment point
  fx.push({ kind: 0, x: cx, y: cy, r: 0, rMax: 190 * v.fx, width: 3.0, t: 0, k: 1.15, age: 0, life: 0.4 })
  fx.push({ kind: 10, x: cx, y: cy, r: 155 * v.fx, t: 0, k: 1.05, seed: 19 + idx * 13, age: 0, life: 0.35 })
  fx.push({ kind: 1, x: cx, y: cy, r: 78 * v.fx, t: 0, k: 1.25, age: 0, life: 0.09 })
  fx.push({
    kind: 8,
    x: cx,
    y: cy,
    vx: dirX * 30,
    vy: -45,
    r: 46,
    r0: 46,
    rMax: 108,
    t: 0,
    k: 1.32,
    seed: 41 + idx * 17,
    age: 0,
    life: 0.64,
  })
  fx.push({
    kind: 9,
    x: cx,
    y: cy - 6,
    vx: dirX * 18,
    vy: -68,
    r: 44,
    r0: 44,
    rMax: 98,
    t: 0,
    k: 0.9,
    seed: 73 + idx * 11,
    age: -0.04,
    life: 1.35,
  })
  addBlastLight(cx, cy, 310, 1.25, 0.35)
  chip(cx, cy, dirX, -0.8, 32, 1.0)
  audio.sfx("shatter", { bus: "impacts", gain: 0.65, rate: rnd(0.92, 1.08), pan: (cx / W) * 1.2 - 0.6 })
  cam.trauma = Math.min(1, cam.trauma + 0.34 * v.shake)
}

function returnTitleWords() {
  for (const w of titleWords) {
    if (w.state === "rest") continue
    w.state = "returning"
    w.returnAge = 0
    w.returnDelay = w.idx * 0.09
    w.fromX = w.x
    w.fromY = w.y
    w.fromRot = ((w.rot + Math.PI) % (Math.PI * 2)) - Math.PI
  }
}

function updateTitleWords(dt) {
  if (reduced) return
  const floorY = Math.max(200, H - 26)
  for (const w of titleWords) {
    if (w.state === "rest") continue
    w.heat = Math.max(0, w.heat - dt * 1.1)

    if (w.state === "jolted") {
      // Damped spring around a slightly cracked/tilted offset until the finale knocks it loose
      w.vx += (-180 * w.x - 14 * w.vx) * dt
      w.vy += (-180 * (w.y - 3) - 14 * w.vy) * dt
      w.vrot += (-160 * (w.rot - w.targetRot) - 12 * w.vrot) * dt
      w.x += w.vx * dt
      w.y += w.vy * dt
      w.rot += w.vrot * dt
    } else if (w.state === "falling") {
      w.vx *= Math.exp(-0.45 * dt)
      w.vy += 1480 * dt
      w.x += w.vx * dt
      w.y += w.vy * dt
      w.rot += w.vrot * dt

      // Keep horizontally inside the viewport
      const worldX = w.rest.cx + w.x
      const minX = w.rest.w * 0.55 + 16
      const maxX = W - w.rest.w * 0.55 - 16
      if (worldX < minX) {
        w.x = minX - w.rest.cx
        w.vx = Math.abs(w.vx) * 0.6
      } else if (worldX > maxX) {
        w.x = maxX - w.rest.cx
        w.vx = -Math.abs(w.vx) * 0.6
      }

      // Floor collision with thud, bounce, and dust/smoke puffs
      const maxDrop = Math.max(60, floorY - w.rest.bottom)
      if (w.y >= maxDrop && w.vy > 0) {
        w.y = maxDrop
        const impactVy = w.vy
        w.bounces++
        if (impactVy > 110 && w.bounces <= 3) {
          w.vy = -impactVy * (w.bounces === 1 ? 0.36 : 0.22)
          w.vx *= 0.72
          w.vrot = w.vrot * 0.55 + rnd(-1.1, 1.1)
          const gx = w.rest.cx + w.x
          const gy = floorY - 6
          const pan = (gx / W) * 1.2 - 0.6
          if (w.bounces === 1) {
            audio.sfx("thump", { bus: "impacts", gain: 0.72, rate: rnd(0.88, 1.04), pan })
            cam.trauma = Math.min(1, cam.trauma + 0.24 * v.shake)
            cam.ky.v += 180 * v.shake
          }
          // Floor dust & smoke puffs expanding sideways along the ground
          for (const side of [-1, 1]) {
            fx.push({
              kind: 9,
              x: gx + side * (w.rest.w * 0.28),
              y: gy,
              vx: side * rnd(45, 95),
              vy: -rnd(24, 52),
              r: w.bounces === 1 ? 34 : 22,
              r0: w.bounces === 1 ? 34 : 22,
              rMax: w.bounces === 1 ? 76 : 46,
              t: 0,
              k: w.bounces === 1 ? 0.88 : 0.52,
              seed: rnd(1, 99),
              age: 0,
              life: w.bounces === 1 ? 0.95 : 0.6,
            })
          }
          fx.push({ kind: 0, x: gx, y: gy, r: 0, rMax: (w.bounces === 1 ? 110 : 60) * v.fx, width: 1.8, t: 0, k: 0.55, age: 0, life: 0.28 })
          chip(gx, gy, 0, -1, w.bounces === 1 ? 16 : 6, 0.55)
        } else {
          w.vy = 0
          w.vx *= 0.8
          w.vrot *= 0.75
        }
      }
    } else if (w.state === "returning") {
      w.returnAge += dt
      const t = clamp((w.returnAge - w.returnDelay) / 0.82, 0, 1)
      // Smooth spring ease-out-back rise into the h1 slot
      const c1 = 1.55
      const c3 = c1 + 1
      const ease = t <= 0 ? 0 : t >= 1 ? 1 : 1 + c3 * Math.pow(t - 1, 3) + c1 * Math.pow(t - 1, 2)
      w.x = lerp(w.fromX, 0, ease)
      w.y = lerp(w.fromY, 0, ease)
      w.rot = lerp(w.fromRot, 0, ease)
      if (t >= 1) {
        w.state = "rest"
        w.x = w.y = w.vx = w.vy = w.rot = w.vrot = w.heat = 0
      }
    }

    applyTitleWordStyle(w)
  }
}

function chip(x, y, nx, ny, n, heat) {
  const list = []
  for (let i = 0; i < n; i++) {
    const a = Math.atan2(-ny, -nx) + rnd(-1.1, 1.1)
    const s = rnd(200, 800) * lerp(0.5, 1.2, heat)
    list.push({ x, y, vx: Math.cos(a) * s, vy: Math.sin(a) * s, life: rnd(0.12, 0.3), heat: lerp(0.15, 0.9, heat), size: rnd(1.1, 2) })
  }
  renderer.spawnSparks(list)
}

// ─── Words ─────────────────────────────────────────────────────────────────────
let fanSide = 1
function launch(text, target, opts = {}) {
  const m = mouthPos()
  const i = opts.i ?? I
  const size = (opts.size ?? lerp(17, 66, Math.pow(i, 1.2))) * v.wordScale
  const slotted = target?.reply && opts.slot != null && target.reply.words[opts.slot]
  // A word headed for a typeset slot is drawn in the reply's font so the handoff matches.
  const weight = slotted ? target.reply.weight : (opts.weight ?? Math.round(lerp(520, 820, i)))
  const entry = words.get(text, size, weight)
  const goal = slotted ? slotPoint(target, opts.slot, entry) : target ? caretOf(target) : opts.point
  const dist = Math.hypot(goal.x - m.x, goal.y - m.y)
  fanSide = -fanSide
  const base = Math.atan2(goal.y - m.y, goal.x - m.x)
  flying.push({
    text,
    entry,
    k: size / entry.bucket,
    land: target ? (target.loud ? 14 : 12.5) / size : 1,
    x: m.x,
    y: m.y + 4,
    sx: m.x,
    sy: m.y + 4,
    bend: fanSide * lerp(0.12, 0.3, i) * rnd(0.5, 1),
    dur: clamp(dist / lerp(800, 2600, i), 0.16, 0.9) * (slotted ? 1.25 : 1),
    slot: slotted ? opts.slot : null,
    back: slotted ? Math.cbrt((27 * lerp(2, 4, i)) / (4 * Math.max(dist, 60))) : 0,
    vx: Math.cos(base) * 1000,
    vy: Math.sin(base) * 1000,
    ax: 0,
    ay: 0,
    angle: 0,
    age: 0,
    alpha: 1,
    heat: lerp(0.1, 1, i),
    heat0: lerp(0.1, 1, i),
    i,
    scale: 0.3,
    target,
    point: opts.point,
    gx: goal.x,
    gy: goal.y,
    onPop: opts.onPop,
    quiet: opts.quiet,
  })
  if (!opts.quiet) fx.push({ kind: 3, x: m.x, y: m.y, r: headR() * lerp(0.3, 0.7, i), t: 0, k: lerp(0.08, 0.7, i), age: 0, life: 0.1 })
  head.mouth.v += i < 0.3 ? 1 : lerp(3, 10, i)
  const recoil = lerp(10, 80, i)
  head.lx.v -= Math.cos(base) * recoil
  head.ly.v -= Math.sin(base) * recoil
  head.stretch.v += lerp(0.3, 1.6, i)
}

function updateWords(dt) {
  for (let n = flying.length - 1; n >= 0; n--) {
    const o = flying[n]
    o.age += dt
    if (o.settle != null) {
      settleWord(o, n, dt)
      continue
    }
    const t = o.target && o.target.state === "typing" ? o.target : null
    if (t) {
      const g = o.slot != null ? slotPoint(t, o.slot, o.entry) : caretOf(t)
      o.gx = g.x
      o.gy = g.y
    }
    const u = clamp(o.age / o.dur, 0, 1)
    // Slotted words decelerate into place with a few px of overshoot (ease-out-back, zero speed at
    // arrival); free words keep the old accelerating arc into their pop.
    const x1 = 1 - u
    const s = o.slot != null ? 1 - (o.back + 1) * x1 * x1 * x1 + o.back * x1 * x1 : u + 0.35 * (u * u - u)
    const dx = o.gx - o.sx
    const dy = o.gy - o.sy
    const cx = o.sx + dx * 0.5 - dy * o.bend
    const cy = o.sy + dy * 0.5 + dx * o.bend
    const nx = (1 - s) * (1 - s) * o.sx + 2 * (1 - s) * s * cx + s * s * o.gx
    const ny = (1 - s) * (1 - s) * o.sy + 2 * (1 - s) * s * cy + s * s * o.gy
    if (dt > 0) {
      const vx = (nx - o.x) / dt
      const vy = (ny - o.y) / dt
      if (o.age > dt * 1.5) {
        const ax = (vx - o.vx) / dt
        const ay = (vy - o.vy) / dt
        const al = Math.hypot(ax, ay)
        const k = al > 60000 ? 60000 / al : 1
        o.ax = ax * k
        o.ay = ay * k
      }
      o.vx = vx
      o.vy = vy
    }
    o.x = nx
    o.y = ny
    // Grow out of the mouth, then shrink into the reply field as it arrives.
    const grow = Math.min(1, o.age * 10)
    let ang = Math.atan2(o.vy, o.vx)
    if (ang > Math.PI / 2) ang -= Math.PI
    if (ang < -Math.PI / 2) ang += Math.PI
    if (o.slot != null && t) {
      // Continuously becomes its typeset self: size, angle, and heat all converge on arrival.
      const e = smooth(0.15, 0.85, u)
      o.scale = lerp(0.3, 1, grow) * lerp(1, slotScale(t, o), e)
      o.angle = lerp(ang * 0.25, t.rot, e)
      o.heat = lerp(o.heat0, Math.max(o.heat0, 0.9), e)
    } else {
      o.scale = lerp(0.3, 1, grow) * lerp(1, o.land, Math.pow(s, 4))
      o.angle = ang * 0.25 * (1 - s)
    }
    if (u >= 1 && o.slot != null && t) {
      land(t, o)
      o.settle = 0
      continue
    }
    if (u >= 1) {
      flying.splice(n, 1)
      if (t) land(t, o)
      else pop(o)
    }
  }
}

// Screen-space centre of a word's sprite when its text sits exactly in its typeset slot.
function slotScale(c, o) {
  return (c.reply.size * c.scale) / (o.entry.bucket * o.k)
}
function slotPoint(c, slot, entry) {
  const w = c.reply.words[slot]
  const p = cardPoint(c, w.x, w.y)
  const k = (c.reply.size * c.scale) / entry.bucket
  const ox = -entry.ox * k
  const oy = -entry.oy * k
  return { x: p.x + ox * Math.cos(c.rot) - oy * Math.sin(c.rot), y: p.y + ox * Math.sin(c.rot) + oy * Math.cos(c.rot) }
}

// Handoff: the sprite stays pinned to its slot while the card's own text fades in under it.
const HANDOFF = 0.12
function settleWord(o, n, dt) {
  o.settle += dt
  const c = o.target
  if (!c || c.state === "dead" || !c.reply) {
    flying.splice(n, 1)
    return
  }
  const g = slotPoint(c, o.slot, o.entry)
  o.vx = o.vy = o.ax = o.ay = 0
  o.x = g.x
  o.y = g.y
  o.angle = c.rot
  o.scale = slotScale(c, o)
  const k = clamp(o.settle / HANDOFF, 0, 1)
  o.alpha = 1 - k
  const st = c.lw[o.slot]
  if (st) st.fade = k
  if (k >= 1) flying.splice(n, 1)
}

// Landed words cool down and the caret glides to the next slot.
const landing = new Set()
function updateLanding(dt) {
  for (const c of landing) {
    if (c.state === "dead" || !c.reply) {
      landing.delete(c)
      continue
    }
    let busy = false
    for (const st of c.lw) {
      if (!st) continue
      st.age += dt
      const h = st.fade >= 1 ? Math.max(0, 1 - Math.max(0, st.age - HANDOFF) / st.cool) : 1
      st.heat = h * h
      if (st.heat > 0 || st.fade < 1) busy = true
    }
    const a = 1 - Math.exp(-dt * 45)
    if (c.caretTo) {
      if (Math.abs(c.caretTo.y - c.caretY) > 1) c.caretY = c.caretTo.y
      c.caretX += (c.caretTo.x - c.caretX) * a
      if (Math.abs(c.caretTo.x - c.caretX) > 0.3) busy = true
    }
    c.now = time
    cards.draw(c)
    if (!busy) landing.delete(c)
  }
}

function pop(o) {
  if (o.quiet) return
  const d = Math.hypot(o.vx, o.vy) || 1
  const big = o.entry.bucket >= 80
  chip(o.x, o.y, o.vx / d, o.vy / d, big ? 40 : 10, 1)
  fx.push({ kind: 0, x: o.x, y: o.y, r: 0, rMax: (big ? 220 : 50) * v.fx, width: big ? 3 : 1.6, t: 0, k: 0.9, age: 0, life: big ? 0.5 : 0.3 })
  fx.push({ kind: 1, x: o.x, y: o.y, r: (big ? 90 : 22) * v.fx, t: 0, k: 1, age: 0, life: big ? 0.12 : 0.06 })
  o.onPop?.()
}

// ─── Head performance ──────────────────────────────────────────────────────────
function aimAt(x, y) {
  const c = headBase()
  const dx = x - c.x
  const dy = y - c.y
  const d = Math.hypot(dx, dy) || 1
  head.dir = { x: dx / d, y: dy / d }
}

const waiting = (c) => c.state === "wait" && !c.pinned && c.kind !== "opencode"
function nextTarget() {
  let best = null
  for (const c of live) if (waiting(c) && time - c.born > lerp(0.5, 0.12, I) && (!best || c.born < best.born)) best = c
  return best
}

// Words in flight with their targets; Saiyan and gibberish lines spray one word per message.
function startLine(c, take, stage, opts = {}) {
  const rate = 1 // fast stages are natively fast clips; never pitch-shift
  let targets = [c]
  if (stage >= 4 && !opts.single) {
    targets = live.filter((o) => waiting(o) && o !== c).sort((x, y) => x.born - y.born)
    targets = [c, ...targets].slice(0, take.words.length)
  }
  for (const t of targets) {
    t.state = "typing"
    t.typing = true
    t.loud = stage >= 3
    t.heat = smooth(0.35, 0.8, I)
    t.words = 0
    t.landed = 0
    t.typed = t.kind === "opencode" ? "" : t.typed
  }
  head.queue = take.words.map((w, k) => {
    const target = targets[Math.min(k, targets.length - 1)]
    target.words++
    return { text: w.text, at: w.t / rate, target, slot: target.words - 1 }
  })
  for (const t of targets) {
    const texts = head.queue.filter((q) => q.target === t).map((q) => q.text)
    t.reply = null
    if (t.layout === "opencode" || t.layout === "messages" || t.fieldGeom) cards.planReply(t, texts, t.loud)
    cards.draw(t)
  }
  head.target = c
  head.take = take
  head.rate = rate
  head.stage = stage
  head.phase = "look"
  head.t = 0
  if (I < 0.3 && rand() < 0.5) head.blink = 1
}

function startReply(c) {
  const stage = currentStage()
  startLine(c, replyFor(stage, c.kind), stage)
}

function startPrompt(beat) {
  const line = manifest.voice[beat.line]
  ocBusy = beat
  oc.status = ""
  oc.statusDone = false
  startLine(oc, takeOf(line), Number(line.stage), { single: true })
}

function updateHead(dt) {
  head.t += dt
  const i = I
  let stretch = 0
  let lean = 0
  let mouth = 0
  let browUp = (1 - smooth(0.1, 0.3, i)) * 0.5
  let anger = smooth(0.28, 0.85, i)
  let squint = smooth(0.5, 0.8, i)
  let lid = 0.5 * smooth(0.2, 0.38, i) * (1 - smooth(0.55, 0.72, i))
  let asym = smooth(0.22, 0.34, i) * (1 - smooth(0.46, 0.6, i))
  let smile = lerp(1, -1, smooth(0.15, 0.42, i))
  let mouthW = 1
  const tremble = (Math.pow(i, 3) * 4 + (head.phase === "speak" ? i * 2 : 0)) * (reduced ? 0 : 1)

  const tgt = head.target && head.target.state !== "dead" ? head.target : null
  if (blastStage === "windup" || powerStage === "charge") aimAt(W / 2, 0)
  else if (blastStage === "fire") aimAt(headBase().x + Math.cos(finale.angle) * 300, headBase().y + Math.sin(finale.angle) * 300)
  else if (blastStage === "stare" || blastStage === "cracking" || blastStage === "shattered" || blastStage === "rebuilding") aimAt(headBase().x, headBase().y + 18)
  else if (head.glance && time < head.glance.until && head.phase !== "speak") aimAt(head.glance.c.x, head.glance.c.y)
  else if (tgt) aimAt(tgt.x, tgt.y)
  else if (pointer.seen) aimAt(pointer.x, pointer.y)
  else aimAt(headBase().x + Math.sin(time * 0.31) * 300, headBase().y + Math.sin(time * 0.23) * 120 + 60)

  const ph = head.phase
  if (blastStage) {
    // handled by the blast sequence below
  } else if (powerStage === "charge") {
    // handled by the power-up acting below
  } else if (ph === "idle") {
    const beat = BEATS[beatIndex]
    if (started && beat && oc && oc.state === "oc" && !ocBusy && clock >= beat.at * k80() - 1.5) {
      beatIndex++
      startPrompt(beat)
    } else {
      const c = started && nextTarget()
      if (c) startReply(c)
    }
  } else if (ph === "look") {
    browUp += 0.5 * (1 - i)
    if (head.t > (head.stage >= 4 ? 0.02 : lerp(0.45, 0.06, i))) {
      head.phase = "windup"
      head.t = 0
    }
  } else if (ph === "windup") {
    const dur = head.stage >= 4 ? 0.05 : lerp(0.22, 0.13, i)
    const k = clamp(head.t / dur, 0, 1)
    stretch = -lerp(0.04, 0.18, i) * k
    lean = -lerp(4, 16, i) * k
    mouth = 0.1 * k
    mouthW = 0.6
    anger += 0.12 * i * k
    if (head.t >= dur) {
      head.phase = "speak"
      head.t = 0
      head.lastWord = -1
      head.speakStart = rtime
      const c = head.target
      say(head.take, { pan: c ? (c.x / W) * 1.2 - 0.6 : 0, duck: i > 0.4, rate: head.rate, level: LEVEL[head.stage] })
      head.stretch.v += lerp(0.5, 4, i)
      if (head.stage === 3 && rand() < smooth(0.35, 0.7, i)) audio.sfx("inhale", { bus: "voice", gain: 0.5 * i, rate: rnd(1, 1.15) })
      if (i > 0.5) cam.trauma = Math.min(1, cam.trauma + 0.08 * i * v.shake)
    }
  } else if (ph === "speak") {
    const loud = i > 0.45
    stretch = lerp(0.02, 0.13, i)
    lean = lerp(3, 12, i)
    // The mouth follows the voice's amplitude envelope.
    const talk = mouthOpen()
    mouth = loud ? lerp(0.3, 1.1, i) * (0.35 + 0.65 * talk) : 0.06 + 0.42 * talk
    mouthW = loud ? 1.05 : 0.8
    anger += 0.15 * i
    smile = loud ? -1 : smile
    const at = rtime - head.speakStart
    while (head.queue.length && at >= head.queue[0].at) {
      const w = head.queue.shift()
      if (w.target.state === "typing") {
        launch(w.text, w.target, { slot: w.slot })
        head.target = w.target
      }
      head.lastWord = w.at
    }
    // Calm replies let the clip finish; furious ones barrel into the next message.
    const tail = head.target?.kind === "opencode" ? head.take.dur / head.rate - head.lastWord : lerp(head.take.dur / head.rate - head.lastWord, 0.06, smooth(0.55, 0.85, i))
    if (!head.queue.length && at - head.lastWord > tail) {
      head.phase = "pause"
      head.t = 0
    }
  } else if (ph === "pause") {
    if (i < 0.25 && head.t > 0.25 && head.t < 0.8 && head.target?.state === "sent") smile = 1.3
    const due = BEATS[beatIndex] && clock >= BEATS[beatIndex].at * k80() - 1.5 && !ocBusy
    if (head.t > (due ? 0.3 : head.stage >= 4 ? 0.02 : lerp(1.0, 0.1, i))) {
      head.phase = "idle"
      head.t = 0
      head.target = null
    }
  }

  // Power-up: crouch, tremble and scream; the mouth rides the scream clip.
  if (powerStage === "charge") {
    const k = clamp(head.t / (3.6 * k80()), 0, 1)
    stretch = -0.25 * smooth(0, 0.3, k) + 0.25 * smooth(0.75, 1, k)
    lean = -10
    mouth = 0.15 + 1.05 * mouthOpen()
    mouthW = lerp(0.6, 1.15, k)
    anger = 1
    squint = 1
    smile = -1
  }
  // Finale acting overrides the rig; the mouth rides the voice envelope throughout.
  let wobble = 0
  if (blastStage === "windup") {
    const k = clamp(head.t / F.fire, 0, 1)
    stretch = -0.26 * smooth(0, 0.6, k) + 0.3 * smooth(0.92, 1, k)
    lean = -18 * k
    mouth = 0.1 + 1.1 * mouthOpen()
    mouthW = lerp(0.6, 1.15, k)
    anger = 1
    squint = 1
    smile = -1
  } else if (blastStage === "fire" || blastStage === "blast") {
    stretch = 0.14 + 0.05 * Math.sin(time * 40)
    lean = 8
    mouth = 0.7 + 0.5 * mouthOpen()
    mouthW = 1.12
    anger = 1
    squint = 1
    smile = -1
  } else if (blastStage === "stare") {
    // Fourth wall Beat 1: squints suspiciously, leans in toward the camera: "Hey. What are you looking at?"
    const talk = mouthOpen()
    stretch = -0.05 + 0.08 * talk
    lean = 14
    mouth = 0.04 + 0.52 * talk
    mouthW = 0.76
    anger = 0.68
    squint = 0.82
    browUp = -0.38
    asym = 0.35
    smile = -0.45
  } else if (blastStage === "cracking") {
    // Fourth wall Beat 2: firing Z-axis words straight at the screen glass
    const talk = mouthOpen()
    stretch = 0.12 * talk
    lean = 20
    mouth = 0.28 + 0.92 * talk
    mouthW = 1.14
    anger = 1
    squint = 0.9
    browUp = -0.5
    smile = -1
  } else if (blastStage === "shattered" || blastStage === "rebuilding") {
    // Fourth wall Beat 3 & 4: smug in the void ("You should've downloaded this earlier... 'cause this is going away.")
    const talk = mouthOpen()
    stretch = 0.04 * talk
    lean = 10
    mouth = 0.06 + 0.58 * talk
    mouthW = 0.88
    anger = 0.18
    squint = 0.32
    browUp = 0.42
    asym = 0.48
    smile = 0.65
  } else if (blastStage === "after") {
    const k = head.t
    mouth = 0.05 + 0.6 * mouthOpen()
    mouthW = 0.55
    stretch = 0
    lid = 0.45 * (1 - smooth(2.4, 3.2, k))
    squint = 0
    smile = k > 2.9 ? 1 : 0
    browUp = 0.3
    wobble = (1 - smooth(0, 3.4, k)) * (reduced ? 0 : 1)
  }

  const zoomTarget =
    blastStage === "stare"
      ? 1.26
      : blastStage === "cracking"
        ? 1.34
        : blastStage === "shattered"
          ? 1.28
          : 1.0
  spring(fourthZoom, zoomTarget, 2.2, 0.78, dt)

  const fast = blastStage ? 1.6 : 1
  const d = head.dir
  spring(head.anger, anger, 1.6 * fast, 1, dt)
  spring(head.stretch, stretch, 4.2, 0.38, dt)
  spring(head.mouth, mouth, 9, 0.55, dt)
  spring(head.browUp, browUp, 6, 0.6, dt)
  spring(head.smile, smile, 5, 0.8, dt)
  spring(head.lid, lid, 5, 0.9, dt)
  spring(head.squint, squint, 4 * fast, 0.9, dt)
  spring(head.asym, asym, 3, 0.8, dt)
  const tr = tremble + (blastStage === "windup" ? 7 * clamp(head.t / F.fire, 0, 1) : 0) + (blastStage === "fire" ? 3 : 0) + (powerStage === "charge" && !reduced ? 9 * clamp(head.t / 3, 0, 1) : 0)
  // Dizzy: the head drifts in a shrinking circle.
  spring(head.lx, d.x * lean + Math.sin(time * 47) * tr + wobble * 22 * Math.cos(time * 3.3), 5, 0.45, dt)
  spring(head.ly, d.y * lean * 0.6 + Math.cos(time * 41) * tr * 0.6 + wobble * 12 * Math.sin(time * 3.3), 5, 0.45, dt)
  spring(head.gx, d.x, 9, 0.85, dt)
  spring(head.gy, d.y, 9, 0.85, dt)
  spring(head.faceX, d.x * 0.09, 3.4, 0.7, dt)
  spring(head.faceY, d.y * 0.06, 3.4, 0.7, dt)
  spring(head.tilt, d.x * 0.14 + (ph === "idle" && !blastStage ? Math.sin(time * 0.7) * 0.03 : 0) + wobble * 0.35 * Math.sin(time * 6.5), 2.2, 0.55, dt)
  head.mouthW = lerp(head.mouthW, mouthW, 1 - Math.exp(-dt * 12))

  head.nextBlink -= dt
  if (head.nextBlink <= 0 && ph !== "speak" && !blastStage) {
    head.blink = 1
    head.nextBlink = rnd(2.4, 5)
  }
  head.blink = Math.max(0, head.blink - dt / 0.14)
}

// ─── Director ──────────────────────────────────────────────────────────────────
function director(dt) {
  if (!started) return
  if (v.intensity < 0) {
    clock += dt
    // Reduced motion stays in the calm stage, OpenCode beats included.
    if (reduced && clock >= arcAt("clipped")) {
      clock = 0
      beatIndex = 0
      for (const b of BEATS) b.spawned = false
    }
    if (!powerStage && !blastStage && clock >= arcAt("powerup") && clock < arcAt("saiyan")) startPowerUp()
    if (!blastStage && clock >= arcAt("blast")) startBlast()
    if (clock >= v.cycle) {
      clock = 0
      blastStage = null
      powerStage = null
      finale.phase = null
      finale.spin = 0
      beatIndex = 0
      ocBusy = null
      cards.counts = {}
      screenBreak.reset()
      returnTitleWords()
      for (const b of BEATS) b.spawned = false
    }
  }
  // OpenCode window and the issue each early prompt is about.
  const beat = BEATS[beatIndex]
  if (!blastStage && beat) {
    if (clock >= (beat.at - (beat.lead ?? 3)) * k80() && (!oc || oc.state === "dead")) spawnOpenCode()
    if (beat.issue && !beat.spawned && clock >= (beat.issueAt ?? beat.at - 3) * k80()) {
      beat.spawned = true
      // The first issue gets a polite reply that stays on the card until OpenCode fixes it.
      const c = spawnCard(null, { kind: "issue", item: beat.issue, pinned: beatIndex > 0 })
      if (c) {
        beat.card = c
        c.hold = beatIndex === 0
      }
    }
  }
  if (blastStage === "fire") {
    // Keep feeding targets while the Tommy gun sweeps.
    spawnAcc += dt * 5 * v.spawn
  } else if (blastStage || powerStage === "charge") return
  // Calm opening: nothing else arrives until the first prompt is underway.
  const opening = beatIndex === 0 && v.intensity < 0
  const st = currentStage()
  const base = opening ? 0 : st >= 4 ? 3.5 + 1.5 * (st - 4) : 0.33 + 4 * Math.pow(I, 2.1)
  const rate = base * v.spawn * (userRage ? 2 : 1)
  spawnAcc += dt * rate
  if (!opening && time > 0.6 && live.length === 0 && spawnAcc < 1) spawnAcc = 1
  while (spawnAcc >= 1) {
    spawnAcc -= 1
    spawnCard()
  }
}

// ─── OpenCode ──────────────────────────────────────────────────────────────────
function spawnOpenCode() {
  const s = cardScale() * 1.12
  const hp = headBase()
  const hw = 180 * s + PAD * 0.75 * s
  const hh = 100 * s + PAD * 0.75 * s
  const rawWant = narrow()
    ? { x: 196 * cardScale() + 6, y: clamp(slot.y + headR() * 0.4, 150, H - 140 * cardScale()) }
    : { x: clamp(hp.x - headR() * 2.2 - 190 * cardScale(), 200 * cardScale(), W - 200 * cardScale()), y: clamp(hp.y - headR() * 0.15, 130, H - 140 * cardScale()) }
  const want = resolveAvoid(rawWant.x, rawWant.y, hw, hh)
  oc = spawnCard(want, { kind: "opencode", force: true })
  if (!oc) return
  // Arrives with presence: a touch larger than the messages, a swoosh, and the head glances over.
  oc.base *= 1.12
  oc.hw *= 1.12
  oc.hh *= 1.12
  oc.pop.v += 2
  audio.sfx("swoosh", { gain: 0.5, rate: 0.9, pan: (oc.x / W) * 1.4 - 0.7 })
  head.glance = { c: oc, until: time + 1.1 }
  renderer.ocIntro?.(oc)
}

function submitPrompt(c) {
  const beat = ocBusy
  c.state = "oc"
  c.pulse = 1
  c.glow = 1
  audio.sfx("submit", { gain: 0.8, pan: (c.x / W) * 1.4 - 0.7 })
  renderer.ocSubmit?.(c)
  c.pop.v += 3
  c.agents = beat?.set ? beat.agents : (c.agents ?? 0) + (beat?.agents ?? 1)
  c.agentFlash = 1
  cards.draw(c)
  const fast = (head.stage ?? 1) >= 3
  setTimeout0(0.35, () => {
    c.typed = ""
    c.reply = null
    c.status = "Working…"
    c.statusDone = false
    cards.draw(c)
  })
  setTimeout0(fast ? 0.7 : 1.3, () => {
    const pinned = beat?.card && beat.card.state === "wait" ? [beat.card] : []
    const others = live.filter((o) => FIXABLE.has(o.kind) && o.state === "wait" && !pinned.includes(o)).sort((x, y) => x.born - y.born)
    const fixes = [...pinned, ...others].slice(0, beat?.fix ?? 2)
    fixes.forEach((o, k) => setTimeout0(k * (fast ? 0.08 : 0.22), () => resolveCard(o, beat?.version ?? "2.3.1")))
    setTimeout0(fixes.length * (fast ? 0.08 : 0.22) + 0.2, () => {
      if (c.state === "dead") return
      c.status = fixes.length ? `${fixes.length} fixed · ${beat?.version ?? "2.3.1"}` : `Shipped ${beat?.version ?? "2.3.1"}`
      c.statusDone = true
      c.glow = 1
      cards.draw(c)
      ocBusy = null
    })
  })
}

// A complaint OpenCode fixed: green state, a soft success cue, then it floats away intact.
let lastSuccess = -1
function resolveCard(c, version) {
  if (c.state !== "wait" && c.state !== "typing") return
  c.state = "resolving"
  c.typing = false
  c.typed = ""
  c.reply = null
  c.resolved = { issue: `Closed · fixed in ${version}`, tweet: `Fixed in ${version}`, review: "Merged" }[c.kind] ?? `Fixed in ${version}`
  c.sentAt = time + lerp(0.6, 0.3, I)
  c.pop.v += 4
  cards.draw(c)
  if (rtime - lastSuccess > 0.16) {
    lastSuccess = rtime
    audio.sfx("success", { gain: lerp(0.6, 0.4, I), rate: rnd(0.97, 1.05) * (saiyan > 0.5 ? 1.12 : 1), pan: (c.x / W) * 1.4 - 0.7 })
  }
  setTimeout0(c.sentAt - time - 0.05, () => audio.sfx("send", { gain: 0.35, rate: rnd(1.05, 1.2), pan: (c.x / W) * 1.4 - 0.7 }))
  if (!renderer.resolveCard?.(c)) resolveFallback(c)
}

function resolveFallback(c) {
  const size = Math.max(c.hw, c.hh) * 2
  fx.push({ kind: 0, x: c.x, y: c.y, r: 0, rMax: size * 0.75 * v.fx, width: 1.6, t: 0, k: 0.6, age: 0, life: 0.5 })
  const sparks = []
  for (let i = 0; i < 16; i++) {
    const a = rand() * Math.PI * 2
    const sp = rnd(60, 260)
    sparks.push({ x: c.x + rnd(-c.hw, c.hw) * 0.8, y: c.y + rnd(-c.hh, c.hh) * 0.8, vx: Math.cos(a) * sp, vy: Math.sin(a) * sp - 120, life: rnd(0.5, 1), heat: 0, size: rnd(1.2, 2.2) })
  }
  renderer.spawnSparks(sparks)
}

// ─── Super Saiyan ──────────────────────────────────────────────────────────────
function startPowerUp() {
  powerStage = "charge"
  head.phase = "power"
  head.t = 0
  head.queue = []
  for (const c of live) {
    if (c.state !== "typing") continue
    Object.assign(c, { state: c.kind === "opencode" ? "oc" : "wait", typing: false, typed: "", reply: null, landed: 0, words: 0 })
    cards.draw(c)
  }
  ocBusy = null
  audio.sfx("charge", { bus: "impacts", gain: 0.9 })
  setTimeout0(0.25, () => say(manifest.voice.powerup.takes[0], { gain: 1.1 }))
}

function updatePower(dt) {
  const charging = powerStage === "charge"
  const forced = v.intensity >= 0 ? (I > 0.85 ? 1 : 0) : 0
  const target = blastStage === "after" ? 0 : powerStage === "on" ? 1 : charging ? 0.85 * smooth(0, 3.6 * k80(), head.t) : forced
  saiyan += (target - saiyan) * (1 - Math.exp(-dt * (charging ? 6 : 2.5)))
  if (charging && head.t >= 3.6 * k80()) {
    powerStage = "on"
    saiyan = 1
    head.phase = "idle"
    head.t = 0
    head.stretch.v += 7
    audio.sfx("powerup", { bus: "impacts", gain: 1 })
    audio.sfx("boom", { bus: "impacts", gain: 0.6, rate: 0.8 })
    renderer.saiyanBurst?.()
    hitStop.remaining = 0.08
    cam.trauma = 1
    const hp = headPos()
    fx.push({ kind: 2, x: 0, y: 0, r: 0, t: 0, k: 0.22 * v.fx, age: 0, life: 0.14 })
    fx.push({ kind: 0, x: hp.x, y: hp.y, r: 0, rMax: Math.hypot(W, H) * 0.6, width: 4, t: 0, k: 1, age: 0, life: 0.6 })
    fx.push({ kind: 8, x: hp.x, y: hp.y, vx: 0, vy: -35, r: headR() * 1.15, r0: headR() * 1.15, rMax: headR() * 2.55, t: 0, k: 1.45, seed: 77.7, age: 0, life: 0.72 })
    addBlastLight(hp.x, hp.y, Math.hypot(W, H) * 0.55, 1.65, 0.52)
    joltTitleWords()
    for (const c of live) {
      const dx = c.x - hp.x
      const dy = c.y - hp.y
      const d = Math.hypot(dx, dy) || 1
      c.kx.v += (dx / d) * 900
      c.ky.v += (dy / d) * 900
    }
    aura()
  }
  renderer.setSaiyan?.(clamp(saiyan, 0, 1))
}
// Seamless 4 s aura bed during Saiyan rapid-fire; stops once the finale starts so finale voice lines stay crystal-clear.
function aura() {
  if (powerStage !== "on" || blastStage) return
  audio.sfx("aura", { bus: "impacts", gain: 0.38, dur: 4.0 })
  setTimeout0(4, aura)
}

// ─── Finale: the Tommy-gun spin ────────────────────────────────────────────────
// windup (yell, spin-up) → fire (two machine-gun torrents, the spinning head sprays words and
// rounds that wreck every card) → blast → a beat of silence → dizzy wobble and "inbox zero".
const finale = { phase: null, t: 0, spin: 0, angle: -Math.PI / 2, firing: false, power: 0, nextShot: 0, lastKill: 0 }
const F = { fire: 4.2, torrent: 4.55, end: 14.7, dizzy: 16.4 }
function startBlast() {
  blastStage = "windup"
  Object.assign(finale, { phase: "windup", t: 0, spin: 0, firing: false, power: 0, nextShot: 0 })
  ocBusy = null
  head.phase = "blast"
  head.t = 0
  head.queue = []
  for (const c of live) if (c.state === "typing") c.typing = false
  say(manifest.voice.windup.takes[0], { gain: 1.05, level: 1 })
  setTimeout0(2.0, () => audio.sfx("spinup", { bus: "impacts", gain: 0.85 }))
  setTimeout0(F.fire, startFire)
}

function startFire() {
  blastStage = "fire"
  finale.phase = "fire"
  finale.firing = true
  head.stretch.v += 5
  cam.trauma = Math.min(1, cam.trauma + 0.5 * v.shake)
  const loop = (name, every, gain) => {
    const go = () => {
      if (!finale.firing) return
      audio.sfx(name, { bus: "impacts", gain })
      setTimeout0(every, go)
    }
    go()
  }
  loop("vortex", 3, 0.5)
  loop("tommy", 2, 0.75)
  loop("casings", 1.15, 0.35)
  // Two torrents back to back, each word a heavy round aimed where the spinning head points.
  const a = pick(manifest.voice["torrent-a"].takes)
  const b = pick(manifest.voice["torrent-b"].takes)
  let at = F.torrent - F.fire
  for (const take of [a, b]) {
    const start = at
    setTimeout0(start, () => say(take, { gain: 1.05, level: 1 }))
    for (const w of take.words) setTimeout0(start + w.t, () => fireWord(w.text))
    at += take.dur + 0.05
  }
  // Knock the three h1 title words ("Speak.", "It", "appears.") loose during the Tommy-gun barrage
  ;[
    { idx: 0, delay: 2.1, word: "SPEAK!" },
    { idx: 1, delay: 4.6, word: "IT!" },
    { idx: 2, delay: 7.1, word: "BOOM!" },
  ].forEach(({ idx, delay, word }) => {
    setTimeout0(delay, () => {
      if (reduced || !titleWords[idx]) return
      measureAvoid()
      const tw = titleWords[idx]
      const goal = { x: tw.rest.cx + tw.x, y: tw.rest.cy + tw.y }
      launch(word, null, {
        point: goal,
        i: 1,
        size: clamp(W * 0.042, 36, 58),
        weight: 880,
        onPop: () => knockTitleWord(idx),
      })
    })
  })
  setTimeout0(Math.max(at, F.end - F.fire), finalBlast)
}

function muzzle() {
  const c = headPos()
  const R = headR() * 1.15
  return { x: c.x + Math.cos(finale.angle) * R, y: c.y + Math.sin(finale.angle) * R }
}

function cardAlong(angle, spread) {
  const c = headPos()
  let best = null
  let bestD = Infinity
  for (const o of live) {
    if (o.state === "dead" || o.state === "resolving") continue
    const d = Math.hypot(o.x - c.x, o.y - c.y)
    let da = Math.atan2(o.y - c.y, o.x - c.x) - angle
    da = Math.atan2(Math.sin(da), Math.cos(da))
    if (Math.abs(da) < Math.atan2(Math.max(o.hw, o.hh), d) + spread && d < bestD) {
      best = o
      bestD = d
    }
  }
  return best
}

function fireWord(text) {
  const m = muzzle()
  const target = cardAlong(finale.angle, 0.35)
  const far = Math.hypot(W, H)
  const point = target ? { x: target.x, y: target.y } : { x: m.x + Math.cos(finale.angle) * far * 0.45, y: m.y + Math.sin(finale.angle) * far * 0.45 }
  launch(text.replace(/-$/, ""), null, {
    point,
    i: 1,
    size: clamp(W * 0.04, 34, 58),
    weight: 860,
    onPop: () => {
      if (target && target.state !== "dead") {
        target.hitDir = { x: Math.cos(finale.angle), y: Math.sin(finale.angle) }
        explode(target, 0.9)
      }
    },
  })
  renderer.finaleShot?.({ ...m, angle: finale.angle, word: text })
}

// The continuous rattle between words: tracers along the spin, cards in the line of fire pop.
function updateFire(dt) {
  if (!finale.firing) return
  finale.nextShot -= dt
  while (finale.nextShot <= 0) {
    finale.nextShot += 0.07
    const m = muzzle()
    const a = finale.angle + rnd(-0.05, 0.05)
    renderer.finaleShot?.({ ...m, angle: a })
    if (!renderer.finaleShot) {
      const sp = []
      for (let k = 0; k < 3; k++) sp.push({ x: m.x, y: m.y, vx: Math.cos(a) * rnd(2600, 3400), vy: Math.sin(a) * rnd(2600, 3400), life: 0.22, heat: 1, size: 2.4 })
      renderer.spawnSparks(sp)
    }
    const hit = cardAlong(a, 0.04)
    if (hit && rtime - finale.lastKill > 0.11) {
      finale.lastKill = rtime
      hit.hitDir = { x: Math.cos(a), y: Math.sin(a) }
      explode(hit, 0.7)
      if (rand() < 0.5) audio.sfx("ricochet", { bus: "impacts", gain: 0.45, rate: rnd(0.85, 1.25), pan: (hit.x / W) * 1.4 - 0.7 })
    }
  }
}

function finalBlast() {
  finale.firing = false
  finale.phase = "blast"
  blastStage = "blast"
  const hp = headPos()
  const diag = Math.hypot(W, H)
  audio.sfx("blast", { bus: "impacts", gain: 1 })
  audio.sfx("thump", { bus: "impacts", gain: 1, rate: 0.8 })
  hitStop.remaining = 0.12
  cam.trauma = 1
  fx.push({ kind: 2, x: 0, y: 0, r: 0, t: 0, k: 0.32 * v.fx, age: 0, life: 0.16 })
  fx.push({ kind: 0, x: hp.x, y: hp.y, r: 0, rMax: diag, width: 5, t: 0, k: 1.2, age: 0, life: 0.75 })
  fx.push({ kind: 0, x: hp.x, y: hp.y, r: 0, rMax: diag * 0.7, width: 2.5, t: 0, k: 0.8, age: -0.08, life: 0.75 })
  fx.push({ kind: 8, x: hp.x, y: hp.y, vx: 0, vy: -30, r: headR() * 1.35, r0: headR() * 1.35, rMax: headR() * 3.1, t: 0, k: 1.55, seed: 88.8, age: 0, life: 0.82 })
  addBlastLight(hp.x, hp.y, diag * 0.65, 1.8, 0.58)
  for (let idx = 0; idx < titleWords.length; idx++) {
    if (titleWords[idx].state !== "falling") knockTitleWord(idx)
  }
  // Keep 6 outer cards on screen during the fourth-wall stare & glass cracking so the cracks
  // visibly refract cards as well as the copy/header/button, then clear them when the screen shatters!
  let kept = 0
  for (const c of live) {
    if (c.state === "dead") continue
    if (kept < 6) {
      kept++
      continue
    }
    const dd = Math.hypot(c.x - hp.x, c.y - hp.y)
    c.blastAt = time + 0.45 * (1 - Math.pow(1 - clamp(dd / diag, 0, 1), 1 / 3))
    c.hitDir = { x: c.x - hp.x, y: c.y - hp.y }
  }
  while (kept < 6 && !reduced) {
    const nc = spawnCard()
    if (!nc) break
    kept++
  }

  const startDizzyExhale = () => {
    screenBreak.reset()
    returnTitleWords()
    blastStage = "after"
    finale.phase = "dizzy"
    head.t = 0
    powerStage = null
    audio.sfx("dizzy", { gain: 0.6 })
    setTimeout0(0.45, () => {
      const ex = manifest.voice.dizzy.takes[0]
      say(ex, { gain: 1.05, level: 0.3, duck: 0.85 })
      const p = headPos()
      ex.words.forEach((w, k) =>
        setTimeout0(w.t, () => launch(w.text, null, { point: { x: p.x + (k - 1.5) * 110, y: p.y + headR() * 1.9 }, size: 18, weight: 500, i: 0, quiet: true })),
      )
    })
  }

  if (reduced) {
    setTimeout0(F.dizzy - F.end, startDizzyExhale)
    return
  }

  // ─── Beat 1 (Fourth Wall Stare): spin stops, beat of silence so explosions decay, then leans in ───
  setTimeout0(0.85, () => {
    blastStage = "stare"
    finale.phase = "stare"
    finale.spin = 0
    head.t = 0
    // Immediate sidechain duck to hush any lingering explosion tails before he speaks
    audio.setDuck(0.92, 3.6)
    setTimeout0(0.6, () => {
      const stareTake = manifest.voice["fourth-stare"]?.takes?.[0]
      if (stareTake) {
        say(stareTake, { gain: 1.65, level: 0.48, duck: 0.94 })
        screenBreak.setCaption({
          words: stareTake.words,
          startRtime: rtime,
          endRtime: rtime + stareTake.dur,
        })
      }
    })
  })

  // ─── Beat 2 (Cracking the Screen): shoots Z-axis words straight at the glass ───
  setTimeout0(4.25, () => {
    blastStage = "cracking"
    finale.phase = "cracking"
    head.t = 0
    const fireTake = manifest.voice["fourth-fire"]?.takes?.[0]
    if (fireTake) {
      say(fireTake, { gain: 1.65, level: 1.0, duck: 0.85 })
      screenBreak.setCaption({
        words: fireTake.words,
        startRtime: rtime,
        endRtime: rtime + fireTake.dur,
      })
    }

    const rootRect = root.getBoundingClientRect()
    const dlEl = root.querySelector(".hero .download")
    const dlRect = dlEl ? dlEl.getBoundingClientRect() : null
    const dlX = dlRect && dlRect.width > 0 ? dlRect.left - rootRect.left + dlRect.width * 0.52 : W * 0.5
    const dlY = dlRect && dlRect.height > 0 ? dlRect.top - rootRect.top + dlRect.height * 0.5 : H * 0.72

    const targets = [
      { x: W * 0.31, y: H * 0.31, hitsDownload: false },
      { x: W * 0.69, y: H * 0.29, hitsDownload: false },
      { x: W * 0.36, y: H * 0.56, hitsDownload: false },
      { x: dlX - 68, y: dlY - 22, hitsDownload: true },
      { x: W * 0.66, y: H * 0.48, hitsDownload: false },
      { x: W * 0.61, y: H * 0.66, hitsDownload: false },
      { x: W * 0.5, y: H * 0.46, hitsDownload: false, final: true },
    ]

    const wordsList = fireTake?.words ?? [
      { text: "TAKE!", t: 0.06 },
      { text: "THIS!", t: 0.55 },
      { text: "AND!", t: 1.11 },
      { text: "THIS!", t: 1.35 },
      { text: "AND!", t: 1.83 },
      { text: "THAT!", t: 2.07 },
      { text: "BREAK!", t: 2.79 },
    ]

    wordsList.forEach((w, idx) => {
      const tgt = targets[Math.min(idx, targets.length - 1)]
      const isFinal = idx === wordsList.length - 1 || !!tgt.final
      setTimeout0(w.t, () => {
        const m = mouthPos()
        head.stretch.v += isFinal ? 5.5 : 2.8
        head.ly.v -= isFinal ? 55 : 26
        screenBreak.fireWordAtScreen({
          text: w.text,
          fromX: m.x,
          fromY: m.y,
          targetX: tgt.x,
          targetY: tgt.y,
          dur: isFinal ? 0.21 : 0.16,
          final: isFinal,
          hitsDownload: tgt.hitsDownload,
          onImpact: () => {
            const pan = (tgt.x / W) * 1.2 - 0.6
            if (!isFinal) {
              // Crisp, ducked glass-crack transient + pitch/time-spread propagation sweep (90–160 ms)
              const primaryCrack = idx % 2 === 0 ? "glass-crack-1" : "glass-crack-2"
              const secondaryCrack = idx % 2 === 0 ? "glass-crack-2" : "glass-crack-1"
              audio.sfx(primaryCrack, {
                bus: "impacts",
                gain: 0.32,
                rate: rnd(0.95, 1.06),
                pan,
                dur: 0.22,
              })
              setTimeout0(0.052, () =>
                audio.sfx(secondaryCrack, {
                  bus: "impacts",
                  gain: 0.21,
                  rate: rnd(1.18, 1.34),
                  pan: clamp(pan * 1.25, -0.85, 0.85),
                  dur: 0.18,
                }),
              )
              setTimeout0(0.105, () =>
                audio.sfx(primaryCrack, {
                  bus: "impacts",
                  gain: 0.14,
                  rate: rnd(1.36, 1.54),
                  pan: clamp(-pan * 0.75, -0.85, 0.85),
                  dur: 0.15,
                }),
              )
              fx.push({ kind: 0, x: tgt.x, y: tgt.y, r: 0, rMax: 165 * v.fx, width: 2.8, t: 0, k: 1.1, age: 0, life: 0.34 })
              fx.push({ kind: 1, x: tgt.x, y: tgt.y, r: 68 * v.fx, t: 0, k: 1.15, age: 0, life: 0.08 })
              chip(tgt.x, tgt.y, 0, -1, 18, 0.9)
              addBlastLight(tgt.x, tgt.y, 260, 1.15, 0.28)
              hitStop.remaining = Math.max(hitStop.remaining, 0.024)
              cam.trauma = Math.min(1, cam.trauma + 0.52 * v.shake)
              cam.kx.v += (tgt.x > W * 0.5 ? 1 : -1) * 240 * v.shake
              cam.ky.v += 220 * v.shake
            } else {
              // ─── Beat 3 (Full-Screen Shatter into Void + Download Knocked Off) ───
              blastStage = "shattered"
              finale.phase = "shattered"
              head.t = 0
              for (const c of live) if (c.state !== "dead") retire(c)
              live.length = 0
              // Brief window for the glass shatter crash + tinkle before sidechain-ducking for the punchline
              audio.setDuck(0, 0)
              audio.sfx("glass-shatter", { bus: "impacts", gain: 0.72, dur: 1.15 })
              audio.sfx("boom", { bus: "impacts", gain: 0.48, rate: 0.88, dur: 0.8 })
              setTimeout0(0.28, () => audio.sfx("glass-tinkle", { bus: "impacts", gain: 0.46, dur: 0.9 }))
              fx.push({ kind: 0, x: tgt.x, y: tgt.y, r: 0, rMax: diag * 0.85, width: 4.5, t: 0, k: 1.35, age: 0, life: 0.62 })
              fx.push({ kind: 1, x: tgt.x, y: tgt.y, r: 160 * v.fx, t: 0, k: 1.4, age: 0, life: 0.12 })
              addBlastLight(tgt.x, tgt.y, diag * 0.65, 1.75, 0.52)
              hitStop.remaining = Math.max(hitStop.remaining, 0.085)
              cam.trauma = 1
              // Hold 1.18s while the screen shatters into the void, then sidechain-duck hard & deliver the punchline!
              setTimeout0(1.18, () => {
                const goneTake = manifest.voice["fourth-gone"]?.takes?.[0]
                if (goneTake) {
                  say(goneTake, { gain: 1.72, level: 0.68, duck: 0.95 })
                  screenBreak.setCaption({
                    words: goneTake.words,
                    startRtime: rtime,
                    endRtime: rtime + goneTake.dur,
                  })
                }
              })
            }
          },
        })
      })
    })
  })

  // ─── Beat 4 (Time-Reverse Crystalline Rebuild): shards, Download button, and title fly back ───
  setTimeout0(12.6, () => {
    blastStage = "rebuilding"
    finale.phase = "rebuilding"
    head.t = 0
    screenBreak.setCaption(null)
    audio.setDuck(0, 0)
    audio.sfx("glass-rebuild", { bus: "impacts", gain: 0.76, dur: 1.8 })
    screenBreak.startRebuild(1.8)
    returnTitleWords()
  })

  // ─── Beat 5 (Dizzy Exhale & Reset to Calm) ───
  setTimeout0(14.55, startDizzyExhale)
}

function updateFinale(dt) {
  if (!finale.phase) {
    renderer.setFinale?.(null)
    return
  }
  finale.t += dt
  const ph = finale.phase
  const target = ph === "windup" ? 6 * smooth(2.0, F.fire, finale.t) : ph === "fire" ? 9 : ph === "blast" ? 2 : 0
  const k = ph === "blast" || ph === "dizzy" ? 1.6 : 3
  finale.spin += (target - finale.spin) * (1 - Math.exp(-dt * k))
  if (ph === "dizzy" && finale.spin < 0.05) finale.spin = 0
  finale.angle += finale.spin * Math.PI * 2 * dt
  finale.power = ph === "windup" ? smooth(0, F.fire, finale.t) : ph === "fire" ? 1 : Math.max(0, finale.power - dt)
  updateFire(dt)
  renderer.setFinale?.(finaleState())
}
const finaleState = () => (finale.phase ? { phase: finale.phase, spin: finale.spin, angle: finale.angle, firing: finale.firing, sweep: finale.angle, power: finale.power } : null)

// Sim-time timers (so hit-stop and capture stay deterministic).
const timers = []
function setTimeout0(delay, fn) {
  timers.push({ at: rtime + delay, fn })
}
function runTimers() {
  for (let n = timers.length - 1; n >= 0; n--) {
    if (rtime >= timers[n].at) {
      const t = timers[n]
      timers.splice(n, 1)
      t.fn()
    }
  }
}

function updateCards(dt) {
  for (let n = live.length - 1; n >= 0; n--) {
    const c = live[n]
    if (c.state === "dead") {
      live.splice(n, 1)
      continue
    }
    if (c.blastAt != null && time >= c.blastAt) {
      explode(c, 1.3)
      continue
    }
    const age = time - c.born
    spring(c.pop, 1, lerp(3.2, 5, I), lerp(0.55, 0.4, I), dt)
    spring(c.ox, 0, lerp(3.4, 6, I), 0.75, dt)
    spring(c.oy, 0, lerp(3.4, 6, I), 0.75, dt)
    spring(c.kx, 0, 7, 0.4, dt)
    spring(c.ky, 0, 7, 0.4, dt)
    const s = c.seed
    const jit = (Math.pow(I, 2.5) * 3 + (blastStage === "windup" ? 4 : 0)) * (c.state === "wait" ? 1 : 0.4)
    let x = c.bx + c.ox.x + c.kx.x + Math.sin(time * 0.8 + s) * 1.5 + Math.sin(time * 53 + s) * jit
    let y = c.by + c.oy.x + c.ky.x + Math.sin(time * 1.1 + s * 2) * 2 + Math.cos(time * 47 + s) * jit
    let scale = c.base * lerp(0.92, 1, clamp(c.pop.x, 0, 1.2)) * (c.pop.x > 1 ? 1 + (c.pop.x - 1) * 0.3 : 1)
    let alpha = clamp(age / 0.09, 0, 1)
    if (c.kind === "opencode" && (c.pulse > 0 || c.glow > 0 || c.agentFlash > 0)) {
      c.pulse = Math.max(0, c.pulse - dt / 0.45)
      c.glow = Math.max(0, c.glow - dt / 1.2)
      c.agentFlash = Math.max(0, c.agentFlash - dt / 0.9)
      cards.draw(c)
    }
    if (c.state === "resolving") {
      // Fixed: a little lift, then it floats up and away intact.
      const k = Math.max(0, time - c.sentAt)
      y -= 10 * clamp((time - c.sentAt + 0.6) / 0.6, 0, 1) + 1600 * k * k
      x += (c.bx > W / 2 ? 1 : -1) * 300 * k * k
      scale *= 1 + 0.04 * clamp((time - c.sentAt + 0.6) * 3, 0, 1) - 0.15 * clamp(k * 3, 0, 1)
      alpha *= 1 - clamp((k - 0.25) * 4, 0, 1)
      if (k > 0.55) {
        retire(c)
        continue
      }
    }
    if (c.state === "sent") {
      const k = Math.max(0, time - c.sentAt)
      const dirx = c.bx > W / 2 ? 1 : -1
      x += dirx * 2200 * k * k
      y -= 500 * k * k
      scale *= 1 - 0.2 * clamp(k * 3, 0, 1)
      alpha *= 1 - clamp((k - 0.15) * 5, 0, 1)
      if (k > 0.45) {
        retire(c)
        continue
      }
    }
    if (c.state === "dying" && time >= c.dieAt) {
      explode(c)
      continue
    }
    // Keep cards out of the hero copy + download button exclusion zone except during the finale climax
    if (c.state !== "resolving" && c.state !== "sent" && !allowCopyOverlap()) {
      const padMargin = PAD * 0.75 * c.base
      const baseRes = resolveAvoid(c.bx, c.by, c.hw + padMargin, c.hh + padMargin)
      if (baseRes.pushed) {
        c.bx = lerp(c.bx, baseRes.x, 1 - Math.exp(-dt * 12))
        c.by = lerp(c.by, baseRes.y, 1 - Math.exp(-dt * 12))
      }
      const curRes = resolveAvoid(x, y, c.hw + padMargin, c.hh + padMargin)
      if (curRes.pushed) {
        const pushX = curRes.x - x
        const pushY = curRes.y - y
        c.kx.v += pushX * 26 * dt
        c.ky.v += pushY * 26 * dt
        c.kx.x += pushX * 0.65
        c.ky.x += pushY * 0.65
        x = curRes.x
        y = curRes.y
      }
    }
    if (dt > 0) {
      c.vx = (x - c.x) / dt
      c.vy = (y - c.y) / dt
    }
    c.x = x
    c.y = y
    c.scale = scale
    c.alpha = alpha
    c.rot = c.spin * (1 - clamp(age * 3, 0, 1)) + c.kx.x * 0.0008
    c.flash = c.state === "dying" ? 1 : Math.max(0, c.flash - dt / 0.08)
  }
  for (let n = releaseQueue.length - 1; n >= 0; n--) {
    if (time >= releaseQueue[n].at) {
      cards.release(releaseQueue[n].c)
      releaseQueue.splice(n, 1)
    }
  }
}

function updateFx(dt) {
  for (let n = fx.length - 1; n >= 0; n--) {
    const f = fx[n]
    f.age += dt
    f.t = clamp(f.age / f.life, 0, 1)
    if (f.age >= f.life) {
      fx.splice(n, 1)
      continue
    }
    f.k0 ??= f.k
    if (f.vx || f.vy) {
      f.x += (f.vx ?? 0) * dt
      f.y += (f.vy ?? 0) * dt
      if (f.vx) f.vx *= Math.exp(-1.8 * dt)
      if (f.vy && f.kind === 8) f.vy *= Math.exp(-1.2 * dt)
    }
    if (f.kind === 0) {
      f.width0 ??= f.width
      f.r = f.rMax * (1 - Math.pow(1 - f.t, 3))
      f.width = f.width0 * (1 - f.t * 0.7)
      f.k = f.k0 * (1 - f.t) * (1 - f.t) * (f.age < 0 ? 0 : 1)
    } else if (f.kind === 8 || f.kind === 9) {
      f.r = lerp(f.r0 ?? f.r, f.rMax ?? f.r * 1.8, 1 - Math.pow(1 - f.t, 2.2))
      f.k = f.k0 * (f.age < 0 ? 0 : 1)
    } else if (f.kind === 10) {
      f.k = f.k0 * (1 - f.t) * (f.age < 0 ? 0 : 1)
    } else f.k = f.k0 * (1 - f.t)
  }
}

let lastScene = null
// ─── Frame ─────────────────────────────────────────────────────────────────────
function step(realDt) {
  realDt = Math.min(realDt, 1 / 20)
  let dt = realDt
  if (hitStop.remaining > 0) {
    const frozen = Math.min(hitStop.remaining, realDt)
    hitStop.remaining -= frozen
    dt = frozen * hitStop.scale + (realDt - frozen)
  }
  if (reduced) hitStop.remaining = 0
  time += dt
  rtime += realDt
  baseShift += (shiftTarget() - baseShift) * (1 - Math.exp(-realDt * 4))
  audio.setIntensity(I)
  director(dt)
  const target = userRage ? Math.max(targetIntensity(), 0.9) : targetIntensity()
  I += (target - I) * (1 - Math.exp(-dt * (blastStage === "after" ? 0.9 : 2.5)))
  runTimers()
  updatePower(dt)
  updateFinale(dt)
  updateHead(dt)
  updateWords(dt)
  updateLanding(dt)
  updateCards(dt)
  updateTitleWords(dt)
  updateFx(realDt)
  updateBlastLights(realDt)

  cam.trauma = Math.max(0, cam.trauma - realDt * 1.18) * Math.exp(-realDt * 2.0)
  spring(cam.kx, 0, 6.2, 0.45, realDt)
  spring(cam.ky, 0, 6.2, 0.45, realDt)
  const tr = Math.pow(cam.trauma, 1.85)
  const nz = (f, o) => Math.sin(time * f + o) * 0.6 + Math.sin(time * f * 1.7 + o * 3) * 0.4
  const calm = reduced ? 0 : 1
  const amp = lerp(6, 17, I)
  const shakeX = (nz(38, 1) * amp * tr + cam.kx.x * 0.032) * calm
  const shakeY = (nz(43, 7) * amp * tr + cam.ky.x * 0.032) * calm
  const roll = nz(29, 3) * 0.01 * tr * calm

  const c = headPos()
  const sy = 1 + head.stretch.x
  renderer.draw((lastScene = {
    w: W,
    h: H,
    time,
    dt,
    shakeX,
    shakeY,
    roll,
    lights: blastLights.slice(0, 2),
    head: {
      x: c.x,
      y: c.y,
      R: headR(),
      tilt: head.tilt.x,
      sx: 1 / Math.pow(sy, 0.7),
      sy,
      anger: clamp(head.anger.x, 0, 1),
      mouth: clamp(head.mouth.x, 0, 1.25),
      gazeX: clamp(head.gx.x, -1, 1),
      gazeY: clamp(head.gy.x, -1, 1),
      blink: head.blink > 0 ? Math.sin(head.blink * Math.PI) : 0,
      happy: clamp(head.smile.x, -1, 1.3),
      faceX: head.faceX.x,
      faceY: head.faceY.x,
      browUp: head.browUp.x,
      mouthW: head.mouthW,
      lid: clamp(head.lid.x, 0, 1),
      squint: clamp(head.squint.x, 0, 1),
      asym: clamp(head.asym.x, 0, 1),
      vein: smooth(0.8, 0.95, I),
      mouthOpen: mouthOpen(),
      speaking: mouthOpen() > 0.02,
      visible: 1,
      flicker: Math.sin(time * 31) * 0.5 + 0.5,
    },
    problems: oc && live.includes(oc) ? [...live.filter((o) => o !== oc), oc] : live, // OpenCode stays on top
    words: flying,
    fx,
    saiyan: clamp(saiyan, 0, 1),
    finale: finaleState(),
    director: { stage: currentStage(), blast: blastStage, power: powerStage },
    started,
    intensity: I,
    look: {
      shutter: (v.shutter / 60) * calm,
      sparkStretch: 1.6,
      halo: lerp(0.7, 1.9, I),
      rim: v.rim,
      shardDrag: 2.6,
      shardGravity: 520,
      sparkDrag: 4.2,
      sparkGravity: 380,
    },
  }))
  screenBreak.setHeadLight(c.x, c.y, headR())
  screenBreak.stepAndDraw(realDt, rtime)
}

// ─── Input ─────────────────────────────────────────────────────────────────────
// Holding ⌥ during the show enrages the head; clicking the page spawns a message.
const off = new AbortController()
const on = (target, type, fn) => target.addEventListener(type, fn, { signal: off.signal })
on(window, "keydown", (e) => {
  if (e.key === "Alt" && !e.repeat && started && !reduced) userRage = true
})
on(window, "keyup", (e) => {
  if (e.key === "Alt") userRage = false
})
on(window, "blur", () => (userRage = false))
on(root, "pointermove", (e) => {
  const r = root.getBoundingClientRect()
  pointer.x = e.clientX - r.left
  pointer.y = e.clientY - r.top
  pointer.seen = e.pointerType === "mouse"
})
on(root, "pointerleave", () => (pointer.seen = false))
on(root, "pointerdown", (e) => {
  if (reduced || !started || e.target.closest("a, button, summary, details, code, input")) return
  const r = root.getBoundingClientRect()
  const c = spawnCard({ x: e.clientX - r.left, y: e.clientY - r.top })
  if (c) c.born -= 1
})

// ─── Loop ──────────────────────────────────────────────────────────────────────
// Runs only while the hero is on screen and the tab is visible. Before the show starts the
// idle head runs at a reduced rate; reduced motion draws one still frame.
let last = performance.now()
let raf = 0
let visible = true
let idleAcc = 0
const frame = (now) => {
  raf = 0
  const dt = (now - last) / 1000
  last = now
  if (started || reduced) step(dt)
  else {
    // Idle: 30 fps is plenty for a breathing head and halves the GPU work before the click.
    idleAcc += dt
    if (idleAcc >= 1 / 31) {
      step(idleAcc)
      idleAcc = 0
    }
  }
  if (visible && !document.hidden && !(reduced && !started)) raf = requestAnimationFrame(frame)
}
const wake = () => {
  if (raf || !visible || document.hidden || (reduced && !started) || capture) return
  last = performance.now()
  raf = requestAnimationFrame(frame)
}
on(document, "visibilitychange", () => {
  audio.setHidden(document.hidden)
  wake()
})
const io = new IntersectionObserver(([e]) => {
  visible = e.isIntersecting
  audio.setHidden(!visible)
  wake()
})
io.observe(root)
const ro = new ResizeObserver(() => {
  resize()
  if (!raf) step(0)
})
ro.observe(root)
for (const el of root.querySelectorAll(".hero, .hero__copy, .hero__action, .platforms")) ro.observe(el)
resize()
if (reduced) head.smile.x = 1
step(0)
await renderer.done?.()
wake()

if (capture) {
  window.__shout = {
    async step(n = 1) {
      for (let k = 0; k < n; k++) step(1 / 60)
      await renderer.done?.()
    },
    async stepDt(dt, n = 1) {
      for (let k = 0; k < n; k++) step(dt)
      await renderer.done?.()
    },
    state: () => ({
      rtime,
      time,
      clock,
      I,
      stage: currentStage(),
      arc: arcNow().id,
      saiyan: +saiyan.toFixed(3),
      blast: blastStage,
      finale: finale.phase,
      power: powerStage,
      cards: live.length,
      words: flying.length,
      phase: head.phase,
      screenBreak: screenBreak.state,
      cracks: screenBreak.cracks.length,
      glassShards: screenBreak.shards.length,
    }),
    titleState: () => titleWords.map((w) => ({ text: w.text, state: w.state, x: +w.x.toFixed(1), y: +w.y.toFixed(1), rot: +w.rot.toFixed(3), bounces: w.bounces })),
    avoidState: () => ({
      copyRects,
      cards: live.map((c) => ({
        kind: c.kind,
        x: +c.x.toFixed(1),
        y: +c.y.toFixed(1),
        hw: +c.hw.toFixed(1),
        hh: +c.hh.toFixed(1),
        overlaps: overlapsAvoid(c.x, c.y, c.hw, c.hh),
      })),
    }),
    pressKey: (down, rect) => pressKey(down, rect),
    explodeKey: (rect) => explodeKeycap(rect),
    focus: () => {
      const c = head.target && head.target.state !== "dead" ? head.target : live.find((o) => o.state === "typing")
      return c ? { x: c.x, y: c.y, kind: c.kind } : null
    },
    audioLog: () => audio.log,
    renderer: renderer.kind,
  }
}

return {
  renderer: renderer.kind,
  pressKey,
  explodeKey: explodeKeycap,
  // Call from a click with an AudioContext created inside that gesture.
  async start({ context, muted, keyRect }) {
    if (started) return
    audio.attach(context, { muted })
    if (!reduced) {
      audio.keyBoom()
      explodeKeycap(keyRect)
    }
    wake()
    const [m] = await Promise.all([import("./audio/manifest.json").then((x) => x.default), cards.ready])
    manifest = m
    for (const [id, line] of Object.entries(manifest.voice)) if (LINES[line.stage] && !line.prompt) LINES[line.stage].push({ id, ...line, next: 0 })
    // The first lines are short; wait for them (not the whole bank) so the opening isn't silent.
    await audio.load(manifest)
    started = true
    clock = 0
    // The key press lands: the head perks up.
    head.stretch.v += 3.2
    head.blink = 1
    wake()
  },
  setMuted(muted, context) {
    audio.setMuted(muted, context)
  },
  destroy() {
    off.abort()
    io.disconnect()
    ro.disconnect()
    screenBreak.destroy()
    if (raf) cancelAnimationFrame(raf)
    raf = 0
    visible = false
    audio.close()
  },
}
}
