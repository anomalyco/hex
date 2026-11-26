// The 2D canvas renderer: the same frame (cast.js G) the WebGPU renderer draws, for browsers that
// have no WebGPU (older Safari, Firefox, no adapter, a lost device). Not a copy of the shaders:
// the same picture, simplified. Two windows on black, the tiles and the beans in them (lacquer,
// a rim of their own colour, eyes, lids, brows, mouths, what they wear, his headphones), and
// then the change: flesh, vessels, more eyes, the cords and filaments, the one mass, the
// heartbeat's light and rings, motes, and a vignette.
//
// Everything is a function of the frame, so any moment can be drawn cold. The only state is
// caches: a few small sprites baked once, and the vessels, tinted once. Big soft fills are what
// a canvas without a GPU pays for, so there are few of them: the vessels go down as plain blits,
// glows are small sprites, rings and the vignette fill only their own band.
import { G, F, STRIDE, MAX, TENDRIL } from './cast.js'
import { vesselCanvas } from './vessels.js'

const TAU = Math.PI * 2
const clamp = (v, a = 0, b = 1) => (v < a ? a : v > b ? b : v)
const mix = (a, b, k) => a + (b - a) * k
const sstep = (a, b, x) => { const k = clamp((x - a) / (b - a)); return k * k * (3 - 2 * k) }
const fract = (x) => x - Math.floor(x)
const hash11 = (n) => fract(Math.sin(n * 127.1 + 311.7) * 43758.5453)
const ch = (v) => Math.round(clamp(v) * 255)
const rgba = (r, g, b, a = 1) => 'rgba(' + ch(r) + ',' + ch(g) + ',' + ch(b) + ',' + clamp(a).toFixed(3) + ')'
// Paths and paint, in one call each.
const stroke = (c, style, w) => { c.strokeStyle = style; c.lineWidth = w; c.stroke() }
const fill = (c, style) => { c.fillStyle = style; c.fill() }
const oval = (c, x, y, rx, ry, a = 0) => { c.beginPath(); c.ellipse(x, y, rx, ry, a, 0, TAU) }
const disc = (c, x, y, r) => { c.beginPath(); c.arc(x, y, r, 0, TAU) }
const stops = (g, ...s) => { for (let i = 0; i < s.length; i += 2) g.addColorStop(s[i], s[i + 1]); return g }
const MINT = [0.2, 0.95, 0.56]
// Where the softbox hangs, as an angle on the egg (upper left).
const SOFTBOX = Math.atan2(-0.748, -0.664)
// Scratch for every outline (eggs, lids, mouths, filaments, swollen boxes): allocated once.
const PX = new Float32Array(512), PY = new Float32Array(512)
const EGG = 48
// A polyline through the scratch points from..to (exclusive), optionally shifted.
function line(c, from, to, dy = 0) {
  c.beginPath()
  c.moveTo(PX[from], PY[from] + dy)
  for (let i = from + 1; i < to; i++) c.lineTo(PX[i], PY[i] + dy)
}

// A small canvas, drawn once.
function sprite(size, paint) {
  const c = document.createElement('canvas')
  c.width = c.height = size
  paint(c.getContext('2d'), size)
  return c
}
// A soft round light of one colour (for 'lighter').
const glowSprite = (r, g, b) => sprite(64, (x, s) => {
  x.fillStyle = stops(x.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s / 2), 0, rgba(r, g, b, 1), 0.35, rgba(r, g, b, 0.38), 1, rgba(r, g, b, 0))
  x.fillRect(0, 0, s, s)
})
// An eyeball: ivory falling off to a cool grey round its curve. Real: yellowed and bloodshot.
const eyeSprite = (real) => sprite(96, (x, s) => {
  const g = x.createRadialGradient(s * 0.44, s * 0.4, 0, s / 2, s / 2, s / 2)
  x.fillStyle = real ? stops(g, 0, '#f2e6d2', 0.45, '#e8c9b4', 0.8, '#d8705e', 1, '#8f2a22') : stops(g, 0, '#f6f5ef', 0.55, '#e6e4dc', 0.85, '#a7adbd', 1, '#5e6680')
  x.fillRect(0, 0, s, s)
  if (!real) return
  // Capillaries, in from the rim.
  x.strokeStyle = 'rgba(170,12,12,0.75)'
  x.lineCap = 'round'
  for (let i = 0; i < 22; i++) {
    const a = hash11(i * 3.1) * TAU, b = a + (hash11(i * 5.9) - 0.5) * 0.7, r0 = s * 0.5, r1 = s * (0.18 + 0.2 * hash11(i * 7.7)), m = (r0 + r1) / 2
    x.lineWidth = 0.6 + 1.1 * hash11(i * 1.3)
    x.beginPath()
    x.moveTo(s / 2 + Math.cos(a) * r0, s / 2 + Math.sin(a) * r0)
    x.quadraticCurveTo(s / 2 + Math.cos(a + 0.2) * m, s / 2 + Math.sin(a + 0.2) * m, s / 2 + Math.cos(b) * r1, s / 2 + Math.sin(b) * r1)
    x.stroke()
  }
})
// A patch of wet meat where the lacquer has gone: dark in its creases, red on its swell, a glint.
const meatSprite = () => sprite(64, (x, s) => {
  x.fillStyle = stops(x.createRadialGradient(s * 0.42, s * 0.38, 0, s / 2, s / 2, s / 2), 0, 'rgba(196,40,36,1)', 0.45, 'rgba(120,10,16,1)', 0.8, 'rgba(40,2,6,0.95)', 1, 'rgba(20,0,3,0)')
  x.fillRect(0, 0, s, s)
  x.fillStyle = stops(x.createRadialGradient(s * 0.36, s * 0.32, 0, s * 0.36, s * 0.32, s * 0.12), 0, 'rgba(255,236,230,0.85)', 1, 'rgba(255,236,230,0)')
  x.fillRect(0, 0, s, s)
})

// A rounded box path (with the arcTo fallback where roundRect is missing).
function rrect(c, x, y, w, h, r) {
  r = Math.max(0, Math.min(r, w / 2, h / 2))
  if (c.roundRect) return c.roundRect(x, y, w, h, r)
  c.moveTo(x + r, y)
  c.arcTo(x + w, y, x + w, y + h, r)
  c.arcTo(x + w, y + h, x, y + h, r)
  c.arcTo(x, y + h, x, y, r)
  c.arcTo(x, y, x + w, y, r)
  c.closePath()
}
// A living edge: how far (px) a surface swells at a point (common.wgsl swell()).
const swell = (x, y, t, py, pb) => Math.sin(y * 0.013 + t * 0.9) * Math.sin(x * 0.011 - t * 0.7) * 5 + Math.sin(y * 0.031 - t * 1.3 + x * 0.017) * 2.5 + py * 5 + pb * 2.5
// A rounded box whose edge swells by amp × swell(): walked round, corner arcs and sides, each
// point pushed out along its normal.
function boxPath(c, cx, cy, hw, hh, r, amp, t, py, pb) {
  r = Math.max(0, Math.min(r, hw, hh))
  if (amp < 0.01) return rrect(c, cx - hw, cy - hh, hw * 2, hh * 2, r)
  const SX = [1, 1, -1, -1], SY = [-1, 1, 1, -1]
  let n = 0
  for (let i = 0; i < 4; i++) {
    const ox = cx + SX[i] * (hw - r), oy = cy + SY[i] * (hh - r), a0 = -Math.PI / 2 + (i * Math.PI) / 2, j = (i + 1) % 4
    for (let k = 0; k <= 4; k++) { const a = a0 + (k / 8) * Math.PI; PX[n] = ox + Math.cos(a) * r; PY[n++] = oy + Math.sin(a) * r }
    const nx = Math.cos(a0 + Math.PI / 2), ny = Math.sin(a0 + Math.PI / 2), sx = PX[n - 1], sy = PY[n - 1]
    const ex = cx + SX[j] * (hw - r) + nx * r, ey = cy + SY[j] * (hh - r) + ny * r, m = Math.min(40, Math.ceil(Math.hypot(ex - sx, ey - sy) / 26))
    for (let k = 1; k < m; k++) { PX[n] = sx + ((ex - sx) * k) / m; PY[n++] = sy + ((ey - sy) * k) / m }
  }
  c.moveTo(PX[0], PY[0])
  for (let i = 0; i < n; i++) {
    const dx = PX[i] - cx, dy = PY[i] - cy
    const qx = Math.max(Math.abs(dx) - (hw - r), 0) * Math.sign(dx), qy = Math.max(Math.abs(dy) - (hh - r), 0) * Math.sign(dy)
    const l = Math.hypot(qx, qy) || 1, s = amp * swell(PX[i], PY[i], t, py, pb)
    c.lineTo(PX[i] + (qx / l) * s, PY[i] + (qy / l) * s)
  }
  c.closePath()
}

export class Canvas2DRenderer {
  static create(canvas) {
    try {
      const ctx = canvas.getContext('2d', { alpha: false })
      return ctx ? new Canvas2DRenderer(canvas, ctx) : null
    } catch (e) {
      console.error('Hex Meetings: no 2D canvas. ' + (e?.message ?? e))
      return null
    }
  }

  constructor(canvas, ctx) {
    this.kind = 'Canvas2D'
    this.canvas = canvas
    this.ctx = ctx
    this.gpuMs = 0
    this.ms = 0 // how long the last draw() took, in JavaScript
    this.w = this.h = this.dpr = 1
    this.shx = this.shy = 0
    this.glows = new Map()
    this.eye = eyeSprite(false)
    this.eyeReal = eyeSprite(true)
    this.meat = meatSprite()
    this.vessels = null
    this.massHeads = []
    this.moteSprites = null
    // The vessels take a moment to grow: grow them while nothing is happening yet.
    if (typeof requestIdleCallback === 'function') requestIdleCallback(() => this.veins(), { timeout: 1500 })
    else setTimeout(() => this.veins(), 400)
    // The vignette's gradient lives in a unit space of its own, so it never changes.
    this.vig = stops(ctx.createRadialGradient(0, 0, 0.45, 0, 0, 0.98), 0, 'rgba(0,0,0,0)', 0.35, 'rgba(0,0,0,0.25)', 1, 'rgba(0,0,0,1)')
  }

  // The vessels (vessels.js), as crimson light with nothing between them, on a tile of 1024.
  veins() {
    if (this.vessels) return this.vessels
    const src = vesselCanvas(1024), x = src.getContext('2d', { willReadFrequently: true })
    const img = x.getImageData(0, 0, 1024, 1024), d = img.data
    for (let i = 0; i < d.length; i += 4) {
      const v = Math.min(1, Math.max(d[i] / 255, (d[i + 1] / 255) * 0.42) + (d[i + 2] / 255) * 0.3)
      d[i] = 222; d[i + 1] = 16 + 34 * v * v; d[i + 2] = 14; d[i + 3] = Math.round(v * 255)
    }
    x.putImageData(img, 0, 0)
    return (this.vessels = src)
  }
  // The vessels over a box of the page, shifted by (ox, oy): whole tiles, one texel to a device
  // pixel, at whole pixels, so every copy is a plain blit. Whatever clip is set keeps them in.
  // Leaves the transform reset; returns false if they have not grown yet.
  veinsOver(x, y, w, h, ox, oy, alpha) {
    const src = this.vessels, c = this.ctx, d = this.dpr, S = 1024
    if (!src || alpha < 0.004) return false
    const X0 = (x + this.shx) * d, Y0 = (y + this.shy) * d, X1 = X0 + w * d, Y1 = Y0 + h * d
    const sx = ((Math.round(ox * d) % S) + S) % S, sy = ((Math.round(oy * d) % S) + S) % S
    c.setTransform(1, 0, 0, 1, 0, 0)
    c.globalAlpha = alpha
    for (let ty = Math.floor((Y0 - sy) / S) * S + sy; ty < Y1; ty += S) for (let tx = Math.floor((X0 - sx) / S) * S + sx; tx < X1; tx += S) c.drawImage(src, Math.round(tx), Math.round(ty))
    return true
  }
  // A soft light sprite of one colour (sixteen steps a channel), made the first time it is asked for.
  glow(r, g, b) {
    const q = (v) => Math.round(clamp(v) * 15), k = q(r) * 256 + q(g) * 16 + q(b)
    let s = this.glows.get(k)
    if (!s) this.glows.set(k, (s = glowSprite(q(r) / 15, q(g) / 15, q(b) / 15)))
    return s
  }

  resize(w, h, dpr) {
    this.w = w
    this.h = h
    this.dpr = dpr
    const cw = Math.max(1, Math.round(w * dpr)), chh = Math.max(1, Math.round(h * dpr))
    if (this.canvas.width !== cw) this.canvas.width = cw
    if (this.canvas.height !== chh) this.canvas.height = chh
  }
  setTiming() {}
  done() { return Promise.resolve() }

  // Page px → canvas: translate, rotate, scale (non-uniform), with the lens's shake.
  place(x, y, a = 0, sx = 1, sy = 1) {
    const d = this.dpr, c = Math.cos(a), s = Math.sin(a)
    this.ctx.setTransform(d * c * sx, d * s * sx, -d * s * sy, d * c * sy, d * (x + this.shx), d * (y + this.shy))
  }
  // Into a head's own space: its middle, its roll, its radius, and (for the bean) its stretch.
  headSpace(o, stretch = true) {
    const f = this.f, R = f[o + F.R], st = stretch ? Math.max(f[o + F.STRETCH], 0.3) : 1
    this.place(f[o + F.CX], f[o + F.CY], f[o + F.ROLL], R / Math.sqrt(st), R * st)
  }

  draw(frame, count, fx) {
    const t0 = performance.now(), c = this.ctx, f = (this.f = frame)
    this.t = f[G.MISC]
    this.shx = f[G.VIEW + 2]
    this.shy = f[G.VIEW + 3]
    this.beat = f[G.PULSE]
    this.breath = f[G.PULSE + 1]
    this.alive = f[G.MISC + 3]
    if (this.alive > 0.002) this.veins()
    c.setTransform(1, 0, 0, 1, 0, 0)
    c.globalAlpha = 1
    c.globalCompositeOperation = 'source-over'
    c.fillStyle = '#070707'
    c.fillRect(0, 0, this.canvas.width, this.canvas.height)
    if (fx.body) this.body()
    for (let i = 0; i < Math.min(count, MAX); i++) this.tile(G.HEADS + i * STRIDE)
    if (fx.mass) this.mass()
    for (let i = 0; i < (fx.tendrils | 0); i++) this.filament(G.TENDRILS + i * TENDRIL)
    this.lights()
    if (fx.motes) this.motes()
    this.post()
    this.ms = performance.now() - t0
  }

  // ——— the desktop: two windows on black ———
  body() {
    const c = this.ctx, f = this.f, t = this.t, A = this.alive, beat = this.beat
    this.place(0, 0)
    for (let n = 0; n < 2; n++) {
      const o = G.WINS + n * 8, a = f[o + 4]
      if (a < 0.004) continue
      const cx = f[o], cy = f[o + 1], hw = f[o + 2], hh = f[o + 3], r = f[o + 5], live = f[o + 6], focus = f[o + 7]
      // A soft shadow under it (strokes: they only touch the band they darken).
      c.globalAlpha = a
      c.beginPath(); rrect(c, cx - hw, cy - hh + 4, hw * 2, hh * 2, r)
      c.strokeStyle = 'rgba(0,0,0,0.2)'
      for (let k = 1; k <= 3; k++) { c.lineWidth = k * 18; c.stroke() }
      // The pane, and a hairline.
      const pane = 0.068 + 0.008 * focus
      c.beginPath(); boxPath(c, cx, cy, hw, hh, r, A > 0.001 ? live : 0, t, this.breath, beat)
      fill(c, rgba(pane, pane, pane + 0.002))
      stroke(c, rgba(1, 1, 1, 0.1 + 0.045 * focus), 1.2 / this.dpr)
      if (live > 0.01 && A > 0.001) {
        // Alive: vessels under the glass, and a wet crimson edge.
        c.save(); c.clip()
        c.globalCompositeOperation = 'lighter'
        this.veinsOver(cx - hw - 40, cy - hh - 40, hw * 2 + 80, hh * 2 + 80, 0, t * 4, a * live * (0.06 + 0.16 * beat))
        this.place(0, 0)
        c.globalAlpha = a * live
        stroke(c, rgba(0.86, 0.07, 0.05, 0.1 + 0.14 * beat), 10 + 26 * A)
        c.globalCompositeOperation = 'source-over'
        stroke(c, 'rgba(4,3,4,0.85)', 4 + 8 * A)
        stroke(c, rgba(1, 0.75, 0.7, 0.16), 1.5)
        c.restore()
      }
    }
    c.globalAlpha = 1
    // Where the two windows have run together: a membrane across the gap, strands at first.
    const S = f[G.SEAM], A0 = G.WINS, B0 = G.WINS + 8
    if (S > 0.01 && f[A0 + 4] > 0.004 && f[B0 + 4] > 0.004) {
      const v = f[G.SEAM + 2] > 0.5 ? 1 : 0, u = 1 - v // stacked: the seam runs across
      const a0 = f[A0 + v] + f[A0 + 2 + v], gap = Math.max(1, f[B0 + v] - f[B0 + 2 + v] - a0)
      const lo = Math.max(f[A0 + u] - f[A0 + 2 + u], f[B0 + u] - f[B0 + 2 + u]), hi = Math.min(f[A0 + u] + f[A0 + 2 + u], f[B0 + u] + f[B0 + 2 + u])
      for (let s = lo + 20; s < hi - 20; s += 14) {
        const strand = sstep(0.3, 0.72, 0.5 + 0.5 * Math.sin(s * 0.019 + t * 0.31 + 1.3) * Math.sin(s * 0.0071 - t * 0.17 + 0.4))
        const k = S * S * 110 * mix(strand, 1, sstep(0.62, 1, S)), cover = clamp((k / 4 - gap / 2) / 4 + 0.5), pad = 6 * cover
        if (cover < 0.02) continue
        c.fillStyle = rgba(0.03 + 0.05 * beat * A, 0.012, 0.014, cover)
        if (v) c.fillRect(s, a0 - pad, 14.5, gap + pad * 2)
        else c.fillRect(a0 - pad, s, gap + pad * 2, 14.5)
      }
    }
    // The four tiles, once their walls are gone: one dark wet surface under them.
    const W = f[G.SEAM + 1]
    if (W > 0.001) {
      c.beginPath(); boxPath(c, f[G.GRID], f[G.GRID + 1], f[G.GRID + 2], f[G.GRID + 3], 18 + 14 * A, A, t, this.breath, beat)
      fill(c, rgba(0.018 + 0.03 * beat * A, 0.012, 0.014, sstep(0, 0.6, W)))
      stroke(c, rgba(0.7, 0.72, 0.8, 0.07 * W), 2)
    }
  }

  // ——— a tile: the room, and (unless the mass has it) the bean in it ———
  tile(o) {
    const c = this.ctx, f = this.f, t = this.t, alpha = f[o + F.ALPHA]
    if (alpha <= 0.002) return
    const tA = f[o + F.TA], fl = f[o + F.FLESH], wallK = f[o + F.FUSE], seed = f[o + F.SEED]
    const tx = f[o + F.TX], ty = f[o + F.TY], hw = f[o + F.THW], hh = f[o + F.THH], tr = f[o + F.TRAD]
    const rim = [f[o + F.RIMR], f[o + F.RIMG], f[o + F.RIMB]], keyK = f[o + F.KEY], warm = f[o + F.WARM]
    const room = () => { c.beginPath(); boxPath(c, tx, ty, hw, hh, tr, fl, t, this.breath, this.beat) }
    this.place(0, 0)
    if (tA > 0.002) {
      // A dark wall, a pool of their own light behind the head, and two lamps out of focus.
      c.globalAlpha = alpha * tA * (1 - 0.35 * sstep(0, 0.4, wallK))
      room()
      fill(c, stops(c.createLinearGradient(0, ty - hh, 0, ty + hh), 0, rgba(0.066 * (1 + 0.25 * warm), 0.072, 0.08 * (1 - 0.22 * warm)), 1, rgba(0.02, 0.022, 0.025)))
      c.save(); c.clip()
      c.globalCompositeOperation = 'lighter'
      const hx = f[o + F.CX], hy = f[o + F.CY] + hh * 0.1
      c.fillStyle = stops(c.createRadialGradient(hx, hy, 0, hx, hy, Math.max(hw, hh) * 1.1), 0, rgba(rim[0], rim[1], rim[2], 0.09 * keyK), 1, 'rgba(0,0,0,0)')
      c.fillRect(tx - hw, ty - hh, hw * 2, hh * 2)
      for (let k = 0; k < 2; k++) {
        const m = 0.35 + 0.4 * k
        disc(c, tx + hw * mix(-0.72, 0.72, hash11(seed * 3.1 + k * 7.7)), ty + hh * mix(-0.78, -0.3, hash11(seed * 5.3 + k * 2.9)), (0.11 + 0.09 * hash11(seed + k * 11)) * hh)
        fill(c, rgba(mix(rim[0], 1, m), mix(rim[1], 0.86, m), mix(rim[2], 0.66, m), 0.03))
      }
      // The room is alive too: vessels in from its border.
      const big = sstep(0, 0.15, fl) * (0.06 + 0.2 * sstep(0.34, 0.85, fl)) * (0.5 + 0.8 * f[o + F.BEAT])
      if (fl > 0.002) this.veinsOver(tx - hw - 30, ty - hh - 30, hw * 2 + 60, hh * 2 + 60, seed * 40, 0, alpha * tA * big)
      c.restore()
      // A hairline at rest; a ring while this face has the floor. Both go with the walls.
      const edges = 1 - sstep(0, 0.2, wallK), ring = f[o + F.RING]
      if (edges > 0.01) {
        this.place(0, 0)
        room()
        c.globalAlpha = alpha * tA * edges
        stroke(c, rgba(0.11, 0.115, 0.125, 1 - ring), 1)
        if (ring > 0.01) {
          const k = clamp(fl * 3), r = mix(0.78, 1, k), g = mix(0.84, 0.39, k), b = mix(0.94, 0.17, k)
          stroke(c, rgba(r, g, b, 0.9 * ring), 2)
          stroke(c, rgba(r, g, b, 0.05 * ring), 12)
        }
      }
    }
    if (f[o + F.INMASS] < 0.5) {
      // A face stays inside its tile until it is flesh; then it swells out over the edge.
      const framed = tA > 0.002, ext = wallK * 12, burst = sstep(0.25, 0.7, fl) * 30
      if (framed) {
        c.save()
        this.place(0, 0)
        c.beginPath(); boxPath(c, tx + f[o + F.SEAMSIDE] * ext, ty + f[o + F.SEAMY] * ext, hw + ext + burst, hh + ext + burst, tr + burst, fl, t, this.breath, this.beat)
        c.clip()
      }
      c.globalAlpha = alpha
      this.bean(o)
      if (f[o + F.HAT] > 0.5) this.hat(o, 1)
      if (f[o + F.PODA] > 0.002) this.phones(o)
      if (framed) c.restore()
    }
    // Dimmed, while Hex's offer is the only thing to look at.
    const dim = f[o + F.DIM]
    if (dim > 0.002 && tA > 0.002) {
      this.place(0, 0)
      c.globalAlpha = alpha * tA
      c.beginPath(); rrect(c, tx - hw - 1, ty - hh - 1, hw * 2 + 2, hh * 2 + 2, tr)
      fill(c, rgba(0.027, 0.027, 0.027, dim))
    }
    c.globalAlpha = 1
  }

  // The egg's outline, in its own space (head radii): squash, stretch, the jaw, and once it is
  // alive a slow wave round it and a lump or two.
  eggPoints(o, grow = 0) {
    const f = this.f, open = clamp(f[o + F.OPEN], 0, 1.35), o1 = Math.min(open, 1)
    const drop = sstep(0.22, 1, o1) * 0.15 + f[o + F.JAW] * 0.12 + Math.max(open - 1, 0) * 0.22
    const rx0 = f[o + F.RX], ry0 = f[o + F.RY], tp = f[o + F.TAPER], liq = f[o + F.LIQUID], time = f[o + F.TIME], seed = f[o + F.SEED]
    const sd = f[o + F.SEAMSIDE], sy = f[o + F.SEAMY], big = seed >= 5 ? 1 : 0, beat = f[o + F.BEAT]
    for (let i = 0; i < EGG; i++) {
      const a = (i / EGG) * TAU, ex = Math.cos(a), ey = Math.sin(a)
      let s = grow
      if (liq > 0.001) {
        const b1x = ex + 0.66 * sd, b1y = ey + 0.52 * sy, b2x = ex + 0.78 * sd, b2y = ey - 0.16 * sy - 0.2
        s += liq * 0.015 * Math.sin(a * 5 + time * 1.7 + seed) * (0.65 + 0.35 * Math.sin(time * 0.9 + seed))
        s += liq * (mix(0.095, 0.055, big) * Math.exp(-(b1x * b1x + b1y * b1y) * 7) + mix(0.05, 0.09, big) * Math.exp(-(b2x * b2x + b2y * b2y) * 8)) * (1 + 0.22 * beat)
      }
      const py = ey * ry0 * (ey > 0 ? 1 + drop : 1)
      PX[i] = ex * rx0 * (1 - tp * py) * (1 - 0.22 * drop * sstep(-0.15, 0.9, py)) * (1 + s)
      PY[i] = py * (1 + s)
    }
  }
  eggPath() {
    line(this.ctx, 0, EGG)
    this.ctx.closePath()
  }

  // ——— the bean: lacquer, the softbox in it, its rim, and (once it starts) the flesh ———
  bean(o) {
    const c = this.ctx, f = this.f, ga = c.globalAlpha
    const fl = f[o + F.FLESH], keyK = f[o + F.KEY], warm = f[o + F.WARM], roll = f[o + F.ROLL], rx = f[o + F.RX], ry = f[o + F.RY]
    const rim = [f[o + F.RIMR], f[o + F.RIMG], f[o + F.RIMB]], peel = sstep(0.12, 0.85, fl)
    this.headSpace(o)
    this.eggPoints(o)
    this.eggPath()
    // Obsidian lacquer: lit from the fill on the upper left, nearly black round its curve.
    const body = c.createRadialGradient(-0.3 * rx, -0.42 * ry, 0, -0.1, -0.1, 1.25)
    fill(c, stops(body, 0, rgba((0.12 + 0.03 * warm) * keyK, (0.125 + 0.005 * warm) * keyK, (0.142 - 0.04 * warm) * keyK), 0.55, rgba(0.05 * keyK, 0.05 * keyK, 0.058 * keyK), 1, rgba(0.008, 0.008, 0.011)))
    c.save(); c.clip()
    if (fl > 0.002) this.flesh(o, fl, f[o + F.CHIMERA])
    // The softbox in the clear coat: a curved window high on the left, a small one on the right.
    // They stay where the studio is when the head tips.
    const sb = f[o + F.GLOSS] * keyK * (1 - peel), a0 = SOFTBOX - roll, a1 = Math.atan2(-0.66, 0.5) - roll
    if (sb > 0.01) {
      c.save()
      c.scale(rx, ry)
      c.lineCap = 'round'
      c.globalAlpha = ga * sb * 0.1
      c.beginPath(); c.arc(0, 0, 0.8, a0 - 0.24, a0 + 0.24); stroke(c, '#f2f6ff', 0.2)
      c.globalAlpha = ga * sb * 0.9
      c.beginPath(); c.arc(0, 0, 0.8, a0 - 0.23, a0 + 0.2); stroke(c, '#f2f6ff', 0.075)
      c.globalAlpha = ga * sb * 0.4
      oval(c, Math.cos(a1) * 0.83, Math.sin(a1) * 0.83, 0.075, 0.04, a1 + Math.PI / 2); fill(c, '#c8ccd6')
      c.restore()
    }
    // The rim: light from behind in the face's own colour, on the side that faces it; a little
    // of the room's cool on the far side.
    const ra = f[o + F.RIMANG], dx = Math.cos(ra), dy = Math.sin(ra), rk = f[o + F.RIMS] * (1 - 0.45 * peel)
    const rg = stops(c.createLinearGradient(-dx, -dy, dx, dy), 0, rgba(0.42, 0.52, 0.7, 0.18 * keyK * (1 - peel)), 0.4, 'rgba(0,0,0,0)', 0.62, rgba(rim[0], rim[1], rim[2], 0.12), 1, rgba(rim[0], rim[1], rim[2], 1))
    c.globalCompositeOperation = 'lighter'
    this.eggPath()
    c.globalAlpha = ga * clamp(0.5 * rk); stroke(c, rg, 0.2)
    c.globalAlpha = ga * clamp(0.95 * rk); stroke(c, rg, 0.045)
    // (Light comes through the meat where it is thin: red, at its edge.)
    if (fl > 0.002) { c.globalAlpha = ga * peel * (0.3 + 0.25 * f[o + F.BEAT]); stroke(c, '#f2120a', 0.14) }
    c.globalCompositeOperation = 'source-over'
    c.globalAlpha = ga
    c.restore()
    this.face(o)
  }

  // Flesh on a bean (clipped to it): the vessels lit from inside, then all of it, patches of meat
  // that grow from where it was entered, wet glints, and the holes the filaments made.
  flesh(o, fl, chim) {
    const c = this.ctx, f = this.f, ga = c.globalAlpha, seed = f[o + F.SEED], beat = f[o + F.BEAT], R = f[o + F.R] * 1.3
    c.globalCompositeOperation = 'lighter'
    if (this.veinsOver(f[o + F.CX] - R, f[o + F.CY] - R, R * 2, R * 2.3, seed * 90, seed * 33, ga * sstep(0.1, 0.5, fl) * (0.45 + 0.9 * beat))) this.headSpace(o)
    c.globalAlpha = ga * fl * (0.1 + 0.25 * beat)
    c.fillStyle = '#d4120c'
    c.fillRect(-1.6, -1.6, 3.2, 3.2)
    c.globalCompositeOperation = 'source-over'
    const whole = sstep(0.42, 0.85, fl)
    if (whole > 0.01) {
      c.globalAlpha = ga * whole
      c.fillStyle = stops(c.createRadialGradient(-0.25, -0.35, 0.05, 0, 0, 1.15), 0, chim > 0.5 ? '#86141c' : '#bc2a26', 0.6, chim > 0.5 ? '#40060c' : '#6c0c12', 1, '#170204')
      c.fillRect(-1.6, -1.6, 3.2, 3.2)
      c.globalCompositeOperation = 'lighter' // one broad soft sheen where it faces the light
      c.globalAlpha = ga * whole * 0.22
      c.drawImage(this.glow(1, 0.85, 0.8), -0.75, -0.85, 0.7, 0.6)
      c.globalCompositeOperation = 'source-over'
    }
    const ex = f[o + F.ENTRYX], ey = f[o + F.ENTRYY]
    c.globalAlpha = ga * (0.85 + 0.15 * (1 - chim))
    for (let i = 0; i < 14; i++) {
      const a = hash11(seed * 13.1 + i * 3.7) * TAU, d = Math.sqrt(hash11(seed * 7.3 + i * 1.9)) * 0.85, px = Math.cos(a) * d, py = Math.sin(a) * d * 1.1
      const r = clamp((fl - 0.06 - 0.5 * hash11(seed + i * 9.1) - 0.18 * Math.hypot(px - ex, py - ey)) / 0.3) * (0.22 + 0.22 * hash11(i * 4.3 + seed))
      if (r > 0.01) c.drawImage(this.meat, px - r, py - r, r * 2, r * 2)
    }
    const wet = sstep(0.5, 0.85, fl)
    c.globalAlpha = ga * wet * 0.8
    for (let i = 0; i < 4 && wet > 0.01; i++) {
      const drift = fract(f[o + F.TIME] * (0.05 + 0.04 * hash11(i)) + hash11(i * 3.3)) * 0.4
      oval(c, -0.55 + 0.9 * hash11(seed * 2.3 + i), -0.6 + 0.9 * hash11(seed * 4.1 + i * 2.1) + drift, 0.028, 0.04)
      fill(c, 'rgba(255,240,235,0.8)')
    }
    // Where each filament went in (vfx.js lands them here): a swollen ring, black down the hole,
    // and the agent's light under the skin.
    const burrow = sstep(0.3, 0.42, fl) * (1 - 0.5 * chim), ys = f[o + F.SEAMY]
    const holes = f[o + F.SEAMSIDE] < 0 ? [0.6, -0.14, 0.5, -0.44, 0.58, 0.24] : [0.4, 0.66 * ys, 0.56, 0.48 * ys, 0.22, 0.8 * ys]
    for (let k = 0; k < 6 && burrow > 0.01; k += 2) {
      const hx = holes[k], hy = holes[k + 1]
      c.globalCompositeOperation = 'lighter'
      c.globalAlpha = ga * burrow * (0.3 + 0.4 * beat)
      c.drawImage(this.glow(MINT[0], MINT[1], MINT[2]), hx - 0.25, hy - 0.25, 0.5, 0.5)
      c.globalCompositeOperation = 'source-over'
      c.globalAlpha = ga * burrow
      disc(c, hx, hy, 0.105); stroke(c, '#4c0620', 0.05)
      disc(c, hx, hy, 0.06); fill(c, '#070102')
    }
    c.globalAlpha = ga
  }

  // ——— the face: eyes, brows, more eyes, glasses, mouth (on a bean, or on the mass) ———
  face(o) {
    const c = this.ctx, f = this.f, real = f[o + F.REAL], keyK = f[o + F.KEY], sp = f[o + F.SPACING]
    const third = f[o + F.THIRD], extra = f[o + F.XEYES], sd = f[o + F.SEED], gx = f[o + F.GAZEX], gy = f[o + F.GAZEY], pd = f[o + F.PUPIL]
    this.headSpace(o)
    c.save()
    c.translate(f[o + F.FACEX], f[o + F.FACEY])
    this.eyeAt(o, -1, sp, real, keyK)
    this.eyeAt(o, 1, sp, real, keyK)
    if (f[o + F.BROWS] > 0.004) { this.brow(o, -1, sp, keyK); this.brow(o, 1, sp, keyK) }
    // More eyes: one in the forehead, which stays; three more while it is at its worst.
    this.orb(0.03 * Math.sin(sd * 3), -0.665, 0.15 * (0.62 + 0.62 * fract(sd * 0.73)), third, gx, gy, pd, 0, keyK)
    if (extra > 0.004) {
      this.orb(-0.5, -0.5, 0.112, extra, gx + 0.2, gy * 0.8, pd, -0.42, keyK)
      this.orb(0.47, -0.545, 0.098, extra * extra, gx - 0.25, gy + 0.1, pd, 0.36, keyK)
      this.orb(-0.63 * f[o + F.SEAMSIDE], 0.165, 0.074, extra * extra, gx * 0.7, gy * 0.7, pd, 0, keyK)
    }
    if (f[o + F.GLASSES] > 0.5) this.glasses(o, sp, keyK)
    this.mouth(o, real, keyK)
    c.restore()
  }

  // An eye: a ball in a socket, its two lids, a dark glassy iris (amber once real), catchlights.
  eyeAt(o, side, sp, real, keyK) {
    const c = this.ctx, f = this.f, ga = c.globalAlpha, right = side > 0
    const size = f[o + F.EYE] * (1 + side * f[o + F.UNEVEN]), erx = 0.16 * size, ery = 0.206 * size, cx = side * sp, cy = -0.105
    const lidU = f[o + (right ? F.LIDR : F.LIDL)], lidL = f[o + (right ? F.LOWR : F.LOWL)], ang = f[o + (right ? F.ANGR : F.ANGL)]
    // The socket: the lacquer dips to meet it, and catches the light on its lower wall.
    oval(c, cx, cy, erx + 0.045, ery + 0.045); fill(c, 'rgba(0,0,0,0.55)')
    if (real > 0.002) stroke(c, rgba(0.44, 0.07, 0.058, 0.75 * real), 0.06)
    c.beginPath(); c.ellipse(cx, cy, erx + 0.02, ery + 0.02, 0, 0, 1.9)
    stroke(c, real > 0.5 ? rgba(0.7, 0.22, 0.19, 0.55 * keyK) : rgba(0.3, 0.31, 0.345, 0.7 * keyK), 0.014)
    // The lids: lacquer domes (raw, once real).
    oval(c, cx, cy, erx, ery); fill(c, real > 0.5 ? rgba(0.3 * keyK, 0.045, 0.04) : rgba(0.07 * keyK, 0.072 * keyK, 0.082 * keyK))
    // The opening: inside the ball, under the upper lid's curve, over the lower's (in the
    // eye's own unit space; the lower edge goes in PY[200..]).
    let open = 0
    for (let i = 0; i <= 14; i++) {
      const x = -1 + i / 7, e = Math.sqrt(Math.max(0, 1 - x * x)), yLo = 1.06 - lidL * (0.98 - 0.36 * x * x)
      const yUp = mix(-1.06, yLo + 0.04, lidU) + mix(0.24, -0.26, lidU) * x * x - ang * 0.42 * x * side * (1 - 0.6 * lidU)
      PX[i] = PX[200 + i] = x
      PY[i] = Math.max(-e, yUp)
      PY[200 + i] = Math.max(PY[i], Math.min(e, yLo))
      open = Math.max(open, PY[200 + i] - PY[i])
    }
    if (open < 0.01) return
    c.save()
    c.translate(cx, cy)
    c.scale(erx, ery)
    line(c, 0, 15)
    for (let i = 14; i >= 0; i--) c.lineTo(PX[200 + i], PY[200 + i])
    c.closePath()
    c.save(); c.clip()
    c.globalAlpha = ga * keyK
    c.drawImage(this.eye, -1, -1, 2, 2)
    if (real > 0.002) { c.globalAlpha = ga * real; c.drawImage(this.eyeReal, -1, -1, 2, 2) }
    c.globalAlpha = ga
    line(c, 0, 15, -0.12); stroke(c, 'rgba(0,0,0,0.3)', 0.42) // the upper lid's shadow on the ball
    // The iris and pupil, where it is looking; the softbox (sliding down with the lid) and its echo.
    const ix = (f[o + F.GAZEX] * 0.058 * size) / erx, iy = (f[o + F.GAZEY] * 0.066 * size) / ery, ri = 0.086 * size * (1 + 0.16 * real)
    const irx = ri / erx, iry = ri / ery, rp = mix(0.6 + 0.3 * f[o + F.PUPIL], 0.24 + 0.4 * f[o + F.PUPIL], real), w = f[o + F.WARM]
    oval(c, ix, iy, irx, iry); fill(c, rgba(mix(0.06, 0.62, real) * keyK, mix(0.07, 0.32, real) * keyK, mix(0.09, 0.1, real) * keyK))
    stroke(c, 'rgba(0,0,0,0.55)', irx * 0.25)
    oval(c, ix, iy, irx * rp, iry * rp); fill(c, '#010101')
    oval(c, ix - 0.36 * irx, iy + mix(-0.37, 0.16, sstep(0.25, 0.8, lidU)) * iry, 0.24 * irx, 0.15 * iry, -0.55); fill(c, rgba(1, mix(1, 0.86, w), mix(1, 0.66, w), 0.35 + 0.65 * keyK))
    oval(c, ix + 0.37 * irx, iy + 0.4 * iry, 0.11 * irx, 0.11 * iry); fill(c, rgba(0.86, 0.9, 0.96, 0.8 * keyK))
    c.restore()
    // The lid's own fine edge (and a wet line along the lower one, once real).
    line(c, 0, 15); stroke(c, real > 0.5 ? rgba(0.56, 0.1, 0.08, 0.8) : rgba(0.3 * keyK, 0.305 * keyK, 0.33 * keyK, 0.85), 0.07)
    if (real > 0.3) { line(c, 200, 215); stroke(c, rgba(1, 0.92, 0.88, 0.45 * real * keyK), 0.05) }
    c.restore()
  }

  // A brow: an arched, tapered tube (thinner at its outer end), with its shadow on the forehead.
  brow(o, side, sp, keyK) {
    const c = this.ctx, f = this.f, ga = c.globalAlpha, right = side > 0, browK = f[o + F.BROWS]
    const up = f[o + (right ? F.UPR : F.UPL)], a = f[o + (right ? F.ANGR : F.ANGL)] * 0.62, arch = f[o + (right ? F.ARCHR : F.ARCHL)]
    const size = f[o + F.EYE] * (1 + side * f[o + F.UNEVEN]), Xx = side * Math.cos(a), Xy = -Math.sin(a), Yx = side * Math.sin(a), Yy = Math.cos(a)
    const bx = side * (sp + 0.012), by = -0.105 - 0.29 * size - 0.05 - up * 0.12, L = 0.172, T = 0.05 * f[o + F.WEIGHT]
    // Upper edge out, lower edge back; the stroke's round joins make the round ends.
    for (let n = 0; n < 22; n++) {
      const pass = n > 10 ? 1 : 0, x = pass ? 1 - (n - 11) / 5 : -1 + n / 5
      const yc = -arch * L * (1 - x * x), th = T * 0.55 * mix(1, 0.62, sstep(-0.3, 1, x)) * (pass ? 1 : -1)
      PX[n] = bx + Xx * x * L + Yx * (yc + th)
      PY[n] = by + Xy * x * L + Yy * (yc + th)
    }
    c.lineJoin = 'round'
    for (let k = 0; k < 2; k++) {
      c.save()
      if (!k) c.translate(0.016, 0.04)
      line(c, 0, 22)
      c.closePath()
      c.globalAlpha = ga * browK * (k ? 1 : 0.55)
      const col = k ? rgba(0.84 * keyK, 0.83 * keyK, 0.79 * keyK) : '#000'
      fill(c, col)
      stroke(c, col, T * 0.9)
      c.restore()
    }
    c.globalAlpha = ga * browK
    line(c, 1, 10); stroke(c, rgba(1, 1, 1, 0.3 * keyK), T * 0.4) // lit along its top
    c.globalAlpha = ga
  }

  // An eye that has pushed out through the flesh: a wet ball in a ring of swollen lid.
  orb(x, y, r, open, gx, gy, pd, a, keyK) {
    if (open < 0.004) return
    const c = this.ctx, ga = c.globalAlpha, half = Math.min(1, open * 1.02), ix = gx * r * 0.3, iy = gy * r * 0.3, ri = r * 0.5
    c.save()
    c.translate(x, y)
    c.rotate(-a)
    c.globalAlpha = ga * sstep(0, 0.3, open)
    disc(c, 0, 0, r * 1.17); stroke(c, rgba(0.46 * keyK, 0.075, 0.06, 0.85), r * 0.36)
    c.beginPath(); c.arc(0, 0, r * 1.15, 3.6, 4.6); stroke(c, rgba(1, 0.9, 0.86, 0.4 * keyK), r * 0.1)
    disc(c, 0, 0, r); fill(c, rgba(0.34 * keyK, 0.05, 0.048))
    if (half > 0.02) {
      oval(c, 0, 0, r, r * half)
      c.save(); c.clip()
      c.drawImage(this.eyeReal, -r, -r, r * 2, r * 2)
      disc(c, ix, iy, ri); fill(c, rgba(0.62 * keyK, 0.33 * keyK, 0.1 * keyK))
      stroke(c, 'rgba(30,8,2,0.7)', ri * 0.22)
      disc(c, ix, iy, ri * (0.3 + 0.34 * pd)); fill(c, '#010101')
      oval(c, ix - 0.34 * ri, iy - 0.36 * ri, 0.22 * ri, 0.14 * ri, -0.55); fill(c, rgba(1, 1, 1, 0.9 * keyK))
      c.restore()
      oval(c, 0, 0, r, r * half); stroke(c, 'rgba(140,16,14,0.8)', r * 0.08)
    }
    c.restore()
  }

  // Gold wire glasses. Knocked crooked, a lens cracked, and then sunk into the flesh.
  glasses(o, sp, keyK) {
    const c = this.ctx, f = this.f, ga = c.globalAlpha, crook = f[o + F.CROOK]
    const lens = (s, d = 0) => { c.beginPath(); rrect(c, s * sp - 0.232 + d * 0.5, -0.18 + d, 0.464, 0.36, 0.072) }
    c.save()
    c.translate(0.03 * crook, -0.11 + 0.07 * crook)
    c.rotate(-0.21 * crook)
    c.globalAlpha = ga * (1 - 0.6 * f[o + F.SUNK])
    c.lineJoin = 'round'
    for (let s = -1; s <= 1; s += 2) {
      lens(s); fill(c, rgba(0.85, 0.92, 1, 0.05 * keyK)) // the softbox in the glass
      lens(s, 0.022); stroke(c, 'rgba(0,0,0,0.4)', 0.028) // the wire's shadow
    }
    const gold = rgba(0.86 * keyK, 0.6 * keyK, 0.2 * keyK)
    for (let s = -1; s <= 1; s += 2) {
      lens(s); stroke(c, gold, 0.025)
      c.beginPath(); c.moveTo(s * (sp + 0.232), -0.03); c.lineTo(s, -0.075); c.stroke()
      c.beginPath(); c.moveTo(s * sp - 0.17, -0.183); c.lineTo(s * sp + 0.15, -0.183); stroke(c, rgba(1, 0.9, 0.62, 0.7 * keyK), 0.01)
    }
    c.beginPath(); c.ellipse(0, -0.005, sp - 0.236, (sp - 0.236) / 1.6, 0, Math.PI, TAU); stroke(c, gold, 0.025)
    if (f[o + F.CRACK] > 0.5) {
      // Three bright lines from where it was hit.
      const x0 = sp + 0.07, y0 = -0.05
      c.beginPath()
      c.moveTo(x0, y0); c.lineTo(sp + 0.21, -0.16)
      c.moveTo(x0, y0); c.lineTo(sp - 0.14, 0.15)
      c.moveTo(x0, y0); c.lineTo(sp + 0.19, 0.12)
      c.moveTo(sp - 0.03, 0.05); c.lineTo(sp - 0.2, -0.02)
      stroke(c, rgba(1, 0.98, 0.94, 0.85), 0.009)
    }
    c.restore()
  }

  // The mouth: closed, a line carved in the lacquer; open, a cavity with teeth and a tongue.
  mouth(o, real, keyK) {
    const c = this.ctx, f = this.f, ga = c.globalAlpha
    const open = f[o + F.OPEN], wide = f[o + F.WIDE], rnd = f[o + F.ROUND], smile = f[o + F.SMILE], smirk = f[o + F.SMIRK], jaw = f[o + F.JAW]
    const o1 = Math.min(open, 1), yell = Math.max(open - 1, 0), raw = real > 0.3
    const W = (0.215 + 0.08 * wide + 0.115 * o1 * (0.4 + 0.6 * wide) + 0.03 * Math.max(smile, 0)) * (1 - 0.46 * rnd) * (1 - 0.3 * jaw * real)
    const hU = (0.01 + 0.05 * rnd) * o1 + 0.02 * yell + 0.22 * jaw * o1 * real, hL = (0.47 * o1 + 0.13 * yell) * (1 + 0.1 * rnd)
    const pL = mix(0.86, 0.62, rnd), H = hU + hL, openK = sstep(0.02, 0.07, H)
    const yC = (t) => -smile * 0.105 * t * t - smirk * 0.075 * t * (0.55 + 0.45 * Math.abs(t))
    // Upper lip in PY[0..20], lower in PY[100..120], and the edge of the teeth (top row, then
    // bottom) in PY[300..320] and PY[400..420].
    for (let i = 0; i <= 20; i++) {
      const t = -1 + i / 10, rem = Math.max(1 - t * t, 0), y = yC(t)
      PX[i] = PX[100 + i] = PX[300 + i] = PX[400 + i] = t * W
      PY[i] = y - hU * Math.pow(rem, 0.6)
      PY[100 + i] = y + hL * Math.pow(rem, pL)
      PY[300 + i] = PY[i] + Math.min(0.07 + 0.05 * o1, H * 0.45) * Math.pow(rem, 0.3) * (1 - 0.35 * real)
      PY[400 + i] = PY[100 + i] - Math.min(0.05 + 0.03 * o1, H * 0.3) * Math.pow(rem, 0.3)
    }
    const band = (a, b) => { line(c, a, a + 21); for (let i = 20; i >= 0; i--) c.lineTo(PX[b + i], PY[b + i]); c.closePath() }
    c.save()
    c.translate(smirk * 0.03, 0.362 + 0.04 * o1)
    c.lineJoin = c.lineCap = 'round'
    if (openK > 0.01) {
      c.globalAlpha = ga * openK
      band(0, 100)
      fill(c, raw ? rgba(0.16 * real, 0.012, 0.014) : '#020203')
      // The lips: a raised rim round it; red, once there is flesh to make them of.
      stroke(c, raw ? rgba(0.55, 0.08, 0.08, 0.9) : rgba(0.36 * keyK, 0.37 * keyK, 0.4 * keyK, 0.7), raw ? 0.045 : 0.022)
      c.save(); c.clip()
      // The tongue on the floor of it.
      const tw = W * (0.52 - 0.12 * rnd), tH = Math.max(hL * (0.4 + 0.22 * f[o + F.TONGUE]), 0.02), cyT = hL - tH * 0.26 + yC(0)
      if (hL > 0.15) {
        c.globalAlpha = ga * openK * sstep(0.15, 0.3, hL) * (1 - 0.7 * rnd)
        // Two lobes, one fill each (so they never double where they overlap).
        const lobes = (dx, dy, rx, ry) => { c.beginPath(); for (let s = -1; s <= 1; s += 2) { c.moveTo(s * dx + rx, dy); c.ellipse(s * dx, dy, rx, ry, 0, 0, TAU) } }
        lobes(tw * 0.4, cyT, tw * 0.62, tH); fill(c, rgba(0.86 * keyK, 0.3 * keyK, 0.32 * keyK))
        lobes(tw * 0.38, cyT - tH * 0.5, tw * 0.3, tH * 0.22); fill(c, rgba(1, 0.6, 0.56, 0.35 * keyK))
      }
      // A strip of teeth along the top (a lower row too, once real).
      c.globalAlpha = ga * openK * sstep(0.03, 0.08, H)
      const teeth = raw ? rgba(0.9 * keyK, 0.86 * keyK, 0.74 * keyK) : rgba(0.97 * keyK, 0.97 * keyK, 0.955 * keyK)
      band(0, 300); fill(c, teeth)
      if (raw && hL > 0.2) { band(400, 100); fill(c, teeth) }
      c.restore()
    }
    if (openK < 0.99) {
      // Closed: a dark carved line, the light caught along its lower lip.
      c.globalAlpha = ga * (1 - openK)
      line(c, 1, 20); stroke(c, raw ? 'rgba(40,3,4,0.95)' : 'rgba(0,0,0,0.92)', 0.022)
      for (let i = 0; i <= 20; i++) PY[300 + i] = yC(-1 + i / 10) + 0.022
      line(c, 303, 318); stroke(c, raw ? rgba(0.6, 0.12, 0.1, 0.6) : rgba(0.5 * keyK, 0.51 * keyK, 0.54 * keyK, 0.55), 0.012)
    }
    // The creases a smile pushes into the cheeks.
    const crease = clamp(Math.max(smile, 0) * 1.2 * sstep(0.15, 0.6, o1))
    c.globalAlpha = ga * crease * 0.7
    for (let s = -1; s <= 1 && crease > 0.02; s += 2) {
      c.beginPath(); c.arc(s * (W - 0.045), yC(1) + 0.03, 0.085, s > 0 ? -0.9 : Math.PI - 0.2, s > 0 ? 0.2 : Math.PI + 0.9)
      stroke(c, rgba(0.2 * keyK, 0.205 * keyK, 0.22 * keyK), 0.014)
    }
    c.restore()
    c.globalAlpha = ga
  }

  // What they wear: 1 a knitted hat, 2 a top-knot and a headset. In the head's space unstretched.
  hat(o, worn) {
    const c = this.ctx, f = this.f, ga = c.globalAlpha, fl = f[o + F.FLESH], keyK = 0.45 + 0.55 * f[o + F.KEY]
    const rx = f[o + F.RX], ry = f[o + F.RY], rim = [f[o + F.RIMR], f[o + F.RIMG], f[o + F.RIMB]]
    this.headSpace(o, false)
    c.globalAlpha = ga * worn
    if (f[o + F.HAT] < 1.5) {
      // The wool goes to oxblood with the rest of them.
      const k = 0.55 * sstep(0.2, 0.8, fl), dim = (1 - 0.35 * f[o + F.CHIMERA]) * keyK
      const wool = (m) => rgba(mix(0.6, 0.3, k) * dim * m, mix(0.4, 0.035, k) * dim * m, mix(0.085, 0.035, k) * dim * m)
      const rxh = rx * 0.845, dry = ry * 0.54, by = -0.56 - dry - 0.07
      // The dome, ribbed up to the crown.
      c.beginPath(); c.ellipse(0, -0.56, rxh, dry, 0, Math.PI, TAU); c.closePath()
      fill(c, stops(c.createRadialGradient(-0.3, -0.95, 0.02, 0, -0.6, 0.95), 0, wool(1.35), 0.6, wool(0.8), 1, wool(0.3)))
      c.save(); c.clip()
      c.beginPath()
      for (let i = -7; i <= 7; i++) { const x0 = (i / 7.5) * rxh; c.moveTo(x0, -0.5); c.quadraticCurveTo(x0 * 1.02, -0.56 - dry * 0.7, x0 * 0.3, -0.56 - dry) }
      stroke(c, 'rgba(0,0,0,0.22)', 0.022)
      c.restore()
      c.beginPath(); c.ellipse(0, -0.56, rxh, dry, 0, Math.PI + 1.6, TAU); stroke(c, rgba(rim[0], rim[1], rim[2], 0.35), 0.02)
      // The bobble; the cuff, folded up, and its shadow on the brow.
      disc(c, 0.06, by, 0.135); fill(c, stops(c.createRadialGradient(0, by - 0.05, 0.01, 0.06, by, 0.14), 0, wool(1.5), 1, wool(0.45)))
      c.beginPath(); rrect(c, -rxh - 0.035, -0.64, (rxh + 0.035) * 2, 0.19, 0.085); fill(c, 'rgba(0,0,0,0.35)')
      c.beginPath(); rrect(c, -rxh - 0.035, -0.68, (rxh + 0.035) * 2, 0.19, 0.085); fill(c, stops(c.createLinearGradient(0, -0.68, 0, -0.49), 0, wool(1.1), 1, wool(0.5)))
      c.beginPath()
      for (let x = -rxh; x <= rxh; x += 0.054) { c.moveTo(x, -0.665); c.lineTo(x, -0.505) }
      stroke(c, 'rgba(0,0,0,0.2)', 0.014)
    } else {
      // The top-knot: lacquer, a band round its root. Gone once the flesh has it.
      const kn = 1 - sstep(0.25, 0.6, fl), ky = -ry - 0.115
      if (kn > 0.01) {
        c.globalAlpha = ga * worn * kn
        oval(c, -0.03, ky, 0.205, 0.183); fill(c, stops(c.createRadialGradient(-0.1, ky - 0.08, 0.01, -0.03, ky, 0.21), 0, rgba(0.2 * keyK, 0.205 * keyK, 0.23 * keyK), 0.6, rgba(0.05, 0.05, 0.058), 1, rgba(0.01, 0.01, 0.013)))
        oval(c, -0.1, ky - 0.09, 0.05, 0.028, -0.6); fill(c, 'rgba(240,244,255,0.75)')
        c.beginPath(); c.ellipse(-0.03, ky, 0.205, 0.183, 0, Math.PI * 0.9, Math.PI * 1.6); stroke(c, rgba(rim[0], rim[1], rim[2], 0.8), 0.022)
        c.beginPath(); c.moveTo(-0.17, ky + 0.15); c.lineTo(0.11, ky + 0.15); stroke(c, rgba(rim[0] * 0.55 + 0.1, rim[1] * 0.55 + 0.1, rim[2] * 0.55 + 0.1, 0.9), 0.04)
      }
      // The headset, on the side away from the others: a boom round the cheek, a capsule, foam.
      const gone = 1 - sstep(0.45, 0.8, fl), s = -f[o + F.SEAMSIDE] || -1, ex = s * (rx - 0.035)
      if (gone > 0.01) {
        c.globalAlpha = ga * worn * gone
        c.lineCap = c.lineJoin = 'round'
        c.beginPath(); c.moveTo(ex, 0.12); c.lineTo(s * (rx + 0.02), 0.44); c.lineTo(s * 0.4, 0.52)
        stroke(c, '#1b1c21', 0.042)
        stroke(c, 'rgba(230,236,255,0.4)', 0.01)
        c.beginPath(); rrect(c, ex - 0.058, -0.135, 0.116, 0.27, 0.058); fill(c, rgba(0.15 * keyK, 0.155 * keyK, 0.175 * keyK))
        stroke(c, rgba(rim[0], rim[1], rim[2], 0.4), 0.012)
        disc(c, s * 0.4, 0.52, 0.082); fill(c, '#0d0e10')
      }
    }
    c.globalAlpha = ga
  }

  // His headphones: over-ear, graphite, lowered onto him from above. A band over the crown
  // (metal at the sides, padded on top), a slider, a cushion and a shell on each ear.
  phones(o) {
    const c = this.ctx, f = this.f, ga = c.globalAlpha, k = f[o + F.PODS], keyK = 0.45 + 0.55 * f[o + F.KEY], warm = f[o + F.WARM]
    const rim = rgba(f[o + F.RIMR], f[o + F.RIMG], f[o + F.RIMB], 0.45), open = (1 - k) * 0.1, top = 1.5 * Math.PI
    const shade = (v) => rgba(v * keyK * (1 + 0.1 * warm), v * keyK, v * keyK * (1.1 - 0.25 * warm))
    this.headSpace(o, false)
    c.translate(0, -(1 - k) * 0.62)
    c.globalAlpha = ga * f[o + F.PODA]
    c.lineCap = 'butt'
    for (let s = -1; s <= 1; s += 2) {
      const a0 = s < 0 ? Math.PI + 0.33 : top, a1 = s < 0 ? top : TAU - 0.33
      c.save()
      c.translate(s * open, 0)
      c.beginPath(); c.ellipse(0, 0.02, 0.992, 1.052, 0, a0, a1); stroke(c, shade(0.16), 0.07)
      c.beginPath(); c.ellipse(0, 0.02, 0.992, 1.052, 0, s < 0 ? top - 0.62 : top, s < 0 ? top : top + 0.62); stroke(c, shade(0.075), 0.11)
      c.beginPath(); c.ellipse(0, 0.02, 1.03, 1.09, 0, a0 + 0.05, a1 - 0.05); stroke(c, rgba(1, 1, 1, 0.2 * keyK), 0.012)
      c.beginPath(); c.moveTo(s * 0.968, -0.4); c.lineTo(s * 0.99, -0.18); stroke(c, shade(0.36), 0.04)
      c.beginPath(); rrect(c, s * 0.858 - 0.064, -0.227, 0.128, 0.524, 0.064); fill(c, shade(0.06))
      c.beginPath(); rrect(c, s * 1.005 - 0.118, -0.265, 0.236, 0.6, 0.106)
      fill(c, stops(c.createLinearGradient(s * 0.887, -0.27, s * 1.12, 0.33), 0, shade(0.24), 0.5, shade(0.12), 1, shade(0.05)))
      stroke(c, rim, 0.014)
      c.beginPath(); rrect(c, s * 1.005 - 0.084, -0.231, 0.168, 0.532, 0.075); stroke(c, shade(0.55), 0.01)
      c.restore()
    }
    c.globalAlpha = ga
  }

  // ——— the one mass the four become ———
  mass() {
    const c = this.ctx, f = this.f, t = this.t, beat = this.beat, heads = this.massHeads
    const hx = f[G.META], hy = f[G.META + 1], R0 = f[G.META + 2], k = f[G.META + 3]
    heads.length = 0
    for (let i = 0; i < 4; i++) { const o = G.HEADS + i * STRIDE; if (f[o + F.INMASS] > 0.5) heads.push(o) }
    if (!heads.length) return
    const chim = f[heads[0] + F.CHIMERA], gx = f[G.GRID], gy = f[G.GRID + 1], ghw = f[G.GRID + 2], ghh = f[G.GRID + 3]
    // Its own crimson light round it.
    this.place(0, 0)
    c.globalCompositeOperation = 'lighter'; c.globalAlpha = clamp(k * (0.45 + 0.5 * beat) * (1 - 0.3 * chim))
    c.drawImage(this.glow(1, 0.12, 0.06), hx - R0 * 3, hy - R0 * 3, R0 * 6, R0 * 6)
    c.globalCompositeOperation = 'source-over'; c.globalAlpha = 1
    // It keeps to the grid (it is one room now).
    c.save()
    c.beginPath(); rrect(c, gx - ghw - 2, gy - ghh - 2, ghw * 2 + 4, ghh * 2 + 4, 18)
    c.clip()
    // One outline: all four eggs, grown a little as they run together, the middle between them,
    // the lumps and the drips. (All wound the same way, so the overlaps fill.)
    c.beginPath()
    for (const o of heads) {
      this.eggPoints(o, 0.06 * k)
      const R = f[o + F.R], st = Math.max(f[o + F.STRETCH], 0.3), a = f[o + F.ROLL], ca = Math.cos(a), sa = Math.sin(a), sq = Math.sqrt(st)
      for (let i = 0; i < EGG; i++) {
        const x = (PX[i] / sq) * R, y = PY[i] * st * R, wx = f[o + F.CX] + ca * x - sa * y, wy = f[o + F.CY] + sa * x + ca * y
        if (i) c.lineTo(wx, wy); else c.moveTo(wx, wy)
      }
      c.closePath()
    }
    if (heads.length === 4 && k > 0.05) {
      const q = [0, 1, 3, 2], kl = sstep(0.25, 0.9, k) * (1 + 0.07 * beat)
      for (let i = 0; i < 4; i++) { const o = G.HEADS + q[i] * STRIDE; if (i) c.lineTo(f[o + F.CX], f[o + F.CY]); else c.moveTo(f[o + F.CX], f[o + F.CY]) }
      c.closePath()
      const lump = (i, ox, oy, r) => {
        const o = G.HEADS + i * STRIDE, R = f[o + F.R], rr = R * r * kl, x = f[o + F.CX] + R * ox, y = f[o + F.CY] + R * oy
        if (rr >= 0.5) { c.moveTo(x + rr, y); c.arc(x, y, rr, 0, TAU) }
      }
      lump(0, -0.72, -0.5, 0.37)
      lump(3, 0.7, 0.46, 0.3 * sstep(0.3, 0.9, f[G.HEADS + 3 * STRIDE + F.META]))
      lump(1, 0.4, -0.86, 0.26)
      lump(2, -0.8, 0.2, 0.22 * sstep(0.3, 0.9, f[G.HEADS + 2 * STRIDE + F.META]))
    }
    this.drips(hx, hy, R0, k)
    // Its edge: thin enough for the light to come through red (under the fill, so only its outside shows).
    c.globalCompositeOperation = 'lighter'; c.globalAlpha = 0.22 + 0.15 * beat
    stroke(c, '#ff1a0c', 12)
    c.globalAlpha = 0.6 + 0.3 * beat; c.lineWidth = 4; c.stroke()
    c.globalCompositeOperation = 'source-over'; c.globalAlpha = 1
    // Wet red meat: bright where it swells toward the light, liver-dark round its edge.
    fill(c, stops(c.createRadialGradient(hx - R0 * 0.5, hy - R0 * 0.7, R0 * 0.1, hx, hy, R0 * 2.3), 0, '#c4242a', 0.35, chim > 0.5 ? '#8a1018' : '#7c0e14', 0.75, '#3c050a', 1, '#180205'))
    c.save(); c.clip()
    // The vessels, lit from inside, and the heartbeat in them.
    c.globalCompositeOperation = 'lighter'
    this.veinsOver(hx - R0 * 2.6, hy - R0 * 2.6, R0 * 5.2, R0 * 5.2, t * 3, 0, 0.35 + 0.5 * beat)
    this.place(0, 0)
    c.globalCompositeOperation = 'source-over'
    // Sinew along the cross where they meet: long swells, dark in their folds, wet along one.
    c.lineCap = 'round'
    for (let n = -1; n <= 1; n += 2) {
      const yy = hy + R0 * (0.08 + n * 0.16), xx = hx + R0 * (n * 0.14 + 0.02)
      c.globalAlpha = 0.32
      c.beginPath(); c.moveTo(hx - R0 * 1.6, yy); c.bezierCurveTo(hx - R0 * 0.5, yy + R0 * 0.12, hx + R0 * 0.5, yy - R0 * 0.08, hx + R0 * 1.6, yy + R0 * 0.04)
      c.moveTo(xx, hy - R0 * 1.8); c.bezierCurveTo(xx + R0 * 0.1, hy - R0 * 0.6, xx - R0 * 0.1, hy + R0 * 0.6, xx + R0 * 0.04, hy + R0 * 1.9)
      stroke(c, '#1a0205', R0 * 0.035)
      c.globalAlpha = 0.16
      c.beginPath(); c.moveTo(hx - R0 * 1.6, yy - R0 * 0.04); c.bezierCurveTo(hx - R0 * 0.5, yy + R0 * 0.08, hx + R0 * 0.5, yy - R0 * 0.12, hx + R0 * 1.6, yy)
      stroke(c, '#ffd8d0', R0 * 0.012)
    }
    // Patches of the old lacquer, sunk in it, dark and glossy (never over a face); wet glints.
    for (let i = 0; i < 9; i++) {
      const a = hash11(i * 5.1) * TAU, d = (0.6 + 0.9 * hash11(i * 2.7)) * R0, px = hx + Math.cos(a) * d * 1.05, py = hy + Math.sin(a) * d, r = R0 * (0.22 + 0.16 * hash11(i * 8.3))
      let near = 9
      for (const o of heads) near = Math.min(near, Math.hypot(px - f[o + F.CX], py - f[o + F.CY]) / f[o + F.R])
      if (near < 0.95) continue
      c.globalAlpha = 0.55 * (0.5 + 0.5 * chim)
      oval(c, px, py, r, r * 0.8, a); fill(c, '#12030a')
      c.globalAlpha = 0.5
      oval(c, px - r * 0.35, py - r * 0.4, r * 0.25, r * 0.08, -0.6); fill(c, '#e8b4b0')
    }
    c.globalAlpha = 0.8
    c.fillStyle = '#fff2ee'
    for (let i = 0; i < 16; i++) {
      const r = R0 * (0.012 + 0.02 * hash11(i * 9.7))
      oval(c, hx + (hash11(i * 3.3) * 2 - 1) * R0 * 1.6, hy + (hash11(i * 6.1) * 2 - 1) * R0 * 1.7 + fract(t * 0.06 + hash11(i)) * R0 * 0.3, r, r * 1.5)
      c.fill()
    }
    c.restore()
    // Whoever's part of the mass it is: their faces, and what they had on, going under.
    for (const o of heads) this.face(o)
    const worn = 1 - sstep(0.42, 0.85, k)
    if (worn > 0.01) for (const o of heads) if (f[o + F.HAT] > 0.5) this.hat(o, worn)
    // The big one in the middle. It opens last.
    const ce = f[G.HOST2 + 3], a0 = heads[0]
    if (ce > 0.004) {
      this.place(hx + R0 * 0.03, hy + R0 * 0.04, 0, R0, R0)
      this.orb(0, 0, 0.3, ce, f[a0 + F.GAZEX], f[a0 + F.GAZEY], f[a0 + F.PUPIL], 0, 1)
    }
    c.restore()
    // The rope and its bead hang out past the grid.
    if (f[G.HOST2] > 0) {
      this.place(0, 0)
      c.beginPath(); this.rope(hx, hy, R0); fill(c, '#7a0c12')
      c.globalCompositeOperation = 'lighter'
      stroke(c, 'rgba(255,40,20,0.5)', 1.5)
      c.globalCompositeOperation = 'source-over'
    }
  }

  // What drips off the two lower lobes: a thread that lengthens, a bead that lets go.
  drips(hx, hy, R, k) {
    const c = this.ctx, f = this.f, t = f[G.HEADS + F.DRIP]
    for (let n = 0; n < 8 && k > 0.35; n++) {
      const o = G.HEADS + (2 + (n % 2)) * STRIDE, on = sstep(0.35, 0.9, f[o + F.META])
      if (on <= 0) continue
      const hcx = (f[o + F.CX] - hx) / R, hcy = (f[o + F.CY] - hy) / R, off = mix(-0.62, 0.62, (Math.floor(n * 0.5) + 0.2 + 0.6 * fract(n * 0.618 + 0.13)) / 4)
      const ph = fract(t / (3.6 + 1.9 * hash11(n * 7.1)) + hash11(n * 1.9)), hang = sstep(0, 0.8, ph), fall = Math.max(ph - 0.8, 0) / 0.2
      const ys = hcy + Math.sqrt(Math.max(1 - (off / 0.8) ** 2, 0)) * f[o + F.RY], tip = ys + 0.03 + (0.16 + 0.34 * hash11(n * 5.7)) * mix(hang, 0.5, sstep(0, 0.3, fall)) * on
      const x = hx + (hcx + off) * R, top = hy + ys * 0.9 * R, bot = hy + tip * R, w = 0.036 * on * R
      c.moveTo(x - w, top); c.lineTo(x + w, top); c.lineTo(x + w * 0.4, bot); c.lineTo(x - w * 0.4, bot); c.closePath()
      const by = hy + (tip + 0.02 + fall * fall * 1.6) * R, br = (0.026 + 0.048 * hang) * on * (1 - 0.3 * fall) * R
      if (br > 0.5) { c.moveTo(x + br, by); c.ellipse(x, by, br, br / 0.72, 0, 0, TAU) }
    }
  }
  // One long rope from the middle, with a heavy bead on it that swings, and on goodbye lets go.
  rope(hx, hy, R) {
    const c = this.ctx, f = this.f, t = f[G.HEADS + F.DRIP], fall = f[G.HOST2 + 1], f1 = Math.min(fall, 1), flat = sstep(1, 1.15, fall)
    const topX = hx + R * 0.05, topY = hy + R, tipX = topX + R * 0.035 * Math.sin(t * 1.3) * (1 - f1), w0 = R * 0.064, w1 = R * 0.014
    const tipY = mix(f[G.HOST2] + R * 0.03 * this.beat, topY + R * 0.4, sstep(0, 0.7, fall))
    c.moveTo(topX - w0, topY); c.lineTo(topX + w0, topY); c.lineTo(tipX + w1, tipY); c.lineTo(tipX - w1, tipY); c.closePath()
    const by = mix(f[G.HOST2] + R * 0.03 * this.beat, f[G.HOST2 + 2], f1 * f1) + R * 0.05 + R * 0.03 * flat
    const br = R * 0.085 * (1 + 0.1 * this.beat) * (1 - sstep(1.3, 2, fall)), bw = br * (1 + 0.9 * flat)
    if (br > 0.5) { c.moveTo(tipX + bw, by); c.ellipse(tipX, by, bw, br / mix(0.8, 2.2, flat), 0, 0, TAU) }
  }

  // ——— a filament: a tapered cubic, dark and wet, with light running down it ———
  filament(o) {
    const c = this.ctx, f = this.f, t = this.t, g = f[o + 8], r0 = f[o + 9], kind = f[o + 11]
    if (g <= 0.002 || r0 <= 0) return
    const x0 = f[o], y0 = f[o + 1], x1 = f[o + 2], y1 = f[o + 3], x2 = f[o + 4], y2 = f[o + 5], x3 = f[o + 6], y3 = f[o + 7]
    const len = f[o + 10], rate = f[o + 15], cr = f[o + 12], cg = f[o + 13], cb = f[o + 14], id = (o - G.TENDRILS) / TENDRIL, N = 28, n = N + 1
    const thick = sstep(1, 3, r0), growing = g < 0.985 && rate > -0.5
    this.place(0, 0)
    c.lineCap = c.lineJoin = 'round'
    // A fibre grows from its root; a strand or a cord grows from both ends and meets in the middle.
    for (let hf = 0; hf < (kind > 0.5 ? 2 : 1); hf++) {
      const s0 = kind > 0.5 && hf ? 1 - g * 0.5 : 0, s1 = kind > 0.5 ? (hf ? 1 : g * 0.5) : g
      // Its two edges (PY[0..], PY[300..]), its core (200..) and its lit side (450..).
      for (let i = 0; i <= N; i++) {
        const s = s0 + ((s1 - s0) * i) / N, k = 1 - s
        const px = k * k * k * x0 + 3 * k * k * s * x1 + 3 * k * s * s * x2 + s * s * s * x3, py = k * k * k * y0 + 3 * k * k * s * y1 + 3 * k * s * s * y2 + s * s * s * y3
        let dx = 3 * k * k * (x1 - x0) + 6 * k * s * (x2 - x1) + 3 * s * s * (x3 - x2), dy = 3 * k * k * (y1 - y0) + 6 * k * s * (y2 - y1) + 3 * s * s * (y3 - y2)
        const dl = Math.hypot(dx, dy) || 1
        dx /= dl; dy /= dl
        let r
        if (kind > 1.5) r = r0 * (0.46 + 0.54 * Math.pow(Math.abs(2 * s - 1), 2.2)) * (0.86 + 0.14 * Math.sin(s * 31 + len)) * sstep(0, 0.5, g)
        else if (kind > 0.5) r = r0 * (0.2 + 0.8 * Math.pow(Math.abs(2 * s - 1), 1.8))
        else {
          // A fibre: thick at the root, a bulb where it is growing, knots, thinning where it lands.
          const bulb = 1 + mix(1.5, 0.35, sstep(2, 6, r0)) * Math.exp(-(((s - g) / 0.022) ** 2)) * (growing ? 1 : 0)
          const land = g > 0.96 ? mix(1, 0.12 + 0.88 * sstep(0, 0.04, 1 - s), sstep(0.96, 1, g)) : 1
          r = r0 * (1 + 0.7 * Math.exp(-s / 0.03)) * mix(1, 0.42, sstep(0, 1, s)) * bulb * (1 + 0.16 * Math.sin(s * 23 + id * 2.9) * Math.sin(s * 7.3 + id)) * land
        }
        r = Math.max(r, 0.45)
        PX[i] = px - dy * r; PY[i] = py + dx * r
        PX[300 + i] = px + dy * r; PY[300 + i] = py - dx * r
        PX[200 + i] = px; PY[200 + i] = py
        PX[450 + i] = px - dy * r * 0.45; PY[450 + i] = py + dx * r * 0.45
      }
      line(c, 0, n)
      for (let i = N; i >= 0; i--) c.lineTo(PX[300 + i], PY[300 + i])
      c.closePath()
      if (kind > 1.5) {
        // Sinew: dark red meat, in shadow underneath, wet along its top.
        const gain = cr / 0.62
        fill(c, rgba(0.3 * gain, 0.035 * gain, 0.03 * gain))
        line(c, 300, 300 + n); stroke(c, 'rgba(0,0,0,0.35)', Math.max(0.6, r0 * 0.5))
        line(c, 450, 450 + n); stroke(c, rgba(1, 0.86, 0.82, 0.32), Math.max(0.5, r0 * 0.22))
        continue
      }
      // Black lacquer (sinew-red once thick), a wet line of light along it.
      fill(c, thick > 0.5 ? '#1c0507' : rgba(0.02 + 0.5 * cr * (1 - thick), 0.01 + 0.2 * cg * (1 - thick), 0.012, 0.6 + 0.4 * thick))
      if (thick > 0.2) { line(c, 450, 450 + n); stroke(c, rgba(1, 0.92, 0.9, 0.24 * thick), Math.max(0.6, r0 * 0.24)) }
      // The core, and the light running down it toward the call in packets.
      c.globalCompositeOperation = 'lighter'
      line(c, 200, 200 + n)
      stroke(c, rgba(cr, cg, cb, mix(0.55, 0.12, thick)), Math.max(0.8, r0 * mix(1.2, 0.5, thick)))
      const pk = Math.max(14, len * 0.045)
      c.setLineDash([pk, pk * 2.6])
      c.lineDashOffset = -(t * 160 + id * 37)
      stroke(c, rgba(cr, cg, cb, mix(0.6, 0.85, thick)), Math.max(1, r0 * mix(1.4, 0.75, thick)))
      c.setLineDash([])
      // Its growing tip is the brightest part of it.
      if (growing) {
        const s = Math.max(8, r0 * 6)
        c.globalAlpha = mix(0.9, 0.6, thick)
        c.drawImage(this.glow(mix(cr, 1, 0.35), mix(cg, 0.96, 0.35), mix(cb, 0.9, 0.35)), PX[200 + N] - s, PY[200 + N] - s, s * 2, s * 2)
        c.globalAlpha = 1
      }
      c.globalCompositeOperation = 'source-over'
    }
  }

  // ——— the heartbeat's light, and whatever else is going off in the room ———
  lights() {
    const c = this.ctx, f = this.f
    this.place(0, 0)
    c.globalCompositeOperation = 'lighter'
    for (let l = 0; l < 4; l++) {
      const L = G.LIGHTS + l * 8, v = f[L + 3], r = Math.min(Math.max(f[L + 2], 1) * 1.6, Math.max(this.w, this.h) * 0.6)
      if (v <= 0.004) continue
      c.globalAlpha = clamp(v * 0.3, 0, 0.6)
      c.drawImage(this.glow(f[L + 4], f[L + 5], f[L + 6]), f[L] - r, f[L + 1] - r, r * 2, r * 2)
    }
    c.globalAlpha = 1
    c.globalCompositeOperation = 'source-over'
  }

  // Motes: slow points of warm light adrift in front of the call (particles.wgsl, closed form).
  motes() {
    const c = this.ctx, f = this.f, beat = this.beat
    const warm = this.moteSprites ?? (this.moteSprites = [this.glow(0.86, 0.07, 0.05), this.glow(1, 0.32, 0.1), this.glow(1, 0.56, 0.16), this.glow(0.2, 0.95, 0.56)])
    this.place(0, 0)
    c.globalCompositeOperation = 'lighter'
    for (let e = 0; e < 4; e++) {
      const o = G.BURSTS + e * 12, count = f[o + 8], life0 = f[o + 9]
      if (count < 1 || life0 <= 0) continue
      const bx = f[o], by = f[o + 1], age0 = f[o + 2], hw = f[o + 4], hh = f[o + 5], speed = f[o + 6], seed = f[o + 7], size0 = f[o + 10]
      for (let k = 0; k < count; k++) {
        const r0 = hash11(k * 1.37 + seed * 17), r1 = hash11(k * 2.91 + seed * 5.3 + 1), r2 = hash11(k * 0.73 + seed * 29 + 2), r3 = hash11(k * 4.19 + seed * 3.1 + 3)
        const life = life0 * (0.5 + 0.5 * r1), age = fract(age0 / life + r0) * life, u = age / life
        const v = sstep(0, 0.2, u) * sstep(1, 0.6, u) * (0.5 + 0.5 * Math.sin(age * (3 + 4 * r2) + r0 * 6)) * (1.2 + 1.6 * beat)
        if (v < 0.02) continue
        const x = bx + (r2 - 0.5) * 2 * hw + Math.sin(age * (0.7 + r1) + r0 * 6.28) * 14 + (r0 - 0.5) * 40 * u, y = by + (r3 - 0.5) * 2 * hh - speed * (0.4 + 0.6 * r2) * age
        const s = size0 * (0.5 + 0.9 * r1) * 2.2
        c.globalAlpha = clamp(v * 0.7)
        c.drawImage(warm[r3 > 0.9 ? 3 : Math.min(2, Math.floor(r1 * 3))], x - s, y - s, s * 2, s * 2)
      }
    }
    c.globalAlpha = 1
    c.globalCompositeOperation = 'source-over'
  }

  // The lens: rings through the picture, the flash of the hit, a vignette, and the exposure.
  post() {
    const c = this.ctx, f = this.f, W = this.w, H = this.h, d = this.dpr
    this.place(0, 0)
    c.globalCompositeOperation = 'lighter'
    for (let r = 0; r < 8; r++) {
      const o = G.RINGS + r * 8, s = f[o + 4]
      if (s <= 0.003) continue
      // Only the band of the ring is filled.
      const x = f[o], y = f[o + 1], rad = f[o + 2], w = Math.max(f[o + 3], 1), inner = Math.max(0, rad - w * 2), outer = rad + w * 2, mid = (rad - inner) / (outer - inner)
      const col = (a) => rgba(f[o + 5] * 1.6, f[o + 6] * 1.6, f[o + 7] * 1.6, a)
      disc(c, x, y, outer)
      if (inner > 1) { c.moveTo(x + inner, y); c.arc(x, y, inner, 0, TAU, true) }
      fill(c, stops(c.createRadialGradient(x, y, inner, x, y, outer), 0, col(0), mid * 0.6, col(0.25 * s), mid, col(0.75 * s), mid + (1 - mid) * 0.4, col(0.25 * s), 1, col(0)))
    }
    const flash = f[G.POST + 1]
    if (flash > 0.002) { c.fillStyle = rgba(1, 0.88, 0.82, flash); c.fillRect(-20, -20, W + 40, H + 40) }
    c.globalCompositeOperation = 'source-over'
    // The vignette, in a space where its ellipse is the unit circle: only outside its clear middle.
    const vig = f[G.POST + 3]
    if (vig > 0.01) {
      c.setTransform((d * W) / 1.5, 0, 0, (d * H) / 1.23, (d * W) / 2, (d * H) / 2)
      c.globalAlpha = clamp(vig)
      c.beginPath(); c.rect(-1, -1, 2, 2); c.moveTo(0.45, 0); c.arc(0, 0, 0.45, 0, TAU, true)
      fill(c, this.vig)
      c.globalAlpha = 1
    }
    // Black after Stop, and the light coming back up on the notes.
    const ex = f[G.GRADE + 2]
    if (ex < 0.999) {
      c.setTransform(1, 0, 0, 1, 0, 0)
      c.fillStyle = rgba(0, 0, 0, 1 - clamp(ex))
      c.fillRect(0, 0, this.canvas.width, this.canvas.height)
    }
  }
}
