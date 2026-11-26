// The page. One loop: advance show time, play what is due, perform the faces, lay out the
// OpenCode session, compose the effects, draw, and put the DOM where show time says it is: the
// calendar's notification he joins the call from, the windows' chrome, Hex's capsule (its offer, and the recording pill the offer closes down into),
// the name plates, Hex's live transcript under the call, the pointer and its clicks, the prompt
// it carries across, the session's items, the composer, the music player, the browser and the
// notes. Everything drawn is a function of show time, so ?t=41.6&paused renders that frame cold.
//   ?perf   frame rate, CPU and GPU time      ?t=SECONDS[&paused]   start there      ?muted
//   ?force=none   draw with the 2D canvas even where there is WebGPU
// The viewer has three buttons (Play or Pause, Restart, Mute), Space, R and M, and a click
// anywhere on the stage is Play or Pause. There is no scrubber: ?t= is for scripts.
import { buildShow, WHO, ME, PROMPT, TABS, TRACK, MEETING, THEM, clock } from './show.js'
import { Stage, G, F, STRIDE } from './cast.js'
import { AudioEngine } from './audio.js'
import { CallMeter, FieldTimeline, drawField } from './scope.js'
import { pointerAt, typedAt, scrollAt, clicks } from './pointer.js'
import { Ambient, Touch, backdrop } from './ambient.js'

const $ = (id) => document.getElementById(id)
const q = new URLSearchParams(location.search)
const clamp = (v, a = 0, b = 1) => (v < a ? a : v > b ? b : v)
const lerp = (a, b, k) => a + (b - a) * k
const smooth = (k) => { k = clamp(k); return k * k * (3 - 2 * k) }
const easeOut = (k) => 1 - Math.pow(1 - clamp(k), 3)
const easeInOut = (k) => { k = clamp(k); return k < 0.5 ? 4 * k * k * k : 1 - Math.pow(-2 * k + 2, 3) / 2 }
// Overshoots a little and settles: something pushed home.
const seat = (k, c = 2.6) => { k = clamp(k); return 1 + (c + 1) * Math.pow(k - 1, 3) + c * Math.pow(k - 1, 2) }
// A spring with no bounce in it, u seconds after it was let go: how far it has got, 0 to 1.
// (w: how stiff. It is most of the way there 4/w seconds in.) Everything of Hex's that changes
// size or arrives is one of these, or a sum of them, so it is the same at any show time.
const crit = (u, w) => (u <= 0 ? 0 : 1 - (1 + w * u) * Math.exp(-w * u))

const el = {
  page: $('page'), call: $('call'), stage: $('stage'), bean: $('bean'), join: $('join'), hint: $('hint'), flash: $('flash'),
  pill: $('pill'), offerIn: $('offer-in'), offerSlot: $('offer-slot'), offerRec: $('offer-rec'), recWord: $('rec-word'), pillHead: $('pill-head'), pillX: $('pill-x'), dotSlot: $('dot-slot'), pillBody: $('pill-body'),
  clock: $('clock'), scope: $('scope'), rowPrompt: $('row-prompt'), rowAll: $('row-all'), rowStop: $('row-stop'),
  winCall: $('win-call'), winRec: $('win-rec'), winOc: $('win-oc'), stream: $('stream'), composer: $('composer'), input: $('input'), chip: $('chip'), typed: $('typed'), ph: $('ph'), send: $('send'), people: $('people'),
  ocTitle: $('oc-title'), railTitle: $('rail-title'),
  cursor: $('cursor'), ripple: $('ripple'), carry: $('carry'),
  notes: $('notes'), notesCard: $('notes-card'), notesRows: [...$('notes-rows').children], notesBean: $('notes-bean'), notesTitle: $('notes-title'), host: $('plate-host'),
  replay: $('replay'), play: $('play'), restart: $('restart'), perf: $('perf'), sound: $('sound'), egg: $('egg'),
  plates: WHO.map((w) => document.querySelector('.plate[data-who="' + w + '"]')),
  auto: $('auto'), autoSw: $('auto-sw'), browser: $('browser'), view: $('view'), url: $('url'), tabs: [...document.querySelectorAll('.tab')], close: $('browser-close'),
  notif: $('notif'), notifJoin: $('notif-join'), slackAgo: $('slack-ago'),
  player: $('player'), playerBtn: $('player-btn'), playerTime: $('player-time'), playerFill: $('player-fill'), bars: [...document.querySelectorAll('#wave i')],
}

const manifest = await (await fetch('audio/manifest.json')).json()
const S = buildShow(manifest)
const cue = S.cue
const stage = new Stage(S)
// The renderer and the effects load on their own: if either cannot (no WebGPU, or a shader
// that does not compile), the page still runs and every word on it is still drawn.
const gpuMod = q.get('force') === 'none' ? null : await import('./gpu.js').catch(() => null)
const NOFRAME = { body: true, tendrils: 0, motes: false, mass: false }
const NOFX = { shakeX: 0, shakeY: 0, compose: () => NOFRAME }
let vfx = NOFX
try {
  const m = await import('./vfx.js')
  vfx = new m.Vfx(S, stage)
} catch { vfx = NOFX }
const audio = new AudioEngine()
if (q.has('muted')) audio.muted = true
void audio.preload(S)
// The call's two tracks for Hex's field, from the same bytes: decoded offline, so they are there
// before any click and in a headless render too.
const meter = new CallMeter(S, audio.bytes)

// What draws the stage. WebGPU where the browser has it (gpu.js). Where it does not (no
// navigator.gpu, no adapter, a shader this browser refuses, a device lost in the middle of the
// show) the same frame is drawn with a 2D canvas (canvas2d.js): the beans, their faces, the
// windows, the mass and its filaments. There is never an empty stage.
let renderer = null
const coarse = matchMedia('(pointer: coarse)').matches
// How many canvas pixels to a CSS pixel: the display's, to 2, and no more than this machine
// should be asked to fill (a phone or a tablet less than a desk; the 2D canvas least of all).
function pixelRatio() {
  const budget = renderer?.kind === 'WebGPU' ? (coarse ? 3.4e6 : 15e6) : coarse ? 1.3e6 : 2.6e6
  return Math.max(1, Math.min(devicePixelRatio || 1, 2, Math.sqrt(budget / Math.max(1, live.W * live.H))))
}
let mounting = false
async function mount(gpu) {
  if (mounting) return
  mounting = true
  const canvas = document.createElement('canvas')
  let r = null
  if (gpu && gpuMod) r = await gpuMod.GpuRenderer.create(canvas).catch(() => null)
  if (!r) r = await import('./canvas2d.js').then((m) => m.Canvas2DRenderer.create(canvas)).catch(() => null)
  mounting = false
  if (!r) return
  // (A lost device: the tab was in the background too long, or the GPU process went away.)
  r.onLost = () => { if (renderer === r) { renderer = null; void mount(false) } }
  el.stage.replaceChildren(canvas)
  renderer = r
  el.page.dataset.renderer = r.kind
  r.setTiming?.(q.has('perf'))
  if (live.W > 1) r.resize(live.W, live.H, pixelRatio())
}
const frame = new Float32Array(gpuMod?.FRAME_FLOATS ?? G.FLOATS)

const state = { phase: 'idle', T: 0, playing: false, fired: 0 }
const live = {
  phase: 'idle', W: 1, H: 1, dt: 1 / 60, pageTime: 0, reduced: matchMedia('(prefers-reduced-motion: reduce)').matches, hoverJoin: false,
  pointer: { x: 0, y: 0, on: false }, pill: { x: 0, y: 0 }, cursor: { x: 0, y: 0, down: 0, alpha: 0, moving: 0 },
  anchor: { merge: { x: 0, y: 0 }, nerve: { x: 0, y: 0 }, send: { x: 0, y: 0 }, under: { x: 0, y: 0 } }, browser: 0,
  // linger: seconds spent on the notes once they have settled. egg: how far the way out to Spaceman has faded in.
  linger: 0, egg: 0,
}
const ambient = new Ambient($('ambient'))
const touch = new Touch()
await mount(true)
el.notesTitle.textContent = 'Q4 sync · ' + S.callLength
$('player-title').innerHTML = TRACK.title.replace(/[<&]/g, '').replace('_', '_<wbr>')
$('player-sub').textContent = TRACK.sub
// How long it is, from the schedule.
{
  const s = cue.end
  const long = s < 52 ? Math.round(s / 5) * 5 + ' seconds' : s < 68 ? 'About a minute' : s < 84 ? 'A minute and a quarter' : s < 100 ? 'A minute and a half' : 'About ' + Math.round(s / 60) + ' minutes'
  if (el.hint) el.hint.textContent = long + ', with sound'
}

// ——— small DOM helpers that only touch the DOM when something changed ———
const memo = new WeakMap()
const cache = (node) => memo.get(node) ?? (memo.set(node, {}), memo.get(node))
function text(node, v) {
  const c = cache(node)
  if (c.text !== v) { c.text = v; node.textContent = v }
}
function attr(node, name, on) {
  const c = cache(node)
  if (c[name] !== !!on) { c[name] = !!on; on ? node.setAttribute(name, '') : node.removeAttribute(name) }
}
// A data attribute with a value ('' takes it off).
function data(node, name, v) {
  const c = cache(node)
  if (c['data:' + name] !== v) { c['data:' + name] = v; v ? node.setAttribute('data-' + name, v) : node.removeAttribute('data-' + name) }
}
function put(node, x, y, opacity, scale = 1) {
  const c = cache(node)
  const tr = 'translate3d(' + x.toFixed(1) + 'px,' + y.toFixed(1) + 'px,0)' + (scale !== 1 ? ' scale(' + scale.toFixed(3) + ')' : '')
  if (c.tr !== tr) { c.tr = tr; node.style.transform = tr }
  const op = opacity < 0.004 ? '0' : opacity.toFixed(2)
  if (c.op !== op) { c.op = op; node.style.opacity = op }
}
function vary(node, name, v) {
  const c = cache(node)
  const s = (Number.isFinite(v) ? v : 0).toFixed(2)
  if (c[name] !== s) { c[name] = s; node.style.setProperty(name, s) }
}
function px(node, prop, v) {
  const c = cache(node)
  const s = v.toFixed(1) + 'px'
  if (c[prop] !== s) { c[prop] = s; node.style[prop] = s }
}
function style(node, prop, s) {
  const c = cache(node)
  if (c[prop] !== s) { c[prop] = s; node.style[prop] = s }
}
// Where a node sits inside an ancestor, from layout alone (no transforms).
function offsetIn(node, root) {
  let x = 0, y = 0, n = node
  while (n && n !== root) { x += n.offsetLeft; y += n.offsetTop; n = n.offsetParent }
  return { x, y, w: node.offsetWidth, h: node.offsetHeight }
}

// ——— the OpenCode session: every item built once, then sized and placed by show time ———
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;')
const code = (s) =>
  esc(s).replace(/("[^"]*")|\b(const|let|mut|fn|static|await)\b|\b([A-Z][A-Za-z]+)\b|\b(\d[\d.]*)\b|([a-z_]\w*)(?=\()/g, (m, str, kw, ty, num, fn) =>
    str ? '<i class="s">' + str + '</i>' : kw ? '<i class="k">' + kw + '</i>' : ty ? '<i class="t">' + ty + '</i>' : num ? '<i class="n">' + num + '</i>' : '<i class="f">' + fn + '</i>')
const SVG = {
  ok: '<svg class="ok" viewBox="0 0 16 16"><path d="M3.500 8.500 6.500 11.500 12.500 4.500"/></svg>',
  doc: '<svg viewBox="0 0 16 16"><path d="M9.250 2.250h-5a1 1 0 0 0-1 1v9.500a1 1 0 0 0 1 1h7.500a1 1 0 0 0 1-1v-7Z"/><path d="M9.250 2.250v3.500h3.500M5.750 8.750h4.500M5.750 11h3"/></svg>',
}
const STATE = '<span class="st"><i class="st__spin"></i>' + SVG.ok + '</span>'
// Who said it. He is You, and stays You; one of the four is their name with Them written under
// it, and the two are crossed when the four become one (applyNames).
const whoTag = (cls, who, name) =>
  who === 'me' ? '<span class="' + cls + '" data-who="me">' + esc(name) + '</span>'
  : '<span class="' + cls + ' swap" data-who="' + who + '" data-name="' + esc(name) + '"><i>' + esc(name) + '</i><i>' + THEM + '</i></span>'
const GAP = 8, ROW = 24
// sx, sy: where the session's stream is inside OpenCode's window. send: its send button.
const M = { comp: 102, padx: 14, streamH: 100, view: 100, lift: 0, sx: 0, sy: 38, send: { x: 0, y: 0 } }
const items = S.oc.map((o) => {
  const n = document.createElement('div')
  n.className = 'oc-item oc-item--' + o.kind + (o.w ? ' w' + o.w : '')
  const it = { o, n, c: {}, lines: [], y: 0, h: 0, cardH: 0, headH: 26, bodyH: 0, fullH: 0, padH: 8, headK: 1 }
  if (o.kind === 'user') {
    n.innerHTML = '<div class="bubble">' + (o.pill ? '<span class="oc-chip" data-on>' + SVG.doc + 'Hex meeting prompt · q4-sync</span>' : '') + '<span>' + esc(o.text) + '</span></div>'
  } else if (o.kind === 'text') {
    n.innerHTML = '<p><span></span><span class="rest"></span></p>'
    it.said = n.querySelector('span')
    it.rest = n.querySelector('.rest')
    it.words = o.text.split(' ')
    it.rest.textContent = o.text
  } else if (o.kind === 'metric') {
    // What a change bought. A number that came down counts down.
    const v = o.from ? '<s>' + esc(o.from) + '</s><i>→</i><b>' + esc(o.to) + '</b>' : (o.arrow ? '<i>→</i>' : '') + '<b>' + esc(o.to) + '</b>'
    n.innerHTML = '<div class="metric">' + SVG.ok + '<span class="metric__k">' + esc(o.label) + '</span><span class="metric__v">' + v + '</span></div>'
    const a = parseFloat(o.from), b = parseFloat(o.to)
    if (o.from && o.from.replace(/[\d.]+/, '') === o.to.replace(/[\d.]+/, '') && a > b) { it.num = n.querySelector('b'); it.count = [a, b, o.to.replace(/[\d.]+/, '')] }
  } else if (o.kind === 'diag') {
    n.innerHTML = '<div class="diag"><span class="diag__k">' + esc(o.label) + ':</span><p><span></span><span class="rest"></span></p></div>'
    it.said = n.querySelector('p span')
    it.rest = n.querySelector('.rest')
    it.words = o.value.split(' ')
    it.rest.textContent = o.value
  } else if (o.kind === 'checks') {
    n.innerHTML = '<div class="checks">' + o.rows.map((r) => '<span class="chk">' + SVG.ok + esc(r) + '</span>').join('') + '</div>'
    n.querySelectorAll('.chk').forEach((c, i) => it.lines.push({ n: c, t: o.each[i] }))
  } else {
    // A tool call, as OpenCode writes one: its name, what it was given, what it changed. What it
    // has just heard is the same command it has been running all along, and the line that came back.
    const heard = o.kind === 'heard'
    const name = heard || o.tool === 'Bash' ? 'Shell' : o.tool
    const cmd = heard ? 'hex meetings show ' + MEETING + ' <em>--from ' + o.from + '</em>' : o.read ? esc(o.cmd).replace(/(--from \d\d:\d\d)/, '<em>$1</em>') : esc(o.cmd)
    let body = ''
    if (heard) body = '<div class="ln ln--o ln--q">' + o.parts.map((p) => whoTag('ln__who', p.who, p.name) + '<span class="ln__t">“' + esc(p.quote) + '”</span>').join('') + '</div>'
    else if (o.steps) body = o.steps.map((s) => '<div class="step">' + STATE + '<span>' + esc(s.text) + '</span></div>').join('')
    else
      body = o.body.map((l) =>
        l.s === 'o' ? '<div class="ln ln--o"><span class="ln__at">' + l.at + '</span>' + whoTag('ln__who', l.who, l.name) + '<span class="ln__t">' + esc(l.text) + '</span></div>'
        : l.s === 'ok' ? '<div class="ln ln--ok"><span class="ln__t">' + esc(l.text) + '</span></div>'
        : '<div class="ln ln--' + (l.s === '+' ? 'add' : l.s === '-' ? 'del' : 'ctx') + '"><i class="g">' + (l.s === ' ' ? '' : l.s === '-' ? '−' : '+') + '</i><span class="ln__t">' + code(l.text) + '</span></div>').join('')
    const auto = o.auto ? '<span class="tool__auto">' + SVG.ok + 'Auto-approved</span>' : ''
    const meta = o.meta ? '<span class="tool__meta"><i class="plus">' + o.meta.split(' ')[0] + '</i><i class="minus">' + o.meta.split(' ')[1] + '</i></span>' : ''
    n.innerHTML = '<div class="tool' + (name === 'Shell' ? ' tool--sh' : '') + (heard ? ' tool--heard' : '') + '" data-state="wait"' + (o.auto ? ' data-auto' : '') + '><div class="tool__head"><b>' + name + '</b><code>' + cmd + '</code>' + meta + auto + STATE + '</div><div class="tool__body">' + body + '</div></div>'
    it.card = n.firstElementChild
    it.head = it.card.firstElementChild
    it.body = it.card.lastElementChild
    const rows = it.body.children
    if (heard) {
      it.heard = true
      it.run = o.t + 0.04
      it.done = o.t + 0.24
      it.bodyStart = o.t + 0.18
      it.lines.push({ n: rows[0], t: o.t + 0.24 })
    } else if (o.steps) o.steps.forEach((s, i) => it.lines.push({ n: rows[i], t: s.t, done: s.done || s.t + 0.32 }))
    else {
      const nl = o.body.length
      o.body.forEach((l, i) => it.lines.push({ n: rows[i], t: o.read ? o.run + 0.12 + i * 0.14 : nl === 1 ? o.done - 0.04 : o.run + 0.1 + ((o.done - o.run) * 0.7 * i) / (nl - 1) }))
      // A command's output arrives when it finishes; a read or an edit opens as it starts.
      it.bodyStart = nl === 1 && !o.read ? o.done - 0.16 : o.run
    }
  }
  el.stream.append(n)
  return it
})
// Every name in the session that becomes Them, newest first: they go one after another, from
// the merge upward, 45 ms apart.
const NAMES = [...el.stream.querySelectorAll('.ln__who.swap')].reverse().map((n, i) => ({ n, a: n.firstElementChild, b: n.lastElementChild, len: n.dataset.name.length, at: cue.them + Math.min(i, 10) * 0.045 }))
// A name crossing to Them at k (0 the name, 1 Them): the name up and out, Them up and in.
function crossName(a, b, k) {
  const c = cache(a), s = k.toFixed(2)
  if (c.k === s) return false
  c.k = s
  a.style.opacity = (1 - k).toFixed(2)
  a.style.transform = k < 0.005 ? 'none' : 'translate3d(0,' + (-4 * k).toFixed(1) + 'px,0)'
  b.style.opacity = s
  b.style.transform = k > 0.995 ? 'none' : 'translate3d(0,' + (4 * (1 - k)).toFixed(1) + 'px,0)'
  return true
}
function applyNames(t) {
  for (const N of NAMES) {
    const k = crit(t - N.at, 24)
    // (The label is in a monospace face: it closes from its name's width to four letters.)
    if (crossName(N.a, N.b, k)) N.n.style.width = lerp(N.len, THEM.length, crit(t - N.at - 0.03, 20)).toFixed(2) + 'ch'
  }
}
const mergeItem = items.find((it) => it.o.steps) ?? items[items.length - 1]
// The last edit before the merge: where the nerves come out of.
const nerveItem = [...items].reverse().find((it) => it.o.tool === 'Edit' && it.o.t < (mergeItem.o.t ?? 1e9)) ?? mergeItem

function measureItems() {
  for (const it of items) {
    it.n.style.height = ''
    if (it.card) { it.card.style.height = ''; it.head.style.height = ''; it.head.style.opacity = '' }
    it.c = {}
  }
  for (const it of items) {
    if (it.card) {
      it.headH = it.head.offsetHeight
      // (a block of output sits 2 px under its line)
      it.bodyH = it.body.offsetHeight + (it.heard ? 0 : 2)
      if (it.o.steps) it.padH = it.bodyH - it.o.steps.length * ROW
    } else it.fullH = it.n.firstElementChild.offsetHeight
  }
}

// How tall an item is at t: it arrives, a tool call opens while it runs, and folds to its
// one line once the session has moved on. (What it heard folds the other way: the command
// goes, and the line that was said stays.)
function heightOf(it, t) {
  const o = it.o
  if (t < o.t) return 0
  const k = easeOut((t - o.t) / 0.24)
  if (it.card) {
    let open = 0, head = it.headH
    if (o.steps) {
      if (t < cue.cut) {
        let rows = 0
        for (const s of o.steps) rows += easeOut((t - s.t) / 0.22)
        open = rows * ROW + it.padH * Math.min(1, rows)
      }
    } else if (it.heard) {
      open = easeOut((t - it.bodyStart) / 0.24) * it.bodyH
      it.headK = 1 - easeOut((t - (o.fold ?? 1e9)) / 0.3)
      head = it.headH * it.headK
    } else open = easeOut((t - it.bodyStart) / 0.26) * (1 - easeOut((t - (o.fold ?? 1e9)) / 0.3)) * it.bodyH
    it.cardH = head + open
    return k * (it.cardH + GAP)
  }
  it.cardH = it.fullH
  return k * (it.fullH + GAP)
}
// Top to bottom; once it is taller than what shows of its window (all of it, or what the
// browser leaves above itself), the newest stays in view.
function layoutStream(t) {
  let y = 10
  for (const it of items) {
    it.h = heightOf(it, t)
    it.y = y
    y += it.h
  }
  const geo = stage.geo
  live.browser = S.browserAt(t)
  M.view = M.streamH - live.browser * Math.max(0, M.streamH - (geo.browser.y - geo.oc.y - M.sy - 10))
  // The newest stays in view: its bottom, or, when it is taller than the view, its top.
  let off = Math.min(0, M.view - 4 - y)
  for (let i = items.length - 1; i >= 0; i--) if (items[i].h > 0.5) { off = Math.min(0, Math.max(off, 4 - items[i].y)); break }
  for (const it of items) it.y += off
  // Where the filaments leave: the left edge of the lines they grow from.
  const O = stage.oc, top = O.y + M.sy
  const A = live.anchor
  const at = (it, out) => {
    out.x = O.x + M.sx + M.padx + 1
    out.y = top + clamp(it.y + Math.min(it.headH || it.cardH, it.cardH || it.headH) / 2, 10, M.view - 10)
  }
  at(mergeItem, A.merge)
  at(nerveItem, A.nerve)
  // And the ones that come out from behind the browser.
  A.under.x = geo.browser.x + 6
  A.under.y = geo.browser.y + geo.browser.h * 0.3
  A.send.x = O.x + M.send.x
  A.send.y = O.y + M.send.y
}
function applyStream(t) {
  for (const it of items) {
    const o = it.o, c = it.c
    const on = it.h > 0.5 && it.y + it.h > -2 && it.y < M.streamH
    if (c.on !== on) { c.on = on; it.n.toggleAttribute('data-on', on) }
    if (!on) continue
    const y = it.y.toFixed(1)
    if (c.y !== y) { c.y = y; it.n.style.transform = 'translate3d(0,' + y + 'px,0)' }
    const h = it.h.toFixed(1)
    if (c.h !== h) { c.h = h; it.n.style.height = h + 'px' }
    const op = clamp((t - o.t) / 0.2).toFixed(2)
    if (c.op !== op) { c.op = op; it.n.style.opacity = op }
    if (it.card) {
      const ch = it.cardH.toFixed(1)
      if (c.ch !== ch) { c.ch = ch; it.card.style.height = ch + 'px' }
      const run = it.run ?? o.run, done = it.done ?? o.done
      const st = t < run ? 'wait' : t < done || !done ? 'run' : 'done'
      if (c.st !== st) { c.st = st; it.card.dataset.state = st }
      if (it.heard) {
        const hk = it.headK.toFixed(3)
        if (c.hk !== hk) { c.hk = hk; it.head.style.height = (it.headH * it.headK).toFixed(1) + 'px'; it.head.style.opacity = hk }
      }
    } else if (it.count) {
      const k = easeOut((t - o.t - 0.1) / 0.55)
      const v = String(Math.round(it.count[0] + (it.count[1] - it.count[0]) * k)) + it.count[2]
      if (c.v !== v) { c.v = v; it.num.textContent = v }
    } else if (it.words) {
      const n = clamp(Math.floor((t - o.t - 0.06) / (o.kind === 'diag' ? 0.05 : 0.03)), 0, it.words.length)
      if (c.n !== n) {
        c.n = n
        it.said.textContent = it.words.slice(0, n).join(' ')
        it.rest.textContent = (n && n < it.words.length ? ' ' : '') + it.words.slice(n).join(' ')
      }
    }
    for (const l of it.lines) {
      const v = t >= l.t
      if (l.on !== v) { l.on = v; l.n.toggleAttribute('data-on', v) }
      if (l.done !== undefined) {
        const d = t >= l.done
        if (l.was !== d) { l.was = d; l.n.toggleAttribute('data-done', d) }
      }
    }
  }
}

// ——— the Hex pill's field: Hex's own Dot scope (scope.js), lit by the call's voices ———
// The field is as wide as the pill's header leaves it (panel.rs field_width): its resting width,
// the whole header while the menu is open, never more than the closing offer's wall allows. In
// the capsule's points, from the same springs composeHex() moves the pill on.
const SCOPE_W = 133, SCOPE_H = 26
const fieldRoom = (t) => {
  const kw = crit(t - cue.rec[0], 14)
  const shut = 1 - crit(t - cue.copy - 0.62, 20)
  const ow = Math.max(crit(t - cue.hover, 19) * shut, crit(t - cue.hover2, 19))
  const wall = lerp(lerp(300, 168, kw), 252, ow) - lerp(18, 42, ow) - 77
  return Math.max(40, Math.min(lerp(168 - 18 - 77, SCOPE_W, ow), wall))
}
const field = new FieldTimeline(meter, cue.rec[0], cue.cut, fieldRoom)
const scope = el.scope.getContext('2d')
let scopeKey = ''
function drawScope(t) {
  const f = field.at(t)
  const k = HX.u * (devicePixelRatio || 1)
  const bw = Math.round(SCOPE_W * k), bh = Math.round(SCOPE_H * k)
  if (el.scope.width !== bw || el.scope.height !== bh) { el.scope.width = bw; el.scope.height = bh; scopeKey = '' }
  const key = f && f.drawn && !f.gone ? field.version + ':' + field.k : ''
  if (key === scopeKey) return
  scopeKey = key
  scope.setTransform(1, 0, 0, 1, 0, 0)
  scope.clearRect(0, 0, bw, bh)
  if (!key) return
  scope.setTransform(k, 0, 0, k, 0, 0)
  drawField(scope, f, SCOPE_W - f.scope.T.width / 2, SCOPE_H / 2, 1 / k)
}

// ——— layout ———
const pt = () => ({ x: 0, y: 0 })
const places = { rest: pt(), join: pt(), record: pt(), pill: pt(), copy: pt(), stop: pt(), input: pt(), auto: pt(), play: pt(), read: pt(), read2: pt(), close: pt(), tabs: TABS.map(pt), acts: {}, egg: pt() }
// The music player: on his desktop beside his own tile.
const P = { dx: 0, dy: 0, w: 0, h: 0 }
// Hex's capsule, in the app's own points (u: one of them, in px). ofW, ofH: the offer. restW,
// head: the pill at rest. openW, bodyH: its menu. rec0: the Record button in the offer (its
// middle from the capsule's middle, and its size). dotR: the red dot's middle from the
// pill's right edge. dot: its size.
const HX = { u: 1, ofW: 300, ofH: 44, head: 36, restW: 168, openW: 252, bodyH: 142, rec0: { x: 0, w: 70, h: 28 }, dotR: 66, dot: 8, rowP: 0, rowS: 0, carryH: 24, chip: pt() }
// The browser's four pages: each with the places its scroll stops at (marks, in px), and the
// visit he pays it. And the things in them he clicks.
const pages = TABS.map((tab, i) => {
  const n = el.view.querySelector('.pg[data-tab="' + i + '"]')
  return { i, n, inner: n.querySelector('.pg__in'), marks: { top: 0 }, visit: cue.tabs.find((v) => v.tab === i) }
})
const ACTS = []
for (const v of cue.tabs) for (const a of v.acts) {
  const pg = pages[v.tab]
  const n = pg.inner.querySelector('[data-act="' + a.act + '"]')
  if (!n) continue
  places.acts[a.act] = pt()
  ACTS.push({ a, n, pg, at: places.acts[a.act], also: a.also ? pg.inner.querySelector('[data-act="' + a.also + '"]') : null })
}
// The marks a page comes to rest on with the thing in the middle of the window, not at its top.
const CENTRE = new Set(['2q', '2u', '3b', '3c'])
const tileOf = (i) => stage.geo.tiles[i] ?? stage.geo.tiles[stage.geo.tiles.length - 1]
// The phone's top safe area in px (0 anywhere without one), read off a probe the height of it.
let safeProbe
function safeTop() {
  if (!safeProbe) {
    safeProbe = document.createElement('i')
    safeProbe.style.cssText = 'position:absolute;top:0;left:0;width:0;height:env(safe-area-inset-top,0px);visibility:hidden;pointer-events:none'
    el.page.append(safeProbe)
  }
  return safeProbe.offsetHeight || 0
}
function measure() {
  const r = el.page.getBoundingClientRect()
  live.W = r.width
  live.H = r.height
  const b = el.bean.getBoundingClientRect()
  const geo = stage.layout(live.W, live.H, { x: b.left - r.left + b.width / 2, y: b.top - r.top + b.height * 0.52, R: b.width * 0.47 }, safeTop())
  const nb = el.notesBean.getBoundingClientRect()
  geo.notes = { x: nb.left - r.left + nb.width / 2, y: nb.top - r.top + nb.height * 0.52, R: nb.width * 0.47 }
  el.page.toggleAttribute('data-narrow', geo.narrow)
  M.comp = geo.narrow ? 112 : 106
  M.padx = geo.narrow ? 10 : 16
  const st = el.call.style
  st.setProperty('--bar', geo.bar + 'px')
  st.setProperty('--comp', M.comp + 'px')
  st.setProperty('--padx', M.padx + 'px')
  el.winCall.style.width = geo.call.w + 'px'
  el.winCall.style.height = geo.call.h + 'px'
  el.winOc.style.width = geo.oc.w + 'px'
  el.winOc.style.height = geo.oc.h + 'px'
  const sr = offsetIn(el.stream, el.winOc)
  M.sx = sr.x
  M.sy = sr.y
  M.streamH = sr.h
  const sd = offsetIn(el.send, el.winOc)
  M.send.x = sd.x + sd.w / 2
  M.send.y = sd.y + sd.h / 2
  live.pill.x = geo.pill.x
  live.pill.y = geo.pill.y

  // The calendar's notification: top right, and its Join button.
  {
    const N = geo.notif
    el.notif.style.width = N.w + 'px'
    const j = offsetIn(el.notifJoin, el.notif)
    places.join.x = N.right - N.w + j.x + j.w * 0.56
    places.join.y = N.y + j.y + j.h / 2 + 3
  }

  // Hex's capsule, at the app's own measurements (a little larger on a desktop, to be read).
  const U = (HX.u = geo.narrow ? 1 : 1.14)
  el.pill.style.setProperty('--u', U + 'px')
  el.pill.removeAttribute('data-tight')
  let nat = el.offerIn.offsetWidth
  if (nat > live.W - 12) {
    el.pill.setAttribute('data-tight', '')
    nat = el.offerIn.offsetWidth
  }
  HX.ofW = nat
  HX.ofH = 44 * U
  HX.head = 36 * U
  HX.restW = 168 * U
  HX.openW = 252 * U
  HX.bodyH = el.pillBody.offsetHeight
  const slot = el.offerSlot
  const so = offsetIn(slot, el.offerIn)
  HX.rec0 = { x: so.x + so.w / 2 - nat / 2, w: so.w, h: so.h }
  const ds = el.dotSlot
  // (the dot sits before the clock, as in the app: 16 of padding, the clock's 38, 7 between, half of 8)
  HX.dotR = 65 * U
  HX.dot = 8 * U
  HX.rowP = el.rowPrompt.offsetTop + el.rowPrompt.offsetHeight / 2
  HX.rowS = el.rowStop.offsetTop + el.rowStop.offsetHeight / 2
  places.record.x = geo.pill.x + HX.rec0.x + HX.rec0.w * 0.12
  places.record.y = geo.pill.y + 3
  places.pill.x = geo.pill.x + HX.restW * 0.08
  places.pill.y = geo.pill.y + 3
  // Beside the words, not over them.
  places.copy.x = geo.pill.x + HX.openW * 0.2
  places.copy.y = geo.pill.y + HX.head / 2 + HX.rowP + 2
  places.stop.x = geo.pill.x - HX.openW * 0.14
  places.stop.y = geo.pill.y + HX.head / 2 + HX.rowS + 2
  places.rest.x = geo.solo.x + geo.solo.w * 0.74
  places.rest.y = geo.solo.y + geo.solo.h * 0.86

  // OpenCode: a new session has its composer in the middle of the empty pane; it drops to the
  // bottom when the message is sent.
  const cp = offsetIn(el.composer, el.winOc)
  M.lift = Math.min(0, geo.bar + (geo.oc.h - geo.bar) * 0.47 - (cp.y + cp.h / 2))
  const inp = offsetIn(el.input, el.winOc)
  places.input.x = geo.oc.x + inp.x + Math.min(150, inp.w * 0.36)
  places.input.y = geo.oc.y + inp.y + inp.h / 2 + 3 + M.lift
  HX.chip.x = geo.oc.x + inp.x + 14
  // (where the chip will sit once it is in: measured with it there)
  el.chip.style.display = 'inline-flex'
  const cr = offsetIn(el.chip, el.winOc)
  el.chip.style.display = ''
  HX.chip.x = geo.oc.x + cr.x
  HX.chip.y = geo.oc.y + cr.y + cr.h / 2 + M.lift
  HX.carryH = el.carry.offsetHeight || 24
  // The Auto-approve switch itself, not its label.
  const sw = offsetIn(el.autoSw, el.winOc)
  places.auto.x = geo.oc.x + sw.x + sw.w * 0.55
  places.auto.y = geo.oc.y + sw.y + sw.h / 2 + 1 + M.lift

  // The music player sits in the room the call leaves beside his tile.
  const me = tileOf(ME)
  const room = geo.call.w - geo.pad - (me.x + me.hw)
  const mgap = geo.narrow ? 8 : 16
  P.w = Math.round(clamp(room - mgap - (geo.narrow ? 0 : 8), 84, 248))
  el.player.toggleAttribute('data-small', P.w < 190)
  el.player.style.width = P.w + 'px'
  P.h = el.player.offsetHeight
  P.dx = me.hw + mgap
  P.dy = -P.h / 2
  const pb = offsetIn(el.playerBtn, el.player)
  places.play.x = geo.call.x + me.x + P.dx + pb.x + pb.w / 2 + 1
  places.play.y = geo.call.y + me.y + P.dy + pb.y + pb.h / 2 + 2

  // The browser: exactly the rectangle it is given; its red dot; its four tabs; somewhere to
  // rest in a page; where each mark on each page puts its scroll; and where the things he
  // clicks are when he clicks them.
  const B = geo.browser
  el.browser.style.width = B.w + 'px'
  el.browser.style.height = B.h + 'px'
  const dot = offsetIn(el.close, el.browser)
  places.close.x = B.x + dot.x + dot.w / 2 + 1
  places.close.y = B.y + dot.y + dot.h / 2 + 2
  el.tabs.forEach((n, i) => {
    const o = offsetIn(n, el.browser)
    places.tabs[i].x = B.x + o.x + Math.min(o.w * 0.5, 54)
    places.tabs[i].y = B.y + o.y + o.h / 2 + 2
  })
  // (On a phone the page is a few lines tall: it rests in the margin, off the words.)
  places.read.x = B.x + B.w * (geo.narrow ? 0.93 : 0.7)
  places.read.y = B.y + B.h * (geo.narrow ? 0.6 : 0.62)
  places.read2.x = B.x + B.w * (geo.narrow ? 0.94 : 0.62)
  places.read2.y = B.y + B.h * (geo.narrow ? 0.9 : 0.76)
  const vw = offsetIn(el.view, el.browser), vh = el.view.clientHeight
  // (Spaceman's still fills its first screen, whatever size that is.)
  el.view.style.setProperty('--vh', vh + 'px')
  for (const pg of pages) {
    const max = Math.max(0, pg.inner.offsetHeight - vh)
    pg.marks = { top: 0 }
    for (const m of pg.inner.querySelectorAll('[data-mark]')) {
      const o = offsetIn(m, pg.inner), k = m.dataset.mark
      pg.marks[k] = k === 'a' ? 0 : clamp(CENTRE.has(pg.i + k) ? o.y + o.h / 2 - vh / 2 : o.y - 16, 0, max)
    }
    // A phone's window is a few lines tall: a mark may name the one thing to put in the middle of it.
    if (geo.narrow) for (const m of pg.inner.querySelectorAll('[data-markn]')) {
      const o = offsetIn(m, pg.inner)
      pg.marks[m.dataset.markn] = clamp(o.y + o.h / 2 - vh / 2, 0, max)
    }
    memo.delete(pg.inner)
  }
  for (const A of ACTS) {
    const o = offsetIn(A.n, A.pg.inner)
    // (A like is its heart, at the left of its number.)
    A.at.x = B.x + vw.x + o.x + (A.a.act === 'like' ? 13 : o.w * 0.62)
    A.at.y = B.y + vw.y + o.y + o.h / 2 + 2 - (A.pg.marks[A.a.mark] ?? 0)
  }
  // The way out to the real Spaceman: the foot of his browser, on the right.
  places.egg.x = B.x + B.w - el.egg.offsetWidth - 12
  places.egg.y = B.y + B.h - el.egg.offsetHeight - 12
  // What the notes must keep clear around them: the card, and the bean over it.
  {
    const c = offsetIn(el.notesCard, el.page), nb = offsetIn(el.notesBean, el.page)
    const x0 = Math.min(c.x, nb.x), y0 = Math.min(c.y, nb.y)
    ambient.resize(live.W, live.H, { x: x0, y: y0, w: Math.max(c.x + c.w, nb.x + nb.w) - x0, h: Math.max(c.y + c.h, nb.y + nb.h) - y0 })
  }
  measureItems()
  renderer?.resize(live.W, live.H, pixelRatio())
}

// ——— everything in the DOM, as a function of show time ———
const ICON = {
  pause: '<svg viewBox="0 0 16 16"><path d="M3.500 2.500h3v11h-3zM9.500 2.500h3v11h-3z"/></svg>',
  play: '<svg viewBox="0 0 16 16"><path d="M4 2.200v11.600L13.500 8z"/></svg>',
  on: '<svg viewBox="0 0 20 20"><path d="M3 7.500h3l4-3.200v11.400l-4-3.200H3z"/><path d="M13 7a4.200 4.200 0 0 1 0 6M15.200 4.800a7.300 7.300 0 0 1 0 10.400" fill="none"/></svg>',
  off: '<svg viewBox="0 0 20 20"><path d="M3 7.500h3l4-3.200v11.400l-4-3.200H3z"/><path d="m13.200 7.600 4 4.800m0-4.800-4 4.800" fill="none"/></svg>',
}
const CLICKS = clicks(S)
const tip = { x: 0, y: 0, down: 0, alpha: 0, moving: 0 }
const GREY = [88, 88, 90], RED = [246, 122, 118]
const host = { x: 0, y: 0, R: 0 }
function tileAlpha(i, C) {
  if (G.HEADS !== undefined && F.TA !== undefined && i < count) return frame[G.HEADS + i * STRIDE + F.TA]
  return C.a
}
function hostAt() {
  const h = stage.host
  if (h && Number.isFinite(h.x)) { host.x = h.x; host.y = h.y; host.R = h.R; return host }
  const R = stage.rect, H = stage.headAt
  let x = 0, y = 0, rr = 0, n = 0
  for (let i = 0; i < 4 && i * 4 + 3 < R.length; i++) { x += R[i * 4]; y += R[i * 4 + 1]; rr += H?.[i * 3 + 2] ?? R[i * 4 + 3] * 0.6; n++ }
  host.x = x / n
  host.y = y / n
  host.R = rr / n
  return host
}
function compose(t) {
  if (state.phase === 'notes') return composeNotes(state.T)
  if (state.phase !== 'show') return
  const geo = stage.geo
  const gone = t >= cue.cut
  // Stop: black, chrome and all, for as long as the silence lasts.
  attr(el.page, 'data-black', gone)
  if (gone) return
  const C = stage.call, O = stage.oc

  // The lens moves the picture; the words on it go with it.
  if (G.BLUR2 !== undefined) {
    const sw = frame[G.BLUR2 + 3] || 0
    const lens = 'translate3d(' + (vfx.shakeX || 0).toFixed(2) + 'px,' + (vfx.shakeY || 0).toFixed(2) + 'px,0) scale(' + (1 - sw).toFixed(5) + ')'
    const cc = cache(el.call)
    if (cc.lens !== lens) {
      cc.lens = lens
      el.call.style.transformOrigin = (frame[G.BLUR2] || 0).toFixed(0) + 'px ' + (frame[G.BLUR2 + 1] || 0).toFixed(0) + 'px'
      el.call.style.transform = lens
    }
  }
  // While Hex's offer is the only thing to look at, the words on the call dim with it.
  const dim = 1 - 0.55 * S.dimAt(t)

  // The two windows' chrome rides the windows.
  put(el.winCall, C.x, C.y, C.a * dim, C.s)
  put(el.winOc, O.x, O.y, O.a)
  const browsing = t >= cue.browser + 0.2 && t < cue.closeTab
  attr(el.winCall, 'data-focus', t < cue.focus)
  attr(el.winOc, 'data-focus', t >= cue.focus && !browsing)
  text(el.people, String(S.peopleAt(t)))
  el.winRec.style.opacity = smooth((t - cue.rec[1] + 0.1) / 0.25).toFixed(2)
  // The session names itself once it has read the meeting.
  const named = t >= cue.send + 0.9 ? 'Q4 sync' : 'New session'
  text(el.ocTitle, named)
  text(el.railTitle, named)

  composeInvite(t)
  composeHex(t)

  // The pointer, a size larger while he sets things up.
  const cur = live.cursor
  const big = 1 + 0.22 * (1 - smooth((t - cue.send - 0.3) / 0.5))
  put(el.cursor, cur.x - 5, cur.y - 3, cur.alpha, big * (cur.down ? 0.86 : 1))
  // A click: one ring at the tip.
  let ck = -1
  for (const [k] of CLICKS) if (t >= k && t < k + 0.4) ck = k
  if (ck >= 0) {
    pointerAt(S, ck, places, tip)
    const k = (t - ck) / 0.38
    put(el.ripple, tip.x, tip.y, 0.9 * (1 - k) * (1 - k), 0.12 + 0.88 * easeOut(k))
  } else put(el.ripple, 0, 0, 0)

  // The prompt he copied rides the pointer across, and lands in the composer.
  if (t >= cue.copy + 0.1 && t < cue.paste + 0.24) {
    let x = cur.x + 14, y = cur.y + 14
    const grow = 0.6 + 0.4 * easeOut((t - cue.copy - 0.1) / 0.2)
    if (t >= cue.paste) {
      const k = easeOut((t - cue.paste) / 0.16)
      x = lerp(x, HX.chip.x, k)
      y = lerp(y, HX.chip.y - HX.carryH / 2, k)
    }
    put(el.carry, x, y, smooth((t - cue.copy - 0.1) / 0.1) * (1 - smooth((t - cue.paste - 0.16) / 0.08)), grow)
  } else put(el.carry, 0, 0, 0)

  // Name plates ride their tiles; once they are one (cue.them), so is the plate, and it reads Them.
  const n = Math.min(WHO.length, geo.tiles.length, stage.rect.length >> 2)
  for (let i = 0; i < WHO.length; i++) {
    const pl = el.plates[i]
    if (!pl) continue
    if (i >= n) { put(pl, 0, 0, 0); continue }
    // (A name is gone before its owner is: no plate is left lying over the mass.)
    const gone = i === ME ? 0 : i === 2 ? smooth((t - cue.late + 0.5) / 0.3) : smooth((t - cue.hit + 0.5) / 0.3)
    const R = stage.rect
    // (Me's waits until his tile is nearly in its seat: it does not ride the glide in.)
    const seated = i === ME ? smooth((S.seatAt(t) - 0.6) / 0.35) : 1
    put(pl, R[i * 4] - R[i * 4 + 2] + (geo.narrow ? 6 : 10), R[i * 4 + 1] + R[i * 4 + 3] - (geo.narrow ? 25 : 34), tileAlpha(i, C) * dim * (1 - gone) * seated)
  }
  if (t >= cue.them - 0.05) {
    const h = hostAt()
    let bottom = -1
    if (geo.grid) bottom = C.y + geo.grid.y + geo.grid.hh
    else for (let i = 0; i < Math.min(4, n); i++) bottom = Math.max(bottom, stage.rect[i * 4 + 1] + stage.rect[i * 4 + 3])
    // (Under its lower left lobe: the rope with the bead on it hangs from the middle.)
    put(el.host, h.x - h.R * 1.45, Math.min(h.y + h.R * 2.05, bottom - (geo.narrow ? 24 : 32)), clamp((t - cue.them) / 0.14) * C.a, 0.9 + 0.1 * crit(t - cue.them, 18))
  } else put(el.host, 0, 0, 0)

  // The session, and the names in it.
  applyStream(t)
  applyNames(t)
  vary(el.stream, '--beat', (stage.beat ?? 0) * clamp((stage.flesh ?? 0) * 2))
  vary(el.stream, '--spin', (t / 0.7) % 1)

  // The composer: in the middle of the empty session until Return, then at the bottom.
  const lift = M.lift * (1 - easeOut((t - cue.send) / 0.32))
  const cl = cache(el.composer)
  const lt = 'translate3d(0,' + lift.toFixed(1) + 'px,0)'
  if (cl.tr !== lt) { cl.tr = lt; el.composer.style.transform = lt }
  const first = t >= cue.focus && t < cue.send
  const chip = t >= cue.paste + 0.16 && t < cue.send
  const typed = first && t >= cue.type[0] ? PROMPT.slice(0, typedAt(PROMPT, cue.type[0], cue.type[1], t)) : ''
  text(el.typed, typed)
  attr(el.chip, 'data-on', chip)
  attr(el.ph, 'data-off', (t >= cue.paste && t < cue.send) || typed.length > 0)
  attr(el.composer, 'data-focus', first)
  const typing = t >= cue.type[0] && t < cue.type[1]
  vary(el.composer, '--caret', first && (typing || Math.sin(t * 7) > -0.2) ? 1 : 0)
  attr(el.auto, 'data-hot', t > cue.auto - 0.3 && t < cue.auto)
  attr(el.auto, 'data-down', t >= cue.auto && t < cue.auto + 0.12)
  attr(el.auto, 'data-on', t >= cue.auto + 0.04)
  vary(el.auto, '--on', crit(t - cue.auto - 0.02, 26))
  attr(el.send, 'data-lit', chip || typed.length > 0)
  attr(el.send, 'data-down', t >= cue.send - 0.03 && t < cue.send + 0.14)

  // The music player: not there until he wants it. Play, the bars on the track's own kicks
  // (taller with each stem), Pause.
  const pk = smooth((t - cue.player) / 0.45)
  const pr = stage.rect, mi = Math.min(ME, n - 1)
  put(el.player, (pr[mi * 4] || 0) + P.dx, (pr[mi * 4 + 1] || 0) + P.dy + (1 - pk) * 10, pk * C.a)
  const on = S.playing(t)
  attr(el.player, 'data-playing', on)
  attr(el.playerBtn, 'data-hot', (t > cue.play - 0.35 && t < cue.play + 0.5) || (t > cue.pause - 0.35 && t < cue.pause + 0.5))
  attr(el.playerBtn, 'data-down', (t >= cue.play && t < cue.play + 0.12) || (t >= cue.pause && t < cue.pause + 0.12))
  const heard = clamp(t - cue.play, 0, cue.pause - cue.play)
  text(el.playerTime, clock(heard).replace(/^0/, ''))
  vary(el.playerFill, '--k', heard / TRACK.length)
  const bt = on ? S.beatsAt(t) : 0, stem = Math.max(0, S.stageAt(t))
  for (let i = 0; i < el.bars.length; i++) {
    const f = (bt * (i % 3 === 0 ? 1 : i % 3 === 1 ? 2 : 0.5) + i * 0.29) % 1
    vary(el.bars[i], '--h', on ? Math.min(1, 0.2 + (0.42 + 0.14 * stem) * Math.pow(1 - f, 1.6) + 0.2 * Math.abs(Math.sin(bt * Math.PI * 0.5 + i * 1.9))) : 0.16)
  }

  // The browser: up over the lower part of OpenCode. Four tabs; the one in front, its page
  // where he has scrolled it to, and what he has clicked in it.
  const bk = live.browser
  attr(el.browser, 'data-on', bk > 0.003)
  if (bk > 0.003) {
    const B = geo.browser, shut = t >= cue.closeTab
    put(el.browser, B.x, B.y + (shut ? 0 : (1 - bk) * (geo.H - B.y + 30)), shut ? bk : Math.min(1, bk * 2.5), shut ? 0.965 + 0.035 * bk : 1)
    const front = S.tabAt(t)
    // Slack: alive; torn, one frame after another, as its workspace is deleted; dead.
    const sk = S.slackAt(t), dead = sk >= 1
    attr(pages[0].n, 'data-dead', dead)
    data(pages[0].n, 'glitch', sk > 0 && !dead ? TEAR[Math.min(TEAR.length - 1, Math.floor(sk * TEAR.length))] : '')
    attr(el.tabs[0], 'data-dead', dead)
    text(SLACK.tab, dead ? TABS[0].dead.short : TABS[0].short)
    if (dead) { const s = Math.floor(t - cue.slackDead); text(el.slackAgo, s + (s === 1 ? ' second ago' : ' seconds ago')) }
    text(el.url, front === 0 && dead ? TABS[0].dead.url : TABS[front].url)
    for (let i = 0; i < el.tabs.length; i++) {
      attr(el.tabs[i], 'data-on', i === front)
      const v = pages[i]?.visit
      attr(el.tabs[i], 'data-hot', !!v && i !== front && t > v.t - 0.3 && t < v.t)
    }
    attr(el.close, 'data-hot', t > cue.closeTab - 0.3)
    for (const pg of pages) {
      attr(pg.n, 'data-on', pg.i === front)
      if (pg.i !== front || !pg.visit) continue
      const y = (-scrollAt(pg.visit, t, pg.marks)).toFixed(1)
      const pc = cache(pg.inner)
      if (pc.y !== y) { pc.y = y; pg.inner.style.transform = 'translate3d(0,' + y + 'px,0)' }
    }
    for (const A of ACTS) {
      const at = A.a.t
      attr(A.n, 'data-hot', t > at - 0.4 && t < at + 0.7)
      attr(A.n, 'data-down', t >= at && t < at + 0.12)
      attr(A.n, 'data-on', t >= at + 0.06)
      if (A.also) attr(A.also, 'data-on', t >= at + 0.06)
      // Spaceman's workspace bar: the marker crosses to 2, and 2's windows take the screen.
      if (A.a.act === 'space') vary(A.n.closest('.sm-spaces') ?? A.n, '--ws', crit(t - at - 0.04, 20))
    }
  }
  // The way out to the real Spaceman, while that tab is in front: under the pointer when it
  // is over his browser (always, where there is no pointer to be over anything).
  const egg = eggAt(t)
  const over = egg && live.pointer.on && inBrowser(live.pointer.x, live.pointer.y)
  live.egg += ((egg && (over || coarse) ? 1 : 0) - live.egg) * Math.min(1, live.dt * 14)
  attr(el.page, 'data-egg', over)
  attr(el.egg, 'data-on', egg && live.egg > 0.02)
  put(el.egg, places.egg.x, places.egg.y + 4 * (1 - live.egg), egg ? live.egg * (coarse ? 0.9 : 1) : 0)
}
// Is the real Spaceman a click away at t: his browser is up, and its tab is the one in front.
const eggAt = (t) => state.phase === 'show' && t < cue.closeTab && live.browser > 0.5 && S.tabAt(t) === 1
function inBrowser(x, y) {
  const B = stage.geo.browser
  return x >= B.x && x <= B.x + B.w && y >= B.y && y <= B.y + B.h
}

// Slack's tab, and the frames its page tears through as it dies (tabs.css: data-glitch).
const SLACK = { tab: el.tabs[0].querySelector('em') }
const TEAR = ['1', '2', '1', '3', '2', '3', '3']

// Beat 0: the calendar's notification. In from the right on a spring with no bounce; Join lights
// under the pointer; and once he has clicked it, out the way it came as the call opens.
function composeInvite(t) {
  const N = stage.geo.notif
  const inK = crit(t - cue.invite, 15), out = t - cue.join - 0.12, outK = crit(out, 20)
  put(el.notif, N.right - N.w + 30 * (1 - inK) + 40 * outK, N.y, clamp((t - cue.invite) / 0.16) * (1 - clamp(out / 0.18)))
  attr(el.notifJoin, 'data-hot', t > cue.join - 0.32 && t < cue.join + 0.3)
  attr(el.notifJoin, 'data-down', t >= cue.join && t < cue.join + 0.12)
}

// Hex's capsule, from the offer to the pill to its menu. Every size is a spring with no bounce
// in it, and the width always leads the height by a few hundredths of a second.
function composeHex(t) {
  const cx = live.pill.x, py = live.pill.y, U = HX.u
  // The offer arrives: down a little, out of a little blur.
  const inK = crit(t - cue.offer, 17)
  // Record: the offer closes down into the pill.
  const u = t - cue.rec[0]
  const kw = crit(u, 14), kh = crit(u - 0.035, 13)
  // The menu, under the pointer: once to copy the prompt, once at the end to stop.
  const shut = 1 - crit(t - cue.copy - 0.62, 20)
  const ow = Math.max(crit(t - cue.hover, 19) * shut, crit(t - cue.hover2, 19))
  const oh = Math.max(crit(t - cue.hover - 0.035, 16) * shut, crit(t - cue.hover2 - 0.035, 16))
  const w = lerp(lerp(HX.ofW, HX.restW, kw), HX.openW, ow)
  const hh = lerp(HX.ofH, HX.head, kh)
  px(el.pill, 'width', w)
  px(el.pill, 'height', hh + oh * HX.bodyH)
  px(el.pill, 'borderRadius', lerp(lerp(14 * U, HX.head / 2, kh), 14 * U, ow))
  put(el.pill, cx - w / 2, py - hh / 2 - 8 * (1 - inK), clamp((t - cue.offer) / 0.14), 0.95 + 0.05 * inK)
  style(el.pill, 'filter', inK > 0.02 && inK < 0.97 ? 'blur(' + (4 * (1 - inK)).toFixed(1) + 'px)' : 'none')

  // What the offer says goes at once; what the pill says is there before the box has settled.
  // The button is on screen throughout: it is what becomes the red dot.
  el.offerIn.style.opacity = (1 - clamp(u / 0.085)).toFixed(2)
  px(el.offerIn, 'height', hh)
  px(el.pillHead, 'height', hh)
  px(el.pillHead, 'paddingLeft', lerp(18, 42, ow) * U)
  el.pillHead.style.opacity = clamp((u - 0.075) / 0.17).toFixed(2)
  el.pillX.style.opacity = clamp(ow * 1.6 - 0.3).toFixed(2)
  // (the clock waits for the button to have closed down to the dot beside it)
  el.clock.style.opacity = clamp((u - 0.3) / 0.16).toFixed(2)
  px(el.pillBody, 'top', hh)
  el.pillBody.style.visibility = oh > 0.004 ? 'visible' : 'hidden'
  el.pillBody.style.opacity = clamp(oh * 1.5).toFixed(2)

  // Record → the dot. Its box closes from the button's to the dot's as it crosses to where
  // the dot sits; its grey goes to red; its word is gone in a tenth of a second.
  const km = crit(u, 21)
  const bw = lerp(HX.rec0.w, HX.dot, km), bh = lerp(HX.rec0.h, HX.dot, km)
  const bx = lerp(w / 2 + HX.rec0.x, w - HX.dotR, km)
  const R = el.offerRec
  px(R, 'width', bw)
  px(R, 'height', bh)
  px(R, 'borderRadius', lerp(7 * U, 5 * U, km))
  const hot = t > cue.record - 0.3 && t < cue.record + 0.05 ? 18 : 0
  const ck = clamp(u / 0.22)
  style(R, 'backgroundColor', 'rgb(' + GREY.map((v, i) => Math.round(lerp(v + hot, RED[i], ck))).join(',') + ')')
  vary(R, '--btn', 1 - ck)
  el.recWord.style.opacity = (1 - clamp(u / 0.1)).toFixed(2)
  const down = t >= cue.record && t < cue.record + 0.1
  put(R, bx - bw / 2, hh / 2 - bh / 2, 1, down ? 0.96 : 1)

  text(el.clock, S.rec(t))
  vary(el.pill, '--alive', stage.fuse ?? 0)
  // The menu: Stop, Copy agent prompt (the one that is raised, and the one he wants), Copy transcript.
  attr(el.rowPrompt, 'data-hot', t > cue.hover + 0.14 && t < cue.copy + 0.85)
  attr(el.rowPrompt, 'data-down', t >= cue.copy && t < cue.copy + 0.12)
  attr(el.rowPrompt, 'data-copied', t >= cue.copy + 0.12 && t < cue.copy + 1.2)
  attr(el.rowStop, 'data-hot', t > cue.cut - 0.45)
  attr(el.rowStop, 'data-down', t > cue.cut - 0.05)
  drawScope(t)
}

// The notes: the card comes up and its rows arrive one at a time.
function composeNotes(T) {
  put(el.egg, 0, 0, 0)
  attr(el.egg, 'data-on', false)
  attr(el.page, 'data-egg', false)
  const k = smooth((T - cue.notes) / 0.5)
  el.notes.style.opacity = k.toFixed(3)
  el.notesCard.style.transform = 'translate3d(0,' + (26 * (1 - easeOut((T - cue.notes) / 0.9))).toFixed(1) + 'px,0)'
  el.notesRows.forEach((r, i) => {
    const a = smooth((T - cue.notes - 0.45 - i * 0.14) / 0.28)
    r.style.opacity = a.toFixed(2)
    r.style.transform = 'translate3d(0,' + (6 * (1 - a)).toFixed(1) + 'px,0)'
  })
}

// ——— the loop ———
let count = 0
function render() {
  const idle = state.phase === 'idle'
  live.phase = idle ? 'idle' : 'show'
  const t = idle ? -1 : state.T
  if (!idle) pointerAt(S, t, places, live.cursor)
  count = stage.perform(t, live, frame)
  if (!idle) layoutStream(t)
  let fx = null
  try { fx = vfx.compose(t, live, frame, count) } catch { vfx = NOFX }
  // The notes' backdrop, once they have settled: on the GPU if there is one (ambient.js), and
  // what the pointer has done to it either way.
  const lingering = state.phase === 'notes' && live.linger > 0
  const gpuNotes = lingering && renderer?.kind === 'WebGPU'
  if (!window.__hex?.hold) touch.step(live.dt, live.pointer, lingering)
  if (renderer) {
    try { renderer.draw(frame, count, fx ?? NOFRAME, gpuNotes ? backdrop(live.linger, live.reduced ? 0 : live.pageTime, live.W, live.H, ambient.hole, touch) : null) } catch {
      // Whatever it was, the show goes on in 2D.
      const was = renderer
      renderer = null
      if (was.kind === 'WebGPU') void mount(false)
    }
  }
  compose(state.T)
  ambient.draw(lingering && !gpuNotes ? live.linger : 0, live.pageTime, live.reduced, touch)
}

let last = 0, perfN = 0, perfT = 0, perfMs = 0
function tick(now) {
  requestAnimationFrame(tick)
  const dt = Math.min(0.1, Math.max(0, (now - last) / 1000))
  last = now
  if (document.hidden || window.__hex.hold) return
  live.dt = dt
  live.pageTime += dt
  if (state.playing) {
    // Show time is the sound's: it follows the audio clock while there is one (audio.js), so
    // a slow frame moves the picture on and never holds a voice back.
    state.T = audio.advance(state.T, dt)
    if (state.T >= S.END) {
      state.T = S.END
      state.playing = false
      setPhase(state.phase)
    }
    if (state.phase === 'show' && state.T >= cue.end) setPhase('notes')
  }
  audio.frame(state.T, state.playing)
  // The notes have settled a second and a half after they come up; from then, staying counts.
  live.linger = state.phase === 'notes' && state.T >= cue.notes + 1.6 ? live.linger + dt : 0
  const a = performance.now()
  render()
  if (!el.perf.hidden) {
    perfMs += performance.now() - a
    perfN++
    perfT += dt
    if (perfT > 0.5) {
      el.perf.textContent = (renderer?.kind ?? 'no GPU') + ' · ' + Math.round(perfN / perfT) + ' fps · cpu ' + (perfMs / perfN).toFixed(2) + ' ms' + (renderer?.gpuMs ? ' · gpu ' + renderer.gpuMs.toFixed(2) + ' ms' : '') + ' · t ' + state.T.toFixed(2)
      perfN = perfT = perfMs = 0
    }
  }
}

function setPhase(p) {
  state.phase = p
  // The notes are still the show as far as the stage is concerned: Me's bean hosts them.
  el.page.dataset.phase = p
  if (p !== 'show') el.page.removeAttribute('data-black'), (cache(el.page)['data-black'] = false)
  el.notes.setAttribute('aria-hidden', p !== 'notes' ? 'true' : 'false')
  if (p !== 'notes') el.notes.style.opacity = '0'
  el.play.innerHTML = state.playing ? ICON.pause : ICON.play
  el.play.setAttribute('aria-label', state.playing ? 'Pause' : 'Play')
}
function syncSound() {
  el.sound.innerHTML = audio.muted ? ICON.off : ICON.on
  el.sound.setAttribute('aria-label', audio.muted ? 'Unmute' : 'Mute')
}
function seek(t, play = true) {
  state.T = clamp(t, 0, S.END)
  state.playing = play && state.T < S.END
  stage.lastT = NaN
  audio.seek(state.T)
  if (state.playing) audio.resume()
  setPhase(state.T >= cue.end ? 'notes' : 'show')
}
function start() {
  if (scrollY > 0) scrollTo(0, 0)
  audio.init()
  seek(0)
}
function toggle() {
  if (state.phase === 'idle') return start()
  if (state.T >= S.END) return start()
  state.playing = !state.playing
  state.playing ? audio.resume() : audio.pause()
  setPhase(state.phase)
}
function mute() {
  audio.setMuted(!audio.muted)
  syncSound()
}

el.join.addEventListener('click', start)
el.join.addEventListener('pointerenter', () => (live.hoverJoin = true))
el.join.addEventListener('pointerleave', () => (live.hoverJoin = false))
el.replay.addEventListener('pointerenter', () => (live.hoverJoin = true))
el.replay.addEventListener('pointerleave', () => (live.hoverJoin = false))
el.replay.addEventListener('click', start)
el.play.addEventListener('click', toggle)
// Restart: from the top, now. (start() stops every sound that is playing or due before it
// starts anything: audio.js, seek() and stopAll().)
el.restart.addEventListener('click', start)
el.sound.addEventListener('click', mute)
// A button clicked with the pointer gives the keyboard back: Space is Play or Pause again, not
// that button a second time. (Reached with Tab, it keeps the focus.)
for (const b of [el.play, el.restart, el.sound]) b.addEventListener('click', (e) => { if (e.detail > 0) b.blur() })
function flash(icon) {
  el.flash.innerHTML = icon
  el.flash.removeAttribute('data-go')
  void el.flash.offsetWidth
  el.flash.setAttribute('data-go', '')
}
// The real Spaceman: the show stops where it is, and anoma.ly/spaceman opens beside it.
function openSpaceman() {
  if (state.playing) {
    state.playing = false
    audio.pause()
    setPhase(state.phase)
    flash(ICON.pause)
  }
  window.open(TABS[1].href, '_blank', 'noopener,noreferrer')
}
el.egg.addEventListener('click', (e) => {
  e.preventDefault()
  e.stopPropagation()
  openSpaceman()
})
// A click anywhere on the stage is Space: play or pause. (Not on the three buttons, a link or
// a button: those do what they say. And not on his browser while Spaceman is in it: that one
// pauses and opens the real page.)
el.page.addEventListener('click', (e) => {
  // (the path as it was when the click began: the mute button swaps its own icon on the way up)
  if (state.phase !== 'show' || e.composedPath().some((n) => n.matches?.('a, button, .bar, .notes__actions'))) return
  const r = el.page.getBoundingClientRect()
  if (eggAt(state.T) && inBrowser(e.clientX - r.left, e.clientY - r.top)) return openSpaceman()
  const was = state.playing
  toggle()
  flash(was ? ICON.pause : ICON.play)
})
// (A phone that was interrupted, by a call or by another app, gives its sound back on a touch.)
addEventListener('pointerdown', () => { if (state.playing) audio.resume() }, { passive: true })
addEventListener('keydown', (e) => {
  if (e.metaKey || e.ctrlKey || e.altKey) return
  const onButton = e.target instanceof HTMLButtonElement || e.target instanceof HTMLAnchorElement
  if (e.code === 'Space' && !onButton) { e.preventDefault(); toggle() }
  if (e.key === 'm' || e.key === 'M') mute()
  if (e.key === 'r' || e.key === 'R') start()
})
addEventListener('pointermove', (e) => {
  const r = el.page.getBoundingClientRect()
  live.pointer.x = e.clientX - r.left
  live.pointer.y = e.clientY - r.top
  live.pointer.on = true
})
document.documentElement.addEventListener('pointerleave', () => (live.pointer.on = false))
// (A finger that has lifted is not pressing on anything.)
for (const type of ['pointerup', 'pointercancel']) addEventListener(type, (e) => { if (e.pointerType !== 'mouse') live.pointer.on = false })
document.addEventListener('visibilitychange', () => {
  if (document.hidden) audio.pause()
  else if (state.playing) audio.resume()
})
matchMedia('(prefers-reduced-motion: reduce)').addEventListener('change', (e) => (live.reduced = e.matches))
addEventListener('resize', measure)
// The window moved to a display with a different pixel ratio: the canvas is sized again.
{
  let mq = null
  const watch = () => {
    mq?.removeEventListener?.('change', changed)
    mq = matchMedia('(resolution: ' + (devicePixelRatio || 1) + 'dppx)')
    mq.addEventListener?.('change', changed)
  }
  const changed = () => { watch(); measure() }
  watch()
}

el.perf.hidden = !q.has('perf')
syncSound()
measure()
document.fonts.ready.then(measure)
// The photographs in the browser's pages: decoded before the browser ever comes up, and the page
// measured again once its size is known.
// (And the four faces of Anomaly, which are backgrounds: fetched and decoded here too.)
const FACES = ['kit', 'dax', 'jay', 'aiden'].map((n) => { const i = new Image(); i.src = 'img/avatar-' + n + '.webp'; return i })
// (And Hex's field: the voices decoded, and its checkpoints laid down to Stop.)
const fieldReady = meter.ready.then(() => field.warm(cue.cut + 0.5)).catch(() => {})
Promise.all([...el.view.querySelectorAll('img'), ...FACES].map((n) => n.decode().catch(() => {})).concat(fieldReady)).then(() => { window.__hex.pictures = true; measure() })

// For the benchmark and for screenshots (perf/bench.mjs): one frame on demand, and a switch
// that stops the page's own loop from drawing.
window.__hex = {
  S, stage, vfx, frame, state, live, audio, meter, field, seek, restart: start, ambient, touch, items, M, places, get gpu() { return renderer }, get renderer() { return renderer?.kind ?? null }, failures: gpuMod?.GpuRenderer?.failures ?? null, hold: false, layout: { G, F, STRIDE },
  get heads() { return count },
  render,
  renderAt(t) {
    if (state.phase === 'idle' || (state.T >= cue.end) !== (t >= cue.end)) setPhase(t >= cue.end ? 'notes' : 'show')
    state.T = t
    live.dt = 1 / 60
    live.pageTime += 1 / 60
    render()
  },
  idle() {
    state.playing = false
    setPhase('idle')
  },
}

if (q.has('t')) {
  seek(+q.get('t') || 0, !q.has('paused'))
  if (!q.has('paused')) audio.init()
} else setPhase('idle')
requestAnimationFrame(tick)
