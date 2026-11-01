// Canvas 2D fallback for browsers without WebGPU: the same scene, drawn with fewer effects.
import { fx as shoutFx } from "./fx.js"

const ember = (h) => {
  const s = [
    [0.35, 0.02, 0.01],
    [0.95, 0.16, 0.06],
    [1, 0.55, 0.22],
    [1, 0.95, 0.88],
  ]
  h = Math.max(0, Math.min(1, h)) * 3
  const i = Math.min(2, Math.floor(h))
  const f = h - i
  return s[i].map((a, k) => a + (s[i + 1][k] - a) * f)
}
const saiyanCol = (h) => {
  const s = [
    [0.88, 0.14, 0.02],
    [1.0, 0.56, 0.06],
    [1.0, 0.88, 0.26],
    [1.0, 0.99, 0.92],
  ]
  h = Math.max(0, Math.min(1, h)) * 3
  const i = Math.min(2, Math.floor(h))
  const f = h - i
  return s[i].map((a, k) => a + (s[i + 1][k] - a) * f)
}
const rgb = (c, a = 1) => `rgba(${Math.round(c[0] * 255)},${Math.round(c[1] * 255)},${Math.round(c[2] * 255)},${a})`
const mix3 = (a, b, t) => a.map((x, i) => x + (b[i] - x) * t)

// Each live shard's face, cut from the card atlas once into its own small canvas. Drawing a pre-cut
// face is one drawImage; clipping the card to the shard outline every frame, twice for the motion blur,
// is what sank this renderer during the rapid-fire stretch. The cut keeps the atlas's own pixels (a 1:1
// copy at integer offsets) and is drawn back with the same mapping, so only the outline's anti-aliasing
// can differ.
const SPRITE_PAD = 2

class ShardSprites {
  constructor(cards) {
    this.cards = cards
  }

  // The shard's cut face, or null when it has none (the caller clips as before).
  get(s) {
    let sp = s.sprite
    if (sp === undefined) sp = s.sprite = this.cut(s)
    if (!sp) return null
    const version = this.cards.slotVersion(s.entry.px, s.entry.py)
    if (sp.version !== version) this.paint(s, sp, version)
    return sp
  }

  cut(s) {
    const e = s.entry
    const ox = -s.base[0] - e.w / 2
    const oy = -s.base[1] - e.h / 2
    const pts = s.v3 ? [s.v0, s.v1, s.v2, s.v3] : [s.v0, s.v1, s.v2]
    let x0 = Infinity
    let y0 = Infinity
    let x1 = -Infinity
    let y1 = -Infinity
    for (const [x, y] of pts) {
      x0 = Math.min(x0, x)
      y0 = Math.min(y0, y)
      x1 = Math.max(x1, x)
      y1 = Math.max(y1, y)
    }
    // Atlas pixels, clamped to the card's source rect exactly as the full drawImage was.
    const u0 = Math.max(e.px, Math.floor(e.px + (x0 - ox) * 2) - SPRITE_PAD)
    const v0 = Math.max(e.py, Math.floor(e.py + (y0 - oy) * 2) - SPRITE_PAD)
    const u1 = Math.min(e.px + e.w * 2, Math.ceil(e.px + (x1 - ox) * 2) + SPRITE_PAD)
    const v1 = Math.min(e.py + e.h * 2, Math.ceil(e.py + (y1 - oy) * 2) + SPRITE_PAD)
    if (!(u1 > u0 && v1 > v0)) return null
    const canvas = document.createElement("canvas")
    canvas.width = Math.ceil(u1 - u0)
    canvas.height = Math.ceil(v1 - v0)
    return {
      canvas,
      ctx: canvas.getContext("2d"),
      u0,
      v0,
      w: u1 - u0,
      h: v1 - v0,
      pts,
      ox,
      oy,
      // Where the cut lands in the shard's local space, at the atlas's half scale.
      dx: ox + (u0 - e.px) / 2,
      dy: oy + (v0 - e.py) / 2,
      version: -1,
    }
  }

  paint(s, sp, version) {
    const e = s.entry
    const g = sp.ctx
    g.setTransform(1, 0, 0, 1, 0, 0)
    g.clearRect(0, 0, sp.canvas.width, sp.canvas.height)
    g.save()
    // The outline in cut pixels: local → atlas is (p - origin) * 2, offset by the cut's corner.
    g.setTransform(2, 0, 0, 2, e.px - sp.u0 - sp.ox * 2, e.py - sp.v0 - sp.oy * 2)
    g.beginPath()
    g.moveTo(...sp.pts[0])
    for (let k = 1; k < sp.pts.length; k++) g.lineTo(...sp.pts[k])
    g.closePath()
    g.clip()
    g.setTransform(1, 0, 0, 1, 0, 0)
    g.drawImage(this.cards.canvas, sp.u0, sp.v0, sp.canvas.width, sp.canvas.height, 0, 0, sp.canvas.width, sp.canvas.height)
    g.restore()
    sp.version = version
  }
}

export class Canvas2DRenderer {
  constructor(canvas, sprites) {
    this.kind = "canvas2d"
    this.canvas = canvas
    this.sprites = sprites
    this.ctx = canvas.getContext("2d")
    this.fx = shoutFx
    this.fx.onSpawnShards = (list) => this.rawSpawnShards(list)
    this.fx.onSpawnSparks = (list) => this.spawnSparks(list)
    this.shards = []
    this.sparks = []
    this.dpr = 1
    this.shardSprites = new ShardSprites(sprites.cards)
  }
  setSaiyan(level) {
    this.fx.setSaiyan(level)
  }
  saiyanBurst() {
    this.fx.saiyanBurst()
  }
  resolveCard(card) {
    return this.fx.resolveCard(card)
  }
  ocIntro(card) {
    this.fx.ocIntro(card)
  }
  ocSubmit(card) {
    this.fx.ocSubmit(card)
  }
  setFinale(state) {
    this.fx.setFinale(state)
  }
  setSpinFinale(state) {
    this.fx.setFinale(state)
  }
  finaleShot(shot) {
    this.fx.finaleShot(shot)
  }
  fireTommyShot(shot) {
    this.fx.finaleShot(shot)
  }
  hitCard(card, impulse, point) {
    this.fx.hitCard(card, impulse, point)
  }
  rawSpawnShards(list) {
    for (const s of list) this.shards.push({ ...s, maxLife: s.life, tumble: s.tumble ?? 1.2 })
    if (this.shards.length > 900) this.shards.splice(0, this.shards.length - 900)
  }
  spawnShards(list) {
    const voronoi = this.fx.fractureVoronoi(list)
    this.rawSpawnShards(voronoi)
  }
  spawnSparks(list) {
    for (const s of list) this.sparks.push({ ...s, maxLife: s.life })
    if (this.sparks.length > 2000) this.sparks.splice(0, this.sparks.length - 2000)
  }
  resize(w, h, dpr) {
    this.dpr = dpr
    const cw = Math.max(1, Math.round(w * dpr))
    const ch = Math.max(1, Math.round(h * dpr))
    if (this.canvas.width !== cw) this.canvas.width = cw
    if (this.canvas.height !== ch) this.canvas.height = ch
  }
  done() {
    return Promise.resolve()
  }

  draw(scene) {
    this.fx.step(scene)
    const { w, h, head, problems, words, fx, look, dt } = scene
    const saiyan = this.fx.saiyan
    const ctx = this.ctx
    const atlas = this.sprites.cards.canvas
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height)
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0)
    ctx.translate(w / 2 + scene.shakeX, h / 2 + scene.shakeY)
    ctx.rotate(scene.roll)
    ctx.translate(-w / 2, -h / 2)

    for (const p of problems) {
      const e = { px: p.px, py: p.py, w: p.sw, h: p.sh }
      const px = p._fxX ?? p.x
      const py = p._fxY ?? p.y
      const rot = p._fxRot ?? p.rot
      const sc = p._fxScale ?? p.scale
      const flash = p._fxFlash ?? p.flash
      const dmg = p._fxDamage ?? 0
      const tiltX = p._fxTiltX ?? 0
      const tiltY = p._fxTiltY ?? 0
      const isResolved = (p._fxSeed ?? 0) >= 50 || p.state === "resolving"
      ctx.save()
      ctx.globalAlpha = p.alpha
      ctx.translate(px, py)
      ctx.rotate(rot)
      ctx.transform(sc * (1 - Math.abs(tiltX) * 0.06), tiltY * 0.04 * sc, tiltX * 0.04 * sc, sc * (1 - Math.abs(tiltY) * 0.06), 0, 0)
      ctx.drawImage(atlas, e.px, e.py, e.w * 2, e.h * 2, -e.w / 2, -e.h / 2, e.w, e.h)

      const ix = (p._fxImpX ?? 0) * (e.w * 0.36)
      const iy = (p._fxImpY ?? 0.2) * (e.h * 0.36)
      if (dmg > 0.04 && !isResolved) {
        ctx.save()
        ctx.beginPath()
        ctx.roundRect(-e.w * 0.44, -e.h * 0.38, e.w * 0.88, e.h * 0.76, 12)
        ctx.clip()
        const col = saiyan > 0.25 ? "rgba(255,205,70,0.65)" : "rgba(255,95,38,0.6)"
        ctx.strokeStyle = col
        ctx.lineWidth = 1.3
        for (let k = 0; k < 7; k++) {
          const a = (k / 7) * Math.PI * 2 + (p.seed ?? 0)
          const r = Math.min(1.2, dmg) * 85 * (0.6 + 0.4 * Math.sin(k * 2.3))
          ctx.beginPath()
          ctx.moveTo(ix, iy)
          ctx.lineTo(ix + Math.cos(a) * r, iy + Math.sin(a) * r)
          ctx.stroke()
        }
        ctx.restore()
      }
      if (isResolved) {
        ctx.strokeStyle = `rgba(56,245,158,${0.65 * p.alpha})`
        ctx.lineWidth = 2
        ctx.beginPath()
        ctx.roundRect(-e.w * 0.44, -e.h * 0.38, e.w * 0.88, e.h * 0.76, 12)
        ctx.stroke()
      } else if (flash > 0.02) {
        ctx.globalCompositeOperation = "lighter"
        const g = ctx.createRadialGradient(ix, iy, 2, ix, iy, Math.max(e.w, e.h) * 0.42)
        g.addColorStop(0, `rgba(255,210,130,${0.52 * flash * p.alpha})`)
        g.addColorStop(1, "rgba(255,110,40,0)")
        ctx.fillStyle = g
        ctx.beginPath()
        ctx.roundRect(-e.w * 0.44, -e.h * 0.38, e.w * 0.88, e.h * 0.76, 12)
        ctx.fill()
      }
      ctx.restore()
    }

    const floorY = h - 14
    for (let i = this.shards.length - 1; i >= 0; i--) {
      const s = this.shards[i]
      const isKeycap = s.half && s.half[1] < 0
      const drag = isKeycap ? look.shardDrag * 0.28 : look.shardDrag
      const grav = isKeycap ? look.shardGravity * 1.75 : look.shardGravity
      s.vx *= Math.exp(-drag * dt)
      s.vy = s.vy * Math.exp(-drag * dt) + grav * dt
      s.x += s.vx * dt
      s.y += s.vy * dt
      s.ang += s.spin * dt
      s.spin *= Math.exp((isKeycap ? -0.25 : -0.95) * dt)
      s.tumble = (s.tumble ?? 1) + (Math.abs(s.spin) * 0.65 + 2.5) * dt
      s.heat *= Math.exp(-dt / 0.28)
      const sFloor = isKeycap ? h + 260 : floorY
      if (s.y > sFloor && s.vy > 0) {
        s.y = sFloor
        s.vy = -s.vy * 0.42
        s.vx *= 0.78
      }
      s.life -= dt
      if (s.life <= 0) {
        this.shards.splice(i, 1)
        continue
      }
      const t = s.life / s.maxLife
      const fade = Math.min(1, t / (isKeycap ? 0.12 : 0.35))
      const tiltX = 0.4 + 0.6 * Math.abs(Math.cos(s.tumble))
      const sp = this.shardSprites.get(s)
      for (const k of [-0.2, 0.2]) {
        ctx.save()
        ctx.globalAlpha = fade * 0.55
        ctx.translate(s.x - s.vx * look.shutter * k, s.y - s.vy * look.shutter * k)
        ctx.rotate(s.ang)
        ctx.scale(tiltX, 1)
        const outline = (s.heat > 0.08 || isKeycap) && k > 0
        if (!sp || outline) {
          ctx.beginPath()
          ctx.moveTo(...s.v0)
          ctx.lineTo(...s.v1)
          ctx.lineTo(...s.v2)
          if (s.v3) ctx.lineTo(...s.v3)
          ctx.closePath()
        }
        if (sp) {
          ctx.drawImage(sp.canvas, 0, 0, sp.w, sp.h, sp.dx, sp.dy, sp.w / 2, sp.h / 2)
        } else {
          ctx.save()
          ctx.clip()
          const e = s.entry
          ctx.drawImage(atlas, e.px, e.py, e.w * 2, e.h * 2, -s.base[0] - e.w / 2, -s.base[1] - e.h / 2, e.w, e.h)
          ctx.restore()
        }
        if (outline) {
          ctx.strokeStyle = isKeycap
            ? `rgba(255,172,58,${fade * Math.max(0.35, Math.min(1, s.heat))})`
            : rgb(saiyan > 0.25 ? saiyanCol(0.75) : ember(0.75), fade * Math.min(1, s.heat))
          ctx.lineWidth = 1.4
          ctx.stroke()
        }
        ctx.restore()
      }
    }

    // Shards sit behind the head so they never slice across the face.
    this.drawHead(ctx, head, look, saiyan, scene.time)

    ctx.textAlign = "center"
    ctx.textBaseline = "middle"
    for (const o of words) {
      const taps = 5
      const ink = rgb(mix3([0.94, 0.94, 0.93], [1, 0.97, 0.92], o.heat), (o.alpha / taps) * 1.6)
      for (let k = 0; k < taps; k++) {
        const s = ((k + 0.5) / taps - 0.5) * look.shutter
        ctx.save()
        ctx.translate(o.x - o.vx * s, o.y - o.vy * s)
        ctx.rotate(o.angle)
        ctx.scale(o.scale, o.scale)
        ctx.font = `${o.entry.bucket > 50 ? 820 : 600} ${o.entry.bucket * o.k}px "Inter Variable"`
        if (k === taps - 1 && o.heat > 0.05) {
          const pal = saiyan > 0.25 ? saiyanCol(0.45 + o.heat * 0.45) : ember(0.35 + o.heat * 0.4)
          ctx.shadowColor = rgb(pal, o.heat)
          ctx.shadowBlur = 12
        }
        ctx.fillStyle = ink
        ctx.fillText(o.entry.text, 0, 2)
        ctx.restore()
      }
    }

    ctx.globalCompositeOperation = "lighter"
    for (const f of fx) {
      if (f.kind === 0) {
        const c = saiyan > 0.25 ? saiyanCol(1 - f.t * 0.5) : mix3([1, 0.96, 0.9], [1, 0.48, 0.22], f.t)
        ctx.strokeStyle = rgb(c, f.k)
        ctx.lineWidth = f.width
        ctx.beginPath()
        ctx.arc(f.x, f.y, Math.max(0, f.r), 0, Math.PI * 2)
        ctx.stroke()
      } else if (f.kind === 2) {
        ctx.fillStyle = `rgba(255,235,195,${f.k * 0.5})`
        ctx.fillRect(-50, -50, w + 100, h + 100)
      } else if (f.kind === 4) {
        ctx.save()
        ctx.translate(f.x, f.y)
        ctx.rotate(f.width || 0)
        ctx.strokeStyle = `rgba(235,250,255,${f.k * (1 - f.t)})`
        ctx.lineWidth = 2.2
        ctx.beginPath()
        ctx.moveTo(0, 0)
        const len = f.r || 90
        for (let s = 1; s <= 5; s++) {
          const u = s / 5
          const jy = s < 5 ? Math.sin(scene.time * 45 + s * 3.1 + f.x) * len * 0.14 : 0
          ctx.lineTo(u * len, jy)
        }
        ctx.stroke()
        ctx.restore()
      } else if (f.kind === 5) {
        // Clean emerald checkmark stroke-draw
        ctx.save()
        ctx.translate(f.x, f.y)
        const fade = (1 - Math.max(0, (f.t - 0.65) / 0.35)) * f.k
        const rad = f.r || 36
        ctx.strokeStyle = `rgba(56,248,158,${fade * 0.85})`
        ctx.lineWidth = 3.2
        ctx.lineCap = "round"
        ctx.lineJoin = "round"
        ctx.beginPath()
        ctx.moveTo(-rad * 0.38, rad * 0.04)
        ctx.lineTo(-rad * 0.08, rad * 0.32)
        ctx.lineTo(rad * 0.44, -rad * 0.28)
        ctx.stroke()
        ctx.restore()
      } else if (f.kind === 6 || f.kind === 7) {
        ctx.save()
        ctx.translate(f.x, f.y)
        ctx.rotate(f.width || 0)
        ctx.strokeStyle = `rgba(255,230,150,${f.k * (1 - f.t)})`
        ctx.lineWidth = f.kind === 6 ? 6 : 2.6
        ctx.beginPath()
        ctx.moveTo(0, 0)
        ctx.lineTo(f.r || 120, 0)
        ctx.stroke()
        ctx.restore()
      } else if (f.kind === 8) {
        // Fireball cooling to smoke
        const rad = Math.max(4, f.r)
        if (f.t > 0.18) {
          ctx.globalCompositeOperation = "source-over"
          const sa = Math.min(0.65, (1 - f.t) * 0.75 * f.k)
          const sg = ctx.createRadialGradient(f.x, f.y, rad * 0.1, f.x, f.y, rad)
          sg.addColorStop(0, `rgba(22,20,26,${sa})`)
          sg.addColorStop(0.7, `rgba(14,13,18,${sa * 0.7})`)
          sg.addColorStop(1, "rgba(10,10,12,0)")
          ctx.fillStyle = sg
          ctx.beginPath()
          ctx.arc(f.x, f.y, rad, 0, Math.PI * 2)
          ctx.fill()
          ctx.globalCompositeOperation = "lighter"
        }
        if (f.t < 0.8) {
          const fg = ctx.createRadialGradient(f.x, f.y, 0, f.x, f.y, rad * 0.85)
          const fireA = Math.max(0, 1 - f.t / 0.8) * f.k
          fg.addColorStop(0, `rgba(255,248,220,${fireA})`)
          fg.addColorStop(0.45, saiyan > 0.3 ? `rgba(255,195,55,${fireA * 0.85})` : `rgba(255,115,35,${fireA * 0.85})`)
          fg.addColorStop(1, "rgba(220,40,10,0)")
          ctx.fillStyle = fg
          ctx.beginPath()
          ctx.arc(f.x, f.y, rad * 0.85, 0, Math.PI * 2)
          ctx.fill()
        }
      } else if (f.kind === 9) {
        // Rising smoke plume
        ctx.globalCompositeOperation = "source-over"
        const rad = Math.max(4, f.r)
        const sa = Math.min(0.55, Math.min(1, f.t * 6) * Math.pow(1 - f.t, 1.4) * f.k * 0.6)
        const sg = ctx.createRadialGradient(f.x, f.y, rad * 0.1, f.x, f.y, rad)
        sg.addColorStop(0, `rgba(24,23,28,${sa})`)
        sg.addColorStop(0.65, `rgba(18,17,22,${sa * 0.65})`)
        sg.addColorStop(1, "rgba(12,12,15,0)")
        ctx.fillStyle = sg
        ctx.beginPath()
        ctx.arc(f.x, f.y, rad, 0, Math.PI * 2)
        ctx.fill()
        ctx.globalCompositeOperation = "lighter"
      } else if (f.kind === 10) {
        // Scorch + shockwave double ring
        const rad = Math.max(4, f.r)
        ctx.strokeStyle = `rgba(255,205,130,${(1 - f.t) * f.k * 0.7})`
        ctx.lineWidth = 2.2
        ctx.beginPath()
        ctx.arc(f.x, f.y, rad * (0.25 + 0.7 * (1 - Math.pow(1 - f.t, 2.2))), 0, Math.PI * 2)
        ctx.stroke()
      } else {
        const g = ctx.createRadialGradient(f.x, f.y, 0, f.x, f.y, f.r)
        const c = f.kind === 1 ? [1, 0.97, 0.92] : saiyan > 0.25 ? [1, 0.78, 0.2] : [1, 0.42, 0.16]
        g.addColorStop(0, rgb(c, f.k))
        g.addColorStop(1, rgb(c, 0))
        ctx.fillStyle = g
        ctx.beginPath()
        ctx.arc(f.x, f.y, f.r, 0, Math.PI * 2)
        ctx.fill()
      }
    }

    ctx.lineCap = "round"
    for (let i = this.sparks.length - 1; i >= 0; i--) {
      const p = this.sparks[i]
      p.vx *= Math.exp(-look.sparkDrag * dt)
      if (p.heat < -0.5 && p.heat > -1.4) {
        p.vy = p.vy * Math.exp(-look.sparkDrag * dt) - look.sparkGravity * 0.6 * dt
      } else {
        p.vy = p.vy * Math.exp(-look.sparkDrag * dt) + look.sparkGravity * (p.heat <= -1.5 ? 1.6 : 1) * dt
      }
      p.x += p.vx * dt
      p.y += p.vy * dt
      if (p.y > h - 10 && p.vy > 0) {
        p.y = h - 10
        p.vy = -p.vy * 0.48
      }
      p.life -= dt
      if (p.life <= 0) {
        this.sparks.splice(i, 1)
        continue
      }
      const t = p.life / p.maxLife
      const c =
        p.heat <= -1.5
          ? [0.95, 0.74, 0.25]
          : p.heat < -0.5
            ? saiyanCol(Math.abs(p.heat) * t)
            : p.heat < -0.1
              ? [0.35, 0.96, 0.65]
              : p.heat
                ? ember(p.heat * t * 1.1)
                : [0.55, 0.57, 0.62]
      const s = look.shutter * look.sparkStretch * 0.5
      ctx.strokeStyle = rgb(c, t)
      ctx.lineWidth = p.size ?? 1.6
      ctx.beginPath()
      ctx.moveTo(p.x - p.vx * s, p.y - p.vy * s)
      ctx.lineTo(p.x + p.vx * s + 0.1, p.y + p.vy * s)
      ctx.stroke()
    }
    ctx.globalCompositeOperation = "source-over"
  }

  drawHead(ctx, hd, look, saiyan = 0, time = 0) {
    if (hd.visible <= 0) return
    const R = hd.R
    const a = hd.anger
    const m = Math.min(hd._fxMouth ?? hd.mouth, 1.3)
    const mwScale = hd._fxMouthW ?? hd.mouthW
    const gazeX = hd._fxGazeX ?? hd.gazeX
    const gazeY = hd._fxGazeY ?? hd.gazeY
    const pupilScale = hd._fxPupil ?? 1
    const dizzy = hd._fxDizzy ?? 0
    ctx.save()
    ctx.translate(hd.x, hd.y)

    const aura = Math.min(1.3, saiyan + Math.max(0, a - 0.72) * 0.5)
    if (aura > 0.02 || a > 0.02) {
      const g = ctx.createRadialGradient(0, 0, R * 0.5, 0, 0, R * (2.2 + aura * 1.1))
      g.addColorStop(0, saiyan > 0.2 ? `rgba(255,215,75,${0.55 * aura})` : `rgba(255,55,20,${0.32 * a * look.halo})`)
      g.addColorStop(0.55, `rgba(255,80,18,${0.28 * (aura + a * 0.5)})`)
      g.addColorStop(1, "rgba(255,46,18,0)")
      ctx.fillStyle = g
      ctx.fillRect(-R * 3.5, -R * 3.5, R * 7, R * 7)
    }

    ctx.rotate(hd.tilt)
    ctx.scale(hd.sx * R, hd.sy * R)

    ctx.beginPath()
    ctx.ellipse(0, 0.05 * m, 0.8 + 0.08 * m, 1 + 0.1 * m, 0, 0, Math.PI * 2)
    ctx.fillStyle = "#0d0e12"
    ctx.fill()
    ctx.lineWidth = 0.035
    ctx.strokeStyle = saiyan > 0.25 ? `rgba(255,225,120,${0.9 * look.rim})` : `rgba(245,245,248,${0.82 * look.rim})`
    ctx.stroke()

    const fx = hd.faceX
    const fy = hd.faceY
    for (const s of [-1, 1]) {
      const ex = s * 0.27 + fx
      const ey = -0.125 + fy
      const open = Math.max(0.06, (1 - hd.blink - hd.lid * 0.5 - hd.squint * 0.45) * (1 + 0.18 * saiyan))
      ctx.fillStyle = "#f7f7f5"
      ctx.beginPath()
      ctx.ellipse(ex, ey + 0.16 * (1 - Math.min(1, open)), 0.148, 0.208 * open, 0, 0, Math.PI * 2)
      ctx.fill()
      if (open > 0.22) {
        const px = ex + gazeX * 0.06
        const py = ey + gazeY * 0.07 + 0.14 * (1 - Math.min(1, open))
        ctx.fillStyle = "#060608"
        ctx.beginPath()
        ctx.arc(px, py, 0.072 * pupilScale, 0, Math.PI * 2)
        ctx.fill()
      }
    }
    const my = 0.31 + 0.18 * m + fy * 0.92
    const mw = (0.16 + 0.24 * m) * mwScale
    const mh = 0.28 * m
    if (m < 0.025) {
      ctx.strokeStyle = "#f7f7f5"
      ctx.lineWidth = 0.032
      ctx.beginPath()
      ctx.moveTo(-mw + fx, my - hd.happy * 0.04)
      ctx.quadraticCurveTo(fx, my + hd.happy * 0.07, mw + fx, my - hd.happy * 0.04)
      ctx.stroke()
    } else {
      ctx.beginPath()
      ctx.roundRect(fx - mw, my - mh * 0.78, mw * 2, mh * 1.72, Math.min(mw, mh) * 0.72)
      ctx.fillStyle = "#180203"
      ctx.fill()
    }

    if (dizzy > 0.05) {
      ctx.fillStyle = `rgba(255,215,75,${dizzy})`
      for (let k = 0; k < 5; k++) {
        const ang = time * 4.2 + (k / 5) * Math.PI * 2
        ctx.beginPath()
        ctx.arc(Math.cos(ang) * 0.68, -1.06 + Math.sin(ang) * 0.16, 0.055, 0, Math.PI * 2)
        ctx.fill()
      }
    }
    ctx.restore()
  }
}
export default Canvas2DRenderer
