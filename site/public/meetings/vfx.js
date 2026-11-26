// The effects director. Everything around the faces is decided here as a function of show time
// and written into the frame's globals (cast.js G): the two windows, how far they and the tiles
// have run together, the dark wet tendrils that come out of OpenCode's auto-approved cards, along
// the gutters of the call and into each of the four, the cords of sinew between neighbours, the
// hit when the four become one, the bead that hangs under it, the heartbeat's rings and light,
// the motes, and the lens. Nothing is simulated from frame to frame, so any moment can be drawn
// cold, and nothing is allocated per frame.
import { G, F, STRIDE, TENDRIL, TENDRILS, SHUTTER } from './cast.js'
import { sinceBeat, ME as ME_I } from './show.js'

const clamp = (v, a = 0, b = 1) => (v < a ? a : v > b ? b : v)
const lerp = (a, b, k) => a + (b - a) * k
const smooth = (k) => { k = clamp(k); return k * k * (3 - 2 * k) }
const easeOut = (k) => 1 - Math.pow(1 - clamp(k), 3)
const MINT = [0.16, 1.0, 0.5]
const PALE = [0.55, 1.0, 0.78]
const CRIMSON = [1.0, 0.1, 0.06]
const FLESH = [0.62, 0.05, 0.05]
// A tendril lunges and lands hard.
const reach = (k) => { k = clamp(k); return 1 - Math.pow(1 - k, 3.2) }
const reachRate = (k) => (k <= 0 || k >= 1 ? 0 : 3.2 * Math.pow(1 - k, 2.2))

export class Vfx {
  constructor(S, stage) {
    this.S = S
    this.stage = stage
    this.fx = { body: false, tendrils: 0, motes: false, mass: false }
    this.shakeX = 0
    this.shakeY = 0
    this.wasShakeX = 0
    this.wasShakeY = 0
    this.wasSwell = 0
    this.n = 0
    this.tip = [0, 0]
  }

  // One filament: a cubic from a to b, leaving a along (ax, ay) and arriving at b along (bx, by).
  // g: how much of it has grown; r: radius in px; kind 0 a tendril, 1 a strand of light between
  // two things, 2 a cord of sinew between the heads.
  strand(frame, x0, y0, ax, ay, x1, y1, bx, by, g, r, kind, rgb, gain, rate, seed, t) {
    if (g <= 0.002 || this.n >= TENDRILS) return
    const o = G.TENDRILS + this.n++ * TENDRIL
    const len = Math.hypot(x1 - x0, y1 - y0)
    // It is never still: its middle drifts. A cord under tension only shivers.
    const sw = kind === 2 ? 1.5 : 0.03 * len + 4
    const sp = kind === 2 ? 9 : 1
    const s1x = Math.sin(t * 0.9 * sp + seed * 3.1) * sw, s1y = Math.cos(t * 0.7 * sp + seed * 1.7) * sw
    const s2x = Math.sin(t * 0.8 * sp + seed * 5.3 + 2) * sw * 0.6, s2y = Math.cos(t * 1.1 * sp + seed * 2.9) * sw * 0.6
    frame[o] = x0
    frame[o + 1] = y0
    frame[o + 2] = x0 + ax + s1x
    frame[o + 3] = y0 + ay + s1y
    frame[o + 4] = x1 + bx + s2x
    frame[o + 5] = y1 + by + s2y
    frame[o + 6] = x1
    frame[o + 7] = y1
    frame[o + 8] = g
    frame[o + 9] = r
    frame[o + 10] = len * 1.25
    frame[o + 11] = kind
    frame[o + 12] = rgb[0] * gain
    frame[o + 13] = rgb[1] * gain
    frame[o + 14] = rgb[2] * gain
    frame[o + 15] = rate * len * 1.25
    if (kind === 0) {
      // Where its tip is, for the light it carries.
      const k = 1 - g
      this.tip[0] = k * k * k * x0 + 3 * k * k * g * frame[o + 2] + 3 * k * g * g * frame[o + 4] + g * g * g * x1
      this.tip[1] = k * k * k * y0 + 3 * k * k * g * frame[o + 3] + 3 * k * g * g * frame[o + 5] + g * g * g * y1
    }
  }

  light(frame, slot, x, y, radius, v, rgb) {
    const L = G.LIGHTS + slot * 8
    frame[L] = x
    frame[L + 1] = y
    frame[L + 2] = radius
    frame[L + 3] = v
    frame[L + 4] = rgb[0]
    frame[L + 5] = rgb[1]
    frame[L + 6] = rgb[2]
  }

  // Fill the frame's globals for show time t. The heads are already in it (cast.js perform()).
  compose(t, live, frame, heads) {
    const S = this.S, stage = this.stage, geo = stage.geo, cue = S.cue, fx = this.fx
    const idle = live.phase === 'idle'
    const on = !idle && !live.reduced
    frame.fill(0, G.POST, G.HEADS)
    if (this.n) frame.fill(0, G.TENDRILS, G.FLOATS)
    this.n = 0
    fx.body = fx.motes = fx.mass = false
    fx.tendrils = 0
    frame[G.MISC] = live.pageTime
    this.shakeX = this.shakeY = 0
    const step = stage.step
    const open = on ? SHUTTER * step : 0
    frame[G.POST + 2] = 0.02
    frame[G.POST + 3] = 0.42
    frame[G.GRADE + 2] = 1

    // How fast the faces and tiles are moving, for the shutter.
    let fast = 0
    for (let i = 0; i < heads; i++) {
      const o = G.HEADS + i * STRIDE
      if (frame[o + F.ALPHA] <= 0.002) continue
      fast = Math.max(fast, Math.abs(frame[o + F.VX]) + Math.abs(frame[o + F.VY]) + Math.abs(frame[o + F.TVX]) + Math.abs(frame[o + F.TVY]) + Math.abs(frame[o + F.TVW]) + Math.abs(frame[o + F.TVH]) + Math.abs(frame[o + F.VGROW]) * frame[o + F.R])
    }
    frame[G.BLUR + 2] = open
    // (2: the scene writes how far each pixel moves. 1: only the lens moves. 0: nothing does.)
    if (idle) {
      frame[G.BLUR + 3] = fast * open > 0.4 ? 2 : 0
      return fx
    }
    frame[G.MISC] = t

    if (t >= cue.cut) {
      // Stop. Black for as long as the silence lasts; then only the notes and their host,
      // under a warmer light.
      frame[G.GRADE + 2] = t < cue.dead[1] ? 0 : smooth((t - cue.dead[1]) / 0.9)
      frame[G.GRADE + 1] = 0.7
      frame[G.POST + 2] = 0.022
      frame[G.POST + 3] = 0.46
      frame[G.BLUR + 3] = fast * open > 0.4 ? 2 : 0
      this.wasShakeX = this.wasShakeY = this.wasSwell = 0
      return fx
    }

    // The two windows.
    const C = stage.call, O = stage.oc, H = stage.host
    const Fl = stage.flesh, beat = stage.beat, breath = stage.breath, fuse = stage.fuse, meta = stage.meta, panic = stage.panic, chim = stage.chimera
    const rad = geo.narrow ? 10 : 12
    const win = (n, w, alive, focus) => {
      const o = G.WINS + n * 8
      frame[o] = w.x + w.w / 2
      frame[o + 1] = w.y + w.h / 2
      // (s: the call's window is a little small as it opens, about its middle)
      frame[o + 2] = (w.w / 2) * (w.s ?? 1)
      frame[o + 3] = (w.h / 2) * (w.s ?? 1)
      frame[o + 4] = w.a
      frame[o + 5] = rad + 22 * smooth(alive * 1.2)
      frame[o + 6] = alive
      frame[o + 7] = focus
    }
    win(0, C, Fl, 1 - stage.dock)
    win(1, O, Fl * smooth((t - cue.wall + 0.3) / 2.2), stage.dock)
    fx.body = C.a > 0.004 || O.a > 0.004
    frame[G.SEAM] = 0.2 * smooth((t - cue.skip) / 1.5) + 0.8 * smooth((t - cue.merge - 0.3) / 2.6)
    frame[G.SEAM + 1] = stage.seamTiles
    frame[G.SEAM + 2] = geo.narrow ? 1 : 0
    frame[G.MISC + 3] = Fl
    frame[G.PULSE] = beat
    frame[G.PULSE + 1] = breath
    frame[G.PULSE + 2] = Fl
    frame[G.PULSE + 3] = sinceBeat(S, t)

    const headX = (i) => frame[G.HEADS + i * STRIDE + F.CX], headY = (i) => frame[G.HEADS + i * STRIDE + F.CY], headR = (i) => frame[G.HEADS + i * STRIDE + F.R]
    // The one mass the four become: where its middle is and how big a head is.
    const mid = H
    const HR = H.R
    frame[G.META] = H.x
    frame[G.META + 1] = H.y
    frame[G.META + 2] = HR
    frame[G.META + 3] = meta
    fx.mass = meta > 0.002
    // The four tiles, as one box.
    const gridX = C.x + geo.grid.x, gridY = C.y + geo.grid.y
    frame[G.GRID] = gridX
    frame[G.GRID + 1] = gridY
    frame[G.GRID + 2] = geo.grid.hw
    frame[G.GRID + 3] = geo.grid.hh
    // The bead: a rope of slime lets itself down out of the mass until it hangs just over his
    // headband, and stays there, swinging, until he says goodbye.
    const sinceHit = t - cue.hit
    if (sinceHit > 1.2 && on) {
      const meR = headR(ME_I), bandTop = headY(ME_I) - meR * 1.14
      const from = H.y + HR * 1.25, rest = Math.max(from + 4, bandTop - HR * 0.21)
      frame[G.HOST2] = lerp(from, rest, easeOut((sinceHit - 1.2) / 2.6))
      frame[G.HOST2 + 1] = t < cue.beadDrop ? 0 : (t - cue.beadDrop) / 0.32
      frame[G.HOST2 + 2] = bandTop - HR * 0.03
    }
    // The big eye in the middle of it: it opens on "pure", and blinks, slowly, by itself.
    if (on && t > cue.eye) {
      const s2 = Math.floor(t / 4.3), u = t - (s2 + 0.45) * 4.3
      frame[G.HOST2 + 3] = smooth((t - cue.eye) / 0.45) * (1 - (u > 0 && u < 0.34 ? Math.sin((u / 0.34) * Math.PI) : 0) * 0.96)
    }
    let moved = false, rings = 0

    if (t > cue.wall && on) {
      const m = t - cue.merge
      const A = live.anchor
      const wide = !geo.narrow
      const edgeX = wide ? O.x + 2 : 0
      const thick = clamp(HR / 110, 0.4, 1.15)
      // Once it is one thing and content, what feeds it thins out.
      const fed = 1 - 0.3 * chim
      // The first of them, before anything has been asked: hairs across the gap.
      const early = smooth((t - cue.wall - 0.7) / 2.4)
      const caps = wide ? [[0.12, 0.7, 0], [0.33, 0.5, 0], [0.22, 1.4, 1], [0.4, 2.0, 1]] : [[0.2, 0.7, 0], [0.7, 0.5, 0], [0.4, 1.2, 1], [0.85, 1.9, 1]]
      caps.forEach(([f, d, late], n) => {
        const g = late ? reach((m - d) / 1.2) : Math.max(early, reach((m - d) / 1.2))
        if (wide) {
          const y0 = O.y + O.h * f
          const tx = C.x + C.w - 30 - 90 * ((n * 0.37) % 1) - (late ? 80 : 0), ty = y0 + (n % 2 ? 60 : -50)
          this.strand(frame, edgeX, y0, -30, 0, tx, ty, 40, n % 2 ? -30 : 30, g, late ? 1.7 : 1.4, 0, CRIMSON, 0.9, late ? reachRate((m - d) / 1.2) / 1.2 : 0, n + 7, t)
        } else {
          // Stacked, they only stitch the gap between the windows.
          const x0 = O.x + O.w * f
          this.strand(frame, x0, O.y + 3, 0, -10, x0 + (n % 2 ? 26 : -26), C.y + C.h - 5 - 9 * ((n * 0.37) % 1), n % 2 ? -14 : 14, 10, g, late ? 1.3 : 1.1, 0, CRIMSON, 0.9, 0, n + 7, t)
        }
      })
      if (m > 0) {
        // Out of the question it never asked, and out from behind the browser. Three for each
        // of them, thick ones first. Into the two nearest (Maya, Dev) from the side; for the
        // far two (Marcus, Priya) one bundle runs along the gutter between the rows to the
        // middle of the grid and splits there, up into him and down into her. None crosses a face.
        // (Each goes in where wgsl/heads.wgsl has the flesh closed round a hole.)
        const near = [[0.60, -0.14], [0.50, -0.44], [0.58, 0.24]]
        const far = [[0.40, 0.66], [0.56, 0.48], [0.22, 0.80]]
        const wd = [10.5, 6.2, 8.2] // thick: they are feeding
        const src = A.merge
        // [which head, which of S.jolts it lands on, how long it takes to get there]
        const plan = [[1, 0, 0.5], [3, 1, 0.55], [0, 2, 0.85], [2, 3, 0.9]]
        for (const [hi, ji, dur] of plan) {
          const R = headR(hi), sy = hi < 2 ? 1 : -1, right = hi % 2 === 1
          for (let n = 0; n < 3; n++) {
            const k = (t - (S.jolts[ji] + n * 0.1 - dur)) / dur
            if (k <= 0) continue
            // The two on the far side are torn out as the four are drawn together: the way in
            // to them is through the others now. They whip back, and are gone by the slam.
            const out = right ? 0 : smooth((t - (cue.hit - 0.3) - n * 0.03) / 0.22)
            if (out >= 1) continue
            const gk = reach(k) * (1 - out), gr = out > 0 ? -0.7 : reachRate(k) / dur
            const e = right ? near[n] : far[n]
            const ex = headX(hi) + R * e[0], ey = headY(hi) + R * e[1] * (right ? 1 : sy)
            const rgb = n === 1 ? PALE : MINT
            const seed = hi * 5 + n + 1
            if (!wide) {
              this.strand(frame, O.x + (right ? O.w - 22 - n * 12 : 22 + n * 12) , O.y + 4, 0, -(O.y - ey) * 0.5, ex, ey, (right ? 1 : -1) * (R * 1.5 + n * 4), R * 1.6, gk, wd[n] * 0.8 * fed, 0, rgb, 1, gr, seed, t)
              continue
            }
            // (Dev's come out from under the browser. Priya's leave with Marcus's, a little
            // lower, so nothing has to cross Dev's hat to reach the gutter.)
            const low = hi === 3
            const x0 = low ? A.under.x : src.x, y0 = low ? A.under.y - 24 + n * 18 : src.y + (n - 1) * 7 + (right ? 0 : hi === 2 ? 44 : 22)
            if (right) {
              const bow = (n % 2 ? 1 : -1) * (26 + 12 * n)
              this.strand(frame, x0, y0, -(x0 - ex) * 0.42, bow, ex, ey, (x0 - ex) * 0.36, -bow * 0.7 + (low ? 46 : -30), gk, wd[n] * thick * fed, 0, rgb, 1, gr, seed, t)
            } else {
              const off = (n - 1) * 5 * sy
              this.strand(frame, x0, y0, -(x0 - gridX) * 0.36, gridY + off - y0, ex, ey, (gridX - ex) * 0.9 + R * 0.5, gridY + off - ey, gk, wd[n] * thick * fed * 0.92, 0, rgb, 1, gr, seed, t)
            }
          }
        }
        // The light each carries in, and leaves under the skin.
        const first = (hi, ji, slot) => {
          const R = headR(hi), u = t - S.jolts[ji]
          if (u > -0.3 && u < 0) this.light(frame, slot, this.tip[0], this.tip[1], R * 1.5, 1.0 * smooth((u + 0.3) / 0.25), MINT)
          else if (u >= 0) this.light(frame, slot, headX(hi) + R * (hi % 2 ? 0.9 : 0.4), headY(hi) + R * (hi % 2 ? 0 : hi < 2 ? 0.9 : -0.9), R * 1.6, (0.3 + 0.9 * Math.exp(-u * 2.5) + 0.35 * beat) * fed * fed, MINT)
        }
        first(1, 0, 1)
        first(0, 2, 2)
        // And then each is tied to its neighbours: cords of sinew on both axes, no two the
        // same gauge, drawn tight. The mass takes them in.
        const gone = 1 - smooth(meta * 3.2)
        if (gone > 0.12) {
          // [a, b, across (0) or down (1), where it leaves a, where it enters b, its radius]
          const cords = [
            [0, 1, 0, -0.34, -0.22, 15], [0, 1, 0, 0.08, 0.2, 8], [0, 1, 0, 0.42, 0.5, 4],
            [2, 3, 0, -0.4, -0.3, 6], [2, 3, 0, -0.04, 0.1, 17], [2, 3, 0, 0.36, 0.3, 9],
            [0, 2, 1, -0.3, -0.2, 9], [0, 2, 1, 0.12, 0.2, 18], [0, 2, 1, 0.46, 0.5, 5],
            [1, 3, 1, -0.42, -0.34, 16], [1, 3, 1, -0.02, 0.04, 5], [1, 3, 1, 0.34, 0.3, 10],
          ]
          cords.forEach(([a, b, down, fa, fb, r], n) => {
            // (Nothing is tied to Priya until she has been reached.)
            const start = cue.merge + 0.95 + (n * 0.37 % 1) * 0.5 + (a === 2 || b === 2 ? 0.55 : 0)
            const g = smooth((t - start) / 0.45)
            if (g <= 0) return
            const Ra = headR(a), Rb = headR(b)
            let x0, y0, x1, y1
            if (down) {
              x0 = headX(a) + Ra * fa; y0 = headY(a) + Ra * (0.86 - 0.45 * fa * fa)
              x1 = headX(b) + Rb * fb; y1 = headY(b) - Rb * (0.80 - 0.45 * fb * fb)
            } else {
              x0 = headX(a) + Ra * (0.72 - 0.5 * fa * fa); y0 = headY(a) + Ra * fa
              x1 = headX(b) - Rb * (0.72 - 0.5 * fb * fb); y1 = headY(b) + Rb * fb
            }
            const sag = (4 + 9 * (n % 3)) * thick * (1 - 0.6 * panic) * (n % 2 ? 1 : -0.6)
            const dx = x1 - x0, dy = y1 - y0
            this.strand(frame, x0, y0, dx * 0.3 + (down ? sag : 0), dy * 0.3 + (down ? 0 : sag), x1, y1, -dx * 0.3 + (down ? sag : 0), -dy * 0.3 + (down ? 0 : sag), g, r * thick * 1.1 * (0.4 + 0.6 * gone), 2, FLESH, 1, 0, n + 31, t)
          })
        }
        // The pill has been in on it the whole time: one thin braided cord, down to the top of it.
        {
          const pg = reach((m - 1.5) / 1.3)
          const P = live.pill
          const tx = H.x + HR * 0.1, ty = H.y - HR * (1.2 + 0.45 * meta)
          const topY = lerp(Math.min(headY(0), headY(1)) - HR * 0.8, ty, smooth(meta * 1.5))
          this.strand(frame, P.x - 9, P.y + 14, -34, (topY - P.y) * 0.45, tx - 5, topY, 30, -(topY - P.y) * 0.3, pg, 2.6 * thick, 2, FLESH, 0.9, 0, 15, t)
          this.strand(frame, P.x + 9, P.y + 14, 34, (topY - P.y) * 0.4, tx + 5, topY, -30, -(topY - P.y) * 0.34, pg, 2.2 * thick, 2, FLESH, 0.9, 0, 16, t)
        }
        // One goes for Me. It gets as far as the edge of his tile, and thinks better of it.
        if (wide) {
          const kg = reach((m - 2.2) / 0.9) * (1 - smooth((m - 3.6) / 0.7)) * 0.9
          const kx = stage.rect[ME_I * 4] + stage.rect[ME_I * 4 + 2], ky = stage.rect[ME_I * 4 + 1] - stage.rect[ME_I * 4 + 3] * 0.5
          this.strand(frame, A.under.x, A.under.y + 90, -60, 20, kx + 12, ky, 90, 40, kg, 1.5, 0, CRIMSON, 1.0, 0, 19, t)
        }
        moved = t < cue.late + 0.4
      }
      // The hit: four become one. A hard light, and a ring through the whole picture.
      const hu = t - cue.hit
      if (hu >= 0 && hu < 1.2) {
        this.light(frame, 3, H.x, H.y, HR * 3.2, 2.4 * Math.exp(-hu * 7), [1.0, 0.62, 0.5])
        const p = clamp(hu / 0.7), o = G.RINGS + rings++ * 8
        frame[o] = H.x
        frame[o + 1] = H.y
        frame[o + 2] = HR * 0.6 + easeOut(p) * Math.max(live.W, live.H) * 0.62
        frame[o + 3] = 34 * (1 + 2.6 * p)
        frame[o + 4] = 0.95 * (1 - p) * (1 - p)
        frame[o + 5] = 0.6
        frame[o + 6] = 0.08
        frame[o + 7] = 0.05
        // (The second wave is Priya.)
        const lu = t - cue.late
        if (lu >= 0) {
          const p2 = clamp(lu / 0.6), o2 = G.RINGS + rings++ * 8
          frame[o2] = headX(2)
          frame[o2 + 1] = headY(2)
          frame[o2 + 2] = HR * 0.5 + easeOut(p2) * Math.max(live.W, live.H) * 0.3
          frame[o2 + 3] = 24 * (1 + 2.2 * p2)
          frame[o2 + 4] = 0.5 * (1 - p2) * (1 - p2)
          frame[o2 + 5] = 0.5
          frame[o2 + 6] = 0.05
          frame[o2 + 7] = 0.03
        }
      }
      // The heart: its light between them, and a ring through the picture on every beat.
      this.light(frame, 0, mid.x, mid.y, Math.max(geo.tiles[0].hw * 1.5, 240), (0.12 + 0.5 * fuse) * (0.35 + beat) * smooth(Fl * 3) * (1 - 0.35 * chim), [1.0, 0.16, 0.08])
      const B = S.beats
      for (let i = B.length - 1; i >= 0 && rings < 7; i--) {
        const u = t - B[i]
        if (u < 0) continue
        if (u > 0.95) break
        const p = u / 0.95, o = G.RINGS + rings++ * 8
        const f = S.fleshAt(B[i] + 0.2) * (1 - 0.6 * chim)
        frame[o] = mid.x
        frame[o + 1] = mid.y
        frame[o + 2] = 30 + easeOut(p) * Math.max(live.W, live.H) * (0.22 + 0.3 * f)
        frame[o + 3] = 26 * (1 + 2.2 * p)
        frame[o + 4] = (0.10 + 0.34 * f) * (1 - p) * (1 - p)
        frame[o + 5] = 0.5
        frame[o + 6] = 0.05
        frame[o + 7] = 0.03
      }
      // Motes, adrift in front of the call.
      if (fuse > 0.1) {
        for (let e = 0; e < 2; e++) {
          const o = G.BURSTS + e * 12
          frame[o] = gridX
          frame[o + 1] = gridY + (e ? 40 : -20)
          frame[o + 2] = t + e * 7.3
          frame[o + 4] = C.w * 0.5
          frame[o + 5] = geo.grid.hh * 1.1
          frame[o + 6] = 16 + 14 * e
          frame[o + 7] = 0.37 + e * 1.9
          frame[o + 8] = Math.round(96 * smooth((fuse - 0.1) / 0.6) * (e ? 0.6 : 1))
          frame[o + 9] = 5.5
          frame[o + 10] = (geo.narrow ? 1.2 : 1.7) + 0.5 * e
        }
        fx.motes = true
      }
    }

    // The lens. It breathes with the rest of it, shakes while they scream, and takes the hit.
    const hu = on ? t - cue.hit : -1
    const kick = hu >= 0 && hu < 0.8 ? Math.exp(-hu * 9) : 0
    const swell = on ? Fl * (0.007 * breath + 0.004 * beat) * smooth((Fl - 0.12) / 0.3) * (1 - 0.5 * chim) + 0.012 * kick : 0
    const thump = on ? Fl * Fl * beat * (1 - 0.7 * chim) + 0.5 * panic : 0
    // (While they scream the whole desk rattles, and worse every second, until the hit.)
    const worse = on && panic > 0 ? smooth((t - cue.merge - 0.4) / Math.max(1, cue.hit - cue.merge - 0.5)) : 0
    const rattle = on ? panic * (1.0 + 3.4 * worse) : 0
    this.shakeX = 1.6 * thump * Math.sin(t * 61) + rattle * (0.6 * Math.sin(t * 47) + 0.4 * Math.sin(t * 83 + 1.3)) + 8 * kick * Math.sin(hu * 74)
    this.shakeY = 1.3 * thump * Math.cos(t * 53) + rattle * 0.85 * (0.6 * Math.cos(t * 41) + 0.4 * Math.sin(t * 67 + 0.4)) + 11 * kick * Math.cos(hu * 61)
    frame[G.POST] = on ? Math.min(1, 0.35 * Fl * beat + 0.12 * Fl + 0.25 * panic + 0.2 * panic * worse + 0.6 * kick) : 0
    frame[G.POST + 1] = on ? 0.16 * Math.exp(-Math.max(hu, 0) * 16) * (hu >= 0 ? 1 : 0) : 0
    frame[G.POST + 2] = 0.02 + 0.008 * Fl
    frame[G.POST + 3] = 0.42 + 0.2 * Fl + 0.07 * Fl * beat
    frame[G.GRADE] = on ? Fl * (0.25 + 0.6 * beat) * (1 - 0.5 * chim) : 0
    frame[G.GRADE + 2] = 1
    frame[G.VIEW + 2] = this.shakeX
    frame[G.VIEW + 3] = this.shakeY
    frame[G.BLUR] = step > 0 ? clamp((0.3 * (this.shakeX - this.wasShakeX)) / step, -900, 900) : 0
    frame[G.BLUR + 1] = step > 0 ? clamp((0.3 * (this.shakeY - this.wasShakeY)) / step, -900, 900) : 0
    // (The lens splits its colours about the middle of the mass, not the middle of the screen.)
    frame[G.BLUR2] = t > cue.wall ? mid.x : geo.narrow ? C.x + C.w / 2 : (C.x + C.w + O.x) / 2
    frame[G.BLUR2 + 1] = geo.narrow && t <= cue.wall ? (C.y + C.h + O.y) / 2 : mid.y
    frame[G.BLUR2 + 2] = step > 0 && open > 0 ? clamp((swell - this.wasSwell) / step, -0.6, 0.6) : 0
    frame[G.BLUR2 + 3] = swell
    this.wasShakeX = this.shakeX
    this.wasShakeY = this.shakeY
    this.wasSwell = swell
    // Does the scene have to say how far each pixel moves (a face, a tile or a tendril is on
    // the move), or is it only the lens (a ring going by, the shake, the breath)?
    const scene = open > 0 && (fast * open > 0.4 || moved)
    const lens = open > 0 && ((Math.abs(frame[G.BLUR]) + Math.abs(frame[G.BLUR + 1])) * open > 0.4 || rings > 0 || Math.abs(frame[G.BLUR2 + 2]) > 0.004)
    frame[G.BLUR + 3] = scene ? 2 : lens ? 1 : 0
    fx.tendrils = this.n
    return fx
  }
}
