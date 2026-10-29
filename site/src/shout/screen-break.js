// Fourth-wall photorealistic screen glass (v3):
// - Real-time outward fracture propagation (90–160 ms): crushed white silica crater blooms first,
//   radial cracks race outward with bright leading fracture-tip sparks, secondary/tertiary branches
//   spawn as the radial front passes their fork node, concentric Hertzian ring cracks zip across
//   right behind the radial front, and new cracks arrest (T-junction) at existing crack lines.
// - Paint-over-matched crystalline white/silver fracture lines with dark total-internal-reflection
//   bevels, subtle green borosilicate inner tint, localized golden Fresnel glints near the glowing
//   head, dense fine micro-cracking around craters, and strong per-wedge RGB chromatic refraction.
// - Paint-over-matched 3D shattered tempered glass slabs with Lloyd-relaxed Voronoi geometry,
//   dark-crystal + razor-white/gold specular bevel streaks, RGB dispersive page refraction,
//   anisotropic surface reflections, foreground bokeh DOF, and a 3-tier cloud of ~620 sparkling
//   golden/white glass dust & bokeh motes suspended in bullet-time.

const SANS = '"Inter Variable", ui-sans-serif, system-ui'
const MONO = 'ui-monospace, "SF Mono", Menlo, monospace'

const clamp = (x, a, b) => Math.max(a, Math.min(b, x))
const lerp = (a, b, t) => a + (b - a) * t
const smooth = (a, b, x) => {
  const t = clamp((x - a) / (b - a), 0, 1)
  return t * t * (3 - 2 * t)
}

function clipPolyHalfPlane(poly, mx, my, nx, ny) {
  const out = []
  const n = poly.length
  if (n === 0) return out
  for (let i = 0; i < n; i++) {
    const a = poly[i]
    const b = poly[(i + 1) % n]
    const da = (a[0] - mx) * nx + (a[1] - my) * ny
    const db = (b[0] - mx) * nx + (b[1] - my) * ny
    const ina = da <= 0
    const inb = db <= 0
    if (ina && inb) {
      out.push(b)
    } else if (ina && !inb) {
      const t = da / (da - db + 1e-9)
      out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
    } else if (!ina && inb) {
      const t = da / (da - db + 1e-9)
      out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
      out.push(b)
    }
  }
  return out
}

// Segment-segment intersection for real glass crack-arrest (T-junctions against existing fractures)
function segmentIntersection(ax, ay, bx, by, cx, cy, dx, dy) {
  const rX = bx - ax
  const rY = by - ay
  const sX = dx - cx
  const sY = dy - cy
  const denom = rX * sY - rY * sX
  if (Math.abs(denom) < 1e-6) return null
  const qpX = cx - ax
  const qpY = cy - ay
  const t = (qpX * sY - qpY * sX) / denom
  const u = (qpX * rY - qpY * rX) / denom
  if (t > 0.04 && t <= 1.0 && u >= 0.0 && u <= 1.0) {
    return { t, x: ax + t * rX, y: ay + t * rY }
  }
  return null
}

// When every part of a crack has finished revealing.
function crackSettle(c) {
  if (c.settle == null) {
    let t = 0.03
    for (const seg of c.segments) t = Math.max(t, seg.t1)
    for (const ring of c.rings) t = Math.max(t, ring.t1)
    for (const mc of c.microCracks) t = Math.max(t, mc.t0 + 0.018)
    for (const w of c.wedges) t = Math.max(t, w.revealDur || 0.09)
    for (const f of c.craterFacets) t = Math.max(t, f.revealAt || 0)
    for (const sp of c.specks) t = Math.max(t, sp.revealAt || 0)
    c.settle = t + 0.02
  }
  return c.settle
}

export class ScreenBreak {
  constructor(root, stageCanvas, rnd = Math.random) {
    this.root = root
    this.stageCanvas = stageCanvas
    this.rnd = (a = 0, b = 1) => a + rnd() * (b - a)

    this.canvas = document.createElement("canvas")
    this.canvas.className = "screen-glass"
    this.canvas.setAttribute("aria-hidden", "true")
    root.appendChild(this.canvas)
    this.ctx = this.canvas.getContext("2d")

    this.basePageCanvas = document.createElement("canvas")
    this.basePageCtx = this.basePageCanvas.getContext("2d")

    this.pageCanvas = document.createElement("canvas")
    this.pageCtx = this.pageCanvas.getContext("2d")

    this.cleanCanvas = document.createElement("canvas")
    this.cleanCtx = this.cleanCanvas.getContext("2d")

    this.W = 0
    this.H = 0
    this.dpr = 1
    this.headX = 0
    this.headY = 0
    this.headR = 90
    this.state = "idle" // "idle" | "cracking" | "shattered" | "rebuilding"
    this.zWords = []
    this.cracks = []
    this.dust = []
    this.shards = []
    this.flash = 0
    this.flashX = 0
    this.flashY = 0
    this.rebuildAge = 0
    this.rebuildDur = 1.8
    this.caption = null
  }

  resize(W, H, dpr = 1) {
    this.W = Math.max(1, W)
    this.H = Math.max(1, H)
    this.dpr = Math.min(dpr, W < 640 ? 1.25 : 1.5)
    // Phones skip the chromatic and motion-blur copies, cache each pane's face and bake settled
    // cracks: redrawing every pane from the full page each frame held the finale near 20 fps.
    this.lite = W < 640 || matchMedia("(pointer: coarse)").matches
    const pw = Math.max(1, Math.round(this.W * this.dpr))
    const ph = Math.max(1, Math.round(this.H * this.dpr))
    if (this.canvas.width !== pw || this.canvas.height !== ph) {
      this.canvas.width = pw
      this.canvas.height = ph
      this.basePageCanvas.width = pw
      this.basePageCanvas.height = ph
      this.pageCanvas.width = pw
      this.pageCanvas.height = ph
      this.cleanCanvas.width = pw
      this.cleanCanvas.height = ph
      this.crackLayer = null
      for (const c of this.cracks) c.baked = false
    }
  }

  setHeadLight(x, y, R) {
    this.headX = x
    this.headY = y
    this.headR = R
  }

  setCaption(caption) {
    this.caption = caption
  }

  reset() {
    this.state = "idle"
    this.zWords.length = 0
    this.cracks.length = 0
    this.crackLayer = null
    this.dust.length = 0
    this.shards.length = 0
    this.flash = 0
    this.rebuildAge = 0
    this.caption = null
    this.root.removeAttribute("data-screen-shattered")
    const dl = this.root.querySelector(".hero .download")
    if (dl) {
      dl.style.transition = ""
      dl.style.transform = ""
    }
    this.ctx.setTransform(1, 0, 0, 1, 0, 0)
    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height)
  }

  destroy() {
    this.reset()
    this.canvas.remove()
  }

  captureViewport(targetCtx, opts = {}) {
    const { W, H, dpr } = this
    if (!W || !H) return
    const cleanMode = !!opts.clean
    const rootRect = this.root.getBoundingClientRect()

    targetCtx.save()
    targetCtx.setTransform(dpr, 0, 0, dpr, 0, 0)
    targetCtx.fillStyle = "#070707"
    targetCtx.fillRect(0, 0, W, H)

    if (this.stageCanvas.width > 0 && this.stageCanvas.height > 0) {
      targetCtx.drawImage(this.stageCanvas, 0, 0, W, H)
    }

    const brand = this.root.querySelector(".brand")
    if (brand) {
      const br = brand.getBoundingClientRect()
      const bx = br.left - rootRect.left
      const by = br.top - rootRect.top
      targetCtx.fillStyle = "#f4f4f6"
      targetCtx.font = `700 14px ${SANS}`
      targetCtx.textBaseline = "middle"
      targetCtx.textAlign = "left"
      targetCtx.fillText("Hex", bx + 24, by + br.height / 2)
      targetCtx.strokeStyle = "#f4f4f6"
      targetCtx.lineWidth = 1.6
      targetCtx.strokeRect(bx + 2, by + br.height / 2 - 6, 12, 12)
    }

    const h1 = this.root.querySelector(".hero__copy h1")
    const h1Style = h1 ? getComputedStyle(h1) : null
    const fontSize = h1Style ? parseFloat(h1Style.fontSize) || 56 : 56
    targetCtx.save()
    targetCtx.font = `680 ${fontSize}px ${SANS}`
    if ("letterSpacing" in targetCtx && h1Style?.letterSpacing) {
      targetCtx.letterSpacing = h1Style.letterSpacing
    }
    targetCtx.textBaseline = "middle"
    targetCtx.textAlign = "center"
    targetCtx.fillStyle = "#f4f4f6"
    if (cleanMode && h1) {
      const hr = h1.getBoundingClientRect()
      targetCtx.fillText("Speak. It appears.", hr.left - rootRect.left + hr.width / 2, hr.top - rootRect.top + hr.height / 2)
    } else {
      for (const wEl of this.root.querySelectorAll("[data-title-word]")) {
        const wr = wEl.getBoundingClientRect()
        const cx = wr.left - rootRect.left + wr.width / 2
        const cy = wr.top - rootRect.top + wr.height / 2
        targetCtx.fillText((wEl.textContent || "").trim(), cx, cy)
      }
    }
    targetCtx.restore()

    const sub = this.root.querySelector(".hero__copy p")
    if (sub) {
      const sr = sub.getBoundingClientRect()
      const subStyle = getComputedStyle(sub)
      const subSize = parseFloat(subStyle.fontSize) || 17
      targetCtx.font = `450 ${subSize}px ${SANS}`
      targetCtx.fillStyle = "rgba(212, 214, 222, 0.88)"
      targetCtx.textAlign = "center"
      targetCtx.textBaseline = "top"
      const text = (sub.textContent || "").replace(/\s+/g, " ").trim()
      const maxW = Math.max(220, sr.width)
      const words = text.split(" ")
      let line = ""
      let ly = sr.top - rootRect.top
      const lh = subSize * 1.48
      for (const word of words) {
        const test = line ? `${line} ${word}` : word
        if (line && targetCtx.measureText(test).width > maxW) {
          targetCtx.fillText(line, sr.left - rootRect.left + sr.width / 2, ly)
          line = word
          ly += lh
        } else {
          line = test
        }
      }
      if (line) targetCtx.fillText(line, sr.left - rootRect.left + sr.width / 2, ly)
    }

    if (!opts.skipDownload) {
      const dl = this.root.querySelector(".hero .download")
      if (dl) {
        const prev = dl.style.transform
        if (cleanMode) dl.style.transform = ""
        const dr = dl.getBoundingClientRect()
        if (cleanMode) dl.style.transform = prev
        this.drawDownloadPill(
          targetCtx,
          dr.left - rootRect.left,
          dr.top - rootRect.top,
          dr.width,
          dr.height,
          (dl.textContent || "Download for Mac").trim(),
        )
      }
    }

    const req = this.root.querySelector(".hero .requirements")
    if (req) {
      const rr = req.getBoundingClientRect()
      targetCtx.font = `450 12.5px ${MONO}`
      targetCtx.fillStyle = "rgba(160, 164, 176, 0.78)"
      targetCtx.textAlign = "center"
      targetCtx.textBaseline = "middle"
      targetCtx.fillText((req.textContent || "").trim(), rr.left - rootRect.left + rr.width / 2, rr.top - rootRect.top + rr.height / 2)
    }

    targetCtx.restore()
  }

  drawDownloadPill(ctx, x, y, w, h, label) {
    ctx.save()
    ctx.shadowColor = "rgba(0, 0, 0, 0.68)"
    ctx.shadowBlur = 22
    ctx.shadowOffsetY = 6
    ctx.beginPath()
    ctx.roundRect(x, y, w, h, h / 2)
    ctx.fillStyle = "#f5f5f7"
    ctx.fill()
    ctx.restore()

    ctx.save()
    ctx.fillStyle = "#090a0d"
    ctx.font = `620 15px ${SANS}`
    ctx.textAlign = "center"
    ctx.textBaseline = "middle"
    ctx.fillText(label, x + w / 2 + 4, y + h / 2 + 0.5)
    ctx.beginPath()
    ctx.arc(x + 25, y + h / 2 + 1, 5.5, 0, Math.PI * 2)
    ctx.fill()
    ctx.restore()
  }

  fireWordAtScreen({ text, fromX, fromY, targetX, targetY, dur = 0.17, final = false, hitsDownload = false, onImpact }) {
    if (this.state === "idle") this.state = "cracking"
    this.zWords.push({
      text,
      fromX,
      fromY,
      targetX,
      targetY,
      age: 0,
      dur,
      final,
      hitsDownload,
      rot: this.rnd(-0.11, 0.11),
      vrot: this.rnd(-1.3, 1.3),
      onImpact,
    })
  }

  // Recursive stress-field crack propagation with time-based outward reveal (t0 -> t1),
  // crack-arrest (T-junctions) against prior cracks, and secondary/tertiary hairline bifurcations.
  buildCrackBranch(
    originX,
    originY,
    x0,
    y0,
    startAng,
    totalLen,
    uStart,
    uSpan,
    tStart,
    tSpan,
    depth,
    priorSegs,
    outSegments,
    outSpecks,
  ) {
    const steps = depth === 0 ? 22 : depth === 1 ? 12 : 6
    let px = x0
    let py = y0
    let ang = startAng
    const stepLen = totalLen / steps

    for (let s = 1; s <= steps; s++) {
      const frac0 = (s - 1) / steps
      const frac1 = s / steps
      const u0 = clamp(uStart + frac0 * uSpan, 0, 1)
      let u1 = clamp(uStart + frac1 * uSpan, 0, 1)
      // Outward propagation time: fast tensile burst near impact, slightly decelerating toward tip
      const t0 = tStart + Math.pow(frac0, 1.18) * tSpan
      let t1 = tStart + Math.pow(frac1, 1.18) * tSpan

      const kink = this.rnd() < 0.22 ? (this.rnd() < 0.5 ? -1 : 1) * this.rnd(0.1, 0.24) : 0
      ang += this.rnd(-0.052, 0.052) + kink
      const rStep = stepLen * this.rnd(0.78, 1.24)
      let nxPos = px + Math.cos(ang) * rStep
      let nyPos = py + Math.sin(ang) * rStep

      // Check crack-arrest against existing fracture segments from earlier impacts
      let arrested = false
      const distFromOwnCrater = Math.hypot(nxPos - originX, nyPos - originY)
      if (distFromOwnCrater > 34 && priorSegs.length > 0) {
        let bestHit = null
        for (let p = 0; p < priorSegs.length; p++) {
          const ps = priorSegs[p]
          const hit = segmentIntersection(px, py, nxPos, nyPos, ps.x0, ps.y0, ps.x1, ps.y1)
          if (hit && (!bestHit || hit.t < bestHit.t)) bestHit = hit
        }
        if (bestHit) {
          nxPos = bestHit.x
          nyPos = bestHit.y
          u1 = lerp(u0, u1, bestHit.t)
          t1 = lerp(t0, t1, bestHit.t)
          arrested = true
          // Spawn a bright T-junction stress glint where this crack arrests against the prior crack
          outSpecks.push({
            x: nxPos,
            y: nyPos,
            r: this.rnd(1.4, 2.8),
            ang: ang,
            stretch: 1.8,
            gold: false,
            star: true,
            revealAt: t1,
            alpha: 0.95,
          })
        }
      }

      const dx = nxPos - px
      const dy = nyPos - py
      const dl = Math.hypot(dx, dy) || 1
      const nx = -dy / dl
      const ny = dx / dl

      outSegments.push({
        x0: px,
        y0: py,
        x1: nxPos,
        y1: nyPos,
        mx: (px + nxPos) * 0.5,
        my: (py + nyPos) * 0.5,
        nx,
        ny,
        u0,
        u1,
        t0,
        t1,
        depth,
        spec: this.rnd(0.72, 1.0),
      })

      // Embedded crystalline micro-chips and silica glints revealed as the crack front passes t1
      if (this.rnd() < (depth === 0 ? 0.72 : 0.38)) {
        const off = (this.rnd() < 0.5 ? -1 : 1) * this.rnd(0.3, 4.4) * (1 - u1 * 0.65)
        outSpecks.push({
          x: (px + nxPos) * 0.5 + nx * off,
          y: (py + nyPos) * 0.5 + ny * off,
          r: this.rnd(0.7, 2.2) * (1 - u1 * 0.45),
          ang: ang + this.rnd(-0.5, 0.5),
          stretch: this.rnd(1.2, 3.0),
          gold: this.rnd() < 0.28,
          star: this.rnd() < 0.16,
          revealAt: t1,
          alpha: this.rnd(0.62, 0.98) * (1 - u1 * 0.32),
        })
      }

      if (arrested) break

      // Secondary and tertiary hairline Y-bifurcations spawn at the exact moment t1 when the parent front passes!
      if (depth === 0 && (s === 5 || s === 11 || s === 16) && this.rnd() < 0.78) {
        const side = this.rnd() < 0.5 ? -1 : 1
        const forkAng = ang + side * this.rnd(0.24, 0.52)
        const forkLen = (totalLen - s * stepLen) * this.rnd(0.46, 0.76)
        const remT = Math.max(0.025, (tStart + tSpan - t1) * this.rnd(0.75, 1.05))
        this.buildCrackBranch(
          originX,
          originY,
          nxPos,
          nyPos,
          forkAng,
          forkLen,
          u1,
          (1 - u1) * 0.75,
          t1,
          remT,
          1,
          priorSegs,
          outSegments,
          outSpecks,
        )
      } else if (depth === 1 && (s === 4 || s === 8) && this.rnd() < 0.52) {
        const side = this.rnd() < 0.5 ? -1 : 1
        const forkAng = ang + side * this.rnd(0.28, 0.58)
        const forkLen = totalLen * this.rnd(0.32, 0.54)
        const remT = Math.max(0.018, (tStart + tSpan - t1) * 0.8)
        this.buildCrackBranch(
          originX,
          originY,
          nxPos,
          nyPos,
          forkAng,
          forkLen,
          u1,
          (1 - u1) * 0.5,
          t1,
          remT,
          2,
          priorSegs,
          outSegments,
          outSpecks,
        )
      }

      px = nxPos
      py = nyPos
    }
  }

  // Adds a photorealistic ballistic impact fracture at (x, y) that propagates outward over 95–165 ms.
  addCrack(x, y, power = 1, hitsDownload = false) {
    if (this.state === "idle") this.state = "cracking"
    if (hitsDownload) {
      const dl = this.root.querySelector(".hero .download")
      if (dl) {
        dl.style.transition = "none"
        dl.style.transform = "translate3d(12px, 14px, 0) rotate(7deg)"
      }
    }
    this.captureViewport(this.basePageCtx, { skipDownload: true })

    this.flash = Math.max(this.flash, 0.34 * power)
    this.flashX = x
    this.flashY = y

    // Collect primary/secondary segments from prior impacts so new cracks can arrest (T-junction) against them
    const priorSegs = []
    for (const prev of this.cracks) {
      for (let k = 0; k < prev.segments.length; k += 2) {
        const sg = prev.segments[k]
        if (sg.depth <= 1) priorSegs.push(sg)
      }
    }

    const segments = []
    const specks = []
    const growDur = 0.135 * lerp(0.88, 1.22, clamp(power, 0.7, 1.4))

    // 1. Primary, medium & dense inner crater rays (42-52 rays per impact, matching target-cracks.png)
    const rayCount = Math.round(lerp(40, 50, clamp(power, 0.7, 1.4)))
    const maxRunnerLen = Math.min(this.W, this.H) * lerp(0.4, 0.66, clamp(power, 0.7, 1.4))
    const primaryRays = []

    for (let i = 0; i < rayCount; i++) {
      const baseAng = (i / rayCount) * Math.PI * 2 + this.rnd(-0.065, 0.065)
      const isLongRunner = i % 3 === 0
      const isMedium = i % 3 === 1
      const len = isLongRunner
        ? maxRunnerLen * this.rnd(0.72, 1.18)
        : isMedium
          ? maxRunnerLen * this.rnd(0.32, 0.62)
          : maxRunnerLen * this.rnd(0.08, 0.22)

      // Crater blooms in first 12 ms; radial rays race out over 90–160 ms
      const rayStart = this.rnd(0.006, 0.018)
      const raySpan = growDur * (isLongRunner ? this.rnd(0.82, 1.15) : isMedium ? this.rnd(0.48, 0.78) : this.rnd(0.22, 0.42))

      const startIdx = segments.length
      this.buildCrackBranch(
        x,
        y,
        x,
        y,
        baseAng,
        len,
        0,
        1,
        rayStart,
        raySpan,
        isLongRunner ? 0 : 1,
        priorSegs,
        segments,
        specks,
      )
      const spine = [{ x, y, t: rayStart }]
      for (let k = startIdx; k < segments.length; k++) {
        if (segments[k].depth === (isLongRunner ? 0 : 1)) {
          spine.push({ x: segments[k].x1, y: segments[k].y1, t: segments[k].t1 })
        }
      }
      primaryRays.push({ baseAng, len, spine, raySpan })
    }

    // 2. Dense fine micro-cracking & concentric Hertzian stress rings (16 rings: super-tight near crater, wider outward)
    const ringDist = [5, 7.5, 10.5, 14, 18.5, 24, 31, 40, 51, 65, 82, 104, 132, 166, 206, 252].map((d) => d * power)
    const rings = []
    const wedges = []

    for (let i = 0; i < primaryRays.length; i++) {
      const rA = primaryRays[i]
      const rB = primaryRays[(i + 1) % primaryRays.length]
      const midAng = (rA.baseAng + rB.baseAng) * 0.5

      // Non-overlapping optical refraction sectors bounded by actual ring cracks (never cutting across the central head!)
      if (i % 2 === 0) {
        const rNext = primaryRays[(i + 2) % primaryRays.length]
        const wR = Math.min(rA.len * 0.72, rNext.len * 0.72, 138 * power)
        if (wR > 28) {
          const midX = x + Math.cos(midAng) * wR * 0.55
          const midY = y + Math.sin(midAng) * wR * 0.55
          const distToHead = Math.hypot(midX - (this.headX || this.W * 0.5), midY - (this.headY || this.H * 0.38))
          const overlapsDownload = midY > this.H * 0.64 && Math.abs(midX - this.W * 0.5) < 165
          // Only refract outside the glowing head aura & Download pill so wedges never mask them with dark background
          if (distToHead > (this.headR || 95) * 1.78 && !overlapsDownload) {
            const disp = this.rnd(5.5, 12.0) * power
            const sign = (i / 2) % 2 === 0 ? 1 : -1
            wedges.push({
              poly: [
                [x + Math.cos(rA.baseAng) * 10, y + Math.sin(rA.baseAng) * 10],
                [x + Math.cos(rA.baseAng) * wR, y + Math.sin(rA.baseAng) * wR],
                [x + Math.cos(midAng) * wR * 1.04, y + Math.sin(midAng) * wR * 1.04],
                [x + Math.cos(rNext.baseAng) * wR, y + Math.sin(rNext.baseAng) * wR],
                [x + Math.cos(rNext.baseAng) * 10, y + Math.sin(rNext.baseAng) * 10],
              ],
              ox: Math.cos(midAng + sign * 1.15) * disp,
              oy: Math.sin(midAng + sign * 1.15) * disp,
              chromaX: Math.cos(midAng) * 2.4 * sign,
              chromaY: Math.sin(midAng) * 2.4 * sign,
              revealDur: growDur * 0.65,
            })
          }
        }
      }

      for (let ri = 0; ri < ringDist.length; ri++) {
        const baseD = ringDist[ri]
        if (baseD > rA.len * 0.95 || baseD > rB.len * 0.95) continue
        const prob = ri < 8 ? 0.96 : ri < 12 ? 0.78 : 0.48
        if (this.rnd() > prob) continue

        const dA = baseD * this.rnd(0.91, 1.09)
        const dB = baseD * this.rnd(0.91, 1.09)
        const ptA = this.pointAlongSpine(rA.spine, dA)
        const ptB = this.pointAlongSpine(rB.spine, dB)
        if (!ptA || !ptB) continue

        const dx = ptB.x - ptA.x
        const dy = ptB.y - ptA.y
        const dl = Math.hypot(dx, dy) || 1
        const nx = -dy / dl
        const ny = dx / dl
        const u = clamp(baseD / (252 * power), 0, 1)
        const arch = this.rnd(-1.8, 1.4)
        const cx = (ptA.x + ptB.x) * 0.5 + nx * arch
        const cy = (ptA.y + ptB.y) * 0.5 + ny * arch

        // Ring crack forms right behind the radial front (after both bounding radials pass this radius)
        const tRingStart = Math.max(ptA.t, ptB.t) + this.rnd(0.006, 0.018)
        const tRingEnd = tRingStart + this.rnd(0.016, 0.032)

        rings.push({
          x0: ptA.x,
          y0: ptA.y,
          cx,
          cy,
          x1: ptB.x,
          y1: ptB.y,
          nx,
          ny,
          u,
          t0: tRingStart,
          t1: tRingEnd,
          fine: ri < 6 || this.rnd() < 0.45,
          spec: this.rnd(0.72, 1.0),
        })
      }
    }

    // 3. Dense fine hairline cross-fissures inside r < 75px (matching the shattered safety-glass core of target-cracks.png)
    const microCracks = []
    for (let i = 0; i < 58; i++) {
      const a0 = this.rnd(0, Math.PI * 2)
      const r0 = Math.pow(this.rnd(), 1.35) * 68 * power
      const len = this.rnd(5, 22) * (1 - r0 / (85 * power))
      const a1 = a0 + (this.rnd() < 0.5 ? 1 : -1) * this.rnd(0.4, 1.35)
      const x0 = x + Math.cos(a0) * r0
      const y0 = y + Math.sin(a0) * r0
      const x1 = x0 + Math.cos(a1) * len
      const y1 = y0 + Math.sin(a1) * len
      const tReveal = 0.008 + (r0 / (68 * power)) * growDur * 0.55
      microCracks.push({ x0, y0, x1, y1, t0: tReveal, alpha: this.rnd(0.45, 0.88) })
    }

    // 4. Bright powdered-white crushed silica impact crater (matching target-cracks.png!)
    const craterFacets = []
    const facetCount = Math.round(92 * power)
    for (let i = 0; i < facetCount; i++) {
      const ang = this.rnd(0, Math.PI * 2)
      const dist = Math.pow(this.rnd(), 1.55) * 32 * power
      const fx = x + Math.cos(ang) * dist
      const fy = y + Math.sin(ang) * dist
      const r = this.rnd(1.2, 5.2) * (1 - dist / (38 * power))
      const a0 = this.rnd(0, Math.PI * 2)
      craterFacets.push({
        pts: [
          [fx + Math.cos(a0) * r, fy + Math.sin(a0) * r],
          [fx + Math.cos(a0 + 2.1) * r * 0.82, fy + Math.sin(a0 + 2.1) * r * 0.82],
          [fx + Math.cos(a0 + 4.2) * r * 1.08, fy + Math.sin(a0 + 4.2) * r * 1.08],
        ],
        dark: dist < 5.5 * power ? this.rnd() < 0.65 : this.rnd() < 0.18,
        revealAt: (dist / (32 * power)) * 0.028,
        alpha: this.rnd(0.75, 1.0),
      })
    }

    // 160 fine crystalline white/silver & golden powder specks around the impact rosette
    for (let i = 0; i < 160; i++) {
      const ang = this.rnd(0, Math.PI * 2)
      const dist = Math.pow(this.rnd(), 1.6) * 96 * power
      specks.push({
        x: x + Math.cos(ang) * dist,
        y: y + Math.sin(ang) * dist,
        r: this.rnd(0.65, 2.3),
        ang: ang + this.rnd(-0.4, 0.4),
        stretch: this.rnd(1.1, 2.6),
        gold: this.rnd() < 0.25,
        star: this.rnd() < 0.16,
        revealAt: (dist / (96 * power)) * growDur * 0.7,
        alpha: this.rnd(0.62, 0.98),
      })
    }

    this.cracks.push({
      x,
      y,
      power,
      growDur,
      segments,
      rings,
      wedges,
      microCracks,
      craterFacets,
      specks,
      age: 0,
    })

    // 5. Spawn sparkling crystalline micro-splinters and glass powder
    const dustCount = Math.round(78 * power)
    for (let i = 0; i < dustCount; i++) {
      const ang = this.rnd(0, Math.PI * 2)
      const spd = this.rnd(35, 310) * power
      const isNeedle = this.rnd() < 0.42
      this.dust.push({
        x: x + this.rnd(-10, 10),
        y: y + this.rnd(-10, 10),
        vx: Math.cos(ang) * spd,
        vy: Math.sin(ang) * spd - this.rnd(25, 120),
        rot: ang + this.rnd(-0.5, 0.5),
        vrot: this.rnd(-14, 14),
        r: isNeedle ? this.rnd(1.8, 4.4) : this.rnd(0.8, 2.2),
        aspect: isNeedle ? this.rnd(0.2, 0.4) : this.rnd(0.65, 1.0),
        kind: this.rnd() < 0.18 ? "bokeh" : isNeedle ? "shard" : "spark",
        gold: this.rnd() < 0.48,
        green: this.rnd() < 0.22,
        phase: this.rnd(0, Math.PI * 2),
        age: 0,
        life: this.rnd(0.75, 1.65),
      })
    }
  }

  pointAlongSpine(spine, targetDist) {
    let acc = 0
    for (let i = 0; i < spine.length - 1; i++) {
      const a = spine[i]
      const b = spine[i + 1]
      const seg = Math.hypot(b.x - a.x, b.y - a.y)
      if (acc + seg >= targetDist) {
        const f = (targetDist - acc) / (seg || 1)
        return { x: lerp(a.x, b.x, f), y: lerp(a.y, b.y, f), t: lerp(a.t, b.t, f) }
      }
      acc += seg
    }
    return spine[spine.length - 1]
  }

  // Shatters the entire cracked viewport into Lloyd-relaxed 3D extruded tempered glass slabs +
  // a 3-tier cloud of ~620 sparkling golden/white glass dust & bokeh motes (matching target-shatter.png!).
  shatter(epicX = this.W * 0.5, epicY = this.H * 0.46) {
    const { W, H, dpr } = this
    if (!W || !H) return

    // Bake fully-grown cracks onto pageCanvas so shards carry the crisp crystalline fracture lines
    for (const c of this.cracks) c.age = Math.max(c.age, 1.0)
    this.captureViewport(this.cleanCtx, { clean: true, skipDownload: true })
    this.captureViewport(this.pageCtx, { skipDownload: true })
    this.pageCtx.save()
    this.pageCtx.setTransform(dpr, 0, 0, dpr, 0, 0)
    this.drawCracksOnto(this.pageCtx, 0.92)
    this.pageCtx.restore()

    this.root.setAttribute("data-screen-shattered", "")
    const dl = this.root.querySelector(".hero .download")
    const rootRect = this.root.getBoundingClientRect()
    let dlRect = null
    if (dl) {
      const r = dl.getBoundingClientRect()
      dlRect = {
        cx: r.left - rootRect.left + r.width / 2,
        cy: r.top - rootRect.top + r.height / 2,
        w: r.width,
        h: r.height,
        label: (dl.textContent || "Download for Mac").trim(),
      }
      dl.style.transition = ""
      dl.style.transform = ""
    }

    this.state = "shattered"
    this.flash = 0.52
    this.flashX = epicX
    this.flashY = epicY
    this.shards.length = 0

    // 1. Well-spaced Voronoi tessellation with minimum seed separation so every slab is a chunky 3D crystal pane
    const cols = W < 640 ? 6 : 9
    const rows = W < 640 ? 5 : 6
    const seeds = []
    const minSep = Math.min(W / cols, H / rows) * 0.52
    for (let iy = 0; iy < rows; iy++) {
      for (let ix = 0; ix < cols; ix++) {
        const u = (ix + 0.5) / cols
        const v = (iy + 0.5) / rows
        let sx = u * W + this.rnd(-W / cols, W / cols) * 0.3
        let sy = v * H + this.rnd(-H / rows, H / rows) * 0.3
        sx = clamp(sx, 28, W - 28)
        sy = clamp(sy, 28, H - 28)
        // Enforce minimum separation so no degenerate needle cells form
        for (const prev of seeds) {
          const dx = sx - prev[0]
          const dy = sy - prev[1]
          const d = Math.hypot(dx, dy)
          if (d > 0.1 && d < minSep) {
            sx += (dx / d) * (minSep - d) * 0.6
            sy += (dy / d) * (minSep - d) * 0.6
          }
        }
        seeds.push([clamp(sx, 24, W - 24), clamp(sy, 24, H - 24)])
      }
    }

    const screenPoly = [
      [0, 0],
      [W, 0],
      [W, H],
      [0, H],
    ]
    const maxDist = Math.hypot(W * 0.55, H * 0.55)
    const headX = this.headX || W * 0.5
    const headY = this.headY || H * 0.38
    const clearR = (this.headR || 95) * 1.9

    for (let i = 0; i < seeds.length; i++) {
      const si = seeds[i]
      let poly = screenPoly
      for (let j = 0; j < seeds.length; j++) {
        if (i === j) continue
        const sj = seeds[j]
        const nx = sj[0] - si[0]
        const ny = sj[1] - si[1]
        if (nx * nx + ny * ny < 1) continue
        const mx = (si[0] + sj[0]) * 0.5
        const my = (si[1] + sj[1]) * 0.5
        poly = clipPolyHalfPlane(poly, mx, my, nx, ny)
        if (poly.length < 3) break
      }
      if (poly.length < 3) continue

      let cx = 0
      let cy = 0
      let area = 0
      for (let k = 0; k < poly.length; k++) {
        const p0 = poly[k]
        const p1 = poly[(k + 1) % poly.length]
        cx += p0[0]
        cy += p0[1]
        area += p0[0] * p1[1] - p1[0] * p0[1]
      }
      cx /= poly.length
      cy /= poly.length
      area = Math.abs(area) * 0.5
      const localPoly = poly.map((p) => [p[0] - cx, p[1] - cy])

      // Internal branching star-fracture inside each 3D slab (matching target-shatter.png!)
      const innerCracks = []
      const gx = localPoly[0][0] * 0.22
      const gy = localPoly[0][1] * 0.22
      for (let k = 0; k < localPoly.length; k++) {
        const vp = localPoly[k]
        const mx = (gx + vp[0]) * 0.5 + this.rnd(-6, 6)
        const my = (gy + vp[1]) * 0.5 + this.rnd(-6, 6)
        innerCracks.push([gx, gy, mx, my, vp[0] * 0.94, vp[1] * 0.94])
        // Secondary fine cross-crack inside the slab
        const vpNext = localPoly[(k + 1) % localPoly.length]
        innerCracks.push([mx, my, (mx + vpNext[0] * 0.55) * 0.5, (my + vpNext[1] * 0.55) * 0.5, vpNext[0] * 0.55, vpNext[1] * 0.55])
      }

      const dx = cx - headX
      const dy = cy - headY
      const dist = Math.hypot(dx, dy)
      const normD = clamp(dist / maxDist, 0, 1)
      const outAng = Math.atan2(dy, dx) + this.rnd(-0.14, 0.14)
      const dirX = Math.cos(outAng)
      const dirY = Math.sin(outAng)

      // Shards over the head open a clear window around the head, then suspend in slow-motion bullet-time
      const needClear = dist < clearR ? (clearR - dist) * 3.2 + 240 : lerp(220, 75, normD)
      const impulse = needClear * this.rnd(0.86, 1.16)
      const zVel = normD > 0.54 ? this.rnd(0.08, 0.22) : this.rnd(-0.04, 0.12)

      this.shards.push({
        isDownload: false,
        isSplinter: false,
        homeX: cx,
        homeY: cy,
        x: cx,
        y: cy,
        z: 1,
        vz: zVel,
        vx: dirX * impulse + this.rnd(-24, 24),
        vy: dirY * impulse * 0.85 - this.rnd(28, 80),
        driftX: dirX * this.rnd(10, 28),
        driftY: dirY * this.rnd(6, 20) + this.rnd(10, 28),
        ang: this.rnd(-0.08, 0.08),
        spin: (dirX >= 0 ? 1 : -1) * this.rnd(0.32, 0.95),
        pitch: this.rnd(0.25, 0.78) * (this.rnd() < 0.5 ? 1 : -1),
        vPitch: this.rnd(0.55, 1.35) * (this.rnd() < 0.5 ? 1 : -1),
        yaw: this.rnd(0.25, 0.82) * (this.rnd() < 0.5 ? 1 : -1),
        vYaw: this.rnd(0.55, 1.35) * (this.rnd() < 0.5 ? 1 : -1),
        thick: lerp(8.5, 16.0, clamp(area / 16000, 0, 1)),
        refractShift: this.rnd(9, 18),
        specSeed: this.rnd(0, Math.PI * 2),
        hasCausticStar: this.rnd() < 0.42,
        poly: localPoly,
        innerCracks,
        normD,
        delay: normD * 0.038,
        age: 0,
        fromX: cx,
        fromY: cy,
        fromZ: 1,
        fromAng: 0,
        fromPitch: 0,
        fromYaw: 0,
      })
    }

    // 2. Secondary 3D angular crystal chunks (well-proportioned triangles/quads, never skinny lines)
    const chunkCount = W < 640 ? 34 : 62
    for (let i = 0; i < chunkCount; i++) {
      const ang = this.rnd(0, Math.PI * 2)
      const rDist = this.rnd(clearR * 0.68, maxDist * 0.94)
      const sx = clamp(headX + Math.cos(ang) * rDist, 24, W - 24)
      const sy = clamp(headY + Math.sin(ang) * rDist, 24, H - 24)
      const normD = clamp(rDist / maxDist, 0, 1)
      const r0 = this.rnd(8, 22)
      const localPoly = [
        [-r0, -r0 * 0.45],
        [r0 * 0.2, -r0 * 0.85],
        [r0 * 1.05, r0 * 0.25],
        [-r0 * 0.25, r0 * 0.78],
      ]
      const spd = this.rnd(75, 220)
      this.shards.push({
        isDownload: false,
        isSplinter: true,
        homeX: sx,
        homeY: sy,
        x: sx,
        y: sy,
        z: this.rnd(0.88, 1.28),
        vz: this.rnd(-0.04, 0.14),
        vx: Math.cos(ang) * spd,
        vy: Math.sin(ang) * spd * 0.8 - this.rnd(18, 52),
        driftX: Math.cos(ang) * this.rnd(6, 20),
        driftY: this.rnd(10, 26),
        ang: ang + this.rnd(-0.8, 0.8),
        spin: this.rnd(-1.5, 1.5),
        pitch: this.rnd(0.25, 0.85),
        vPitch: this.rnd(-1.8, 1.8),
        yaw: this.rnd(0.25, 0.85),
        vYaw: this.rnd(-1.8, 1.8),
        thick: this.rnd(4.5, 8.5),
        refractShift: 8,
        specSeed: this.rnd(0, Math.PI * 2),
        hasCausticStar: false,
        poly: localPoly,
        innerCracks: [[-r0 * 0.7, -r0 * 0.2, 0, 0, r0 * 0.8, r0 * 0.2]],
        normD,
        delay: normD * 0.03,
        age: 0,
        fromX: sx,
        fromY: sy,
        fromZ: 1,
        fromAng: 0,
        fromPitch: 0,
        fromYaw: 0,
      })
    }

    // 3. Dedicated hero piece for the knocked-off Download button
    if (dlRect) {
      this.shards.push({
        isDownload: true,
        isSplinter: false,
        homeX: dlRect.cx,
        homeY: dlRect.cy,
        w: dlRect.w,
        h: dlRect.h,
        label: dlRect.label,
        x: dlRect.cx + 10,
        y: dlRect.cy - 28,
        z: 1.06,
        vz: 0.02,
        vx: 38,
        vy: -240,
        driftX: 12,
        driftY: -6,
        ang: 0.14,
        spin: 0.22,
        pitch: 0.12,
        vPitch: 0.28,
        yaw: 0.14,
        vYaw: 0.24,
        thick: 9,
        normD: 0.15,
        delay: 0,
        age: 0,
        fromX: dlRect.cx,
        fromY: dlRect.cy,
        fromZ: 1,
        fromAng: 0,
        fromPitch: 0,
        fromYaw: 0,
      })
    }

    // 4. Dense 3-tier nebula of ~620 golden/white glass glitter, micro-shards & warm DOF bokeh motes (matching target-shatter.png!)
    const glitterCount = W < 640 ? 280 : 620
    for (let i = 0; i < glitterCount; i++) {
      const ang = this.rnd(0, Math.PI * 2)
      const dist = Math.pow(this.rnd(), 0.76) * maxDist * 0.96
      const gx = headX + Math.cos(ang) * dist * 0.78 + this.rnd(-28, 28)
      const gy = headY + Math.sin(ang) * dist * 0.78 + this.rnd(-28, 28)
      const spd = this.rnd(18, 165)
      const roll = this.rnd()
      const kind = roll < 0.13 ? "bokeh" : roll < 0.36 ? "shard" : "spark"
      this.dust.push({
        x: gx,
        y: gy,
        vx: Math.cos(ang) * spd + this.rnd(-18, 18),
        vy: Math.sin(ang) * spd * 0.75 - this.rnd(12, 55),
        BulletTime: true,
        rot: this.rnd(0, Math.PI * 2),
        vrot: this.rnd(-7, 7),
        r: kind === "bokeh" ? this.rnd(3.6, 8.5) : kind === "shard" ? this.rnd(2.2, 4.8) : this.rnd(1.15, 3.1),
        aspect: kind === "shard" ? this.rnd(0.32, 0.62) : 1.0,
        kind,
        star: kind === "spark" && this.rnd() < 0.28,
        gold: this.rnd() < (kind === "bokeh" ? 0.92 : 0.76),
        green: this.rnd() < 0.18,
        phase: this.rnd(0, Math.PI * 2),
        age: 0,
        life: this.rnd(4.6, 6.4),
      })
    }
  }

  startRebuild(dur = 1.8) {
    if (this.state !== "shattered") return
    this.state = "rebuilding"
    this.rebuildAge = 0
    this.rebuildDur = dur
    this.needsCleanCapture = true
    for (const s of this.shards) {
      s.fromX = clamp(s.x, -180, this.W + 180)
      s.fromY = Math.min(s.y, this.H + 240)
      s.fromZ = s.z ?? 1
      s.fromAng = ((s.ang + Math.PI) % (Math.PI * 2)) - Math.PI
      s.fromPitch = ((s.pitch + Math.PI) % (Math.PI * 2)) - Math.PI
      s.fromYaw = ((s.yaw + Math.PI) % (Math.PI * 2)) - Math.PI
    }
  }

  // Draws the propagating ballistic fractures: mostly crystalline white/silver with refraction,
  // thinner hairline secondaries, dense fine micro-cracking near craters, bright powdered-white
  // impact cores, and localized golden glints from the head's light.
  drawCracksOnto(ctx, alpha = 1, cracks = this.cracks) {
    const { W, H } = this
    const hx = this.headX || W * 0.5
    const hy = this.headY || H * 0.38
    const headGlowRange = (this.headR || 95) * 3.1

    for (const c of cracks) {
      const age = c.age

      // 1. Optical refraction across angular glass facets + RGB chromatic dispersion (grows with c.age)
      for (const w of c.wedges) {
        const wProg = clamp(age / (w.revealDur || 0.09), 0, 1)
        if (wProg <= 0.02) continue
        const ox = w.ox * wProg
        const oy = w.oy * wProg
        ctx.save()
        ctx.beginPath()
        ctx.moveTo(w.poly[0][0], w.poly[0][1])
        for (let i = 1; i < w.poly.length; i++) ctx.lineTo(w.poly[i][0], w.poly[i][1])
        ctx.closePath()
        ctx.clip()
        ctx.globalAlpha = 0.96 * alpha
        ctx.drawImage(this.basePageCanvas, 0, 0, this.basePageCanvas.width, this.basePageCanvas.height, ox, oy, W, H)
        if (this.lite) {
          ctx.restore()
          continue
        }
        // Chromatic dispersion shift across fracture boundary
        ctx.globalCompositeOperation = "lighter"
        ctx.globalAlpha = 0.16 * wProg * alpha
        ctx.drawImage(
          this.basePageCanvas,
          0,
          0,
          this.basePageCanvas.width,
          this.basePageCanvas.height,
          ox + w.chromaX * wProg,
          oy + w.chromaY * wProg,
          W,
          H,
        )
        ctx.restore()
      }

      // 2. Dense fine micro-cracking near impact center (silver-white safety-glass hairlines)
      ctx.save()
      ctx.lineCap = "round"
      if (c.microCracks) {
        for (const mc of c.microCracks) {
          if (age < mc.t0) continue
          const mProg = clamp((age - mc.t0) / 0.018, 0, 1)
          const ex = lerp(mc.x0, mc.x1, mProg)
          const ey = lerp(mc.y0, mc.y1, mProg)
          ctx.strokeStyle = `rgba(232, 244, 255, ${mc.alpha * alpha})`
          ctx.lineWidth = 0.48
          ctx.beginPath()
          ctx.moveTo(mc.x0, mc.y0)
          ctx.lineTo(ex, ey)
          ctx.stroke()
        }
      }

      // 3. Exponentially spaced concentric Hertzian stress rings (forming just behind the radial front)
      for (const ring of c.rings) {
        if (age < ring.t0) continue
        const rProg = clamp((age - ring.t0) / Math.max(0.008, ring.t1 - ring.t0), 0, 1)
        const taper = Math.pow(1 - ring.u, 0.58)
        const lx = hx - ring.cx
        const ly = hy - ring.cy
        const ld = Math.hypot(lx, ly) || 1
        const ndotl = Math.abs((ring.nx * lx + ring.ny * ly) / ld)
        // Gold ONLY as localized glints where close to the glowing head and angled toward it
        const goldGlint = Math.pow(ndotl, 3.8) * clamp(1 - ld / headGlowRange, 0, 1)

        const xEnd = rProg >= 1 ? ring.x1 : lerp(ring.x0, ring.x1, rProg)
        const yEnd = rProg >= 1 ? ring.y1 : lerp(ring.y0, ring.y1, rProg)
        const cxMid = rProg >= 1 ? ring.cx : lerp(ring.x0, ring.cx, rProg)
        const cyMid = rProg >= 1 ? ring.cy : lerp(ring.y0, ring.cy, rProg)

        const wBase = ring.fine ? 0.52 * taper + 0.2 : 0.85 * taper + 0.24

        // Crisp dark total-internal-reflection shadow line
        ctx.strokeStyle = `rgba(0, 4, 10, ${(0.76 * taper + 0.14) * alpha})`
        ctx.lineWidth = wBase * 1.45
        ctx.beginPath()
        ctx.moveTo(ring.x0 + 0.85, ring.y0 + 0.85)
        ctx.quadraticCurveTo(cxMid + 0.85, cyMid + 0.85, xEnd + 0.85, yEnd + 0.85)
        ctx.stroke()

        // Crystalline silver-white ring crack (with warm gold glint only when catching the head's light)
        if (goldGlint > 0.22) {
          ctx.strokeStyle = `rgba(255, 222, 138, ${Math.min(0.95, goldGlint * 0.9) * alpha})`
          ctx.lineWidth = wBase * 1.35
          ctx.beginPath()
          ctx.moveTo(ring.x0, ring.y0)
          ctx.quadraticCurveTo(cxMid, cyMid, xEnd, yEnd)
          ctx.stroke()
        }

        ctx.strokeStyle = `rgba(244, 250, 255, ${Math.min(1, (0.48 + 0.52 * ring.spec) * (taper + 0.15)) * alpha})`
        ctx.lineWidth = wBase
        ctx.beginPath()
        ctx.moveTo(ring.x0, ring.y0)
        ctx.quadraticCurveTo(cxMid, cyMid, xEnd, yEnd)
        ctx.stroke()
      }

      // 4. Outward-propagating radial cracks + thinner hairline secondaries/tertiaries + leading tip glints
      const activeTips = []
      for (const seg of c.segments) {
        if (age < seg.t0) continue
        const sProg = clamp((age - seg.t0) / Math.max(0.005, seg.t1 - seg.t0), 0, 1)
        const ex = sProg >= 1 ? seg.x1 : lerp(seg.x0, seg.x1, sProg)
        const ey = sProg >= 1 ? seg.y1 : lerp(seg.y0, seg.y1, sProg)
        if (sProg < 1 && seg.depth <= 1) {
          activeTips.push({
            x: ex,
            y: ey,
            ang: Math.atan2(seg.y1 - seg.y0, seg.x1 - seg.x0),
            depth: seg.depth,
          })
        }

        const depthScale = seg.depth === 0 ? 1.0 : seg.depth === 1 ? 0.46 : 0.28
        const taper = Math.pow(1 - seg.u0, 0.72) * depthScale
        const lx = hx - seg.mx
        const ly = hy - seg.my
        const ld = Math.hypot(lx, ly) || 1
        const ndotl = Math.abs((seg.nx * lx + seg.ny * ly) / ld)
        // Localized golden Fresnel glint ONLY near the head on strongly aligned segments
        const goldGlint = Math.pow(ndotl, 3.6) * clamp(1.08 - ld / headGlowRange, 0, 1)

        // Pass A: Dark total-internal-reflection shadow bevel
        const shOff = 0.95 * taper + 0.28
        ctx.strokeStyle = `rgba(0, 3, 9, ${(0.84 * taper + 0.16) * alpha})`
        ctx.lineWidth = 1.75 * taper + 0.32
        ctx.beginPath()
        ctx.moveTo(seg.x0 + shOff, seg.y0 + shOff)
        ctx.lineTo(ex + shOff, ey + shOff)
        ctx.stroke()

        // Pass B: Subtle green borosilicate glass edge tint on inner primary bevels
        if (seg.depth === 0 && seg.u0 < 0.42) {
          ctx.strokeStyle = `rgba(115, 242, 208, ${0.24 * (1 - seg.u0 / 0.42) * alpha})`
          ctx.lineWidth = 1.9 * taper + 0.35
          ctx.beginPath()
          ctx.moveTo(seg.x0 - seg.nx * 0.45, seg.y0 - seg.ny * 0.45)
          ctx.lineTo(ex - seg.nx * 0.45, ey - seg.ny * 0.45)
          ctx.stroke()
        }

        // Pass C: Localized golden specular glint catching the Super Saiyan head light
        if (goldGlint > 0.24) {
          ctx.strokeStyle = `rgba(255, 212, 108, ${Math.min(0.92, goldGlint * 0.85) * alpha})`
          ctx.lineWidth = 1.55 * taper + 0.28
          ctx.beginPath()
          ctx.moveTo(seg.x0 + seg.nx * 0.35, seg.y0 + seg.ny * 0.35)
          ctx.lineTo(ex + seg.nx * 0.35, ey + seg.ny * 0.35)
          ctx.stroke()
        }

        // Pass D: Crisp crystalline white/silver fracture line (primary bold -> secondary/tertiary razor hairline)
        const whiteAlpha = Math.min(1, (0.55 + 0.45 * seg.spec) * (seg.depth === 0 ? taper + 0.22 : taper + 0.38)) * alpha
        ctx.strokeStyle = `rgba(248, 252, 255, ${whiteAlpha})`
        ctx.lineWidth = seg.depth === 0 ? 1.18 * taper + 0.26 : seg.depth === 1 ? 0.58 * taper + 0.2 : 0.36
        ctx.beginPath()
        ctx.moveTo(seg.x0, seg.y0)
        ctx.lineTo(ex, ey)
        ctx.stroke()
      }

      // 5. Razor-sharp anisotropic stress-intensity needle glints racing along the growing crack fronts!
      for (const tip of activeTips) {
        const len = tip.depth === 0 ? 7.5 : 4.6
        const wid = tip.depth === 0 ? 1.35 : 0.95
        ctx.save()
        ctx.translate(tip.x, tip.y)
        ctx.rotate(tip.ang)
        ctx.fillStyle = `rgba(255, 255, 255, ${0.98 * alpha})`
        ctx.beginPath()
        ctx.ellipse(-len * 0.25, 0, len, wid, 0, 0, Math.PI * 2)
        ctx.fill()
        if (tip.depth === 0) {
          ctx.strokeStyle = `rgba(210, 245, 255, ${0.85 * alpha})`
          ctx.lineWidth = 0.75
          ctx.beginPath()
          ctx.moveTo(0, -3.6)
          ctx.lineTo(0, 3.6)
          ctx.stroke()
        }
        ctx.restore()
      }
      ctx.restore()

      // 6. Bright powdered-white crushed silica impact crater (blooms in first 28 ms, matching target-cracks.png!)
      const craterBloom = smooth(0, 0.026, age)
      if (craterBloom > 0.01) {
        ctx.save()
        const rCore = 32 * c.power * (0.45 + 0.55 * craterBloom)
        const halo = ctx.createRadialGradient(c.x, c.y, 1.5, c.x, c.y, rCore * 1.4)
        halo.addColorStop(0, `rgba(255, 255, 255, ${0.96 * craterBloom * alpha})`)
        halo.addColorStop(0.24, `rgba(246, 251, 255, ${0.78 * craterBloom * alpha})`)
        halo.addColorStop(0.52, `rgba(215, 236, 248, ${0.26 * craterBloom * alpha})`)
        halo.addColorStop(1, "rgba(160, 215, 235, 0)")
        ctx.fillStyle = halo
        ctx.beginPath()
        ctx.arc(c.x, c.y, rCore * 1.4, 0, Math.PI * 2)
        ctx.fill()

        for (const f of c.craterFacets) {
          if (age < (f.revealAt || 0)) continue
          ctx.beginPath()
          ctx.moveTo(f.pts[0][0], f.pts[0][1])
          ctx.lineTo(f.pts[1][0], f.pts[1][1])
          ctx.lineTo(f.pts[2][0], f.pts[2][1])
          ctx.closePath()
          ctx.fillStyle = f.dark
            ? `rgba(5, 9, 16, ${f.alpha * 0.92 * alpha})`
            : `rgba(252, 254, 255, ${f.alpha * 0.98 * alpha})`
          ctx.fill()
        }

        // 7. Embedded crystalline micro-specks & star glints revealed as the fracture front passes
        for (const sp of c.specks) {
          if (age < (sp.revealAt || 0)) continue
          ctx.save()
          ctx.translate(sp.x, sp.y)
          ctx.rotate(sp.ang)
          ctx.fillStyle = sp.gold
            ? `rgba(255, 226, 142, ${sp.alpha * alpha})`
            : `rgba(248, 253, 255, ${sp.alpha * alpha})`
          ctx.beginPath()
          ctx.ellipse(0, 0, sp.r * sp.stretch, sp.r * 0.68, 0, 0, Math.PI * 2)
          ctx.fill()
          if (sp.star) {
            ctx.strokeStyle = `rgba(255, 252, 240, ${sp.alpha * 0.88 * alpha})`
            ctx.lineWidth = 0.6
            const sl = sp.r * 2.6
            ctx.beginPath()
            ctx.moveTo(-sl, 0)
            ctx.lineTo(sl, 0)
            ctx.moveTo(0, -sl * 0.7)
            ctx.lineTo(0, sl * 0.7)
            ctx.stroke()
          }
          ctx.restore()
        }
        ctx.restore()
      }
    }
  }

  // Phones: a crack stops changing once its fronts finish growing, so bake it into one layer and
  // redraw only the cracks still propagating.
  drawCracksLite(ctx) {
    const { W, H, dpr } = this
    const growing = []
    for (const c of this.cracks) {
      if (!c.baked && c.age >= crackSettle(c)) {
        if (!this.crackLayer) {
          this.crackLayer = document.createElement("canvas")
          this.crackLayer.width = this.canvas.width
          this.crackLayer.height = this.canvas.height
        }
        const g = this.crackLayer.getContext("2d")
        g.setTransform(dpr, 0, 0, dpr, 0, 0)
        this.drawCracksOnto(g, 1, [c])
        c.baked = true
      }
      if (!c.baked) growing.push(c)
    }
    if (this.crackLayer) ctx.drawImage(this.crackLayer, 0, 0, W, H)
    if (growing.length) this.drawCracksOnto(ctx, 1, growing)
  }

  // Draws one 3D extruded glass slab matching target-shatter.png:
  // - Dark crystal slab thickness with green borosilicate inner rim + sharp white/gold specular streaks
  // - Strong per-shard optical refraction + RGB chromatic dispersion of the page behind it
  // - Sharp anisotropic surface reflection streaks + internal caustic star-flare
  draw3DGlassShard(ctx, s, healMix, healFade, seamGlint, isGhost = false) {
    if (s.isSplinter && this.state === "rebuilding" && healMix > 0.35) return
    const { W, H } = this
    const hx = this.headX || W * 0.5
    const hy = this.headY || H * 0.38
    const lightRange = Math.min(W, H) * 0.88

    // Clamp 3D foreshortening so slabs stay chunky 3D crystal panes and never collapse into 2D needles
    const tiltX = this.state === "rebuilding" ? lerp(0.58 + 0.42 * Math.cos(s.yaw), 1, healMix) : 0.56 + 0.44 * Math.cos(s.yaw)
    const tiltY = this.state === "rebuilding" ? lerp(0.64 + 0.36 * Math.cos(s.pitch), 1, healMix) : 0.62 + 0.38 * Math.cos(s.pitch)
    const zScale = this.state === "rebuilding" ? lerp(s.z ?? 1, 1, healMix) : clamp(s.z ?? 1, 0.82, 1.46)

    const thickFactor = (1 - healMix) * (isGhost ? 0.6 : 1)
    const rawEx = Math.sin(s.yaw) * s.thick
    const rawEy = Math.sin(s.pitch) * s.thick
    const minBevel = s.thick * 0.42
    const ex = (Math.abs(rawEx) < minBevel ? (rawEx >= 0 ? minBevel : -minBevel) : rawEx) * thickFactor
    const ey = (Math.abs(rawEy) < minBevel ? (rawEy >= 0 ? minBevel : -minBevel) : rawEy) * thickFactor

    const lx = hx - s.x
    const ly = hy - s.y
    const ld = Math.hypot(lx, ly) || 1
    const lDirX = lx / ld
    const lDirY = ly / ld
    const headProx = clamp(1.38 - ld / lightRange, 0.28, 1.38)
    const faceSpec = (0.24 + 0.76 * Math.pow(0.5 + 0.5 * Math.sin(s.yaw * 1.7 + s.pitch * 1.4 + (s.specSeed ?? 0)), 3)) * (1 - healMix)

    const splinterFade = s.isSplinter && this.state === "rebuilding" ? 1 - healMix / 0.35 : 1

    ctx.save()
    ctx.globalAlpha = healFade * splinterFade * (isGhost ? 0.24 : 1)
    ctx.translate(s.x, s.y)
    ctx.scale(zScale, zScale)
    ctx.rotate(s.ang)
    ctx.scale(tiltX, tiltY)

    if (s.isDownload) {
      if (thickFactor > 0.05) {
        ctx.fillStyle = "rgba(175, 218, 212, 0.88)"
        ctx.beginPath()
        ctx.roundRect(-s.w / 2 + ex * 0.55, -s.h / 2 + ey * 0.55, s.w, s.h, s.h / 2)
        ctx.fill()
      }
      this.drawDownloadPill(ctx, -s.w / 2, -s.h / 2, s.w, s.h, s.label)
      ctx.strokeStyle = `rgba(255, 236, 172, ${Math.min(1, 0.55 + faceSpec * 0.45 + seamGlint)})`
      ctx.lineWidth = 2.0
      ctx.beginPath()
      ctx.roundRect(-s.w / 2, -s.h / 2, s.w, s.h, s.h / 2)
      ctx.stroke()
      ctx.restore()
      return
    }

    const poly = s.poly
    const n = poly.length

    // 1. 3D extruded glass side bevels: dark refractive borosilicate core + green inner rim +
    //    sharp gradient specular streak (white core -> golden flare -> dark glass, never a flat yellow block!)
    if (thickFactor > 0.04) {
      const cosA = Math.cos(s.ang)
      const sinA = Math.sin(s.ang)
      for (let k = 0; k < n; k++) {
        const p0 = poly[k]
        const p1 = poly[(k + 1) % n]
        const edx = p1[0] - p0[0]
        const edy = p1[1] - p0[1]
        const el = Math.hypot(edx, edy) || 1
        const lnx = edy / el
        const lny = -edx / el
        const wnx = lnx * cosA - lny * sinA
        const wny = lnx * sinA + lny * cosA
        const rimDot = Math.max(0, wnx * lDirX + wny * lDirY) * headProx

        ctx.beginPath()
        ctx.moveTo(p0[0], p0[1])
        ctx.lineTo(p1[0], p1[1])
        ctx.lineTo(p1[0] + ex, p1[1] + ey)
        ctx.lineTo(p0[0] + ex, p0[1] + ey)
        ctx.closePath()

        if (rimDot > 0.25) {
          // Sharp linear specular gradient along the bevel edge so it reads as polished crystal, not flat yellow paint!
          const gIntensity = clamp(rimDot * 1.15, 0, 1)
          const bevGrad = ctx.createLinearGradient(p0[0], p0[1], p1[0] + ex, p1[1] + ey)
          bevGrad.addColorStop(0, `rgba(18, 42, 40, ${0.85 * (1 - gIntensity * 0.3)})`)
          bevGrad.addColorStop(0.32, `rgba(255, 202, 88, ${0.78 * gIntensity})`)
          bevGrad.addColorStop(0.52, `rgba(255, 253, 245, ${0.98 * gIntensity})`)
          bevGrad.addColorStop(0.74, `rgba(135, 245, 215, ${0.65 * gIntensity})`)
          bevGrad.addColorStop(1, "rgba(12, 28, 30, 0.88)")
          ctx.fillStyle = bevGrad
        } else {
          ctx.fillStyle = "rgba(12, 34, 32, 0.86)"
        }
        ctx.fill()

        // Razor-thin outer bevel ridge
        ctx.strokeStyle =
          rimDot > 0.35
            ? `rgba(255, 252, 238, ${Math.min(1, rimDot * 0.92)})`
            : "rgba(115, 242, 206, 0.52)"
        ctx.lineWidth = 0.75
        ctx.stroke()
      }
    }

    // 2. Front glass face: strong page refraction + RGB chromatic dispersion + sharp specular streaks + caustic star
    ctx.beginPath()
    ctx.moveTo(poly[0][0], poly[0][1])
    for (let k = 1; k < n; k++) ctx.lineTo(poly[k][0], poly[k][1])
    ctx.closePath()

    ctx.save()
    ctx.clip()

    if (!s.isSplinter && this.lite) {
      // One cached sprite per pane: the refracted page with its fracture lines baked in, then the
      // clean page faded over it while the glass heals.
      if (healMix < 0.99) this.drawPaneSprite(ctx, s, false)
      if (healMix > 0.01) {
        ctx.globalAlpha = healMix * healFade
        this.drawPaneSprite(ctx, s, true)
      }
    } else if (!s.isSplinter) {
      const refX = Math.sin(s.yaw) * (s.refractShift ?? 13) * (1 - healMix)
      const refY = Math.sin(s.pitch) * (s.refractShift ?? 13) * (1 - healMix)

      if (healMix < 0.99) {
        ctx.drawImage(this.pageCanvas, 0, 0, this.pageCanvas.width, this.pageCanvas.height, -s.homeX + refX, -s.homeY + refY, W, H)
        // RGB chromatic dispersion pass inside the shard face so cards & text visibly split into prism edges!
        if (!isGhost && healMix < 0.82) {
          const dispAmt = 3.2 * (1 - healMix)
          ctx.globalCompositeOperation = "lighter"
          ctx.globalAlpha = 0.24 * (1 - healMix)
          ctx.drawImage(
            this.pageCanvas,
            0,
            0,
            this.pageCanvas.width,
            this.pageCanvas.height,
            -s.homeX + refX + dispAmt,
            -s.homeY + refY - dispAmt * 0.6,
            W,
            H,
          )
          ctx.globalCompositeOperation = "source-over"
          ctx.globalAlpha = healFade
        }
      }
      if (healMix > 0.01) {
        ctx.globalAlpha = healMix * healFade
        ctx.drawImage(this.cleanCanvas, 0, 0, this.cleanCanvas.width, this.cleanCanvas.height, -s.homeX, -s.homeY, W, H)
      }

      // Sharp anisotropic Fresnel specular streak across the crystal face (matching target-shatter.png!)
      if (!isGhost && healMix < 0.9) {
        const cosA = Math.cos(-s.ang)
        const sinA = Math.sin(-s.ang)
        const locLx = (lDirX * cosA - lDirY * sinA) * 68
        const locLy = (lDirX * sinA + lDirY * cosA) * 68
        const gloss = ctx.createLinearGradient(locLx, locLy, -locLx, -locLy)
        const gA = (0.14 + faceSpec * 0.44) * headProx * (1 - healMix)
        gloss.addColorStop(0, "rgba(10, 22, 24, 0.15)")
        gloss.addColorStop(0.34, `rgba(120, 242, 210, ${Math.min(0.28, gA * 0.42)})`)
        gloss.addColorStop(0.47, `rgba(255, 254, 248, ${Math.min(0.68, gA * 1.25)})`)
        gloss.addColorStop(0.56, `rgba(255, 208, 102, ${Math.min(0.48, gA * 0.85)})`)
        gloss.addColorStop(1, "rgba(6, 12, 16, 0.22)")
        ctx.fillStyle = gloss
        ctx.fill()
      }

      // Crystalline internal fracture hairlines + bright star-flare caustic (like bottom-left pane in target-shatter.png!)
      if (!isGhost && s.innerCracks && healMix < 0.88) {
        const icAlpha = (0.42 + faceSpec * 0.56) * (1 - healMix)
        for (let idx = 0; idx < s.innerCracks.length; idx++) {
          const ic = s.innerCracks[idx]
          const isPrimary = idx % 2 === 0
          ctx.strokeStyle = `rgba(0, 5, 12, ${icAlpha * 0.75})`
          ctx.lineWidth = isPrimary ? 1.2 : 0.65
          ctx.beginPath()
          ctx.moveTo(ic[0] + 0.8, ic[1] + 0.8)
          ctx.lineTo(ic[2] + 0.8, ic[3] + 0.8)
          ctx.lineTo(ic[4] + 0.8, ic[5] + 0.8)
          ctx.stroke()

          ctx.strokeStyle = isPrimary
            ? `rgba(248, 252, 255, ${Math.min(0.96, icAlpha)})`
            : `rgba(205, 238, 248, ${Math.min(0.75, icAlpha * 0.8)})`
          ctx.lineWidth = isPrimary ? 0.85 : 0.45
          ctx.beginPath()
          ctx.moveTo(ic[0], ic[1])
          ctx.lineTo(ic[2], ic[3])
          ctx.lineTo(ic[4], ic[5])
          ctx.stroke()
        }

        const gv = s.innerCracks[0]
        if (gv && (s.hasCausticStar || faceSpec > 0.42)) {
          const sg = ctx.createRadialGradient(gv[0], gv[1], 0.5, gv[0], gv[1], 22)
          sg.addColorStop(0, `rgba(255, 255, 252, ${Math.min(0.98, (0.45 + faceSpec * 0.6) * (1 - healMix))})`)
          sg.addColorStop(0.32, `rgba(255, 218, 118, ${Math.min(0.68, faceSpec * 0.72 * (1 - healMix))})`)
          sg.addColorStop(1, "rgba(255, 190, 60, 0)")
          ctx.fillStyle = sg
          ctx.beginPath()
          ctx.arc(gv[0], gv[1], 22, 0, Math.PI * 2)
          ctx.fill()

          // Crisp 4-ray star flare spikes at the caustic vertex
          ctx.strokeStyle = `rgba(255, 252, 238, ${Math.min(0.9, (0.35 + faceSpec * 0.55) * (1 - healMix))})`
          ctx.lineWidth = 0.85
          ctx.beginPath()
          ctx.moveTo(gv[0] - 16, gv[1] - 4)
          ctx.lineTo(gv[0] + 16, gv[1] + 4)
          ctx.moveTo(gv[0] - 4, gv[1] + 14)
          ctx.lineTo(gv[0] + 4, gv[1] - 14)
          ctx.stroke()
        }
      }
    } else {
      // Secondary 3D crystal chunk: dark glass core with bright white/gold/emerald specular facet
      const spGrad = ctx.createLinearGradient(-10, -8, 10, 8)
      spGrad.addColorStop(0, "rgba(14, 34, 34, 0.78)")
      spGrad.addColorStop(0.45, `rgba(255, 252, 242, ${0.72 + faceSpec * 0.28})`)
      spGrad.addColorStop(0.75, `rgba(255, 208, 102, ${0.52 + faceSpec * 0.35})`)
      spGrad.addColorStop(1, "rgba(115, 242, 208, 0.42)")
      ctx.fillStyle = spGrad
      ctx.fill()
    }
    ctx.restore()

    // 3. Razor-sharp directional specular streaks along the front polygon edges
    const disp = clamp(Math.hypot(s.x - s.homeX, s.y - s.homeY) / 36 + Math.abs(s.ang) * 1.4, 0, 1)
    const edgeVis = (this.state === "shattered" ? 1 : disp * (1 - healMix * 0.9)) + seamGlint
    if (edgeVis > 0.03) {
      const cosA = Math.cos(s.ang)
      const sinA = Math.sin(s.ang)
      for (let k = 0; k < n; k++) {
        const p0 = poly[k]
        const p1 = poly[(k + 1) % n]
        const edx = p1[0] - p0[0]
        const edy = p1[1] - p0[1]
        const el = Math.hypot(edx, edy) || 1
        const wnx = (edy / el) * cosA - (-edx / el) * sinA
        const wny = (edy / el) * sinA + (-edx / el) * cosA
        const rim = Math.max(0, wnx * lDirX + wny * lDirY) * headProx

        ctx.beginPath()
        ctx.moveTo(p0[0], p0[1])
        ctx.lineTo(p1[0], p1[1])
        if (seamGlint > 0.15) {
          ctx.strokeStyle = `rgba(255, 250, 225, ${Math.min(1, edgeVis)})`
          ctx.lineWidth = 1.8
        } else if (rim > 0.32) {
          ctx.strokeStyle = `rgba(255, 252, 240, ${Math.min(1, (0.55 + rim * 0.55) * edgeVis)})`
          ctx.lineWidth = 1.35 * clamp(edgeVis, 0.4, 1) + 0.3
        } else {
          ctx.strokeStyle = `rgba(165, 244, 224, ${Math.min(0.85, 0.48 * edgeVis)})`
          ctx.lineWidth = 0.85 * clamp(edgeVis, 0.4, 1) + 0.2
        }
        ctx.stroke()
      }
    }

    ctx.restore()
  }

  // Bakes a pane's face (page under it, fracture hairlines and caustic) into its own small canvas
  // the first time it is drawn, so later frames cost one drawImage instead of a clipped full-page copy.
  drawPaneSprite(ctx, s, clean) {
    const key = clean ? "cleanSprite" : "sprite"
    let sp = s[key]
    if (!sp) {
      const { W, H, dpr } = this
      let x0 = Infinity
      let y0 = Infinity
      let x1 = -Infinity
      let y1 = -Infinity
      for (const [x, y] of s.poly) {
        x0 = Math.min(x0, x)
        y0 = Math.min(y0, y)
        x1 = Math.max(x1, x)
        y1 = Math.max(y1, y)
      }
      x0 = Math.floor(x0 - 1)
      y0 = Math.floor(y0 - 1)
      const w = Math.ceil(x1 + 1) - x0
      const h = Math.ceil(y1 + 1) - y0
      const canvas = document.createElement("canvas")
      canvas.width = Math.max(1, Math.round(w * dpr))
      canvas.height = Math.max(1, Math.round(h * dpr))
      const g = canvas.getContext("2d")
      g.setTransform(dpr, 0, 0, dpr, -x0 * dpr, -y0 * dpr)
      g.beginPath()
      g.moveTo(s.poly[0][0], s.poly[0][1])
      for (let k = 1; k < s.poly.length; k++) g.lineTo(s.poly[k][0], s.poly[k][1])
      g.closePath()
      g.clip()
      const src = clean ? this.cleanCanvas : this.pageCanvas
      g.drawImage(src, 0, 0, src.width, src.height, -s.homeX, -s.homeY, W, H)
      if (!clean && s.innerCracks) {
        g.lineCap = "round"
        for (let idx = 0; idx < s.innerCracks.length; idx++) {
          const ic = s.innerCracks[idx]
          const primary = idx % 2 === 0
          g.strokeStyle = "rgba(0, 5, 12, 0.5)"
          g.lineWidth = primary ? 1.2 : 0.65
          g.beginPath()
          g.moveTo(ic[0] + 0.8, ic[1] + 0.8)
          g.lineTo(ic[2] + 0.8, ic[3] + 0.8)
          g.lineTo(ic[4] + 0.8, ic[5] + 0.8)
          g.stroke()
          g.strokeStyle = primary ? "rgba(248, 252, 255, 0.8)" : "rgba(205, 238, 248, 0.6)"
          g.lineWidth = primary ? 0.85 : 0.45
          g.beginPath()
          g.moveTo(ic[0], ic[1])
          g.lineTo(ic[2], ic[3])
          g.lineTo(ic[4], ic[5])
          g.stroke()
        }
        const gv = s.innerCracks[0]
        if (gv && s.hasCausticStar) {
          const sg = g.createRadialGradient(gv[0], gv[1], 0.5, gv[0], gv[1], 22)
          sg.addColorStop(0, "rgba(255, 255, 252, 0.85)")
          sg.addColorStop(0.32, "rgba(255, 218, 118, 0.45)")
          sg.addColorStop(1, "rgba(255, 190, 60, 0)")
          g.fillStyle = sg
          g.beginPath()
          g.arc(gv[0], gv[1], 22, 0, Math.PI * 2)
          g.fill()
        }
      }
      sp = s[key] = { canvas, x: x0, y: y0, w, h }
    }
    ctx.drawImage(sp.canvas, sp.x, sp.y, sp.w, sp.h)
  }

  // Draws bold comic/cinema word-synced captions with smooth continuous word swell (zero 1-frame pops at 0.25x).
  drawCaption(ctx, rtime) {
    const cap = this.caption
    if (!cap || rtime < cap.startRtime - 0.08 || rtime > cap.endRtime + 0.35) return
    const { W, H } = this
    const elapsed = rtime - cap.startRtime
    const fadeIn = smooth(-0.08, 0.08, elapsed)
    const fadeOut = 1 - smooth(cap.endRtime - cap.startRtime, cap.endRtime - cap.startRtime + 0.32, elapsed)
    const alpha = fadeIn * fadeOut
    if (alpha <= 0.01) return

    const fontSize = clamp(Math.round(W * 0.019), 17, 26)
    ctx.save()
    ctx.globalAlpha = alpha
    ctx.font = `760 ${fontSize}px ${SANS}`
    ctx.textBaseline = "middle"

    const spaceW = Math.max(10, ctx.measureText(" ").width * 1.65)
    const wordMetrics = cap.words.map((w, idx) => {
      const nextT = cap.words[idx + 1]?.t ?? cap.endRtime - cap.startRtime + 0.12
      const rise = smooth(w.t - 0.045, w.t + 0.025, elapsed)
      const fall = 1 - smooth(nextT - 0.02, nextT + 0.075, elapsed)
      const activeAmt = rise * fall
      const spokenAmt = rise
      const width = ctx.measureText(w.text).width
      return { ...w, width, activeAmt, spokenAmt }
    })
    const totalW = wordMetrics.reduce((acc, m) => acc + m.width, 0) + Math.max(0, wordMetrics.length - 1) * spaceW
    const padX = 26
    const padY = 13
    const boxW = Math.min(W - 32, totalW + padX * 2)
    const boxH = fontSize + padY * 2
    const boxX = (W - boxW) * 0.5
    const cy = H * 0.905
    const boxY = cy - boxH * 0.5

    ctx.save()
    ctx.shadowColor = "rgba(0, 0, 0, 0.82)"
    ctx.shadowBlur = 24
    ctx.shadowOffsetY = 6
    ctx.fillStyle = "rgba(8, 10, 14, 0.9)"
    ctx.strokeStyle = "rgba(255, 196, 72, 0.62)"
    ctx.lineWidth = 1.6
    ctx.beginPath()
    ctx.roundRect(boxX, boxY, boxW, boxH, 12)
    ctx.fill()
    ctx.stroke()
    ctx.restore()

    let curX = (W - totalW) * 0.5
    ctx.textAlign = "left"
    for (const m of wordMetrics) {
      ctx.save()
      const wcx = curX + m.width * 0.5
      ctx.translate(wcx, cy)
      const sc = 1 + 0.075 * m.activeAmt
      ctx.scale(sc, sc)
      if (m.activeAmt > 0.05) {
        ctx.shadowColor = `rgba(255, 188, 46, ${(0.8 * m.activeAmt).toFixed(3)})`
        ctx.shadowBlur = 12 * m.activeAmt
      }
      const rC = Math.round(lerp(lerp(205, 249, m.spokenAmt), 255, m.activeAmt))
      const gC = Math.round(lerp(lerp(210, 250, m.spokenAmt), 203, m.activeAmt))
      const bC = Math.round(lerp(lerp(220, 251, m.spokenAmt), 71, m.activeAmt))
      const aC = lerp(0.52, 1.0, Math.max(m.spokenAmt, m.activeAmt))
      ctx.fillStyle = `rgba(${rC}, ${gC}, ${bC}, ${aC.toFixed(3)})`
      ctx.fillText(m.text, -m.width * 0.5, 0)
      ctx.restore()
      curX += m.width + spaceW
    }

    ctx.restore()
  }

  stepAndDraw(dt, rtime = 0) {
    if (this.state === "idle" && this.zWords.length === 0 && !this.caption) return
    const { ctx, W, H, dpr } = this
    if (!W || !H) return
    if (this.needsCleanCapture) {
      this.needsCleanCapture = false
      this.captureViewport(this.cleanCtx, { clean: true, skipDownload: true })
    }

    ctx.save()
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, W, H)

    this.flash = Math.max(0, this.flash - dt * 14)
    for (const c of this.cracks) c.age += dt

    // 1. If in "cracking" state, draw the outward-propagating ballistic fractures & page refraction
    if (this.state === "cracking" && this.cracks.length > 0) {
      if (this.lite) this.drawCracksLite(ctx)
      else this.drawCracksOnto(ctx, 1)
    }

    // 2. If in "shattered" or "rebuilding" state, simulate & draw the 3D tempered glass slabs + Download button
    if (this.state === "shattered" || this.state === "rebuilding") {
      let allDone = false
      let healFade = 1
      if (this.state === "rebuilding") {
        this.rebuildAge += dt
        const overallT = clamp(this.rebuildAge / this.rebuildDur, 0, 1)
        healFade = 1 - smooth(0.88, 1.0, overallT)
        if (overallT >= 1) allDone = true
      }

      for (const s of this.shards) {
        let healMix = 0
        let seamGlint = 0
        if (this.state === "shattered") {
          s.age += dt
          if (s.age > s.delay) {
            const effDt = dt
            const drag = s.age < 0.28 ? 2.4 : 4.2
            s.vx = lerp(s.vx, s.driftX ?? 0, 1 - Math.exp(-drag * effDt))
            s.vy = lerp(s.vy, s.driftY ?? 22, 1 - Math.exp(-drag * effDt))
            s.vy += (s.isDownload ? 38 : 30) * effDt
            s.x += s.vx * effDt
            s.y += s.vy * effDt
            s.z = clamp((s.z ?? 1) + (s.vz ?? 0) * effDt, 0.82, 1.46)
            s.ang += s.spin * effDt
            s.pitch += s.vPitch * effDt
            s.yaw += s.vYaw * effDt
          }
        } else {
          const startT = (1 - s.normD) * 0.24
          const span = 0.62
          const loc = clamp((this.rebuildAge / this.rebuildDur - startT) / span, 0, 1)
          const ease = loc < 0.5 ? 4 * loc * loc * loc : 1 - Math.pow(-2 * loc + 2, 3) / 2
          const arcLift = Math.sin(loc * Math.PI) * -42 * (0.4 + s.normD)
          s.x = lerp(s.fromX, s.homeX, ease)
          s.y = lerp(s.fromY, s.homeY, ease) + arcLift
          s.z = lerp(s.fromZ ?? 1, 1, ease)
          s.ang = lerp(s.fromAng, 0, ease)
          s.pitch = lerp(s.fromPitch, 0, ease)
          s.yaw = lerp(s.fromYaw, 0, ease)
          healMix = smooth(0.48, 0.92, loc)
          seamGlint = Math.exp(-Math.pow((loc - 0.92) / 0.055, 2))
        }

        if (s.y - 220 > H && this.state === "shattered") continue

        // Foreground depth-of-field / motion-blur ghost pass on outer panes
        if (this.state === "shattered" && !this.lite && !s.isDownload && !s.isSplinter && (s.normD > 0.56 || s.z > 1.12)) {
          const mbX = clamp(s.vx * 0.025 + (s.x - W * 0.5) * 0.012, -10, 10)
          const mbY = clamp(s.vy * 0.025 + (s.y - H * 0.5) * 0.012, -10, 10)
          ctx.save()
          ctx.translate(mbX, mbY)
          this.draw3DGlassShard(ctx, s, healMix, healFade, seamGlint, true)
          ctx.restore()
        }

        this.draw3DGlassShard(ctx, s, healMix, healFade, seamGlint, false)
      }

      if (allDone) {
        this.reset()
        ctx.restore()
        return
      }
    }

    // 3. Simulate & draw the 3-tier cloud of sparkling glass dust, faceted micro-shards & warm bokeh motes
    for (let i = this.dust.length - 1; i >= 0; i--) {
      const d = this.dust[i]
      d.age += dt
      if (d.age >= d.life) {
        this.dust.splice(i, 1)
        continue
      }
      if (this.state === "rebuilding") {
        const cx = this.headX || W * 0.5
        const cy = this.headY || H * 0.45
        d.vx = lerp(d.vx, (cx - d.x) * 2.2, dt * 5)
        d.vy = lerp(d.vy, (cy - d.y) * 2.2 - 120, dt * 5)
      } else if (d.BulletTime) {
        d.vx *= Math.exp(-2.8 * dt)
        d.vy = lerp(d.vy, 16, dt * 3.2)
      } else {
        d.vx *= Math.exp(-1.1 * dt)
        d.vy += 310 * dt
      }
      d.x += d.vx * dt
      d.y += d.vy * dt
      d.rot += d.vrot * dt

      const lifeAlpha = (1 - d.age / d.life) * (this.state === "rebuilding" ? 1 - this.rebuildAge / this.rebuildDur : 1)
      const twinkle = 0.42 + 0.58 * Math.pow(0.5 + 0.5 * Math.sin(d.age * 22 + d.phase), 3)
      const a = clamp(lifeAlpha * twinkle, 0, 1)
      if (a <= 0.02) continue

      ctx.save()
      ctx.translate(d.x, d.y)
      if (d.kind === "bokeh") {
        // Out-of-focus warm golden bokeh dust mote (like target-shatter.png!)
        const bg = ctx.createRadialGradient(0, 0, 0.2, 0, 0, d.r)
        bg.addColorStop(0, `rgba(255, 238, 175, ${a * 0.72})`)
        bg.addColorStop(0.45, `rgba(255, 192, 68, ${a * 0.36})`)
        bg.addColorStop(1, "rgba(255, 170, 40, 0)")
        ctx.fillStyle = bg
        ctx.beginPath()
        ctx.arc(0, 0, d.r, 0, Math.PI * 2)
        ctx.fill()
      } else {
        ctx.rotate(d.rot)
        ctx.fillStyle = d.gold
          ? `rgba(255, 226, 128, ${a})`
          : d.green
            ? `rgba(155, 248, 222, ${a})`
            : `rgba(255, 255, 255, ${a})`
        ctx.beginPath()
        const rx = d.r
        const ry = d.r * d.aspect
        ctx.moveTo(0, -ry)
        ctx.lineTo(rx, 0)
        ctx.lineTo(0, ry)
        ctx.lineTo(-rx, 0)
        ctx.closePath()
        ctx.fill()
        if (d.star && a > 0.55) {
          ctx.strokeStyle = d.gold ? `rgba(255, 244, 195, ${a * 0.85})` : `rgba(245, 252, 255, ${a * 0.85})`
          ctx.lineWidth = 0.65
          const sl = d.r * 2.4
          ctx.beginPath()
          ctx.moveTo(-sl, 0)
          ctx.lineTo(sl, 0)
          ctx.moveTo(0, -sl * 0.75)
          ctx.lineTo(0, sl * 0.75)
          ctx.stroke()
        }
      }
      ctx.restore()
    }

    // 4. Simulate & draw Z-axis words rocketing straight at the camera lens with 7-step smooth zoom motion blur
    for (let i = this.zWords.length - 1; i >= 0; i--) {
      const zw = this.zWords[i]
      zw.age += dt
      const p = clamp(zw.age / zw.dur, 0, 1)

      for (let b = 6; b >= 0; b--) {
        const bp = clamp(p - b * 0.024, 0, 1)
        const bz = Math.pow(bp, 2.35)
        const bx = lerp(zw.fromX, zw.targetX, bz)
        const by = lerp(zw.fromY, zw.targetY, bz)
        const bScale = lerp(0.36, zw.final ? 8.2 : 4.8, Math.pow(bp, 2.65))
        const bRot = zw.rot + zw.vrot * (zw.age - b * 0.004)
        const alpha = b === 0 ? 1 : (0.26 / b) * p

        ctx.save()
        ctx.translate(bx, by)
        ctx.rotate(bRot)
        ctx.scale(bScale, bScale)
        ctx.font = `900 28px ${SANS}`
        ctx.textAlign = "center"
        ctx.textBaseline = "middle"
        if (b === 0) {
          ctx.shadowColor = zw.final ? "rgba(255, 195, 55, 0.95)" : "rgba(255, 135, 35, 0.9)"
          ctx.shadowBlur = 18
        }
        ctx.fillStyle = b === 0 ? "#fffdf6" : `rgba(255, 202, 104, ${alpha.toFixed(3)})`
        ctx.strokeStyle = `rgba(20, 12, 6, ${(alpha * 0.65).toFixed(3)})`
        ctx.lineWidth = 2.2
        ctx.strokeText(zw.text, 0, 0)
        ctx.fillText(zw.text, 0, 0)
        ctx.restore()
      }

      if (p >= 1) {
        this.zWords.splice(i, 1)
        if (zw.final) {
          this.shatter(zw.targetX, zw.targetY)
        } else {
          this.addCrack(zw.targetX, zw.targetY, zw.hitsDownload ? 1.22 : 1.0, zw.hitsDownload)
        }
        zw.onImpact?.(zw)
      }
    }

    // 5. Tight localized impact flash
    if (this.flash > 0.01) {
      const fx = this.flashX ?? W * 0.5
      const fy = this.flashY ?? H * 0.5
      const fr = this.state === "shattered" ? Math.min(W, H) * 0.36 : 52
      const fg = ctx.createRadialGradient(fx, fy, 0, fx, fy, fr)
      fg.addColorStop(0, `rgba(255, 252, 245, ${Math.min(0.62, this.flash)})`)
      fg.addColorStop(0.35, `rgba(225, 244, 255, ${Math.min(0.24, this.flash * 0.42)})`)
      fg.addColorStop(1, "rgba(180, 225, 255, 0)")
      ctx.fillStyle = fg
      ctx.beginPath()
      ctx.arc(fx, fy, fr, 0, Math.PI * 2)
      ctx.fill()
    }

    // 6. Bold comic/cinema captions for the finale voice lines
    this.drawCaption(ctx, rtime)

    ctx.restore()
  }
}
