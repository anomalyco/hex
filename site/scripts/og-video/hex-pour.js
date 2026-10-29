// Hex 1 · Pour: the icon speaks in phrases; each word pops out from behind it,
// arcs, falls and piles on the floor. The floor lets go, the pile drops away,
// and the empty frame is the first frame again.
;(() => {
  const FRAMES = 270
  const FLOOR_DROP = 204
  const ICON = { x: 600, y: 236, size: 176 }
  const PHRASES = [
    "hey can we push",
    "the review to thursday",
    "I want the windows build",
    "signed first",
    "then ship it to everyone",
    "and send dax",
    "the notes from today",
    "also the sync with jay",
    "moved to ten",
    "so grab coffee after",
    "and remind me to call mom",
    "tonight about the weekend",
  ]
  const FONT = '600 29px "Inter Variable"'
  let icon, sim, words, spawns

  register({
    frames: FRAMES,
    async init(ctx) {
      icon = await loadImage("/scripts/og/hex-icon.png")
      ctx.font = FONT
      const r = rng(7)
      words = []
      let f = 2
      for (const phrase of PHRASES) {
        for (const w of phrase.split(" ")) {
          const side = r() < 0.5 ? -1 : 1
          words.push({
            text: w,
            at: f,
            w: ctx.measureText(w).width + 10,
            h: 32,
            vx: side * (1 + r() * 11),
            vy: -(8 + r() * 6.5),
            av: (r() - 0.5) * 0.16,
          })
          f += 2
        }
        f += 3
      }
      spawns = words.map((w) => w.at)
      sim = simulate({
        steps: FRAMES,
        gravity: 2.3,
        setup(engine, world) {
          const { Bodies, Composite } = Matter
          const opts = { isStatic: true, label: "static", friction: 0.8 }
          const floor = Bodies.rectangle(W / 2, H - 14 + 50, W * 2, 100, opts)
          Composite.add(world, [floor, Bodies.rectangle(-50, 0, 100, H * 4, opts), Bodies.rectangle(W + 50, 0, 100, H * 4, opts)])
          return { floor }
        },
        onStep(f, engine, world, c) {
          const { Bodies, Body, Composite } = Matter
          if (f === FLOOR_DROP) Composite.remove(world, c.floor)
          for (const w of words) {
            if (w.at !== f) continue
            const b = Bodies.rectangle(ICON.x, ICON.y, w.w, w.h, { chamfer: { radius: 6 }, friction: 0.5, frictionStatic: 0.9, restitution: 0.12, density: 0.002 })
            Body.setVelocity(b, { x: w.vx, y: w.vy })
            Body.setAngularVelocity(b, w.av)
            w.id = b.id
            Composite.add(world, b)
          }
        },
      })
    },
    draw(ctx, f) {
      const g = ctx.createRadialGradient(ICON.x, ICON.y, 0, ICON.x, ICON.y, 760)
      g.addColorStop(0, "#151515")
      g.addColorStop(1, "#060606")
      ctx.fillStyle = g
      ctx.fillRect(0, 0, W, H)

      const rec = sim.frames[f]
      ctx.font = FONT
      ctx.textAlign = "center"
      ctx.textBaseline = "middle"
      ctx.fillStyle = "#ededea"
      for (const w of words) {
        const p = rec.get(w.id)
        if (!p || p[1] > H + 80) continue
        ctx.save()
        ctx.translate(p[0], p[1])
        ctx.rotate(p[2])
        ctx.fillText(w.text, 0, 1)
        ctx.restore()
      }

      // Each word is a small push from the icon: a quick press, then a calm return.
      let push = 0
      for (const s of spawns) {
        const dt = f - s
        if (dt >= 0 && dt < 40) push += (dt / 3) * Math.exp(1 - dt / 3)
      }
      const scale = 1 - 0.03 * Math.min(1, push)
      const size = ICON.size * scale
      ctx.save()
      ctx.shadowColor = "rgba(0,0,0,0.6)"
      ctx.shadowBlur = 40
      ctx.shadowOffsetY = 14
      ctx.drawImage(icon, ICON.x - size / 2, ICON.y - size / 2, size, size)
      ctx.restore()
    },
  })
})()
