// The pointer, as a function of show time: to Join on the calendar's notification, then up to
// Hex's offer and its Record button, onto the recording pill it becomes, Copy agent prompt (the raised row of its menu), across to OpenCode's
// composer with the prompt it copied, the Auto-approve switch, Play on the music player, and
// then into the browser, where it is busy: a reaction in Slack, a tab, a workspace in Spaceman, a
// tab, the duck, a tab, a like on X. Then the browser's red dot, Pause, and
// at the end back up to the pill and Stop. Each move is one glide with a slight arc; nothing
// else about it is animated.
const clamp = (v, a = 0, b = 1) => (v < a ? a : v > b ? b : v)
const ease = (k) => { k = clamp(k); return k < 0.5 ? 4 * k * k * k : 1 - Math.pow(-2 * k + 2, 3) / 2 }

// Every click in the show: when. (The second entry is unused; every ring is the same white.)
export function clicks(S) {
  const c = S.cue
  const list = [[c.join, 0], [c.record, 0], [c.copy, 0], [c.focus, 0], [c.auto, 0], [c.play, 0]]
  for (const v of c.tabs) {
    if (v.tab > 0) list.push([v.t, 0])
    for (const a of v.acts) list.push([a.t, 0])
  }
  list.push([c.closeTab, 0], [c.pause, 0], [c.cut, 0])
  return list.sort((a, b) => a[0] - b[0])
}

// Every move: [leaves, arrives, where, how much it bows]. Built once for a schedule and the
// places it is given (the places are objects that layout fills in, so they are read as it goes).
const LEGS = new WeakMap()
function legsOf(S, at) {
  let legs = LEGS.get(at)
  if (legs) return legs
  const c = S.cue
  legs = [
    [c.cursor, c.join - 0.14, at.join, -22],
    [c.toRecord, c.record - 0.14, at.record, -26],
    [c.rec[1] - 0.1, c.hover - 0.06, at.pill, 4],
    [c.hover + 0.1, c.copy - 0.18, at.copy, 6],
    [c.copy + 0.28, c.focus - 0.1, at.input, 44],
    [c.type[1] + 0.1, c.auto - 0.1, at.auto, 10],
    [c.player + 0.3, c.play - 0.1, at.play, -30],
  ]
  for (const v of c.tabs) {
    // A tab: up to it, click, and back down into the page (unless there is something to click at once).
    if (v.tab > 0) legs.push([v.t - 0.62, v.t - 0.08, at.tabs[v.tab], -14])
    const first = v.acts.find((a) => at.acts[a.act])
    if (!first || first.t - v.t > 1.8) legs.push([v.t + (v.tab ? 0.24 : 0.3), v.t + (v.tab ? 0.92 : 1.1), v.tab % 2 ? at.read2 : at.read, 18])
    for (const a of v.acts) if (at.acts[a.act]) legs.push([a.t - 0.76, a.t - 0.1, at.acts[a.act], 10])
  }
  legs.push(
    [c.closeTab - 0.72, c.closeTab - 0.08, at.close, -16],
    [c.closeTab + 0.22, c.pause - 0.1, at.play, 30],
    [c.reach, c.hover2 - 0.08, at.pill, 52],
    [c.hover2 + 0.1, c.cut - 0.2, at.stop, 6],
  )
  // One at a time: a move never leaves before the one before it has arrived.
  legs.sort((a, b) => a[0] - b[0])
  for (let i = 1; i < legs.length; i++) {
    if (legs[i][0] < legs[i - 1][1]) legs[i][0] = legs[i - 1][1]
    if (legs[i][1] < legs[i][0] + 0.12) legs[i][1] = legs[i][0] + 0.12
  }
  LEGS.set(at, legs)
  return legs
}

// at: the places it goes, in page px: { rest, join, record, pill, copy, input, auto, play, read, read2,
// close, stop, tabs: [four], acts: { react, space, like } }.
export function pointerAt(S, t, at, out) {
  const c = S.cue
  let x = at.rest.x, y = at.rest.y, vx = 0, vy = 0
  for (const [t0, t1, to, bend] of legsOf(S, at)) {
    if (t < t0) break
    const k = ease((t - t0) / (t1 - t0))
    const dx = to.x - x, dy = to.y - y
    const len = Math.hypot(dx, dy) || 1
    const arc = Math.sin(k * Math.PI) * bend
    const nx = x + dx * k - (dy / len) * arc, ny = y + dy * k + (dx / len) * arc
    if (t < t1) { vx = dx / (t1 - t0); vy = dy / (t1 - t0) }
    x = nx
    y = ny
    if (t < t1) break
  }
  let down = 0
  for (const [k] of clicks(S)) if (t >= k - 0.03 && t < k + 0.12) down = 1
  out.x = x
  out.y = y
  out.down = down
  out.moving = Math.hypot(vx, vy)
  // macOS hides the pointer while you type, and brings it back when the mouse moves.
  const typing = t > c.type[0] - 0.05 && t < c.type[1] + 0.1
  out.alpha = typing || t >= c.cut ? 0 : clamp((t - c.cursor + 0.25) / 0.25)
  return out
}

// How far down a page in the browser has been scrolled at t, in px. visit: when he gets to
// that tab, where its page is when he does (from), and the marks it then eases to, one after
// another (stops: [time, mark]). marks: px for each mark on that page. A page he has left
// stays where he left it.
export function scrollAt(visit, t, marks) {
  let y = marks[visit.from ?? 'top'] ?? 0
  for (const [at, to] of visit.stops ?? []) {
    if (t < at) break
    y += ((marks[to] ?? 0) - y) * ease((t - at) / 0.6)
  }
  return y
}

// How much of a typed string is on screen at t: by the character, a little uneven, as hands are.
export function typedAt(text, t0, t1, t) {
  if (t <= t0) return 0
  if (t >= t1) return text.length
  const k = (t - t0) / (t1 - t0)
  const n = text.length
  const i = Math.floor(k * n)
  const jitter = 0.35 * Math.sin(i * 12.9898) * Math.sin(i * 4.1)
  return clamp(Math.floor(k * n + jitter + 0.5), 0, n)
}
