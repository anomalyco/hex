// Hex 2 · Line: a waveform pill under the icon speaks; each word hops out of the
// pill and lands in its place on a typed line. The line lifts away and the next
// sentence starts. Two sentences per loop; the seam is an empty line.
;(() => {
  const PER = 180
  const SENTENCES = ["Move standup to ten tomorrow.", "Looks great, ship it tonight."]
  const FRAMES = PER * SENTENCES.length
  const ICON = { x: 600, y: 150, size: 132 }
  const PILL = { x: 600, y: 292, w: 172, h: 54 }
  const LINE_Y = 468
  const FONT = '600 54px "Inter Variable"'
  const FLIGHT = 28
  const G = 0.95
  const BARS = 11
  let icon, lines

  const bump = (dt, k = 4) => (dt < 0 || dt > 60 ? 0 : (dt / k) * Math.exp(1 - dt / k))

  register({
    frames: FRAMES,
    async init(ctx) {
      icon = await loadImage("/scripts/og/hex-icon.png")
      ctx.font = FONT
      const space = ctx.measureText(" ").width
      const r = rng(11)
      lines = SENTENCES.map((s, si) => {
        const words = s.split(" ").map((text) => ({ text, w: ctx.measureText(text).width }))
        const total = words.reduce((a, w) => a + w.w, 0) + space * (words.length - 1)
        let x = W / 2 - total / 2
        words.forEach((w, i) => {
          w.x = x + w.w / 2
          x += w.w + space
          w.at = si * PER + 10 + i * 12 + (i > 2 ? 4 : 0)
          w.spin = (r() - 0.5) * 0.9
        })
        return { words, start: W / 2 - total / 2, end: W / 2 + total / 2, base: si * PER }
      })
    },
    draw(ctx, f) {
      const g = ctx.createRadialGradient(600, 230, 0, 600, 230, 760)
      g.addColorStop(0, "#141414")
      g.addColorStop(1, "#060606")
      ctx.fillStyle = g
      ctx.fillRect(0, 0, W, H)

      const li = Math.floor(f / PER)
      const line = lines[li]
      const local = f - line.base

      // Words in flight are drawn first so they emerge from behind the pill.
      ctx.font = FONT
      ctx.textAlign = "center"
      ctx.textBaseline = "middle"
      const exit = ease.exit(seg(local, 148, 168))
      const landed = []
      const flying = []
      for (const w of line.words) {
        const dt = f - w.at
        if (dt < 0) continue
        if (dt >= FLIGHT) landed.push(w)
        else flying.push(w)
      }

      for (const w of flying) {
        const t = f - w.at
        const T = FLIGHT
        // Dropped out of the bottom of the pill: falling, drifting to its slot.
        const x0 = PILL.x, y0 = PILL.y + 6, y1 = LINE_Y
        const v0 = 2.5
        const g = (2 * (y1 - y0 - v0 * T)) / (T * T)
        const p = t / T
        const x = lerp(x0, w.x, ease.inOut(p) * 0.5 + p * 0.5)
        const y = y0 + v0 * t + 0.5 * g * t * t
        const s = lerp(0.4, 1, ease.enter(clamp(p / 0.6)))
        ctx.save()
        ctx.translate(x, y)
        ctx.rotate(w.spin * 0.35 * Math.sin(Math.PI * p))
        ctx.scale(s, s)
        ctx.fillStyle = "#ededea"
        ctx.fillText(w.text, 0, 0)
        ctx.restore()
      }

      // Pill with a voice-driven waveform.
      let env = 0
      for (const w of line.words) env = Math.max(env, clamp(1 - Math.abs(f - (w.at - 3)) / 9))
      let push = 0
      for (const w of line.words) push += bump(f - w.at, 3)
      const ps = 1 + 0.035 * Math.min(1, push)
      ctx.save()
      ctx.translate(PILL.x, PILL.y)
      ctx.scale(ps, ps)
      roundRect(ctx, -PILL.w / 2, -PILL.h / 2, PILL.w, PILL.h, PILL.h / 2)
      ctx.fillStyle = "#161616"
      ctx.fill()
      ctx.strokeStyle = "rgba(255,255,255,0.10)"
      ctx.lineWidth = 1.5
      ctx.stroke()
      const bw = 5, gap = 8
      const span = BARS * bw + (BARS - 1) * gap
      for (let i = 0; i < BARS; i++) {
        const c = 1 - Math.abs(i - (BARS - 1) / 2) / ((BARS - 1) / 2)
        const wob = 0.45 + 0.55 * Math.abs(Math.sin(f * 0.31 + i * 1.7) * Math.cos(f * 0.17 + i * 0.6))
        const h = 6 + 30 * env * (0.35 + 0.65 * c) * wob
        roundRect(ctx, -span / 2 + i * (bw + gap), -h / 2, bw, h, bw / 2)
        ctx.fillStyle = `rgba(237,237,234,${0.55 + 0.45 * env})`
        ctx.fill()
      }
      ctx.restore()

      // The typed line, with a landing squash per word.
      ctx.save()
      ctx.globalAlpha = 1 - exit
      if (exit > 0) ctx.filter = `blur(${(4 * exit).toFixed(2)}px)`
      ctx.translate(W / 2, LINE_Y - 14 * exit)
      ctx.scale(1 - 0.04 * exit, 1 - 0.04 * exit)
      ctx.translate(-W / 2, -LINE_Y)
      ctx.fillStyle = "#ededea"
      for (const w of landed) {
        const k = bump(f - w.at - FLIGHT, 3)
        ctx.save()
        ctx.translate(w.x, LINE_Y + 27)
        ctx.scale(1 + 0.05 * k, 1 - 0.1 * k)
        ctx.fillText(w.text, 0, -27)
        ctx.restore()
      }
      ctx.restore()

      // Caret: follows the last landed word, travels to the next line's start during the gap.
      const next = lines[(li + 1) % lines.length]
      let cx = line.start - 14
      let lastLand = -1
      // The caret steps aside just before each word lands.
      for (const w of line.words) {
        const go = w.at + FLIGHT - 9
        if (f < go) break
        cx = lerp(cx, w.x + w.w / 2 + 10, spring((f - go) / FPS, 0.2))
        if (f >= w.at + FLIGHT) lastLand = w.at + FLIGHT
      }
      if (local >= 150) cx = lerp(cx, next.start - 14, spring((local - 150) / FPS, 0.3))
      const idle = lastLand < 0 ? local : f - lastLand
      const blink = idle > 20 && Math.floor((idle - 20) / 32) % 2 === 1 ? 0.15 : 1
      ctx.fillStyle = `rgba(237,237,234,${0.85 * blink})`
      roundRect(ctx, cx - 1.5, LINE_Y - 30, 3, 60, 1.5)
      ctx.fill()

      ctx.save()
      ctx.shadowColor = "rgba(0,0,0,0.6)"
      ctx.shadowBlur = 34
      ctx.shadowOffsetY = 12
      ctx.drawImage(icon, ICON.x - ICON.size / 2, ICON.y - ICON.size / 2, ICON.size, ICON.size)
      ctx.restore()
    },
  })
})()
