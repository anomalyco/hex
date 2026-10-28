// Hex 3 · Key: hold ⌥ and the words leave the key in arcs, landing on a new line.
// Release, and the page scrolls up a line. Three sentences repeat, so after three
// presses the column is exactly where it started.
;(() => {
  const CYCLE = 108
  const SENTENCES = ["Ship it tonight.", "Lunch at noon?", "Looks good to me."]
  const FRAMES = CYCLE * SENTENCES.length
  const KEY = { x: 318, y: 380, s: 196, r: 42, lip: 10 }
  const COL_X = 520
  const LINE_Y = 408
  const LH = 80
  const FONT = '600 56px "Inter Variable"'
  const FLIGHT = 24
  const G = 0.62
  let lines

  const bump = (dt, k = 3) => (dt < 0 || dt > 60 ? 0 : (dt / k) * Math.exp(1 - dt / k))

  register({
    frames: FRAMES,
    async init(ctx) {
      ctx.font = FONT
      const space = ctx.measureText(" ").width
      lines = SENTENCES.map((s) => {
        let x = COL_X
        const words = s.split(" ").map((text, i) => {
          const w = ctx.measureText(text).width
          const word = { text, w, x: x + w / 2, at: 7 + i * 9 }
          x += w + space
          return word
        })
        return { words, release: words[words.length - 1].at + 10 }
      })
    },
    draw(ctx, f) {
      const g = ctx.createRadialGradient(KEY.x, KEY.y, 0, KEY.x, KEY.y, 900)
      g.addColorStop(0, "#151515")
      g.addColorStop(1, "#060606")
      ctx.fillStyle = g
      ctx.fillRect(0, 0, W, H)

      const cycle = Math.floor(f / CYCLE)
      const local = f - cycle * CYCLE
      const cur = lines[cycle]
      const scroll = spring((local - 80) / FPS, 0.36)

      ctx.font = FONT
      ctx.textAlign = "center"
      ctx.textBaseline = "middle"

      // Older lines and the landed words of the current one, under a soft top fade.
      for (let age = 0; age < 5; age++) {
        const line = lines[(((cycle - age) % 3) + 3) % 3]
        const y = LINE_Y - LH * (age + scroll)
        const alpha = clamp((y - 90) / 160)
        if (alpha <= 0) continue
        const dim = age === 0 ? scroll : 1
        ctx.fillStyle = `rgba(237,237,234,${alpha * lerp(1, 0.5, dim)})`
        for (const w of line.words) {
          let k = 0
          if (age === 0) {
            const dt = local - w.at
            if (dt < FLIGHT) continue
            k = bump(dt - FLIGHT)
          }
          ctx.save()
          ctx.translate(w.x, y + 28)
          ctx.scale(1 + 0.05 * k, 1 - 0.1 * k)
          ctx.fillText(w.text, 0, -28)
          ctx.restore()
        }
      }

      // Words in flight, launched from behind the key.
      ctx.fillStyle = "#ededea"
      for (const w of cur.words) {
        const t = local - w.at
        if (t < 0 || t >= FLIGHT) continue
        const x0 = KEY.x + 30, y0 = KEY.y - 16
        const vy = (LINE_Y - y0 - 0.5 * G * FLIGHT * FLIGHT) / FLIGHT
        const p = t / FLIGHT
        const x = lerp(x0, w.x, ease.inOut(p) * 0.35 + p * 0.65)
        const y = y0 + vy * t + 0.5 * G * t * t
        const s = lerp(0.5, 1, ease.enter(clamp(p / 0.5)))
        ctx.save()
        ctx.translate(x, y)
        ctx.rotate(-0.5 * (1 - ease.inOut(p)))
        ctx.scale(s, s)
        ctx.fillText(w.text, 0, 0)
        ctx.restore()
      }

      // The ⌥ key: pressed before the first word, released after the last.
      const down = ease.enter(seg(local, 0, 6)) * (1 - spring((local - cur.release) / FPS, 0.22))
      const depth = KEY.lip * 0.75 * down
      const { x, y, s, r, lip } = KEY
      ctx.save()
      ctx.shadowColor = "rgba(0,0,0,0.65)"
      ctx.shadowBlur = 36 - 16 * down
      ctx.shadowOffsetY = 18 - 10 * down
      roundRect(ctx, x - s / 2, y - s / 2 + lip, s, s, r)
      ctx.fillStyle = "#0b0b0c"
      ctx.fill()
      ctx.restore()
      const top = y - s / 2 + depth
      roundRect(ctx, x - s / 2, top, s, s, r)
      const kg = ctx.createLinearGradient(0, top, 0, top + s)
      kg.addColorStop(0, "#2c2c2f")
      kg.addColorStop(1, "#1b1b1d")
      ctx.fillStyle = kg
      ctx.fill()
      ctx.strokeStyle = "rgba(255,255,255,0.09)"
      ctx.lineWidth = 2
      ctx.stroke()
      ctx.font = '400 92px "Inter Variable"'
      ctx.fillStyle = `rgba(237,237,234,${0.82 + 0.18 * down})`
      ctx.fillText("⌥", x, top + s / 2 + 4)
    },
  })
})()
