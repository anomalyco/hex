// Shout FX: 2.5D card physics, Voronoi fracture, mouth/eye rig, Super Saiyan and the Tommy-gun finale.

const clamp = (x, a, b) => Math.max(a, Math.min(b, x))
const lerp = (a, b, t) => a + (b - a) * t
const smooth = (a, b, x) => {
  const t = clamp((x - a) / (b - a), 0, 1)
  return t * t * (3 - 2 * t)
}

// Clip a convex polygon (array of [x, y]) against the half-plane dot(p, [nx, ny]) <= d
function clipHalfPlane(poly, nx, ny, d) {
  const n = poly.length
  if (n === 0) return poly
  const out = []
  let prev = poly[n - 1]
  let prevVal = prev[0] * nx + prev[1] * ny - d
  for (let i = 0; i < n; i++) {
    const cur = poly[i]
    const curVal = cur[0] * nx + cur[1] * ny - d
    if (curVal <= 0) {
      if (prevVal > 0) {
        const t = prevVal / (prevVal - curVal)
        out.push([prev[0] + (cur[0] - prev[0]) * t, prev[1] + (cur[1] - prev[1]) * t])
      }
      out.push(cur)
    } else if (prevVal <= 0) {
      const t = prevVal / (prevVal - curVal)
      out.push([prev[0] + (cur[0] - prev[0]) * t, prev[1] + (cur[1] - prev[1]) * t])
    }
    prev = cur
    prevVal = curVal
  }
  return out
}

// Classify a spoken word into vowel roundness/width + onset consonant closure
export function analyzeViseme(word = "") {
  const w = word.toUpperCase().replace(/[^A-Z]/g, "")
  if (!w) return { open: 0.5, wide: 0.95, round: 0.15, closure: 0.1 }
  const bilabial = /^[MBPFV]/.test(w)
  const sibilant = /^[SZTDNL]/.test(w)
  const isRoundWord = /^([WHO]*O+|YOU|TOO|WHO|NO|GO|SO|TO|DO|YO|ZIO|FLOOP|BLORP|SNORP|WUB|OO+)$/.test(w)
  const hasRoundVowel = /OO|OU|OW|WHO|NO|GO|SO|TO|DO|YO/.test(w)
  const hasWide = /AA|AH|EE|EA|AY|AI|EY|LEAVE|ALONE|READ|FIX|SHIP|NEXT|PIPE|MAP|WRAP|ALL|ISSUE|EVERY|SUBAGENT|OVER|THOUSAND/.test(w)
  const exclaim = word.includes("!") || w.length >= 4
  let wide = isRoundWord ? 0.64 : hasWide ? 1.18 : hasRoundVowel ? 0.84 : 1.0
  let open = hasWide ? 1.08 : isRoundWord ? 0.82 : 0.94
  if (exclaim && !isRoundWord) {
    open *= 1.12
    wide = Math.max(wide, 1.12)
  }
  const closure = bilabial ? 0.72 : sibilant ? 0.42 : 0.16
  const round = isRoundWord ? 0.95 : hasRoundVowel ? 0.55 : 0.12
  return { open, wide, round, closure }
}

export class ShoutFx {
  constructor() {
    this.seed = 0x9e3779b9
    this.saiyan = 0
    this.saiyanBurstAge = 99
    this.aberration = 0
    this.heatHaze = 0
    this.cards = new WeakMap()
    this.liveCards = []
    this.liveFx = null
    this.headState = null
    this.viewport = { w: 1440, h: 900 }
    this.intensity = 0


    // Continuous phonetic mouth & jaw state
    this.mouthOpen = 0
    this.mouthVel = 0
    this.mouthWide = 1
    this.mouthWideVel = 0
    this.mouthRound = 0
    this.jawSkew = 0
    this.tonguePhase = 0

    // Ballistic eye saccade state
    this.saccadeX = 0
    this.saccadeY = 0
    this.nextSaccade = 0.25
    this.microX = 0
    this.microY = 0
    this.pupilScale = 1

    // 2D physical googly eyes (dizzy state)
    this.prevHeadX = null
    this.prevHeadY = null
    this.prevHeadVx = 0
    this.prevHeadVy = 0
    this.googlyL = { x: 0, y: 0.12, vx: 0, vy: 0 }
    this.googlyR = { x: 0, y: 0.12, vx: 0, vy: 0 }
    this.hairBounce = { x: 0, y: 0, vx: 0, vy: 0 }

    // Tommy-gun spin finale & recoil state
    this.finale = null
    this.dizzy = 0
    this.recoilX = 0
    this.recoilY = 0
    this.recoilVx = 0
    this.recoilVy = 0

    // Super Saiyan ember & lightning accumulators
    this.emberAcc = 0
    this.arcAcc = 0

    // Renderer hooks (assigned by GpuRenderer / Canvas2DRenderer)
    this.onSpawnShards = null
    this.onSpawnSparks = null
  }

  rand() {
    this.seed = (this.seed + 0x6d2b79f5) >>> 0
    let t = this.seed
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
  rnd(a, b) {
    return a + this.rand() * (b - a)
  }


  setSaiyan(level) {
    const prev = this.saiyan
    this.saiyan = clamp(level ?? 0, 0, 1.25)
    if (prev < 0.85 && this.saiyan >= 0.95 && this.saiyanBurstAge > 2) {
      this.saiyanBurst()
    }
  }

  saiyanBurst() {
    this.saiyanBurstAge = 0
    this.aberration = Math.max(this.aberration, 0.85)
    this.heatHaze = Math.max(this.heatHaze, 0.9)
    const hp = this.headState ?? { x: this.viewport.w / 2, y: this.viewport.h * 0.42, R: 80 }
    if (this.liveFx) {
      const diag = Math.hypot(this.viewport.w, this.viewport.h)
      this.liveFx.push({ kind: 0, x: hp.x, y: hp.y, r: hp.R * 0.6, rMax: diag * 0.78, width: 5.5, t: 0, k: 0.95, age: 0, life: 0.58 })
      this.liveFx.push({ kind: 0, x: hp.x, y: hp.y, r: hp.R * 0.3, rMax: diag * 0.46, width: 3.0, t: 0, k: 0.75, age: -0.05, life: 0.50 })
      this.liveFx.push({ kind: 1, x: hp.x, y: hp.y, r: hp.R * 2.8, t: 0, k: 1.15, age: 0, life: 0.16 })
      for (let k = 0; k < 8; k++) {
        const a = (k / 8) * Math.PI * 2 + this.rnd(-0.2, 0.2)
        const len = this.rnd(hp.R * 1.3, hp.R * 3.2)
        this.liveFx.push({
          kind: 4,
          x: hp.x + Math.cos(a) * hp.R * 0.75,
          y: hp.y + Math.sin(a) * hp.R * 0.75,
          r: len,
          width: a,
          t: 0,
          k: 1.2,
          age: 0,
          life: this.rnd(0.18, 0.32),
        })
      }
    }
    if (this.onSpawnSparks) {
      const sparks = []
      for (let i = 0; i < 96; i++) {
        const a = this.rnd(0, Math.PI * 2)
        const sp = this.rnd(240, 1280)
        sparks.push({
          x: hp.x + Math.cos(a) * hp.R * this.rnd(0.3, 1.0),
          y: hp.y + Math.sin(a) * hp.R * this.rnd(0.3, 1.0),
          vx: Math.cos(a) * sp,
          vy: Math.sin(a) * sp - this.rnd(120, 420),
          life: this.rnd(0.32, 0.95),
          heat: -this.rnd(0.75, 1.0),
          size: this.rnd(1.5, 2.6),
        })
      }
      this.onSpawnSparks(sparks)
    }
    for (const c of this.liveCards) {
      if (c.state === "dead" || c.kind === "opencode") continue
      const rb = this.ensureCard(c)
      const dx = c.x - hp.x
      const dy = c.y - hp.y
      const d = Math.hypot(dx, dy) || 1
      const falloff = clamp(1 - d / 900, 0.15, 1)
      rb.vx += (dx / d) * 320 * falloff
      rb.vy += (dy / d) * 320 * falloff
      rb.vrot += this.rnd(-3.2, 3.2) * falloff
      rb.tiltX += (dx / d) * 0.32 * falloff
      rb.tiltY += (dy / d) * 0.32 * falloff
      rb.flash = Math.max(rb.flash, 0.45 * falloff)
    }
  }

  // ─── Note 3: Clean, satisfying OpenCode resolve (stroke-draw check, soft glow, lift, NO sparks!) ──
  ocIntro(card) {
    if (!card) return
    const rb = this.ensureCard(card)
    rb.damage = 0
    rb.flash = 0.24
  }

  ocSubmit(card) {
    if (!card) return
    const rb = this.ensureCard(card)
    rb.damage = 0
    rb.flash = 0.35
    rb.vy -= 22
  }

  resolveCard(card) {
    if (!card) return false
    const rb = this.ensureCard(card)
    rb.resolved = true
    rb.resolveAge = 0
    rb.damage = 0
    rb.tiltX = 0
    rb.tiltY = 0
    rb.vrot = 0
    rb.rot = 0
    rb.vy -= 95
    rb.flash = 0.55
    if (this.liveFx) {
      // A badge-sized check over the card; `width` pads the effect quad, so keep it 0.
      const r = Math.min(card.hw ?? 120, card.hh ?? 54) * 0.9
      this.liveFx.push({
        kind: 5,
        x: card.x,
        y: card.y,
        r,
        rMax: r * 1.15,
        width: 0,
        t: 0,
        k: 1.0,
        age: 0,
        life: 0.52,
      })
    }
    return true
  }

  // ─── Note 5: Tommy-Gun Spin Finale hooks ───────────────────────────────────────
  setFinale(state) {
    this.finale = state || null
  }
  setSpinFinale(state) {
    this.setFinale(state)
  }

  finaleShot(shot) {
    if (!shot) return
    const { x, y, angle = 0, word } = shot
    const ca = Math.cos(angle)
    const sa = Math.sin(angle)
    const isHeavy = !!word

    // 1. Snappy opposite recoil kick on the spinning head & googly eyes
    const kick = isHeavy ? 6.5 : 2.6
    this.recoilVx -= ca * kick * 35
    this.recoilVy -= sa * kick * 35
    this.googlyL.vx -= ca * kick * 2.8 + this.rnd(-4, 4)
    this.googlyL.vy -= sa * kick * 2.8 + this.rnd(-4, 4)
    this.googlyR.vx -= ca * kick * 2.8 + this.rnd(-4, 4)
    this.googlyR.vy -= sa * kick * 2.8 + this.rnd(-4, 4)

    // 2. Directional muzzle flash cone (kind 6) + ballistic laser tracer streak (kind 7)
    if (this.liveFx) {
      this.liveFx.push({
        kind: 6, // Directional Tommy-gun muzzle flash cone
        x,
        y,
        r: isHeavy ? 82 : 58,
        width: angle,
        t: 0,
        k: isHeavy ? 1.45 : 1.12,
        age: 0,
        life: isHeavy ? 0.16 : 0.12,
      })
      const tracerLen = isHeavy ? this.rnd(540, 860) : this.rnd(380, 680)
      this.liveFx.push({
        kind: 7, // High-velocity ballistic tracer streak
        x: x + ca * 14,
        y: y + sa * 14,
        r: tracerLen,
        width: angle,
        t: 0,
        k: isHeavy ? 1.35 : 1.05,
        age: 0,
        life: isHeavy ? 0.22 : 0.17,
      })
    }

    // 3. Eject spinning brass / letter shell casings tangentially from the breech
    if (this.onSpawnSparks) {
      const casings = []
      const ejectAng = angle - Math.PI * 0.55 + this.rnd(-0.32, 0.32)
      const count = isHeavy ? 2 : 1
      for (let i = 0; i < count; i++) {
        const sp = this.rnd(240, 480)
        casings.push({
          x: x - ca * 12 + this.rnd(-4, 4),
          y: y - sa * 12 + this.rnd(-4, 4),
          vx: Math.cos(ejectAng) * sp,
          vy: Math.sin(ejectAng) * sp - this.rnd(90, 210),
          life: this.rnd(0.45, 0.85),
          heat: -2.0, // <= -1.5 marks tumbling brass/letter shell casings!
          size: this.rnd(3.2, 4.4),
        })
      }
      // Plus 2-3 hot forward muzzle sparks
      for (let i = 0; i < (isHeavy ? 4 : 2); i++) {
        const a = angle + this.rnd(-0.22, 0.22)
        const sp = this.rnd(900, 1900)
        casings.push({
          x,
          y,
          vx: Math.cos(a) * sp,
          vy: Math.sin(a) * sp,
          life: this.rnd(0.08, 0.18),
          heat: -0.92,
          size: this.rnd(1.6, 2.4),
        })
      }
      this.onSpawnSparks(casings)
    }
  }
  fireTommyShot(shot) {
    this.finaleShot(shot)
  }

  ensureCard(c) {
    let rb = this.cards.get(c)
    if (!rb) {
      rb = {
        px: 0,
        py: 0,
        vx: 0,
        vy: 0,
        rot: 0,
        vrot: 0,
        tiltX: 0,
        tiltY: 0,
        vtiltX: 0,
        vtiltY: 0,
        damage: 0,
        impX: 0,
        impY: 0.2,
        ripple: 1,
        flash: 0,
        resolved: false,
        resolveAge: 0,
        lastLanded: c.landed ?? 0,
        lastWord: null,
        seed: ((c.seed ?? this.rnd(1, 9)) % 9) + 0.13,
      }
      this.cards.set(c, rb)
    }
    return rb
  }

  // ─── Note 2 & 4: Physical 2.5D card response gated by intensity (calm = gentle nudge, zero cracks) ──
  hitCard(card, wordOrImpulse = {}, point = null) {
    if (!card) return
    const rb = this.ensureCard(card)

    // OpenCode window never cracks or sprays debris when prompt words type into it
    if (card.kind === "opencode") {
      rb.vy += 18
      rb.flash = 0.08
      return
    }

    const pt = point ?? {
      x: wordOrImpulse.x ?? card.x + this.rnd(-card.hw * 0.4, card.hw * 0.4),
      y: wordOrImpulse.y ?? card.y + card.hh * 0.25,
    }
    const wvx = wordOrImpulse.vx ?? 0
    const wvy = wordOrImpulse.vy ?? -400
    const speed = Math.hypot(wvx, wvy) || 1
    const ux = wvx / speed
    const uy = wvy / speed
    const intensity = clamp(wordOrImpulse.i ?? this.intensity ?? 0.3, 0, 1)

    const rx = clamp(pt.x - card.x, -(card.hw || 120), card.hw || 120)
    const ry = clamp(pt.y - card.y, -(card.hh || 50), card.hh || 50)
    const normX = clamp(rx / Math.max(1, card.hw || 120), -0.92, 0.92)
    const normY = clamp(ry / Math.max(1, card.hh || 50), -0.92, 0.92)

    // Calm stage (intensity < 0.36): gentle tactile nudge, zero cracks, zero chips!
    const isCalm = intensity < 0.36 && this.saiyan < 0.1
    const push = isCalm ? lerp(22, 48, intensity / 0.36) : lerp(75, 220, intensity) * (1 + 0.22 * this.saiyan)
    rb.vx += ux * push
    rb.vy += uy * push

    const cross = (rx * uy - ry * ux) / Math.max(60, card.hw || 120)
    rb.vrot += cross * (isCalm ? 0.45 : lerp(1.6, 5.2, intensity))
    rb.vtiltX += normX * (isCalm ? 0.5 : lerp(1.8, 5.5, intensity)) + ux * (isCalm ? 0.3 : 1.6)
    rb.vtiltY += normY * (isCalm ? 0.5 : lerp(1.8, 5.5, intensity)) + uy * (isCalm ? 0.3 : 1.6)

    rb.impX = normX
    rb.impY = normY
    rb.ripple = 0
    rb.flash = isCalm ? 0.14 : Math.max(rb.flash, lerp(0.28, 0.72, intensity))

    // Gate cracks strictly by intensity: ZERO cracks below 0.38!
    if (intensity >= 0.38 || this.saiyan > 0.2) {
      const crackAdd = smooth(0.38, 0.95, Math.max(intensity, this.saiyan)) * 0.34
      rb.damage = clamp(rb.damage + crackAdd, 0, 1.15)
    }

    if (isCalm) return

    // Impact ring for medium/high intensity hits only
    if (this.liveFx && intensity > 0.42) {
      const rMax = lerp(24, 68, intensity)
      this.liveFx.push({
        kind: 0,
        x: pt.x,
        y: pt.y,
        r: 3,
        rMax,
        width: lerp(1.6, 2.8, intensity),
        t: 0,
        k: lerp(0.35, 0.75, intensity),
        age: 0,
        life: 0.22,
      })
    }

    // Sparks on medium/hard impacts
    if (this.onSpawnSparks && intensity > 0.40) {
      const sparks = []
      const refAng = Math.atan2(-uy, -ux)
      const count = Math.round(lerp(4, 16, intensity) * (1 + 0.35 * this.saiyan))
      for (let i = 0; i < count; i++) {
        const graze = i % 3 === 0 ? (i % 2 === 0 ? 1.25 : -1.25) : this.rnd(-0.75, 0.75)
        const a = refAng + graze
        const sp = this.rnd(180, 720) * lerp(0.55, 1.15, intensity)
        sparks.push({
          x: pt.x + this.rnd(-3, 3),
          y: pt.y + this.rnd(-3, 3),
          vx: Math.cos(a) * sp + ux * 60,
          vy: Math.sin(a) * sp - this.rnd(20, 100),
          life: this.rnd(0.14, 0.36),
          heat: this.saiyan > 0.3 ? -this.rnd(0.65, 1.0) : this.rnd(0.45, 0.95),
          size: this.rnd(1.3, 2.3),
        })
      }
      this.onSpawnSparks(sparks)
    }

    // Voronoi corner chips only at high intensity (>= 0.65)
    if (this.onSpawnShards && (intensity >= 0.65 || this.saiyan > 0.4) && card.uv && card.sw) {
      const chips = []
      const chipCount = intensity > 0.82 ? 2 : 1
      const half = [card.sw / 2, card.sh / 2]
      const bx = clamp(rx / (card.scale || 1), -half[0] * 0.8, half[0] * 0.8)
      const by = clamp(ry / (card.scale || 1), -half[1] * 0.8, half[1] * 0.8)
      for (let k = 0; k < chipCount; k++) {
        const a0 = this.rnd(0, Math.PI * 2)
        const r1 = this.rnd(5, 11)
        const r2 = this.rnd(5, 11)
        const refA = Math.atan2(-uy, -ux) + this.rnd(-0.9, 0.9)
        const sp = this.rnd(180, 520)
        const v2p = [Math.cos(a0 + 1.2) * r2, Math.sin(a0 + 1.2) * r2]
        chips.push({
          x: pt.x,
          y: pt.y,
          vx: Math.cos(refA) * sp,
          vy: Math.sin(refA) * sp - 90,
          ang: card.rot ?? 0,
          spin: this.rnd(-16, 16),
          life: this.rnd(0.38, 0.68),
          v0: [0, 0],
          v1: [Math.cos(a0) * r1, Math.sin(a0) * r1],
          v2: v2p,
          v3: v2p,
          heat: 0.9,
          tumble: this.rnd(0, 6.28),
          base: [bx + this.rnd(-6, 6), by + this.rnd(-5, 5)],
          half,
          uv: card.uv,
          entry: { px: card.px, py: card.py, w: card.sw, h: card.sh },
          _voronoi: true,
        })
      }
      this.onSpawnShards(chips)
    }

    // Subtle shockwave push to neighboring cards on heavy impacts
    if (intensity > 0.58) {
      this.pushNeighbors(card, pt.x, pt.y, 140 * intensity)
    }
  }

  pushNeighbors(sourceCard, x, y, strength = 150) {
    for (const c of this.liveCards) {
      if (c === sourceCard || c.state === "dead" || c.kind === "opencode") continue
      const dx = c.x - x
      const dy = c.y - y
      const d = Math.hypot(dx, dy)
      if (d > 380 || d < 1) continue
      const falloff = Math.pow(1 - d / 380, 2)
      const rb = this.ensureCard(c)
      rb.vx += (dx / d) * strength * falloff
      rb.vy += (dy / d) * strength * falloff
      rb.vrot += ((dx * (c.y - y) - dy * (c.x - x)) / (d * 180)) * 2.2 * falloff
      rb.vtiltX += (dx / d) * 1.4 * falloff
      rb.vtiltY += (dy / d) * 1.4 * falloff
    }
  }

  // Convert a grid shard list (from explode(c)) into 4-vertex convex Voronoi polygons
  fractureVoronoi(rawList) {
    if (!rawList?.length || rawList[0]._voronoi) return rawList
    const first = rawList[0]
    const half = first.half
    const uv = first.uv
    const entry = first.entry
    if (!half || !uv || !entry) return rawList

    let cx0 = 0
    let cy0 = 0
    let vxAvg = 0
    let vyAvg = 0
    for (const s of rawList) {
      cx0 += s.x
      cy0 += s.y
      vxAvg += s.vx
      vyAvg += s.vy
    }
    const invN = 1 / rawList.length
    cx0 *= invN
    cy0 *= invN
    vxAvg *= invN
    vyAvg *= invN
    const ang = first.ang ?? 0
    const ca = Math.cos(ang)
    const sa = Math.sin(ang)

    const hw = half[0]
    const hh = half[1]
    const pad = 20
    const rx = Math.max(40, hw - pad)
    const ry = Math.max(24, hh - pad)
    const rawSpan = Math.hypot(first.v1[0] - first.v0[0], first.v1[1] - first.v0[1])
    const estCellW = (hw * 2) / 9
    const sc = clamp(rawSpan / Math.max(8, estCellW), 0.55, 1.35)

    this.pushNeighbors(null, cx0, cy0, 210)

    const speedBase = Math.max(420, Math.hypot(vxAvg, vyAvg))
    const dirX = vxAvg / (speedBase || 1)
    const dirY = vyAvg / (speedBase || 1)
    const epicX = clamp(-dirX * rx * 0.35 + this.rnd(-rx * 0.2, rx * 0.2), -rx * 0.65, rx * 0.65)
    const epicY = clamp(-dirY * ry * 0.35 + this.rnd(-ry * 0.2, ry * 0.2), -ry * 0.65, ry * 0.65)

    const cols = 6
    const rows = 3
    const seeds = []
    for (let iy = 0; iy < rows; iy++) {
      for (let ix = 0; ix < cols; ix++) {
        const u = (ix + 0.5) / cols
        const v = (iy + 0.5) / rows
        let sx = (u * 2 - 1) * rx + this.rnd(-rx / cols, rx / cols) * 0.72
        let sy = (v * 2 - 1) * ry + this.rnd(-ry / rows, ry / rows) * 0.72
        const pull = (ix + iy) % 2 === 0 ? 0.22 : 0.0
        sx = lerp(sx, epicX, pull)
        sy = lerp(sy, epicY, pull)
        seeds.push([clamp(sx, -rx * 0.94, rx * 0.94), clamp(sy, -ry * 0.94, ry * 0.94)])
      }
    }

    const out = []
    const initRect = [
      [-rx, -ry],
      [rx, -ry],
      [rx, ry],
      [-rx, ry],
    ]

    for (let i = 0; i < seeds.length; i++) {
      const si = seeds[i]
      let poly = initRect
      for (let j = 0; j < seeds.length; j++) {
        if (i === j) continue
        const sj = seeds[j]
        const nx = sj[0] - si[0]
        const ny = sj[1] - si[1]
        const len2 = nx * nx + ny * ny
        if (len2 < 1e-3) continue
        const mx = (si[0] + sj[0]) * 0.5
        const my = (si[1] + sj[1]) * 0.5
        const d = mx * nx + my * ny
        poly = clipHalfPlane(poly, nx, ny, d)
        if (poly.length < 3) break
      }
      if (poly.length < 3) continue

      while (poly.length > 4) {
        let minLen = 1e9
        let minIdx = 0
        for (let k = 0; k < poly.length; k++) {
          const a = poly[k]
          const b = poly[(k + 1) % poly.length]
          const l = Math.hypot(b[0] - a[0], b[1] - a[1])
          if (l < minLen) {
            minLen = l
            minIdx = k
          }
        }
        const nextIdx = (minIdx + 1) % poly.length
        const mid = [(poly[minIdx][0] + poly[nextIdx][0]) * 0.5, (poly[minIdx][1] + poly[nextIdx][1]) * 0.5]
        poly.splice(minIdx, 1, mid)
        poly.splice(nextIdx < minIdx ? nextIdx : minIdx + 1, 1)
      }

      let cx = 0
      let cy = 0
      for (const p of poly) {
        cx += p[0]
        cy += p[1]
      }
      cx /= poly.length
      cy /= poly.length

      const wx = cx0 + (cx * ca - cy * sa) * sc
      const wy = cy0 + (cx * sa + cy * ca) * sc

      const dx = cx - epicX
      const dy = cy - epicY
      const dist = Math.hypot(dx, dy) + 18
      const proximity = clamp(1 - dist / (rx * 1.3), 0, 1)
      const sp = speedBase * (0.65 + 0.85 * proximity) * this.rnd(0.78, 1.22)
      const ox = (dx / dist) * 0.92 + dirX * 0.72 + this.rnd(-0.18, 0.18)
      const oy = (dy / dist) * 0.92 + dirY * 0.72 - 0.18 + this.rnd(-0.18, 0.18)
      const cellVx = ox * sp
      const cellVy = oy * sp
      const cellSpin = ((dx * oy - dy * ox) / rx) * 9 + this.rnd(-11, 11)
      const cellLife = this.rnd(0.55, 1.05) // Crisp, fast clean-up so stage stays uncluttered
      const cellHeat = 0.85 + 0.15 * proximity
      const cellTumble = this.rnd(0.4, 6.0)

      const rel = (pt) => [(pt[0] - cx) * sc, (pt[1] - cy) * sc]
      const v0 = rel(poly[0])
      const v1 = rel(poly[1])
      const v2 = rel(poly[2])
      const v3 = rel(poly[3] ?? poly[2])
      out.push({
        x: wx,
        y: wy,
        vx: cellVx,
        vy: cellVy,
        ang,
        spin: cellSpin,
        life: cellLife,
        v0,
        v1,
        v2,
        v3,
        heat: cellHeat,
        tumble: cellTumble,
        base: [cx, cy],
        half,
        uv,
        entry,
        _voronoi: true,
      })
    }
    return out.length ? out : rawList
  }

  step(scene) {
    const dt = clamp(scene.dt || 1 / 60, 0, 1 / 15)
    const time = scene.time || 0
    this.viewport.w = scene.w || 1440
    this.viewport.h = scene.h || 900
    this.liveCards = scene.problems || []
    this.liveFx = scene.fx || null
    const hd = scene.head
    this.headState = hd
    this.intensity = clamp(scene.intensity ?? hd?.anger ?? 0, 0, 1)

    const shoutSt = scene.director
    const inferredSaiyan = shoutSt && shoutSt.stage >= 4 && shoutSt.blast !== "after" ? 1 : scene.saiyan ?? 0
    this.saiyan = lerp(this.saiyan, Math.max(scene.saiyan ?? 0, inferredSaiyan), 1 - Math.exp(-dt * 8))

    // Sync finale state from scene.finale or setFinale()
    if (scene.finale !== undefined) this.finale = scene.finale
    const finPhase = this.finale?.phase ?? shoutSt?.finale ?? null
    const targetDizzy = finPhase === "dizzy" || shoutSt?.blast === "after" ? 1 : 0
    this.dizzy = targetDizzy > this.dizzy ? lerp(this.dizzy, targetDizzy, 1 - Math.exp(-dt * 10)) : Math.max(0, this.dizzy - dt * 0.32)

    // Spring-damper for head recoil from Tommy-gun shots
    const wRec = 2 * Math.PI * 14
    // Substepped for stability at 1/60 s (main.js agent fix, 2026-10-20).
    for (let k = 0, n = Math.max(1, Math.ceil(dt / (1 / 240))), h = dt / n; k < n; k++) {
      this.recoilVx += (-wRec * wRec * this.recoilX - 2 * 0.65 * wRec * this.recoilVx) * h
      this.recoilVy += (-wRec * wRec * this.recoilY - 2 * 0.65 * wRec * this.recoilVy) * h
      this.recoilX = clamp(this.recoilX + this.recoilVx * h, -18, 18)
      this.recoilY = clamp(this.recoilY + this.recoilVy * h, -18, 18)
    }

    if (this.liveFx) {
      for (const f of this.liveFx) {
        if (f.kind === 5 && f.rMax) {
          f.r = f.rMax * (1 - Math.pow(1 - (f.t ?? 0), 3))
        }
      }
    }

    this.saiyanBurstAge += dt
    this.aberration = Math.max(0, this.aberration - dt * 2.6) * Math.exp(-dt * 4.0)
    this.heatHaze = lerp(this.heatHaze, clamp(this.saiyan * 0.65 + (hd?.anger ?? 0) * 0.25, 0, 1.0), 1 - Math.exp(-dt * 6))

    // 1. Track flying words approaching their target cards
    let latestWord = null
    let minWordAge = 99
    if (scene.words) {
      for (const w of scene.words) {
        if (w.target) {
          const rb = this.ensureCard(w.target)
          rb.lastWord = { x: w.x, y: w.y, vx: w.vx, vy: w.vy, i: w.i ?? this.intensity, text: w.entry?.text ?? "" }
        }
        const total = w.total || 0.2
        const age = Math.max(0, total - (w.rem ?? 0))
        if (age < minWordAge) {
          minWordAge = age
          latestWord = { text: w.entry?.text ?? "", i: w.i ?? this.intensity, vx: w.vx, vy: w.vy }
        }
      }
    }

    // 2. Step 2.5D rigid-body physics for every card (clamped so resting text stays 100% sharp & readable!)
    for (const c of this.liveCards) {
      const rb = this.ensureCard(c)
      const landed = c.landed ?? 0
      if (landed > rb.lastLanded) {
        const hits = landed - rb.lastLanded
        rb.lastLanded = landed
        const lw = rb.lastWord ?? {
          x: c.x + this.rnd(-c.hw * 0.35, c.hw * 0.35),
          y: c.y + c.hh * 0.22,
          vx: (c.x - (hd?.x ?? this.viewport.w / 2)) * 2.2,
          vy: (c.y - (hd?.y ?? this.viewport.h * 0.42)) * 2.2,
          i: clamp(this.intensity, 0.05, 1),
        }
        for (let k = 0; k < Math.min(2, hits); k++) {
          this.hitCard(c, lw, { x: lw.x, y: lw.y })
        }
      }

      // Critically-damped spring back to slot so resting cards never blur or smear text!
      const freq = 9.2
      const zeta = 0.72
      const w = 2 * Math.PI * freq
      const sub = 4
      const h = dt / sub
      for (let k = 0; k < sub; k++) {
        rb.vx += (-w * w * rb.px - 2 * zeta * w * rb.vx) * h
        rb.vy += (-w * w * rb.py - 2 * zeta * w * rb.vy) * h
        rb.px += rb.vx * h
        rb.py += rb.vy * h

        rb.vrot += (-w * w * 1.3 * rb.rot - 2 * zeta * w * 1.15 * rb.vrot) * h
        rb.rot += rb.vrot * h

        rb.vtiltX += (-w * w * 1.4 * rb.tiltX - 2 * 0.75 * w * rb.vtiltX) * h
        rb.vtiltY += (-w * w * 1.4 * rb.tiltY - 2 * 0.75 * w * rb.vtiltY) * h
        rb.tiltX += rb.vtiltX * h
        rb.tiltY += rb.vtiltY * h
      }

      rb.px = clamp(rb.px, -12, 12)
      rb.py = clamp(rb.py, -12, 12)
      rb.rot = clamp(rb.rot, -0.08, 0.08)
      rb.tiltX = clamp(rb.tiltX, -0.42, 0.42)
      rb.tiltY = clamp(rb.tiltY, -0.42, 0.42)

      // Strip main.js's high-frequency card jitter (Math.sin(time * 53 + s) * jit) so card text is razor-sharp!
      const s = c.seed ?? 0
      const baseBreathX = Math.sin(time * 0.8 + s) * 1.2
      const baseBreathY = Math.sin(time * 1.1 + s * 2) * 1.5
      const isMovingOut = c.state === "resolving" || c.state === "sent"
      const slotX = isMovingOut ? c.x : (c.bx ?? c.x) + (c.ox?.x ?? 0) + (c.kx?.x ?? 0) * 0.35 + baseBreathX
      const slotY = isMovingOut ? c.y : (c.by ?? c.y) + (c.oy?.x ?? 0) + (c.ky?.x ?? 0) * 0.35 + baseBreathY

      rb.ripple = Math.min(1.5, rb.ripple + dt * 2.8)
      if (rb.resolved) rb.resolveAge += dt
      const blastWindow = c.blastAt != null ? c.blastAt - time : 99
      if (c.state === "dying" || (blastWindow >= 0 && blastWindow < 0.24)) {
        const kWave = c.state === "dying" ? 1 : 1 - blastWindow / 0.24
        rb.damage = Math.min(1.35, Math.max(rb.damage, 0.82 * kWave) + dt * 3.5)
        rb.flash = (0.58 + 0.32 * Math.sin(time * 65)) * kWave
        if (c.hitDir) {
          const dl = Math.hypot(c.hitDir.x, c.hitDir.y) || 1
          rb.impX = clamp((-c.hitDir.x / dl) * 0.65, -0.85, 0.85)
          rb.impY = clamp((-c.hitDir.y / dl) * 0.65, -0.85, 0.85)
          rb.tiltX = clamp((c.hitDir.x / dl) * 0.38 * kWave, -0.5, 0.5)
          rb.tiltY = clamp((c.hitDir.y / dl) * 0.38 * kWave, -0.5, 0.5)
        }
        const tension = 2.0 * kWave
        c._fxX = slotX + rb.px + Math.sin(time * 89 + s) * tension
        c._fxY = slotY + rb.py + Math.cos(time * 97 + s) * tension
      } else {
        rb.flash = Math.max(0, rb.flash - dt * 6.5)
        c._fxX = slotX + rb.px
        c._fxY = slotY + rb.py
      }
      c._fxRot = (c.rot ?? 0) * 0.35 + rb.rot
      c._fxScale = (c.scale ?? 1) * (1 - 0.025 * Math.min(1, Math.hypot(rb.tiltX, rb.tiltY)))
      c._fxTiltX = rb.tiltX
      c._fxTiltY = rb.tiltY
      c._fxFlash = rb.flash
      c._fxDamage = rb.damage
      c._fxImpX = rb.impX
      c._fxImpY = rb.impY
      c._fxRipple = rb.ripple
      c._fxSeed = rb.seed + (rb.resolved || c.state === "resolving" ? 100 : 0)
    }

    // 3. Step continuous mouth viseme, eye saccades & 2D physical googly eyes
    if (hd) {
      hd.x += this.recoilX
      hd.y += this.recoilY

      const env = hd.mouthOpen ?? 0
      const baseMouth = hd.mouth ?? 0
      if (this.dizzy > 0.05) {
        hd.anger = (hd.anger ?? 0) * Math.max(0, 1 - this.dizzy * 1.6)
        hd.vein = 0
        this.saiyan = this.saiyan * Math.max(0, 1 - this.dizzy * 1.8)
      }
      const anger = hd.anger ?? 0
      const effIntensity = this.dizzy > 0.05 ? 0.08 : Math.max(this.intensity, anger, this.saiyan)

      // Note 2: Calm stage mouth gating!
      // At low intensity (Calm stage), limit mouth opening & width to small friendly conversational shapes!
      const maxMouthByIntensity = this.dizzy > 0.05 ? 0.22 : lerp(0.15, 1.25, smooth(0.24, 0.65, effIntensity))
      const vis = latestWord && minWordAge < 0.32 ? analyzeViseme(latestWord.text) : { open: 0.78, wide: hd.mouthW ?? 1, round: 0.2, closure: 0 }

      const onsetDip = latestWord && minWordAge < 0.034 ? (1 - minWordAge / 0.034) * vis.closure * 0.55 : 0
      // In calm stage, ignore main.js's cumulative word-launch velocity spike on baseMouth so mouth stays gentle!
      const calmMouthBase = effIntensity < 0.28 ? Math.min(baseMouth * 0.32, 0.14) : baseMouth
      const rawOpen = Math.max(calmMouthBase, env * lerp(0.32, 1.15, effIntensity) * vis.open) * (1 - onsetDip)
      const targetOpen = clamp(rawOpen, 0, maxMouthByIntensity)

      const rawWide = hd.speaking || latestWord ? lerp(hd.mouthW ?? 1, vis.wide, 0.68) : hd.mouthW ?? 1
      const calmWideCap = this.dizzy > 0.05 ? 0.62 : lerp(0.68, 1.22, smooth(0.24, 0.65, effIntensity))
      const targetWide = clamp(
        lerp(rawWide, Math.max(rawWide, 1.14), smooth(0.48, 0.88, targetOpen)),
        0.52,
        calmWideCap,
      )

      // Substepped: these stiff springs diverge at a 1/60 s step (main.js agent fix, 2026-10-20).
      const subN = Math.max(1, Math.ceil(dt / (1 / 240)))
      const sdt = dt / subN
      for (let k = 0; k < subN; k++) {
        const freqOpen = targetOpen > this.mouthOpen ? 18 : 11
        const wO = 2 * Math.PI * freqOpen
        this.mouthVel += (-wO * wO * (this.mouthOpen - targetOpen) - 2 * 0.68 * wO * this.mouthVel) * sdt
        this.mouthOpen = clamp(this.mouthOpen + this.mouthVel * sdt, 0, 1.35)

        const wW = 2 * Math.PI * 14
        this.mouthWideVel += (-wW * wW * (this.mouthWide - targetWide) - 2 * 0.72 * wW * this.mouthWideVel) * sdt
        this.mouthWide = clamp(this.mouthWide + this.mouthWideVel * sdt, 0.45, 1.35)
      }

      this.mouthRound = lerp(this.mouthRound, vis.round * smooth(0.06, 0.35, this.mouthOpen), 1 - Math.exp(-dt * 16))
      const targetSkew = latestWord ? clamp((latestWord.vx || 0) / 2400, -0.35, 0.35) : 0
      this.jawSkew = lerp(this.jawSkew, targetSkew, 1 - Math.exp(-dt * 12))
      this.tonguePhase += dt * (22 + 32 * this.saiyan + 18 * anger)

      // Ballistic eye saccades
      this.nextSaccade -= dt
      const baseGx = hd.gazeX ?? 0
      const baseGy = hd.gazeY ?? 0
      const distGaze = Math.hypot(baseGx - this.saccadeX, baseGy - this.saccadeY)
      if (distGaze > 0.22 || this.nextSaccade <= 0) {
        this.saccadeX = lerp(this.saccadeX, baseGx, distGaze > 0.22 ? 0.78 : 0.35)
        this.saccadeY = lerp(this.saccadeY, baseGy, distGaze > 0.22 ? 0.78 : 0.35)
        this.nextSaccade = this.rnd(0.14, 0.38) / (1 + 0.8 * this.saiyan)
        const amp = 0.025 + 0.055 * anger + 0.07 * this.saiyan
        this.microX = this.rnd(-amp, amp)
        this.microY = this.rnd(-amp, amp) * 0.7
      }

      // 2D Physical Googly Eyes simulation (inertia + gravity + rage rattle + lens wall bounce + dizzy counter-orbit)
      const hx = hd.x ?? this.viewport.w / 2
      const hy = hd.y ?? this.viewport.h * 0.42
      const hvx = this.prevHeadX != null && dt > 0 ? (hx - this.prevHeadX) / dt : 0
      const hvy = this.prevHeadY != null && dt > 0 ? (hy - this.prevHeadY) / dt : 0
      const hax = dt > 0 ? clamp((hvx - this.prevHeadVx) / dt, -8000, 8000) : 0
      const hay = dt > 0 ? clamp((hvy - this.prevHeadVy) / dt, -8000, 8000) : 0
      this.prevHeadX = hx
      this.prevHeadY = hy
      this.prevHeadVx = hvx
      this.prevHeadVy = hvy

      // Secondary curly-hair bounce spring
      const wHair = 2 * Math.PI * 6.5
      this.hairBounce.vx += (-wHair * wHair * this.hairBounce.x - 2 * 0.38 * wHair * this.hairBounce.vx - hax * 0.0025) * dt
      this.hairBounce.vy += (-wHair * wHair * this.hairBounce.y - 2 * 0.38 * wHair * this.hairBounce.vy - hay * 0.0025) * dt
      this.hairBounce.x = clamp(this.hairBounce.x + this.hairBounce.vx * dt, -0.35, 0.35)
      this.hairBounce.y = clamp(this.hairBounce.y + this.hairBounce.vy * dt, -0.35, 0.35)

      const rattleAmp = (anger > 0.45 || this.saiyan > 0.2 ? (anger * 0.7 + this.saiyan * 1.2) * (0.4 + 0.8 * this.mouthOpen) : 0) * 26
      const stepGoogly = (g, dirSign) => {
        if (this.dizzy > 0.08) {
          // Counter-rotating dizzy orbits!
          const a = time * 11.5 * dirSign + (dirSign > 0 ? 0 : 1.8)
          const tx = Math.cos(a) * 0.62 * this.dizzy + this.saccadeX * (1 - this.dizzy)
          const ty = Math.sin(a) * 0.62 * this.dizzy + this.saccadeY * (1 - this.dizzy)
          g.x = lerp(g.x, tx, 1 - Math.exp(-dt * 18))
          g.y = lerp(g.y, ty, 1 - Math.exp(-dt * 18))
          return
        }
        // Spring toward gaze + gravity + inertial impulse from head acceleration + rage rattle
        const gxTarget = clamp(this.saccadeX * 0.65, -0.68, 0.68)
        const gyTarget = clamp(this.saccadeY * 0.65 + 0.10, -0.68, 0.68)
        const kSpring = 48
        const damp = 5.2
        const rx = rattleAmp * Math.sin(time * 67 + dirSign * 2.1)
        const ry = rattleAmp * Math.cos(time * 79 - dirSign * 1.4)
        g.vx += (kSpring * (gxTarget - g.x) - damp * g.vx - hax * 0.0042 + rx) * dt
        g.vy += (kSpring * (gyTarget - g.y) - damp * g.vy - hay * 0.0042 + 4.5 + ry) * dt
        g.x += g.vx * dt
        g.y += g.vy * dt
        const r = Math.hypot(g.x, g.y)
        const maxR = 0.68
        if (r > maxR) {
          const nx = g.x / r
          const ny = g.y / r
          g.x = nx * maxR
          g.y = ny * maxR
          const vn = g.vx * nx + g.vy * ny
          if (vn > 0) {
            g.vx -= 1.72 * vn * nx
            g.vy -= 1.72 * vn * ny
          }
        }
      }
      stepGoogly(this.googlyL, 1)
      stepGoogly(this.googlyR, -1)

      const targetPupil = clamp(
        1.0 + 0.10 * clamp(hd.browUp ?? 0, 0, 1) - 0.36 * Math.pow(anger, 1.4) - 0.22 * this.saiyan,
        0.40,
        1.14,
      )
      this.pupilScale = lerp(this.pupilScale, targetPupil, 1 - Math.exp(-dt * 14))

      hd._fxMouth = this.mouthOpen
      hd._fxMouthW = this.mouthWide
      hd._fxMouthRound = this.mouthRound
      hd._fxJawSkew = this.jawSkew
      hd._fxTongue = Math.sin(this.tonguePhase) * 0.5 + 0.5
      hd._fxGazeX = clamp(this.saccadeX + this.microX, -1.15, 1.15)
      hd._fxGazeY = clamp(this.saccadeY + this.microY, -1.15, 1.15)
      hd._fxPupil = this.pupilScale
      hd._fxGooglyLX = this.googlyL.x
      hd._fxGooglyLY = this.googlyL.y
      hd._fxGooglyRX = this.googlyR.x
      hd._fxGooglyRY = this.googlyR.y
      hd._fxHairX = this.hairBounce.x
      hd._fxHairY = this.hairBounce.y
      hd._fxDizzy = this.dizzy

      // Drive 360° turntable head spin directly from finale.spin & finale.angle (bypassing slow 3.4Hz spring)
      if (this.finale && this.finale.spin > 0.05) {
        const spK = clamp(this.finale.spin / 3.2, 0, 1)
        const fAng = this.finale.angle ?? 0
        const ca = Math.cos(fAng)
        const sa = Math.sin(fAng)
        hd.faceX = lerp(hd.faceX ?? 0, ca * 0.28, spK)
        hd.faceY = lerp(hd.faceY ?? 0, sa * 0.18, spK)
        hd.tilt = lerp(hd.tilt ?? 0, ca * 0.34, spK)
        hd._fxGazeX = lerp(hd._fxGazeX, ca * 0.95, spK)
        hd._fxGazeY = lerp(hd._fxGazeY, sa * 0.85, spK)
        hd._fxGooglyLX = lerp(hd._fxGooglyLX, ca * 0.68, spK)
        hd._fxGooglyLY = lerp(hd._fxGooglyLY, sa * 0.68, spK)
        hd._fxGooglyRX = lerp(hd._fxGooglyRX, ca * 0.68, spK)
        hd._fxGooglyRY = lerp(hd._fxGooglyRY, sa * 0.68, spK)
      }

    }

    // 4. Continuous Super Saiyan rising embers & crackling lightning arcs
    if (this.saiyan > 0.08 && hd) {
      this.emberAcc += dt * (42 + 90 * this.saiyan)
      const sparks = []
      while (this.emberAcc >= 1) {
        this.emberAcc -= 1
        const a = this.rnd(0, Math.PI * 2)
        const r = hd.R * this.rnd(0.55, 1.35)
        sparks.push({
          x: hd.x + Math.cos(a) * r,
          y: hd.y + Math.sin(a) * r,
          vx: Math.cos(a) * this.rnd(20, 140) + this.rnd(-40, 40),
          vy: -this.rnd(160, 480) * (0.6 + 0.6 * this.saiyan),
          life: this.rnd(0.28, 0.72),
          heat: -this.rnd(0.7, 1.0),
          size: this.rnd(1.4, 2.4),
        })
      }
      if (sparks.length && this.onSpawnSparks) this.onSpawnSparks(sparks)

      this.arcAcc += dt * (3.2 + 6.5 * this.saiyan)
      if (this.arcAcc >= 1 && this.liveFx) {
        this.arcAcc -= 1
        const a = this.rnd(0, Math.PI * 2)
        const d = hd.R * this.rnd(0.65, 1.25)
        this.liveFx.push({
          kind: 4,
          x: hd.x + Math.cos(a) * d,
          y: hd.y + Math.sin(a) * d,
          r: hd.R * this.rnd(0.7, 1.5),
          width: a + this.rnd(-0.8, 0.8),
          t: 0,
          k: 0.75 + 0.45 * this.saiyan,
          age: 0,
          life: this.rnd(0.09, 0.18),
        })
      }
    }
  }
}

export const fx = new ShoutFx()
export default fx
