// Vessels: the texture every living surface on the page reads (wgsl/veins.wgsl). Trees of them,
// grown once at start: a trunk wanders, forks into a thinner continuation and a thinner side
// branch, and those fork again until they are capillaries. It wraps, so it tiles.
//   red    the vessels themselves
//   green  the hairs off them
//   blue   the light they throw into what they run through
export const VESSELS = 2048
const STEP = 6

function mulberry(seed) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

// Every vessel as a polyline: { pts: [x, y, …], w (texels), hair }.
export function vesselPaths(size = VESSELS, seed = 11) {
  const rnd = mulberry(seed)
  const paths = []
  const TRUNK = 4.6
  // Where vessels already run, coarsely: a new one leans toward the emptier side, so between
  // them they reach everywhere.
  const CELL = 32, GN = size / CELL, grid = new Float32Array(GN * GN)
  const at = (x, y) => (((Math.floor(y / CELL) % GN) + GN) % GN) * GN + ((((Math.floor(x / CELL) % GN) + GN) % GN))
  const crowd = (x, y, a, turn) => {
    let sum = 0
    for (const d of [48, 96, 150]) sum += grid[at(x + Math.cos(a + turn) * d, y + Math.sin(a + turn) * d)]
    return sum
  }
  const hair = (x, y, a, n) => {
    const pts = [x, y]
    let curve = 0
    const steps = 7 + Math.floor(rnd() * 13)
    for (let i = 0; i < steps; i++) {
      curve = curve * 0.84 + (rnd() - 0.5) * 0.16
      a += curve
      x += Math.cos(a) * STEP
      y += Math.sin(a) * STEP
      pts.push(x, y)
      grid[at(x, y)] += 0.3
    }
    paths.push({ pts, w: 0.9, hair: true })
    if (n > 0 && rnd() < 0.6) hair(x, y, a + (rnd() < 0.5 ? 0.8 : -0.8), n - 1)
  }
  const grow = (x, y, a, w) => {
    if (w < 0.85) return
    const steps = Math.round((18 + rnd() * 26) * (0.5 + 0.5 * w / TRUNK))
    const pts = [x, y]
    let curve = (rnd() - 0.5) * 0.05
    for (let i = 0; i < steps; i++) {
      const lean = crowd(x, y, a, 0.7) - crowd(x, y, a, -0.7)
      curve = curve * 0.86 + (rnd() - 0.5) * 0.06 - Math.max(-0.03, Math.min(0.03, lean * 0.012))
      a += curve
      x += Math.cos(a) * STEP
      y += Math.sin(a) * STEP
      pts.push(x, y)
      grid[at(x, y)] += 1
      if (rnd() < 0.045) hair(x, y, a + (rnd() < 0.5 ? 1 : -1) * (0.7 + rnd() * 0.5), 1)
    }
    paths.push({ pts, w, hair: false })
    // It forks: the emptier side gets the stronger branch.
    const side = crowd(x, y, a, 0.8) < crowd(x, y, a, -0.8) ? 1 : -1
    const fork = 0.5 + rnd() * 0.5
    grow(x, y, a + side * fork * 0.3, w * 0.8)
    grow(x, y, a - side * fork, w * 0.6)
  }
  const COLS = 4, ROWS = 3
  for (let n = 0; n < COLS * ROWS; n++) {
    const gx = n % COLS, gy = Math.floor(n / COLS)
    grow(((gx + 0.15 + 0.7 * rnd()) / COLS) * size, ((gy + 0.15 + 0.7 * rnd()) / ROWS) * size, rnd() * Math.PI * 2, TRUNK * (0.8 + 0.4 * rnd()))
  }
  return paths
}

// Drawn with the 2D canvas, which is good at lines.
export function vesselCanvas(size = VESSELS) {
  const canvas = document.createElement('canvas')
  canvas.width = canvas.height = size
  const c = canvas.getContext('2d')
  c.fillStyle = '#000'
  c.fillRect(0, 0, size, size)
  c.globalCompositeOperation = 'lighter'
  c.lineCap = 'round'
  c.lineJoin = 'round'
  const paths = vesselPaths(size)
  const stroke = (p, width, style, margin) => {
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity
    for (let i = 0; i < p.pts.length; i += 2) {
      x0 = Math.min(x0, p.pts[i]); x1 = Math.max(x1, p.pts[i]); y0 = Math.min(y0, p.pts[i + 1]); y1 = Math.max(y1, p.pts[i + 1])
    }
    c.lineWidth = width
    c.strokeStyle = style
    // Wherever it leaves the square it comes back in on the other side.
    for (let oy = Math.floor((y0 - margin) / size) * -size; y1 + oy > -margin; oy -= size) {
      if (y0 + oy > size + margin) continue
      for (let ox = Math.floor((x0 - margin) / size) * -size; x1 + ox > -margin; ox -= size) {
        if (x0 + ox > size + margin) continue
        c.beginPath()
        c.moveTo(p.pts[0] + ox, p.pts[1] + oy)
        for (let i = 2; i < p.pts.length; i += 2) c.lineTo(p.pts[i] + ox, p.pts[i + 1] + oy)
        c.stroke()
      }
    }
  }
  for (const p of paths) {
    if (p.hair) { stroke(p, 1.15, 'rgba(0,255,0,0.85)', 4); continue }
    // Its light: three soft strokes, wider and fainter.
    stroke(p, p.w * 2.0 + 5, 'rgba(0,0,255,0.20)', 40)
    stroke(p, p.w * 3.2 + 12, 'rgba(0,0,255,0.12)', 40)
    stroke(p, p.w * 4.5 + 24, 'rgba(0,0,255,0.07)', 40)
    stroke(p, Math.max(p.w, 1.2), 'rgba(255,0,0,' + Math.min(1, 0.35 + p.w * 0.42).toFixed(3) + ')', 8)
  }
  return canvas
}
