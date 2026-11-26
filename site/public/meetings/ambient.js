// What comes up around the notes if you stay on them, and what it does when it is touched.
//
// It starts as the card settles and takes a quarter of a minute to arrive: dark-oxblood
// tendrils creeping in from the edges of the page toward the card, a long way back in the
// dark, fine vessels between them, a slow beat running down them. It never stops getting a
// little worse. The card and the bean above it are left alone. The tendrils are shy: put the
// pointer among them and the ones it is on draw back toward the edge they came from; take it
// away and they creep back.
//
// Three things here:
//   Touch     the pointer as the tissue feels it: where it is (on a damped spring: it arrives
//             late and without overshoot), how much it is minded, and where it has just been,
//             each place forgotten slowly. Both pictures below read it.
//   backdrop  the picture, on WebGPU: wgsl/backdrop.wgsl draws it at the start of the scene
//             pass, every frame, from the forty floats packed here.
//   Ambient   the picture without WebGPU (?force=none, or none to be had): one 2D canvas
//             screened over the stage. The veins are drawn into a layer of their own as they
//             grow and never drawn again; the rest is a few gradients a frame.
const clamp = (v, a = 0, b = 1) => (v < a ? a : v > b ? b : v)
const smooth = (k) => { k = clamp(k); return k * k * (3 - 2 * k) }
const rnd = (seed) => { let s = seed >>> 0; return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296) }

// How far it has got: 0 to 1. Most of it over the first ARRIVE seconds, the rest over the
// next two minutes.
export const ARRIVE = 12
export const dread = (linger) => (linger <= 0 ? 0 : 0.6 * smooth(linger / ARRIVE) + 0.4 * (1 - Math.exp(-Math.max(0, linger - 8) / 45)))

// ——— the pointer, as the tissue feels it ———
export const SHY = 8
// How wide a berth the tendrils give the pointer (px), how much wider a fast one, and how
// fast is fast (px/s).
const RADIUS = 120, RADIUS_FAST = 40, FAST = 1100
// The spring the shy place follows the pointer on: stiffness, and damping (critical: 2 √stiffness).
const STIFF = 64, DAMP = 16
// Seconds for the tendrils to draw back, and (much longer) to creep out again once let alone.
const GIVE = 0.3, CREEP = 3.4
// Where it has been: a place is left every this many px. The oldest are forgotten faster, so
// that a long stroke never has one cut off.
const TRAIL_EVERY = 80, TRAIL_KEPT = 3, TRAIL_HURRY = 2.5

export class Touch {
  constructor() {
    this.x = this.y = this.vx = this.vy = 0
    this.mind = 0 // 0 not minded .. 1 minded
    this.there = false
    this.trail = [] // { x, y, r, d }, newest first: where it has been, and how much each is still minded
    this.lx = this.ly = 0
    this.n = 0
    this.data = new Float32Array(SHY * 4) // x, y, radius, minded (0..1): what the pictures are given
  }

  get radius() {
    return RADIUS + RADIUS_FAST * Math.min(1, Math.hypot(this.vx, this.vy) / FAST)
  }

  // dt: seconds. pointer: { x, y, on } in page px. active: the tissue is there to be touched.
  step(dt, pointer, active) {
    const on = active && pointer.on
    if (on && !this.there) {
      this.there = true
      this.x = this.lx = pointer.x
      this.y = this.ly = pointer.y
      this.vx = this.vy = 0
    }
    // (Gone: where it was is one more place it has been.)
    if (!on && this.there) this.leave()
    for (let left = Math.min(dt, 0.1); left > 1e-5; left -= 1 / 120) {
      const h = Math.min(left, 1 / 120)
      if (on) {
        this.vx += (STIFF * (pointer.x - this.x) - DAMP * this.vx) * h
        this.vy += (STIFF * (pointer.y - this.y) - DAMP * this.vy) * h
        this.x += this.vx * h
        this.y += this.vy * h
        this.mind += (1 - this.mind) * (1 - Math.exp(-h / GIVE))
      }
      this.trail.forEach((p, i) => (p.d *= Math.exp((-h / CREEP) * (1 + TRAIL_HURRY * Math.max(0, i - TRAIL_KEPT)))))
    }
    // (A new place is left where the pointer is now: nothing changes as it is left.)
    if (on && Math.hypot(this.x - this.lx, this.y - this.ly) > TRAIL_EVERY) this.mark()
    this.trail = this.trail.filter((p, i) => p.d > 0.004 && i < SHY - 1)
    this.pack()
  }

  mark() {
    this.trail.unshift({ x: this.x, y: this.y, r: this.radius, d: this.mind })
    this.lx = this.x
    this.ly = this.y
  }

  leave() {
    this.mark()
    this.there = false
    this.mind = 0
    this.vx = this.vy = 0
  }

  // The pointer has been here for ever: no spring, no trail. (Screenshots, and the benchmark.)
  rest(x, y) {
    this.there = true
    this.x = this.lx = x
    this.y = this.ly = y
    this.vx = this.vy = 0
    this.mind = 1
    this.trail = []
    this.pack()
  }

  clear() {
    this.there = false
    this.mind = 0
    this.trail = []
    this.pack()
  }

  pack() {
    const D = this.data
    let n = 0
    if (this.there && this.mind > 0.004) {
      D[0] = this.x; D[1] = this.y; D[2] = this.radius; D[3] = this.mind
      n = 1
    }
    for (const p of this.trail) {
      if (n >= SHY) break
      D[n * 4] = p.x; D[n * 4 + 1] = p.y; D[n * 4 + 2] = p.r; D[n * 4 + 3] = p.d
      n++
    }
    this.n = n
  }
}

// ——— the picture, on WebGPU: what wgsl/backdrop.wgsl is given (struct Bk) ———
export const BACKDROP_FLOATS = 8 + SHY * 4
const packed = new Float32Array(BACKDROP_FLOATS)
// linger: seconds on the settled notes. clock: the page's. w, h: the page. hole: the card and
// its bean ({ x, y, w, h }, page px). Returns the same array every time.
export function backdrop(linger, clock, w, h, hole, touch) {
  const cx = hole.x + hole.w / 2, cy = hole.y + hole.h / 2
  packed[0] = cx; packed[1] = cy; packed[2] = hole.w / 2; packed[3] = hole.h / 2
  packed[4] = linger
  packed[5] = clock
  // How far from the hole the farthest corner of the page is: where the tendrils set out from.
  packed[6] = Math.hypot(Math.max(cx, w - cx) - hole.w / 2, Math.max(cy, h - cy) - hole.h / 2) + 20
  packed[7] = touch.n
  packed.set(touch.data, 8)
  return packed
}

// ——— the picture, without WebGPU ———

export class Ambient {
  constructor(canvas) {
    this.canvas = canvas
    this.ctx = canvas.getContext('2d')
    this.layer = document.createElement('canvas')
    this.lx = this.layer.getContext('2d')
    this.w = this.h = 0
    this.k = 1
    this.hole = { x: 0, y: 0, w: 0, h: 0 }
    this.veins = []
    this.drawn = 0
    this.reach = 0
    this.on = false
  }

  // w, h: the page, in CSS px. hole: the card and its bean, which nothing may touch.
  resize(w, h, hole) {
    const k = Math.min(1, Math.sqrt(1.5e6 / Math.max(1, w * h)))
    if (Math.abs(w - this.w) < 1 && Math.abs(h - this.h) < 1 && Math.abs(hole.w - this.hole.w) < 1 && Math.abs(hole.y - this.hole.y) < 1) return
    this.w = w
    this.h = h
    this.k = k
    this.hole = { ...hole }
    for (const c of [this.canvas, this.layer]) {
      c.width = Math.max(1, Math.round(w * k))
      c.height = Math.max(1, Math.round(h * k))
    }
    this.grow()
    this.drawn = 0
    this.reach = 0
  }

  // The veins: each sets out from an edge of the page toward the card, wanders, forks, and
  // gives out before it gets there, some far short of it and a few almost touching. Kept as
  // short segments in the order they will appear: [x0, y0, x1, y1, how far along its vein, width].
  grow() {
    const { w, h, hole } = this
    const R = rnd(0x51ed)
    const cx = hole.x + hole.w / 2, cy = hole.y + hole.h / 2
    const pad = 26
    const near = (x, y) => {
      // How far outside the card's box (with its margin) a point is; negative inside.
      const dx = Math.max(hole.x - pad - x, 0, x - (hole.x + hole.w + pad)), dy = Math.max(hole.y - pad - y, 0, y - (hole.y + hole.h + pad))
      return dx || dy ? Math.hypot(dx, dy) : -1
    }
    const segs = []
    const walk = (x, y, a, len, width, at, depth, shy) => {
      const step = 9
      for (let d = 0; d < len; d += step) {
        // Toward the card, loosely, and curling away as it gets close.
        const to = Math.atan2(cy - y, cx - x)
        const n = near(x, y)
        a += Math.atan2(Math.sin(to - a), Math.cos(to - a)) * (n < shy * 2 ? -0.05 : 0.05) + (R() - 0.5) * 0.6
        const nx = x + Math.cos(a) * step, ny = y + Math.sin(a) * step
        if (near(nx, ny) < shy || nx < -20 || ny < -20 || nx > w + 20 || ny > h + 20) break
        const taper = width * (1 - 0.8 * (d / len))
        segs.push([x, y, nx, ny, at + d, taper])
        x = nx
        y = ny
        if (depth < 2 && R() < 0.055) walk(x, y, a + (R() < 0.5 ? 1 : -1) * (0.5 + R() * 0.7), len * (0.25 + R() * 0.3), taper * 0.7, at + d, depth + 1, shy + 20 + R() * 60)
      }
    }
    const roots = Math.round(clamp((w + h) / 190, 8, 15))
    for (let i = 0; i < roots; i++) {
      // Round the edge of the page, more of them from below and from the sides.
      const u = (i + R() * 0.8) / roots
      const side = u < 0.34 ? 2 : u < 0.6 ? 3 : u < 0.86 ? 1 : 0
      const f = R()
      const x = side === 1 ? w + 6 : side === 3 ? -6 : f * w
      const y = side === 0 ? -6 : side === 2 ? h + 6 : f * h
      const a = Math.atan2(cy - y, cx - x) + (R() - 0.5) * 0.9
      // (They do not all set out at once, and most stop well short of the card.)
      walk(x, y, a, Math.hypot(w, h) * (0.2 + R() * 0.3), 1.3 + R() * 1.0, R() * 220, 0, R() < 0.25 ? 14 + R() * 30 : 70 + R() * 150)
    }
    segs.sort((p, q) => p[4] - q[4])
    this.veins = segs
    this.far = segs.length ? segs[segs.length - 1][4] : 1
    // The patches, and the filaments adrift.
    this.blobs = []
    for (let i = 0; i < 6; i++) {
      const a = (i / 6) * Math.PI * 2 + R() * 0.7
      this.blobs.push({ x: cx + Math.cos(a) * w * (0.36 + R() * 0.12), y: cy + Math.sin(a) * h * (0.4 + R() * 0.12), r: Math.min(w, h) * (0.3 + R() * 0.2), ph: R() * 6.28, sp: 0.85 + R() * 0.4 })
    }
    this.motes = []
    for (let i = 0; i < Math.round(clamp((w * h) / 42000, 12, 34)); i++) this.motes.push({ x: R() * w, y: R() * h, len: 6 + R() * 12, a: R() * 6.28, ph: R() * 6.28, sp: 0.4 + R() * 0.9, born: R() })
  }

  // linger: seconds on the settled notes. now: the page's clock. still: no drifting (reduced
  // motion). touch: where the pointer is and has been, which it is shy of.
  draw(linger, now, still = false, touch = null) {
    const I = dread(linger)
    const on = I > 0.002
    if (on !== this.on) {
      this.on = on
      this.canvas.toggleAttribute('data-on', on)
      if (!on) { this.canvas.style.opacity = '0'; this.drawn = 0; this.reach = 0; this.lx.clearRect(0, 0, this.layer.width, this.layer.height) }
    }
    if (!on || !this.w) return
    const t = still ? 0 : now
    const { ctx, k, w, h, hole } = this
    // (Low: it is a long way back.)
    this.canvas.style.opacity = Math.min(0.85, I * 1.4).toFixed(3)

    // The veins that have grown since last time, onto their layer: a dark sleeve, and the
    // thread of crimson in it.
    const reach = this.far * Math.min(1, linger / ARRIVE)
    if (reach < this.reach) { this.drawn = 0; this.lx.clearRect(0, 0, this.layer.width, this.layer.height) }
    this.reach = reach
    const lx = this.lx, V = this.veins
    if (this.drawn < V.length && V[this.drawn][4] <= reach) {
      lx.setTransform(k, 0, 0, k, 0, 0)
      lx.lineCap = 'round'
      for (; this.drawn < V.length && V[this.drawn][4] <= reach; this.drawn++) {
        const s = V[this.drawn]
        // (oxblood, a darker red thread in it, and on the thick ones a sheen)
        lx.strokeStyle = 'rgba(78, 11, 18, 0.5)'
        lx.lineWidth = s[5] * 3.4
        lx.beginPath(); lx.moveTo(s[0], s[1]); lx.lineTo(s[2], s[3]); lx.stroke()
        lx.strokeStyle = 'rgba(112, 16, 22, 0.42)'
        lx.lineWidth = Math.max(0.55, s[5] * 0.8)
        lx.beginPath(); lx.moveTo(s[0], s[1]); lx.lineTo(s[2], s[3]); lx.stroke()
        if (s[5] > 1.2) {
          lx.strokeStyle = 'rgba(190, 124, 116, 0.07)'
          lx.lineWidth = 0.7
          lx.beginPath(); lx.moveTo(s[0] - s[5] * 0.7, s[1] - s[5] * 0.9); lx.lineTo(s[2] - s[5] * 0.7, s[3] - s[5] * 0.9); lx.stroke()
        }
      }
    }

    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.globalCompositeOperation = 'source-over'
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height)
    ctx.setTransform(k, 0, 0, k, 0, 0)
    // The patches: breathing, a little out of step with each other.
    ctx.globalCompositeOperation = 'lighter'
    const breath = 0.5 + 0.5 * Math.sin(t * 1.05)
    for (const b of this.blobs) {
      const v = 0.5 + 0.5 * Math.sin(t * 1.05 * b.sp + b.ph)
      const r = b.r * (0.92 + 0.12 * v)
      const g = ctx.createRadialGradient(b.x, b.y, 0, b.x, b.y, r)
      const a = (0.05 + 0.07 * v) * (0.55 + 0.45 * I)
      g.addColorStop(0, 'rgba(78, 11, 18,' + a.toFixed(3) + ')')
      g.addColorStop(0.5, 'rgba(46, 5, 10,' + (a * 0.5).toFixed(3) + ')')
      g.addColorStop(1, 'rgba(40, 0, 6, 0)')
      ctx.fillStyle = g
      ctx.fillRect(b.x - r, b.y - r, r * 2, r * 2)
    }
    // The veins, with a pulse in them: two beats close together, then a wait.
    const u = (t * 0.5) % 1
    const beat = Math.exp(-u * 9) + 0.6 * Math.exp(-Math.max(0, u - 0.2) * 12) * (u > 0.2 ? 1 : 0)
    ctx.globalAlpha = clamp(0.4 + 0.12 * breath + 0.16 * beat * I)
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.drawImage(this.layer, 0, 0)
    ctx.setTransform(k, 0, 0, k, 0, 0)
    ctx.globalAlpha = 1
    // The filaments: short pale hairs, turning as they drift. More of them the longer it has been.
    ctx.lineCap = 'round'
    ctx.lineWidth = 0.9
    for (const m of this.motes) {
      if (m.born > I * 1.25) continue
      const x = (((m.x + t * 5 * m.sp + 22 * Math.sin(t * 0.21 * m.sp + m.ph)) % (w + 60)) + w + 60) % (w + 60) - 30
      const y = (((m.y - t * 3.2 * m.sp + 16 * Math.cos(t * 0.17 * m.sp + m.ph * 1.7)) % (h + 60)) + h + 60) % (h + 60) - 30
      const a = m.a + 0.9 * Math.sin(t * 0.33 * m.sp + m.ph)
      const bend = Math.sin(t * 0.8 * m.sp + m.ph * 2.3) * m.len * 0.4
      const dx = Math.cos(a) * m.len * 0.5, dy = Math.sin(a) * m.len * 0.5
      ctx.strokeStyle = 'rgba(120, 34, 34,' + (0.2 * (0.6 + 0.4 * Math.sin(t * 0.5 + m.ph))).toFixed(3) + ')'
      ctx.beginPath()
      ctx.moveTo(x - dx, y - dy)
      ctx.quadraticCurveTo(x + Math.cos(a + 1.57) * bend, y + Math.sin(a + 1.57) * bend, x + dx, y + dy)
      ctx.stroke()
    }
    // Shy of the pointer: all of it fades out where the pointer is and has been, and from
    // there in toward the card (what was rooted past the pointer has drawn back through here).
    ctx.globalCompositeOperation = 'destination-out'
    const hx = hole.x + hole.w / 2, hy = hole.y + hole.h / 2
    if (touch) for (let n = 0; n < touch.n; n++) {
      const px = touch.data[n * 4], py = touch.data[n * 4 + 1], pr = touch.data[n * 4 + 2], d = touch.data[n * 4 + 3]
      // (The pointer itself: five soft rounds toward the card. A place it has been: two.)
      const rounds = n ? 2 : 5
      for (let j = 0; j < rounds; j++) {
        const f = (j / rounds) * 0.9
        const x = px + (hx - px) * f, y = py + (hy - py) * f, r = pr * (1.5 - 0.5 * f)
        const g = ctx.createRadialGradient(x, y, 0, x, y, r)
        g.addColorStop(0, 'rgba(0,0,0,' + (d * 0.9).toFixed(3) + ')')
        g.addColorStop(0.45, 'rgba(0,0,0,' + (d * 0.6).toFixed(3) + ')')
        g.addColorStop(1, 'rgba(0,0,0,0)')
        ctx.fillStyle = g
        ctx.fillRect(x - r, y - r, r * 2, r * 2)
      }
    }
    // The card and the bean are not to be touched, and the nearer them the less there is: it
    // is whole only out at the edges of the page.
    const sx = hole.w / 2 + 20, sy = hole.h / 2 + 20
    ctx.save()
    ctx.translate(hx, hy)
    ctx.scale(sx, sy)
    const g = ctx.createRadialGradient(0, 0, 0.9, 0, 0, 2.6)
    g.addColorStop(0, 'rgba(0,0,0,1)')
    g.addColorStop(0.27, 'rgba(0,0,0,0.72)')
    g.addColorStop(0.6, 'rgba(0,0,0,0.34)')
    g.addColorStop(1, 'rgba(0,0,0,0)')
    ctx.fillStyle = g
    ctx.fillRect(-2.7, -2.7, 5.4, 5.4)
    ctx.restore()
    ctx.globalCompositeOperation = 'source-over'
  }
}
