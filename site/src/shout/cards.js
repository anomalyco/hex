// Message cards (banner, chat, mail, messages, calendar, review, DM, issue, post, Discord,
// OpenCode), drawn with Canvas 2D into fixed atlas slots. Typing a reply redraws the slot and
// marks it dirty.
import peopleUrl from "./assets/people.webp"

export const SIZE = 4096
export const S = 2 // atlas px per CSS px
export const PAD = 22 // CSS px around each card for its shadow
const SLOT_W = 404 // CSS
const SLOT_H = 186
const COLS = Math.floor(SIZE / (SLOT_W * S))
const ROWS = Math.floor(SIZE / (SLOT_H * S))

const SANS = '"Inter Variable", ui-sans-serif, system-ui'
const MONO = 'ui-monospace, "SF Mono", Menlo, monospace'

// Dax, Jay and Frank are Kit's teammates (their real X avatars); every other sender is invented,
// with a generated portrait. Everything anyone "says" here is invented and meant affectionately.
const COMMUNITY = ["Rin Ota", "Max Boyd", "Ivy Chen", "Noah Park", "Lena Ruiz", "Omar Haddad"]

// people.webp: 96 px cells, 7 columns, in this order.
const SPRITE = ["Dax", "Jay", "Frank", "anomalyco", "kitlangton", "hex", "scrawl", "effect", "hex-ci", "Ana Silva", "Sam Ortiz", "Mo Reyes", "Priya Das", "Lee Park", "Jo Kim", "Rin Ota", "Max Boyd", "Ivy Chen", "Noah Park", "Lena Ruiz", "Omar Haddad"]
const CELL = 96
let people = null
function sprite(ctx, name, x, y, size, radius) {
  const i = SPRITE.indexOf(name)
  if (i < 0 || !people) return false
  ctx.save()
  rr(ctx, x, y, size, size, radius)
  ctx.clip()
  ctx.drawImage(people, (i % 7) * CELL, Math.floor(i / 7) * CELL, CELL, CELL, x, y, size, size)
  ctx.restore()
  return true
}
// The project a repo notification is about.
const repoIcon = (repo) => ({ "anomalyco/hex": "hex", "kitlangton/scrawl": "scrawl" })[repo] ?? repo.split("/")[0]

// Glyphs, 24-unit boxes: Simple Icons (CC0) for brands, Lucide (ISC) for generic apps.
const GLYPH = {
  x: "M14.234 10.162 22.977 0h-2.072l-7.591 8.824L7.251 0H.258l9.168 13.343L.258 24H2.33l8.016-9.318L16.749 24h6.993zm-2.837 3.299-.929-1.329L3.076 1.56h3.182l5.965 8.532.929 1.329 7.754 11.09h-3.182z",
  slack: "M5.042 15.165a2.528 2.528 0 0 1-2.52 2.523A2.528 2.528 0 0 1 0 15.165a2.527 2.527 0 0 1 2.522-2.52h2.52v2.52zM6.313 15.165a2.527 2.527 0 0 1 2.521-2.52 2.527 2.527 0 0 1 2.521 2.52v6.313A2.528 2.528 0 0 1 8.834 24a2.528 2.528 0 0 1-2.521-2.522v-6.313zM8.834 5.042a2.528 2.528 0 0 1-2.521-2.52A2.528 2.528 0 0 1 8.834 0a2.528 2.528 0 0 1 2.521 2.522v2.52H8.834zM8.834 6.313a2.528 2.528 0 0 1 2.521 2.521 2.528 2.528 0 0 1-2.521 2.521H2.522A2.528 2.528 0 0 1 0 8.834a2.528 2.528 0 0 1 2.522-2.521h6.312zM18.956 8.834a2.528 2.528 0 0 1 2.522-2.521A2.528 2.528 0 0 1 24 8.834a2.528 2.528 0 0 1-2.522 2.521h-2.522V8.834zM17.688 8.834a2.528 2.528 0 0 1-2.523 2.521 2.527 2.527 0 0 1-2.52-2.521V2.522A2.527 2.527 0 0 1 15.165 0a2.528 2.528 0 0 1 2.523 2.522v6.312zM15.165 18.956a2.528 2.528 0 0 1 2.523 2.522A2.528 2.528 0 0 1 15.165 24a2.527 2.527 0 0 1-2.52-2.522v-2.522h2.52zM15.165 17.688a2.527 2.527 0 0 1-2.52-2.523 2.526 2.526 0 0 1 2.52-2.52h6.313A2.527 2.527 0 0 1 24 15.165a2.528 2.528 0 0 1-2.522 2.523h-6.313z",
  github: "M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12",
}
const STROKE = {
  mail: "M22 7l-8.991 5.727a2 2 0 0 1-2.009 0L2 7M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
  message: "M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092 10 10 0 1 0-4.777-4.719",
}
const paths = new Map()
const path2d = (d) => {
  let p = paths.get(d)
  if (!p) paths.set(d, (p = new Path2D(d)))
  return p
}
function glyph(ctx, name, x, y, size, color = "#fff") {
  ctx.save()
  ctx.translate(x, y)
  ctx.scale(size / 24, size / 24)
  if (GLYPH[name]) {
    ctx.fillStyle = color
    ctx.fill(path2d(GLYPH[name]))
  } else {
    ctx.strokeStyle = color
    ctx.lineWidth = 2.2
    ctx.lineCap = "round"
    ctx.lineJoin = "round"
    ctx.stroke(path2d(STROKE[name]))
  }
  ctx.restore()
}
// OpenCode's mark, from opencode.ai's favicon (512 box).
function openCodeMark(ctx, x, y, s) {
  const k = s / 512
  rr(ctx, x, y, s, s, s * 0.22)
  ctx.fillStyle = "#131010"
  ctx.fill()
  ctx.fillStyle = "#5a5858"
  ctx.fillRect(x + 192 * k, y + 224 * k, 128 * k, 128 * k)
  ctx.fillStyle = "#fff"
  ctx.beginPath()
  ctx.rect(x + 128 * k, y + 96 * k, 256 * k, 320 * k)
  ctx.rect(x + 192 * k, y + 160 * k, 128 * k, 192 * k)
  ctx.fill("evenodd")
}
const first = (n) => n.split(" ")[0]
const hueOf = (s) => {
  let h = 0
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) >>> 0
  return h % 360
}

export const CONTENT = {
  chat: [
    ["#opencode", "Dax", "can you look at the effect migration PR before 5"],
    ["#opencode", "Dax", "why are there 40 subagents running on my laptop"],
    ["#hex", "Jay", "hex just typed my whole standup into figma"],
    ["#opencode", "Frank", "ship it or i'm shipping it"],
    ["#hex", "Dax", "bumping this 👆"],
    ["#opencode", "Jay", "is the 2.0 changelog done?"],
    ["#hex", "Frank", "people want mouse-button hotkeys. today."],
    ["#opencode", "Dax", "who added another Layer"],
    ["#hex", "Jay", "the issues tab is doing numbers"],
    ["#hex", "hex-ci", "release build: 9,001 warnings, 0 errors. shipping"],
  ],
  mail: [
    ["GitHub", "[anomalyco/hex] 37 new issues", "You're receiving this because you're Kit."],
    ["GitHub", "[anomalyco/opencode] Review requested", "Dax requested your review on #9420."],
    ["GitHub", "[kitlangton/scrawl] New discussion", "Can Scrawl record my dreams?"],
    ["Effect", "Effect Days speaker check-in", "Can you send your slides again?"],
  ],
  messages: [
    ["Dax", "did you see the hex issues 😬"],
    ["Jay", "opencode is trending again"],
    ["Jay", "lunch? (it's 4pm)"],
    ["Frank", "your phone is on silent right"],
    ["Frank", "ok one more PR"],
  ],
  calendar: [
    ["OpenCode 2.0 launch sync", "Today · 9:00 – 9:30 AM", "Jay"],
    ["Hex Linux triage", "Tomorrow · 8:00 – 8:30 AM", "Dax"],
    ["Effect office hours", "Thursday · 10:00 – 11:00 AM", "Effect"],
    ["Sync about the sync", "Friday · 4:30 – 5:30 PM", "Frank"],
  ],
  review: [
    ["anomalyco/opencode", "#9420", "effect: migrate the last Promise", "+412", "−3,880", "Dax"],
    ["anomalyco/hex", "#176", "fix: stop transcribing keyboard clacks", "+38", "−6", "Jay"],
    ["kitlangton/scrawl", "#91", "feat: ink that fades faster", "+120", "−44", "Frank"],
    ["anomalyco/opencode", "#9433", "chore: rename Layer to Layer2", "+1", "−1", "Dax"],
  ],
  dm: [["Dax", "you around?"], ["Jay", "quick q"], ["Frank", "got a sec?"], ["Jay", "call?"], ["Dax", "hey"], ["Frank", "???"]],
  issue: [
    ["anomalyco/hex", "#171", "Hold-to-dictate stops when I sneeze", "bug"],
    ["anomalyco/hex", "#172", "Transcribes “Effect” as “a fact” every time", "bug"],
    ["anomalyco/hex", "#173", "Meeting offer pops up during my standup", "bug"],
    ["anomalyco/hex", "#174", "Wayland HUD shows on the wrong monitor", "linux"],
    ["anomalyco/hex", "#175", "Feature request: dictate in Klingon", "enhancement"],
    ["anomalyco/opencode", "#9412", "Subagent spawned a subagent that spawned a subagent", "bug"],
    ["anomalyco/opencode", "#9415", "Too much Effect in my stack trace", "question"],
    ["kitlangton/scrawl", "#88", "Red ink won't come off my screen", "bug"],
    ["kitlangton/ghui", "#203", "ghui opened 300 tabs of ghui", "bug"],
    ["kitlangton/visual-effect", "#61", "Animations too delightful, can't work", "bug"],
  ],
  tweet: [
    ["Ana Silva", "@anadev", "@kitlangton hex pasted my dictation into the wrong window lol"],
    ["Sam Ortiz", "@samships", "@kitlangton when mouse-button hotkeys"],
    ["Mo Reyes", "@mo_types", "@kitlangton over 9,000x developer and the issue is still open?"],
    ["Priya Das", "@priyacodes", "@kitlangton effect is just promises with extra steps"],
    ["Lee Park", "@leewrites", "@kitlangton hex heard me sigh and typed “sigh”"],
    ["Jo Kim", "@jo_dev", "@kitlangton is it opencode or hex that's yelling at me"],
  ],
  discord: [
    ["#help", "Rin Ota", "is there a ZIO.foreachPar in effect?"],
    ["#beginners", "Max Boyd", "what's a Layer and why is it angry"],
    ["#help", "Ivy Chen", "effect.institute chapter 7 when?"],
    ["#help", "Noah Park", "Effect.gen or pipe? fight"],
    ["#beginners", "Lena Ruiz", "my fiber has been running since tuesday"],
  ],
}

// Complaints arrive more often than everything else.
export const KINDS = ["issue", "issue", "tweet", "tweet", "chat", "dm", "discord", "review", "messages", "mail", "calendar", "issue"]
export const FIXABLE = new Set(["issue", "tweet", "review"])

// Card geometry in CSS px (content box, excluding PAD). field = reply area.
const LAYOUT = {
  banner: { w: 340, h: 112 },
  chat: { w: 360, h: 124 },
  mail: { w: 360, h: 142 },
  messages: { w: 300, h: 128 },
  calendar: { w: 330, h: 126 },
  review: { w: 360, h: 132 },
  dm: { w: 300, h: 104 },
  issue: { w: 360, h: 128 },
  tweet: { w: 340, h: 132 },
  discord: { w: 340, h: 118 },
  opencode: { w: 360, h: 200 },
  keycap: { w: 124, h: 102 },
}

function rr(ctx, x, y, w, h, r) {
  ctx.beginPath()
  ctx.roundRect(x, y, w, h, r)
}

function panel(ctx, w, h, r = 14) {
  ctx.save()
  ctx.shadowColor = "rgba(0,0,0,0.6)"
  ctx.shadowBlur = 26
  ctx.shadowOffsetY = 10
  rr(ctx, 0, 0, w, h, r)
  ctx.fillStyle = "#18181b"
  ctx.fill()
  ctx.restore()
  const g = ctx.createLinearGradient(0, 0, 0, h)
  g.addColorStop(0, "#1f1f23")
  g.addColorStop(1, "#161619")
  rr(ctx, 0, 0, w, h, r)
  ctx.fillStyle = g
  ctx.fill()
  rr(ctx, 0.5, 0.5, w - 1, h - 1, r - 0.5)
  ctx.strokeStyle = "rgba(255,255,255,0.085)"
  ctx.lineWidth = 1
  ctx.stroke()
  ctx.beginPath()
  ctx.moveTo(r, 1)
  ctx.lineTo(w - r, 1)
  ctx.strokeStyle = "rgba(255,255,255,0.07)"
  ctx.stroke()
}

function avatar(ctx, name, x, y, size, round = false) {
  if (sprite(ctx, name, x, y, size, round ? size / 2 : size * 0.24)) return
  const h = hueOf(name)
  const g = ctx.createLinearGradient(x, y, x + size, y + size)
  g.addColorStop(0, `hsl(${h} 62% 58%)`)
  g.addColorStop(1, `hsl(${(h + 40) % 360} 58% 38%)`)
  if (round) {
    ctx.beginPath()
    ctx.arc(x + size / 2, y + size / 2, size / 2, 0, Math.PI * 2)
  } else rr(ctx, x, y, size, size, size * 0.24)
  ctx.fillStyle = g
  ctx.fill()
  const parts = name.split(" ")
  ctx.fillStyle = "rgba(255,255,255,0.95)"
  ctx.font = `600 ${Math.round(size * 0.38)}px ${SANS}`
  ctx.textAlign = "center"
  ctx.textBaseline = "middle"
  ctx.fillText((parts[0][0] + (parts[1]?.[0] ?? "")).toUpperCase(), x + size / 2, y + size / 2 + 0.5)
  ctx.textAlign = "left"
}

// The app (or project) a notification comes from: a rounded tile at x, y of size s.
function appIcon(ctx, card, x, y, s) {
  const kind = card.kind
  const tile = (fill) => {
    rr(ctx, x, y, s, s, s * 0.26)
    ctx.fillStyle = fill
    ctx.fill()
  }
  if ((kind === "issue" || kind === "review") && sprite(ctx, repoIcon(card.repo), x, y, s, s * 0.26)) return
  if (kind === "discord" && sprite(ctx, "effect", x, y, s, s * 0.26)) return
  if (kind === "opencode") return openCodeMark(ctx, x, y, s)
  if (kind === "chat" || kind === "dm") {
    tile("#4a154b")
    return glyph(ctx, "slack", x + s * 0.2, y + s * 0.2, s * 0.6)
  }
  if (kind === "tweet") {
    tile("#000")
    rr(ctx, x + 0.5, y + 0.5, s - 1, s - 1, s * 0.26)
    ctx.strokeStyle = "rgba(255,255,255,0.16)"
    ctx.lineWidth = 1
    ctx.stroke()
    return glyph(ctx, "x", x + s * 0.24, y + s * 0.24, s * 0.52)
  }
  if (kind === "mail" || kind === "messages") {
    const g = ctx.createLinearGradient(x, y, x, y + s)
    g.addColorStop(0, kind === "mail" ? "#4aa1ff" : "#5fe07a")
    g.addColorStop(1, kind === "mail" ? "#1a6ff0" : "#22b14c")
    tile(g)
    return glyph(ctx, kind === "mail" ? "mail" : "message", x + s * 0.2, y + s * 0.2, s * 0.6)
  }
  // Calendar (and anything else): a white page with a red header.
  tile("#fff")
  ctx.fillStyle = "#e5483b"
  ctx.save()
  rr(ctx, x, y, s, s, s * 0.26)
  ctx.clip()
  ctx.fillRect(x, y, s, s * 0.3)
  ctx.restore()
  ctx.fillStyle = "#1a1a1a"
  ctx.font = `700 ${Math.round(s * 0.46)}px ${SANS}`
  ctx.textAlign = "center"
  ctx.textBaseline = "middle"
  ctx.fillText("19", x + s / 2, y + s * 0.64)
  ctx.textAlign = "left"
}

function badge(ctx, n, x, y) {
  const t = n > 99 ? "99+" : String(n)
  ctx.font = `650 10.5px ${SANS}`
  const w = Math.max(17, ctx.measureText(t).width + 9)
  rr(ctx, x - w / 2, y - 8.5, w, 17, 8.5)
  ctx.fillStyle = "#ff453a"
  ctx.fill()
  ctx.strokeStyle = "#18181b"
  ctx.lineWidth = 2
  ctx.stroke()
  ctx.fillStyle = "#fff"
  ctx.textAlign = "center"
  ctx.textBaseline = "middle"
  ctx.fillText(t, x, y + 0.5)
  ctx.textAlign = "left"
}

function ellipsize(ctx, text, max) {
  if (ctx.measureText(text).width <= max) return text
  let t = text
  while (t.length > 1 && ctx.measureText(t + "…").width > max) t = t.slice(0, -1)
  return t + "…"
}

function wrap(ctx, text, max, lines) {
  const words = text.split(" ")
  const out = []
  let line = ""
  for (const w of words) {
    const next = line ? line + " " + w : w
    if (ctx.measureText(next).width > max && line) {
      out.push(line)
      line = w
    } else line = next
  }
  if (line) out.push(line)
  if (out.length > lines) {
    out.length = lines
    out[lines - 1] = ellipsize(ctx, out[lines - 1] + " …", max)
  }
  return out
}

function header(ctx, card, x, y, label) {
  appIcon(ctx, card, x, y, 18)
  ctx.font = `500 11px ${SANS}`
  ctx.fillStyle = "#8e8e94"
  ctx.textBaseline = "middle"
  ctx.fillText(label, x + 26, y + 9.5)
  const lw = ctx.measureText(label).width
  ctx.fillStyle = "#5e5e64"
  ctx.fillText(` · ${card.when}`, x + 26 + lw, y + 9.5)
  if (card.badge > 1) badge(ctx, card.badge, x + 17, y + 1)
}

// Reply field: placeholder or typed words, a caret while typing. Text shrinks to fit.
function field(ctx, card, x, y, w, h, placeholder, opts = {}) {
  const active = card.typing || card.typed
  rr(ctx, x, y, w, h, opts.radius ?? 8)
  ctx.fillStyle = active ? "rgba(255,255,255,0.06)" : "rgba(255,255,255,0.035)"
  ctx.fill()
  ctx.strokeStyle = card.typing ? `rgba(255,${Math.round(255 - 150 * card.heat)},${Math.round(255 - 190 * card.heat)},0.35)` : "rgba(255,255,255,0.07)"
  ctx.lineWidth = 1
  ctx.stroke()
  const tx = x + 10
  const maxW = w - 20 - (opts.send ? 46 : 0)
  card.fieldGeom = { tx, y, h, maxW }
  ctx.textBaseline = "middle"
  let caretX = tx
  if (card.reply) {
    ctx.save()
    ctx.beginPath()
    ctx.rect(x + 2, y, maxW + 10, h)
    ctx.clip()
    drawReply(ctx, card, card.loud ? FINAL_LOUD : FINAL)
    ctx.restore()
  } else if (!card.typed) {
    ctx.font = `400 12.5px ${SANS}`
    ctx.fillStyle = "#5c5c62"
    ctx.fillText(placeholder, tx, y + h / 2 + 0.5)
  } else {
    const loud = card.loud
    let size = loud ? 14 : 12.5
    const weight = loud ? 750 : 450
    ctx.font = `${weight} ${size}px ${SANS}`
    while (ctx.measureText(card.typed).width > maxW && size > 8.5) {
      size -= 0.5
      ctx.font = `${weight} ${size}px ${SANS}`
    }
    const hot = card.heat
    ctx.fillStyle = `rgb(${Math.round(237 + 18 * hot)},${Math.round(237 - 120 * hot)},${Math.round(237 - 160 * hot)})`
    ctx.save()
    ctx.beginPath()
    ctx.rect(x + 2, y, maxW + 10, h)
    ctx.clip()
    ctx.fillText(card.typed, tx, y + h / 2 + 0.5)
    ctx.restore()
    caretX = tx + Math.min(maxW, ctx.measureText(card.typed).width) + 2
  }
  if (card.typing && !card.reply) {
    ctx.fillStyle = hotCaret(card.heat)
    ctx.fillRect(caretX, y + h / 2 - 7, 1.5, 14)
  }
  if (opts.send) {
    rr(ctx, x + w - 46, y + 5, 40, h - 10, 6)
    ctx.fillStyle = card.typed ? (card.heat > 0.5 ? "#e5483b" : "#3b7cf0") : "rgba(255,255,255,0.06)"
    ctx.fill()
    ctx.fillStyle = card.typed ? "#fff" : "#6a6a70"
    ctx.font = `600 11px ${SANS}`
    ctx.textAlign = "center"
    ctx.fillText("Send", x + w - 26, y + h / 2 + 0.5)
    ctx.textAlign = "left"
  }
  if (!card.reply) card.caret = { x: caretX + 4, y: y + h / 2 }
}
const hotCaret = (h) => (h > 0.5 ? "#ff7a5a" : "#7aa7ff")

// ─── Typeset replies ───────────────────────────────────────────────────────────
// A reply is laid out once, before its first word flies (planReply), so every flying word aims at
// its final slot and nothing reflows. Landed words are drawn here from `card.lw[i]`, which main.js
// animates: `fade` (0→1 handoff from the flying word) and `heat` (1 hot → 0 cooled).
const FINAL = [237, 237, 237]
const FINAL_LOUD = [255, 226, 212]
const FINAL_BUBBLE = [255, 255, 255]
const HOT = [255, 247, 226]
function drawReply(ctx, c, final) {
  const r = c.reply
  ctx.font = r.font
  ctx.textBaseline = "alphabetic"
  for (let i = 0; i < r.words.length; i++) {
    const st = c.lw[i]
    if (!st || st.fade <= 0) continue
    const word = r.words[i]
    const h = st.heat
    const col = final.map((v, k) => Math.round(v + (HOT[k] - v) * h))
    ctx.globalAlpha = Math.min(1, st.fade)
    ctx.fillStyle = `rgb(${col[0]},${col[1]},${col[2]})`
    if (h > 0.02) {
      // Ember glow that decays with the heat, with a faint shimmer.
      const flick = 0.85 + 0.15 * Math.sin((c.now ?? 0) * 37 + i * 1.7)
      ctx.shadowColor = `rgba(255,${Math.round(120 + 90 * (1 - h))},60,${(0.9 * h * flick).toFixed(3)})`
      ctx.shadowBlur = 1.5 + 9 * h
    }
    ctx.fillText(word.text, word.x, word.y)
    ctx.shadowBlur = 0
    ctx.shadowColor = "transparent"
  }
  ctx.globalAlpha = 1
  if (c.typing) {
    ctx.fillStyle = r.loud ? "#ff9a7a" : "#9dbcff"
    ctx.fillRect(c.caretX, c.caretY - r.size * 0.82, 1.5, r.size * 1.1)
  }
  c.caret = { x: c.caretX + 4, y: c.caretY - r.size * 0.35 }
}

// Words placed left to right with the real font; `lines` of word indices.
function placeLine(ctx, texts, idx, x, y, out) {
  let prefix = ""
  for (const i of idx) {
    out[i] = { text: texts[i], x: x + (prefix ? ctx.measureText(prefix + " ").width : 0), y, w: ctx.measureText(texts[i]).width }
    prefix = prefix ? `${prefix} ${texts[i]}` : texts[i]
  }
}

const DRAW = {
  banner(ctx, c, w, h) {
    panel(ctx, w, h, 16)
    header(ctx, c, 14, 12, c.app)
    ctx.font = `600 13px ${SANS}`
    ctx.fillStyle = "#ededed"
    ctx.textBaseline = "alphabetic"
    ctx.fillText(ellipsize(ctx, c.title, w - 28), 14, 52)
    ctx.font = `400 12.5px ${SANS}`
    ctx.fillStyle = "#a0a0a6"
    ctx.fillText(ellipsize(ctx, c.body, w - 28), 14, 69)
    field(ctx, c, 12, h - 36, w - 24, 26, "Reply")
  },
  chat(ctx, c, w, h) {
    panel(ctx, w, h)
    header(ctx, c, 14, 12, c.channel)
    avatar(ctx, c.from, 14, 40, 30)
    ctx.textBaseline = "alphabetic"
    ctx.font = `650 13px ${SANS}`
    ctx.fillStyle = "#ededed"
    ctx.fillText(first(c.from), 54, 52)
    const nw = ctx.measureText(first(c.from)).width
    ctx.font = `400 11px ${SANS}`
    ctx.fillStyle = "#6a6a70"
    ctx.fillText(c.time, 60 + nw, 52)
    ctx.font = `400 12.5px ${SANS}`
    ctx.fillStyle = "#c4c4c9"
    ctx.fillText(ellipsize(ctx, c.body, w - 70), 54, 69)
    field(ctx, c, 12, h - 36, w - 24, 26, "Reply in thread…")
  },
  dm(ctx, c, w, h) {
    panel(ctx, w, h)
    avatar(ctx, c.from, 14, 14, 30, true)
    ctx.fillStyle = "#3ec37a"
    ctx.beginPath()
    ctx.arc(39, 39, 4.5, 0, Math.PI * 2)
    ctx.fill()
    ctx.strokeStyle = "#1c1c20"
    ctx.lineWidth = 2
    ctx.stroke()
    ctx.textBaseline = "alphabetic"
    ctx.font = `650 13px ${SANS}`
    ctx.fillStyle = "#ededed"
    ctx.fillText(c.from, 54, 26)
    ctx.font = `400 12.5px ${SANS}`
    ctx.fillStyle = "#c4c4c9"
    ctx.fillText(c.body, 54, 43)
    ctx.fillStyle = "#4c8dff"
    ctx.beginPath()
    ctx.arc(w - 16, 22, 4, 0, Math.PI * 2)
    ctx.fill()
    if (c.badge > 1) badge(ctx, c.badge, w - 34, 22)
    field(ctx, c, 12, h - 36, w - 24, 26, `Message ${first(c.from)}`)
  },
  mail(ctx, c, w, h) {
    panel(ctx, w, h)
    header(ctx, c, 14, 12, "Mail")
    ctx.textBaseline = "alphabetic"
    ctx.font = `600 13px ${SANS}`
    ctx.fillStyle = "#ededed"
    ctx.fillText(ellipsize(ctx, c.title, w - 40), 14, 52)
    ctx.fillStyle = "#4c8dff"
    ctx.beginPath()
    ctx.arc(w - 16, 48, 4, 0, Math.PI * 2)
    ctx.fill()
    if (c.from === "GitHub") glyph(ctx, "github", 14, 58.5, 12, "#c4c4c9")
    else sprite(ctx, "effect", 14, 58.5, 12, 3)
    ctx.font = `400 11.5px ${SANS}`
    ctx.fillStyle = "#7c7c82"
    ctx.fillText(c.from === "GitHub" ? "GitHub <notifications@github.com>" : `${c.from} <hello@effect.website>`, 31, 68)
    ctx.font = `400 12.5px ${SANS}`
    ctx.fillStyle = "#a8a8ae"
    ctx.fillText(ellipsize(ctx, c.body, w - 28), 14, 86)
    field(ctx, c, 12, h - 40, w - 24, 30, "Write a reply…", { send: true })
  },
  messages(ctx, c, w, h) {
    panel(ctx, w, h, 18)
    ctx.textBaseline = "alphabetic"
    avatar(ctx, c.from, 14, 9, 16, true)
    ctx.font = `500 11px ${SANS}`
    ctx.fillStyle = "#7c7c82"
    ctx.fillText(first(c.from), 36, 22)
    appIcon(ctx, c, w - 30, 8, 16)
    ctx.font = `400 13px ${SANS}`
    const tw = Math.min(w - 60, ctx.measureText(c.body).width)
    rr(ctx, 12, 30, tw + 24, 30, 15)
    ctx.fillStyle = "#2c2c31"
    ctx.fill()
    ctx.fillStyle = "#ededed"
    ctx.fillText(ellipsize(ctx, c.body, w - 60), 24, 50)
    // Reply bubble: sized for the whole planned reply up front, so words land without reflow.
    ctx.font = `${c.loud ? 750 : 450} 13px ${SANS}`
    const typed = c.typed || ""
    const bw = c.reply ? c.reply.bubbleW : Math.min(w - 40, ctx.measureText(typed || "Message").width + 26)
    const bx = w - 12 - bw
    rr(ctx, bx, h - 46, bw, 32, 16)
    if (typed) {
      const g = ctx.createLinearGradient(0, h - 46, 0, h - 14)
      g.addColorStop(0, c.heat > 0.5 ? "#f2553e" : "#3f86f5")
      g.addColorStop(1, c.heat > 0.5 ? "#c9301f" : "#2b6ee0")
      ctx.fillStyle = g
    } else ctx.fillStyle = "rgba(255,255,255,0.05)"
    ctx.fill()
    if (c.reply) {
      drawReply(ctx, c, FINAL_BUBBLE)
      return
    }
    ctx.save()
    ctx.beginPath()
    ctx.rect(bx + 6, h - 46, bw - 12, 32)
    ctx.clip()
    ctx.fillStyle = typed ? "#fff" : "#5c5c62"
    ctx.fillText(typed || "Message", bx + 13, h - 25)
    ctx.restore()
    const cx = typed ? bx + 13 + Math.min(bw - 26, ctx.measureText(typed).width) + 3 : bx + 13
    if (c.typing) {
      ctx.fillStyle = "#fff"
      ctx.fillRect(cx, h - 37, 1.5, 14)
    }
    c.caret = { x: cx + 4, y: h - 30 }
  },
  calendar(ctx, c, w, h) {
    panel(ctx, w, h)
    rr(ctx, 12, 12, 4, 46, 2)
    ctx.fillStyle = "#e5483b"
    ctx.fill()
    ctx.textBaseline = "alphabetic"
    ctx.font = `600 13px ${SANS}`
    ctx.fillStyle = "#ededed"
    ctx.fillText(ellipsize(ctx, c.title, w - 40), 26, 28)
    ctx.font = `400 11.5px ${SANS}`
    ctx.fillStyle = "#8e8e94"
    ctx.fillText(c.body, 26, 45)
    ctx.fillStyle = "#6a6a70"
    if (c.from === "Effect") sprite(ctx, "effect", 26, 50.5, 12, 3)
    else avatar(ctx, c.from, 26, 50.5, 12, true)
    ctx.fillText(c.from === "Effect" ? "Effect community" : `${c.from} invited you`, 43, 60)
    appIcon(ctx, c, w - 32, 12, 18)
    field(ctx, c, 12, h - 38, w - 132, 28, "Add a note")
    for (const [i, t] of ["Decline", "Accept"].entries()) {
      const bx = w - 114 + i * 54
      rr(ctx, bx, h - 38, 50, 28, 7)
      ctx.fillStyle = i ? "rgba(76,141,255,0.18)" : "rgba(255,255,255,0.05)"
      ctx.fill()
      ctx.fillStyle = i ? "#8fb6ff" : "#a0a0a6"
      ctx.font = `600 11px ${SANS}`
      ctx.textAlign = "center"
      ctx.textBaseline = "middle"
      ctx.fillText(t, bx + 25, h - 23.5)
      ctx.textAlign = "left"
    }
  },
  review(ctx, c, w, h) {
    panel(ctx, w, h)
    header(ctx, c, 14, 12, `${c.repo} · review requested`)
    avatar(ctx, c.from, 14, 40, 22, true)
    ctx.textBaseline = "alphabetic"
    ctx.font = `500 12.5px ${MONO}`
    ctx.fillStyle = "#8e8e94"
    ctx.fillText(c.num, 44, 56)
    const nw = ctx.measureText(c.num + " ").width
    ctx.font = `600 13px ${SANS}`
    ctx.fillStyle = "#ededed"
    ctx.fillText(ellipsize(ctx, c.title, w - 60 - nw), 44 + nw, 56)
    ctx.font = `500 11.5px ${MONO}`
    ctx.fillStyle = "#3ec37a"
    ctx.fillText(c.add, 44, 76)
    const aw = ctx.measureText(c.add + " ").width
    ctx.fillStyle = "#f2553e"
    ctx.fillText(c.del, 44 + aw, 76)
    const dw = ctx.measureText(c.del).width
    for (let i = 0; i < 5; i++) {
      ctx.fillStyle = i < 1 ? "#3ec37a" : i < 4 ? "#f2553e" : "#3a3a40"
      ctx.fillRect(52 + aw + dw + i * 9, 69, 7, 7)
    }
    field(ctx, c, 12, h - 38, w - 24, 28, "Leave a comment")
  },
}

Object.assign(DRAW, {
  issue(ctx, c, w, h) {
    panel(ctx, w, h)
    header(ctx, c, 14, 12, c.repo)
    ctx.textBaseline = "alphabetic"
    ctx.font = `500 12.5px ${MONO}`
    ctx.fillStyle = "#8e8e94"
    ctx.fillText(c.num, 14, 52)
    const nw = ctx.measureText(c.num + " ").width
    ctx.font = `600 13px ${SANS}`
    ctx.fillStyle = "#ededed"
    ctx.fillText(ellipsize(ctx, c.title, w - 28 - nw), 14 + nw, 52)
    ctx.font = `500 10.5px ${SANS}`
    const lw = ctx.measureText(c.label).width + 14
    const hue = { bug: 4, linux: 45, enhancement: 200, question: 280 }[c.label] ?? 0
    rr(ctx, 14, 61, lw, 17, 8.5)
    ctx.fillStyle = `hsl(${hue} 70% 55% / 0.18)`
    ctx.fill()
    ctx.fillStyle = `hsl(${hue} 80% 72%)`
    ctx.fillText(c.label, 21, 73.5)
    ctx.fillStyle = "#6a6a70"
    ctx.font = `400 11px ${SANS}`
    ctx.fillText(`opened by @${c.author}`, 22 + lw, 73.5)
    field(ctx, c, 12, h - 38, w - 24, 28, "Leave a comment")
  },
  tweet(ctx, c, w, h) {
    panel(ctx, w, h, 16)
    avatar(ctx, c.from, 14, 14, 30, true)
    ctx.textBaseline = "alphabetic"
    ctx.font = `650 13px ${SANS}`
    ctx.fillStyle = "#ededed"
    ctx.fillText(c.from, 54, 26)
    const nw = ctx.measureText(c.from + " ").width
    ctx.font = `400 12px ${SANS}`
    ctx.fillStyle = "#6a6a70"
    ctx.fillText(`${c.handle} · ${c.when}`, 54 + nw, 26)
    glyph(ctx, "x", w - 28, 14, 13, "#d8d8dc")
    ctx.font = `400 12.5px ${SANS}`
    const lines = wrap(ctx, c.body, w - 70, 2)
    lines.forEach((l, i) => {
      const at = l.indexOf("@kitlangton")
      ctx.fillStyle = "#d8d8dc"
      if (at === 0) {
        ctx.fillStyle = "#4c9dff"
        ctx.fillText("@kitlangton", 54, 44 + i * 16)
        const mw = ctx.measureText("@kitlangton").width
        ctx.fillStyle = "#d8d8dc"
        ctx.fillText(l.slice(11), 54 + mw, 44 + i * 16)
      } else ctx.fillText(l, 54, 44 + i * 16)
    })
    field(ctx, c, 12, h - 38, w - 24, 28, "Post your reply", { radius: 14 })
  },
  discord(ctx, c, w, h) {
    panel(ctx, w, h, 12)
    header(ctx, c, 14, 12, `Effect · ${c.channel}`)
    avatar(ctx, c.from, 14, 40, 26, true)
    ctx.textBaseline = "alphabetic"
    ctx.font = `600 12.5px ${SANS}`
    ctx.fillStyle = `hsl(${hueOf(c.from)} 70% 72%)`
    ctx.fillText(first(c.from), 48, 51)
    ctx.font = `400 12.5px ${SANS}`
    ctx.fillStyle = "#c4c4c9"
    ctx.fillText(ellipsize(ctx, c.body, w - 62), 48, 67)
    field(ctx, c, 12, h - 36, w - 24, 26, `Message ${c.channel}`)
  },
  // OpenCode desktop composer: window header, subagent pill, prompt box, model row, send.
  opencode(ctx, c, w, h) {
    ctx.save()
    ctx.shadowColor = "rgba(0,0,0,0.7)"
    ctx.shadowBlur = 30
    ctx.shadowOffsetY = 12
    rr(ctx, 0, 0, w, h, 16)
    ctx.fillStyle = "#0c0c0e"
    ctx.fill()
    ctx.restore()
    rr(ctx, 0.5, 0.5, w - 1, h - 1, 15.5)
    ctx.strokeStyle = c.glow > 0 ? `rgba(80,220,140,${0.12 + 0.5 * c.glow})` : "rgba(255,255,255,0.08)"
    ctx.lineWidth = 1
    ctx.stroke()
    // header
    const lx = 14
    openCodeMark(ctx, lx, 9, 18)
    ctx.textBaseline = "middle"
    ctx.font = `600 12px ${SANS}`
    ctx.fillStyle = "#ededed"
    ctx.fillText("OpenCode", lx + 24, 18.5)
    const ow = ctx.measureText("OpenCode").width
    ctx.font = `400 11px ${MONO}`
    ctx.fillStyle = "#5e5e64"
    ctx.fillText("~/code/hex", lx + 32 + ow, 18.5)
    // subagent pill
    const n = c.agents ?? 0
    const pill = `${n.toLocaleString("en-US")} ${n === 1 ? "subagent" : "subagents"}`
    ctx.font = `500 11px ${SANS}`
    const pw = ctx.measureText(pill).width + 34
    rr(ctx, 12, 36, pw, 22, 11)
    ctx.fillStyle = c.agentFlash > 0 ? `rgba(80,220,140,${0.1 + 0.25 * c.agentFlash})` : "#1b1b1e"
    ctx.fill()
    ctx.strokeStyle = "rgba(255,255,255,0.08)"
    ctx.stroke()
    ctx.fillStyle = c.agentFlash > 0 ? "#7fe0a8" : "#9a9aa0"
    for (let i = 0; i < 4; i++) ctx.fillRect(21 + (i % 2) * 5, 42 + Math.floor(i / 2) * 5, 3.5, 3.5)
    ctx.fillStyle = c.agentFlash > 0 ? "#c9f5da" : "#c4c4c9"
    ctx.fillText(pill, 36, 47.5)
    // composer
    const cy = 66
    const ch = h - cy - 10
    rr(ctx, 10, cy, w - 20, ch, 14)
    ctx.fillStyle = "#1a1a1d"
    ctx.fill()
    ctx.strokeStyle = c.typing ? "rgba(255,255,255,0.18)" : "rgba(255,255,255,0.07)"
    ctx.stroke()
    const tx = 24
    const maxW = w - 48
    ctx.textBaseline = "alphabetic"
    let caret = { x: tx, y: cy + 22 }
    if (c.reply) {
      drawReply(ctx, c, c.loud ? FINAL_LOUD : FINAL)
    } else if (c.typed) {
      const loud = c.loud
      const weight = loud ? 700 : 450
      let size = 13.5
      let lines
      for (;;) {
        ctx.font = `${weight} ${size}px ${SANS}`
        lines = wrap(ctx, c.typed, maxW, 9)
        if (lines.length <= 3 || size <= 10) break
        size -= 0.5
      }
      lines = lines.slice(-3)
      const lh = size * 1.35
      ctx.fillStyle = loud ? "#ffd2c4" : "#ededed"
      lines.forEach((l, i) => ctx.fillText(l, tx, cy + 26 + i * lh))
      const last = lines[lines.length - 1]
      caret = { x: tx + ctx.measureText(last).width + 2, y: cy + 22 + (lines.length - 1) * lh }
    } else if (c.status) {
      ctx.font = `500 13px ${SANS}`
      ctx.fillStyle = c.statusDone ? "#7fe0a8" : "#9a9aa0"
      ctx.fillText(c.statusDone ? `✓ ${c.status}` : c.status, tx, cy + 26)
    } else {
      ctx.font = `400 13px ${SANS}`
      ctx.fillStyle = "#5c5c62"
      ctx.fillText("Ask anything…", tx, cy + 26)
    }
    if (c.typing && !c.reply) {
      ctx.fillStyle = "#ededed"
      ctx.fillRect(caret.x, caret.y - 10, 1.5, 15)
    }
    if (!c.reply) c.caret = { x: caret.x + 4, y: caret.y - 3 }
    // bottom row: left cluster (+, model, effort) flows from the left, the shortcut hint hangs off the
    // send button, and whatever does not fit is dropped (the hint first, then the provider prefix)
    // instead of overlapping.
    const by = cy + ch - 20
    const sx = w - 34
    const gap = 10
    const chevron = (x, color) => {
      ctx.beginPath()
      ctx.moveTo(x - 3, by - 1.5)
      ctx.lineTo(x, by + 1.5)
      ctx.lineTo(x + 3, by - 1.5)
      ctx.strokeStyle = color
      ctx.lineWidth = 1.3
      ctx.lineCap = "round"
      ctx.lineJoin = "round"
      ctx.stroke()
    }
    ctx.textBaseline = "middle"
    ctx.textAlign = "left"
    ctx.font = `400 15px ${SANS}`
    ctx.fillStyle = "#8e8e94"
    ctx.fillText("+", 24, by)
    ctx.font = `500 11px ${SANS}`
    const textW = (t) => ctx.measureText(t).width
    const chevW = 6
    const hint = "Queue ⌘↵"
    ctx.font = `500 10.5px ${SANS}`
    const hintW = textW(hint)
    ctx.font = `500 11px ${SANS}`
    const effortW = textW("High") + 5 + chevW
    const leftStart = 42
    const rightEdge = sx - 12 - 8
    let showHint = true
    let model = "Anthropic / Claude Opus 5.5"
    const fits = () => leftStart + textW(model) + 5 + chevW + gap + effortW <= (showHint ? rightEdge - hintW - gap : rightEdge)
    if (!fits()) model = "Claude Opus 5.5"
    if (!fits()) showHint = false
    if (!fits()) model = ellipsize(ctx, model, Math.max(40, rightEdge - leftStart - 5 - chevW - gap - effortW))
    ctx.fillStyle = "#8e8e94"
    ctx.fillText(model, leftStart, by)
    let x = leftStart + textW(model) + 5
    chevron(x + chevW / 2, "#8e8e94")
    x += chevW + gap
    ctx.fillStyle = "#6a6a70"
    ctx.fillText("High", x, by)
    chevron(x + textW("High") + 5 + chevW / 2, "#6a6a70")
    if (showHint) {
      ctx.font = `500 10.5px ${SANS}`
      ctx.fillStyle = "#6a6a70"
      ctx.textAlign = "right"
      ctx.fillText(hint, rightEdge, by)
      ctx.textAlign = "left"
    }
    if (c.pulse > 0) {
      ctx.beginPath()
      ctx.arc(sx, by, 13 + 10 * (1 - c.pulse), 0, Math.PI * 2)
      ctx.strokeStyle = `rgba(255,255,255,${0.6 * c.pulse})`
      ctx.lineWidth = 2
      ctx.stroke()
    }
    ctx.beginPath()
    ctx.arc(sx, by, 12 * (1 + 0.15 * (c.pulse ?? 0)), 0, Math.PI * 2)
    ctx.fillStyle = c.typed || c.pulse > 0 ? "#f4f4f5" : "#4a4a50"
    ctx.fill()
    ctx.strokeStyle = "#0c0c0e"
    ctx.lineWidth = 1.8
    ctx.lineCap = "round"
    ctx.lineJoin = "round"
    ctx.beginPath()
    ctx.moveTo(sx, by + 5)
    ctx.lineTo(sx, by - 5)
    ctx.moveTo(sx - 4.5, by - 0.5)
    ctx.lineTo(sx, by - 5)
    ctx.lineTo(sx + 4.5, by - 0.5)
    ctx.stroke()
  },
  // Tactile ⌥ Option keycap rendered into the atlas so it shatters into real Voronoi chunks.
  keycap(ctx, _c, w, h) {
    // 1. Switch well & glowing amber LED underglow
    ctx.save()
    const glow = ctx.createRadialGradient(w * 0.5, h * 0.78, 4, w * 0.5, h * 0.68, w * 0.54)
    glow.addColorStop(0, "rgba(255, 176, 64, 0.92)")
    glow.addColorStop(0.52, "rgba(255, 118, 32, 0.42)")
    glow.addColorStop(1, "rgba(255, 90, 20, 0)")
    ctx.fillStyle = glow
    rr(ctx, 2, 4, w - 4, h - 2, 18)
    ctx.fill()
    ctx.restore()

    rr(ctx, 4, 4, w - 8, h - 4, 18)
    ctx.fillStyle = "#050508"
    ctx.fill()
    ctx.strokeStyle = "rgba(255, 180, 80, 0.28)"
    ctx.lineWidth = 1.2
    ctx.stroke()

    // 2. Anodized outer cap skirt
    const capX = 6
    const capY = 2
    const capW = w - 12
    const capH = h - 8
    const capGrad = ctx.createLinearGradient(0, capY, 0, capY + capH)
    capGrad.addColorStop(0, "#2f333d")
    capGrad.addColorStop(0.38, "#181b22")
    capGrad.addColorStop(0.82, "#0f1116")
    capGrad.addColorStop(1, "#08090c")
    rr(ctx, capX, capY, capW, capH, 16)
    ctx.fillStyle = capGrad
    ctx.fill()
    rr(ctx, capX + 0.6, capY + 0.6, capW - 1.2, capH - 1.2, 15.4)
    ctx.strokeStyle = "rgba(255, 255, 255, 0.28)"
    ctx.lineWidth = 1.2
    ctx.stroke()

    // 3. Sculpted concave dish
    const dX = capX + 7
    const dY = capY + 4
    const dW = capW - 14
    const dH = capH - 16
    const dishGrad = ctx.createLinearGradient(dX, dY, dX + dW * 0.6, dY + dH)
    dishGrad.addColorStop(0, "#242730")
    dishGrad.addColorStop(0.5, "#13151b")
    dishGrad.addColorStop(1, "#191c24")
    rr(ctx, dX, dY, dW, dH, 12)
    ctx.fillStyle = dishGrad
    ctx.fill()
    rr(ctx, dX + 0.6, dY + 0.6, dW - 1.2, dH - 1.2, 11.4)
    ctx.strokeStyle = "rgba(255, 220, 165, 0.24)"
    ctx.lineWidth = 1.1
    ctx.stroke()

    // 4. ⌥ Option glyph at top-right of dish
    ctx.save()
    ctx.translate(dX + dW - 12 - 22, dY + 10)
    ctx.scale(22 / 24, 18 / 18)
    ctx.strokeStyle = "rgba(248, 249, 252, 0.96)"
    ctx.lineWidth = 2.25
    ctx.lineCap = "round"
    ctx.lineJoin = "round"
    ctx.shadowColor = "rgba(255, 196, 110, 0.65)"
    ctx.shadowBlur = 8
    ctx.stroke(path2d("M2.5 3.5h5.2l6.2 11h7.6M14.2 3.5h7.3"))
    ctx.restore()

    // 5. "option" legend at bottom-left of dish
    ctx.save()
    ctx.font = `560 14.5px ${SANS}`
    ctx.fillStyle = "rgba(248, 249, 252, 0.96)"
    ctx.textBaseline = "alphabetic"
    ctx.shadowColor = "rgba(255, 196, 110, 0.55)"
    ctx.shadowBlur = 8
    ctx.fillText("option", dX + 12, dY + dH - 10)
    ctx.restore()
  },
})

// A complaint that OpenCode fixed: green edge and a resolution bar over the reply field.
function resolvedOverlay(ctx, c, w, h) {
  rr(ctx, 0.5, 0.5, w - 1, h - 1, 14)
  ctx.strokeStyle = "rgba(80,220,140,0.75)"
  ctx.lineWidth = 1.5
  ctx.stroke()
  const bh = 28
  rr(ctx, 12, h - bh - 9, w - 24, bh, 8)
  ctx.fillStyle = "#123d27"
  ctx.fill()
  ctx.strokeStyle = "rgba(80,220,140,0.5)"
  ctx.lineWidth = 1
  ctx.stroke()
  ctx.beginPath()
  ctx.arc(30, h - 9 - bh / 2, 8, 0, Math.PI * 2)
  ctx.fillStyle = c.kind === "issue" ? "#8957e5" : "#2fb66b"
  ctx.fill()
  ctx.strokeStyle = "#fff"
  ctx.lineWidth = 1.8
  ctx.lineCap = "round"
  ctx.beginPath()
  ctx.moveTo(26.5, h - 9 - bh / 2)
  ctx.lineTo(29, h - 9 - bh / 2 + 2.6)
  ctx.lineTo(33.8, h - 9 - bh / 2 - 2.8)
  ctx.stroke()
  ctx.textBaseline = "middle"
  ctx.font = `600 12px ${SANS}`
  ctx.fillStyle = "#c9f5da"
  ctx.fillText(c.resolved, 46, h - 9 - bh / 2 + 0.5)
}

export class Cards {
  constructor() {
    this.ready = people
      ? Promise.resolve()
      : new Promise((resolve) => {
          const img = new Image()
          img.decoding = "async"
          img.onload = () => {
            people = img
            resolve()
          }
          img.onerror = () => resolve()
          img.src = peopleUrl
        })
    this.canvas = document.createElement("canvas")
    this.canvas.width = this.canvas.height = SIZE
    this.ctx = this.canvas.getContext("2d")
    this.free = []
    // The OpenCode window is taller than a slot: it owns two stacked slots in the last column.
    this.ocSlots = [(ROWS - 2) * COLS + COLS - 1, (ROWS - 1) * COLS + COLS - 1]
    for (let i = 0; i < COLS * ROWS; i++) if (!this.ocSlots.includes(i)) this.free.push(i)
    this.dirty = []
    this.version = 0
    this.counts = {}
    this.serial = 0
    this.inUse = new Set()
  }

  get capacity() {
    return COLS * ROWS
  }

  // Create a card's content from a random pick; rand() is the scene's seeded RNG.
  // `item` forces specific content (e.g. the issue an OpenCode prompt is about to fix).
  make(kind, rand, item) {
    const pick = (a) => a[Math.floor(rand() * a.length)]
    if (!item && kind !== "opencode") {
      // Avoid showing the same message twice at once.
      for (let k = 0; k < 6 && (!item || this.inUse.has(item)); k++) item = pick(CONTENT[kind])
    }
    item ??= []
    this.counts[kind] = (this.counts[kind] ?? 0) + 1
    const minutes = Math.floor(rand() * 59)
    const c = { kind, badge: this.counts[kind], when: "now", time: `${9 + Math.floor(rand() * 8)}:${String(minutes).padStart(2, "0")}`, typed: "", typing: false, loud: false, heat: 0 }
    if (kind === "chat") Object.assign(c, { channel: item[0], from: item[1], body: item[2], layout: "chat" })
    else if (kind === "mail") Object.assign(c, { from: item[0], title: item[1], body: item[2], layout: rand() < 0.35 ? "banner" : "mail", app: "Mail" })
    else if (kind === "messages") Object.assign(c, { from: item[0], body: item[1], layout: "messages" })
    else if (kind === "calendar") Object.assign(c, { title: item[0], body: item[1], from: item[2], layout: "calendar" })
    else if (kind === "review") Object.assign(c, { repo: item[0], num: item[1], title: item[2], add: item[3], del: item[4], from: item[5], layout: "review" })
    else if (kind === "issue") Object.assign(c, { repo: item[0], num: item[1], title: item[2], label: item[3], author: item[4] ?? first(pick(COMMUNITY)).toLowerCase() + Math.floor(rand() * 90 + 10), from: "x", layout: "issue" })
    else if (kind === "tweet") Object.assign(c, { from: item[0], handle: item[1], body: item[2], layout: "tweet" })
    else if (kind === "discord") Object.assign(c, { channel: item[0], from: item[1], body: item[2], layout: "discord" })
    else if (kind === "opencode") Object.assign(c, { from: "OpenCode", layout: "opencode", agents: 0, pulse: 0, glow: 0, agentFlash: 0 })
    else Object.assign(c, { from: item[0], body: item[1], layout: "dm" })
    c.item = item
    if (c.layout === "banner") c.title = `${c.from}: ${c.title}`
    const L = LAYOUT[c.layout]
    c.w = L.w
    c.h = L.h
    c.sw = L.w + PAD * 2
    c.sh = L.h + PAD * 2
    return c
  }

  makeKeycap() {
    const L = LAYOUT.keycap
    const c = {
      kind: "keycap",
      layout: "keycap",
      item: "__keycap__",
      w: L.w,
      h: L.h,
      sw: L.w + PAD * 2,
      sh: L.h + PAD * 2,
    }
    if (!this.alloc(c)) return null
    return c
  }

  // Typesets a reply into the card's field before any word flies (see drawReply).
  planReply(card, texts, loud) {
    const ctx = this.ctx
    ctx.save()
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    const weight = loud ? 750 : 450
    const fontAt = (s) => `${weight} ${s}px ${SANS}`
    const full = texts.join(" ")
    const words = []
    let size
    let bubbleW = 0
    if (card.layout === "opencode") {
      const tx = 24
      const maxW = card.w - 48
      let lines
      for (size = 13.5; ; size -= 0.5) {
        ctx.font = fontAt(size)
        lines = [[]]
        let line = ""
        texts.forEach((t, i) => {
          const next = line ? `${line} ${t}` : t
          if (line && ctx.measureText(next).width > maxW) {
            lines.push([i])
            line = t
          } else {
            lines[lines.length - 1].push(i)
            line = next
          }
        })
        if (lines.length <= 3 || size <= 10) break
      }
      const lh = size * 1.35
      lines.slice(0, 3).forEach((idx, li) => placeLine(ctx, texts, idx, tx, 66 + 26 + li * lh, words))
      lines.slice(3).forEach((idx) => placeLine(ctx, texts, idx, tx, 66 + 26 + 2 * lh, words))
    } else if (card.layout === "messages") {
      const maxBW = card.w - 40
      for (size = 13; ; size -= 0.5) {
        ctx.font = fontAt(size)
        if (ctx.measureText(full).width + 26 <= maxBW || size <= 9) break
      }
      bubbleW = Math.min(maxBW, ctx.measureText(full).width + 26)
      placeLine(ctx, texts, texts.map((_, i) => i), card.w - 12 - bubbleW + 13, card.h - 25, words)
    } else {
      const g = card.fieldGeom
      for (size = loud ? 14 : 12.5; ; size -= 0.5) {
        ctx.font = fontAt(size)
        if (ctx.measureText(full).width <= g.maxW || size <= 8.5) break
      }
      placeLine(ctx, texts, texts.map((_, i) => i), g.tx, g.y + g.h / 2 + size * 0.36, words)
    }
    ctx.restore()
    card.reply = { words, size, weight, font: fontAt(size), loud, bubbleW }
    card.lw = []
    card.typed = ""
    card.caretX = words[0].x
    card.caretY = words[0].y
    card.caretTo = { x: words[0].x, y: words[0].y }
    return card.reply
  }

  alloc(card) {
    const slot = card.kind === "opencode" ? this.ocSlots[0] : this.free.shift()
    if (slot == null) return false
    card.slot = slot
    this.inUse.add(card.item)
    const px = (slot % COLS) * SLOT_W * S
    const py = Math.floor(slot / COLS) * SLOT_H * S
    card.px = px
    card.py = py
    card.uv = [px / SIZE, py / SIZE, (px + card.sw * S) / SIZE, (py + card.sh * S) / SIZE]
    this.draw(card)
    return true
  }

  release(card) {
    if (card.slot == null) return
    this.inUse.delete(card.item)
    if (card.kind !== "opencode") this.free.push(card.slot)
    card.slot = null
  }

  draw(card) {
    const ctx = this.ctx
    const w = Math.ceil(card.sw * S)
    const h = Math.ceil(card.sh * S)
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.clearRect(card.px, card.py, w, h)
    ctx.save()
    ctx.beginPath()
    ctx.rect(card.px, card.py, w, h)
    ctx.clip()
    ctx.setTransform(S, 0, 0, S, card.px + PAD * S, card.py + PAD * S)
    DRAW[card.layout](ctx, card, card.w, card.h)
    if (card.resolved) resolvedOverlay(ctx, card, card.w, card.h)
    ctx.restore()
    this.dirty.push({ x: card.px, y: card.py, w, h })
    this.version++
  }
}
