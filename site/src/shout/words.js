// Word atlas in size buckets so every word is drawn near its on-screen size.
// R = sharp ink, G = blurred glow, on black. Dirty rects are uploaded by the renderer.

export const WSIZE = 4096
const S = 2
export const BUCKETS = [24, 36, 54, 80, 120]

export class Words {
  constructor() {
    this.canvas = document.createElement("canvas")
    this.canvas.width = this.canvas.height = WSIZE
    this.ctx = this.canvas.getContext("2d")
    this.dirty = []
    this.version = 0
    this.reset()
  }

  reset() {
    this.ctx.setTransform(1, 0, 0, 1, 0, 0)
    this.ctx.fillStyle = "#000"
    this.ctx.fillRect(0, 0, WSIZE, WSIZE)
    this.cache = new Map()
    this.x = 2
    this.y = 2
    this.row = 0
    this.dirty.push({ x: 0, y: 0, w: WSIZE, h: WSIZE })
    this.version++
  }

  bucket(size) {
    for (const b of BUCKETS) if (b >= size * 0.92) return b
    return BUCKETS[BUCKETS.length - 1]
  }

  // Returns an entry sized for `bucket` px; callers scale the quad by size / bucket.
  get(text, size, weight) {
    const b = this.bucket(size)
    weight = Math.round(weight)
    const key = `${b}|${weight}|${text}`
    let e = this.cache.get(key)
    if (e) return e
    const ctx = this.ctx
    const font = `${weight} ${b}px "Inter Variable"`
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.font = font
    const m = ctx.measureText(text)
    const glow = b * 0.34
    const pad = glow * 1.9
    const inkW = m.actualBoundingBoxLeft + m.actualBoundingBoxRight
    const w = inkW + pad * 2
    const h = b * 1.05 + pad * 2
    const pw = Math.ceil(w * S) + 2
    const ph = Math.ceil(h * S) + 2
    if (this.x + pw > WSIZE) {
      this.x = 2
      this.y += this.row + 2
      this.row = 0
    }
    if (this.y + ph > WSIZE) {
      this.reset()
      return this.get(text, size, weight)
    }
    const px = this.x + 1
    const py = this.y + 1
    this.x += pw + 2
    this.row = Math.max(this.row, ph)
    ctx.save()
    ctx.beginPath()
    ctx.rect(px - 1, py - 1, pw, ph)
    ctx.clip()
    ctx.setTransform(S, 0, 0, S, px, py)
    ctx.globalCompositeOperation = "lighter"
    ctx.font = font
    ctx.textBaseline = "alphabetic"
    const x = pad + m.actualBoundingBoxLeft
    const y = pad + b * 0.8
    ctx.fillStyle = "rgb(255,0,0)"
    ctx.fillText(text, x, y)
    ctx.fillStyle = "rgb(0,170,0)"
    ctx.filter = `blur(${glow * 0.45}px)`
    ctx.fillText(text, x, y)
    ctx.filter = `blur(${glow}px)`
    ctx.fillText(text, x, y)
    ctx.restore()
    this.dirty.push({ x: px - 1, y: py - 1, w: pw, h: ph })
    this.version++
    // ox/oy: the text origin (left edge, alphabetic baseline) relative to the quad center, in
    // bucket px, so a word can be placed exactly where a Canvas fillText would put it.
    e = { text, bucket: b, w, h, ox: x - w / 2, oy: y - h / 2, uv: [px / WSIZE, py / WSIZE, (px + w * S) / WSIZE, (py + h * S) / WSIZE] }
    this.cache.set(key, e)
    return e
  }
}
