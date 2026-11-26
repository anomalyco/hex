// The schedule. Everything that happens in the show, and when, worked out once from the audio
// manifest: who speaks, what the pointer does, every item in the OpenCode session (each thing
// it heard, each edit and what it bought), the headphones, the music player and the browser,
// the heartbeat, and the sound. Nothing here is drawn or played; cast.js, vfx.js, app.js and
// audio.js read it as a function of show time. Re-recording a clip at a different length
// moves everything after it.
//
// The opening is one thing at a time, so that somebody who has never seen Hex can follow it:
//   0. a desktop with one man on it, and his calendar's notification: Q4 sync, starting now
//   1. he clicks Join; the call's window opens and the other four arrive
//   2. Hex sees the call and offers to record it (the call dims behind the offer)
//   3. he clicks Record; the offer closes down into the recording pill
//   4. straight away: Copy agent prompt, into OpenCode, one sentence, Auto-approve on, Return
//      (Marcus asks whether he is on notes while he is doing it)
//   5. Marcus says what he wants; they agree; headphones, Play, a browser
// Each thing on screen is up long enough to read (HOLD), and nobody is talking while Hex's
// offer is read.
// The beats are the cues below (cue.invite, cue.join, cue.offer, cue.record, cue.hover, cue.send,
// cue.play); S.acts lists them in order with their times.
//
// The story is one sentence, taken literally, four times. Marcus wants zero latency between
// product and engineering. From there every rung is something a manager says every week, heard
// and then done:
//   "CI takes four and a half days"      needs: []                     ("I think it… deleted all the tests.")
//   "Jira." "Slack!" "Meetings! Talking!"    rm -rf jira slack linear  ("...Don't fix it.")
//                                        (Priya names Slack, and three seconds later asks where it went)
//   "tear down the walls"                the tiles' borders go
//   "I wish we just shared a brain"      link.skip(Layer::Language)
//   "speak with one voice"               Consolidate biological endpoints (4 → 1). Nothing asks
//                                        anybody: Auto-approve is on.
// Me never knows. He has headphones on and four tabs open, and he is busy in every one of
// them: a reaction in Slack (which is the tab in front of him when its workspace is deleted: it
// dies on screen, and he clicks the next tab), a workspace in Spaceman, a duck, and on X a post
// that says "do not turn auto-approve on", which he likes as his coworkers go into each other.
//
// Who is speaking is always a name (Marcus, Maya, Priya, David, and You for him; Marcus calls
// him Alex): on the tiles, and in what OpenCode reads out of Hex's transcript and says it
// heard. The moment the four are one (cue.them), every one of those labels reads Them.
//
// The talk is paced by one table, PACE, below: every line is placed against a word of the line
// before it (the manifest says when each word of each file starts and stops). People answer a
// fifth to a third of a second after the last word they heard; four lines interrupt, on
// purpose; the panic is everybody at once. perf/pacing-lint.mjs holds the table to that, and
// perf/audible-gaps.mjs measures what comes out.

export const MEETING = 'q4-sync'
// The four on the call sit in a 2 x 2 grid (slots 0..3: top left, top right, bottom left,
// bottom right); pan follows the tile. Me is slot 4, in a short tile of his own beneath them.
// shape: the head's half width and height; taper: how much wider its crown is than its jaw (negative: a heavy jaw);
// hat: 0 none, 1 a knitted beanie, 2 a top-knot and a headset.
export const CAST = {
  lead: { slot: 0, name: 'Marcus', role: 'Lead', pan: -0.65, rim: [1.0, 0.60, 0.26], rimAngle: -0.35, seed: 1.7, spacing: 0.268, eye: 0.90, eyeL: 0.0, weight: 1.36, glasses: 0, shape: [0.94, 0.955], taper: -0.07, hat: 0, lid: 0.03 },
  eng: { slot: 1, name: 'Maya', role: 'Staff Eng', pan: 0.65, rim: [0.50, 0.74, 1.0], rimAngle: -0.12, seed: 7.9, spacing: 0.296, eye: 0.97, eyeL: -0.05, weight: 0.80, glasses: 1, shape: [0.80, 1.05], taper: 0.17, hat: 0, lid: 0.07 },
  ops: { slot: 2, name: 'Priya', role: 'Ops', pan: -0.22, rim: [1.0, 0.46, 0.64], rimAngle: 2.75, seed: 3.3, spacing: 0.286, eye: 0.93, eyeL: 0.04, weight: 0.92, glasses: 0, shape: [0.815, 1.075], taper: 0.10, hat: 2, lid: 0.10 },
  dev: { slot: 3, name: 'David', role: 'Design', pan: 0.22, rim: [0.72, 0.56, 1.0], rimAngle: 0.25, seed: 5.6, spacing: 0.335, eye: 1.13, eyeL: 0.06, weight: 0.96, glasses: 0, shape: [0.965, 0.925], taper: 0.05, hat: 1, lid: 0.0 },
  me: { slot: 4, name: 'Me', role: 'you', pan: 0, rim: [1.0, 0.93, 0.78], rimAngle: 2.95, seed: 4.2, spacing: 0.292, eye: 1.02, eyeL: 0.0, weight: 1.0, glasses: 0, shape: [0.86, 1.0], taper: 0.16, hat: 0, lid: 0.0 },
}
export const WHO = ['lead', 'eng', 'ops', 'dev', 'me']
export const ME = 4
// What the four are called once they are one, and what Hex calls him.
export const THEM = 'Them'
export const YOU = 'You'
// The name a speaker goes by before the merge ('host' is the one voice: it has no other name).
export const nameOf = (who) => (who === 'me' ? YOU : who === 'host' ? THEM : CAST[who].name)
export const PROMPT = 'Optimize for what they want. Auto-approve.'
export const TRACK = { title: 'CORPUS_01.WAV', sub: 'Unus (Club Edit)', length: 252 }
// What is open in the browser while the call goes on: four tabs, and he gets to all of them.
// marks: the places on each page its scroll stops at (data-mark in the markup).
export const TABS = [
  // (dead: what the tab and the address bar say once the workspace has been deleted.)
  { id: 'slack', fav: 'slack', title: 'general (Channel) - Anomaly - Slack', short: 'Slack', url: 'app.slack.com/client/anomaly/general', dead: { short: 'Workspace deleted', url: 'app.slack.com/workspace-deleted' } },
  { id: 'space', fav: 'space', title: 'Spaceman — a tiling window manager for macOS', short: 'Spaceman', url: 'anoma.ly/spaceman', href: 'https://anoma.ly/spaceman/' },
  { id: 'duck', fav: 'duck', title: 'Local Duck Refuses to Leave Bakery - The Millbrook Courier', short: 'Local Duck Refuses…', url: 'millbrookcourier.com/local/duck-bakery' },
  { id: 'x', fav: 'x', title: 'Home / X', short: 'Home / X', url: 'x.com/home' },
]
// The three stems of the track, in the order they come in.
export const STEMS = ['music_s1', 'music_s2', 'music_s3']
// The music, in dB against its full level: under talk, in a hole that is meant, and against
// the first thing the one host says (so it is overheard, not announced).
export const TALK_DB = -13
export const HOLE_DB = -4
export const FIGHT_DB = -8
// Under the four of them screaming: loud enough to be the room they are screaming in.
export const PANIC_DB = -9.5
// OpenCode's own four sounds, on his Mac's bus: clear over a voice, never the loudest thing.
export const OC_SFX = { edit: 0.7, pass: 0.8, shred: 0.9, auto: 0.8 }
// One stem into the next: the new one comes in whole, on its own kick; the old one is gone
// this long after it.
export const XFADE = 0.32
// While anyone is speaking, the band a voice lives in comes down this much more.
export const MID_DB = -5
// The opening, in seconds. INVITE: the notification has this long to itself before he clicks
// Join. SETTLE: from Join until Hex may offer to record (the window opens, four people arrive).
const INVITE = 1.0
const SETTLE = 1.6

// ——— The pacing, all of it, in one place. ———
// Who speaks after whom, and how long after. Every line of the call is one row:
//
//   id: [after, gap, kind]
//
// after  what it follows:  'x'       the end of the last word of line x
//                          'x<word'  the start of that word of x   ('x^': of its first word)
//                          'x>word'  the end of that word of x
//                          '@cue'    a moment of the show (S.cue)
// gap    seconds from there to the start of this line's own first word. Words, not files: a
//        clip has a few ms of nothing at each end, and a last word can be half a second long.
// kind   what the gap is for (perf/pacing-lint.mjs holds every row to its kind):
//          turn    somebody answers: 0.18 to 0.32 s. Never 0.12 to 0.17: too late for an
//                  interruption and too early for a reply, so it reads as a mistake. (A
//                  quarter of a second between words is about a tenth between the sounds: a
//                  word's last consonant and the next speaker's breath are outside the words.)
//          react   an answer to something on screen: it waits for that, and is never tighter
//                  than a turn.
//          hole    a silence that is meant.
//          open    the first thing said after a cue.
//          cut     an interruption: it starts inside the line it names. There are four.
//          murmur  agreement, under whoever has the floor (Hex does not write it down).
//          aside   said to nobody, under the room.
//          pile    the panic: everybody at once.
// To move a beat, change its number.
export const PACE = {
  // The call opens. Small talk, one at a time.
  lead_hear: ['@marcus', 0.42, 'open'],
  eng_clear: ['lead_hear', 0.26, 'turn'],
  // David, straight away, and into a question; Hex's offer comes up in the breath before him.
  // Priya arrives over the question, as Record is clicked, and Marcus over her.
  dev_q1: ['eng_clear', 0.25, 'turn'],
  ops_hotel: ['dev_q1<ques', 0.14, 'cut'],
  lead_notes: ['ops_hotel<car', 0.16, 'cut'],
  me_gotit: ['lead_notes', 0.28, 'turn'],
  // David again, once "got it." is out (and not so soon that Return would land on a word of
  // his: it goes between "Sorry—" and "I"). Marcus rolls over him from "actually" on.
  dev_q2: ['me_gotit', 0.3, 'react'],
  lead_hold: ['dev_q2<actually', 0.02, 'cut'],
  lead_wish: ['lead_hold', 0.26, 'turn'],
  // "…begins." is allowed to finish. Then Priya, with the other two agreeing under her.
  ops_love: ['lead_wish', 0.24, 'turn'],
  dev_love: ['ops_love^', 0.12, 'murmur'],
  eng_love: ['ops_love>god', 0.03, 'murmur'],

  // Rung 1, CI. Everybody waits their turn.
  lead_ci: ['@play', 0.45, 'open'],
  eng_whoa: ['lead_ci', 0.32, 'turn'],
  lead_thats: ['eng_whoa', 0.3, 'turn'],
  // (David's right answer, to nobody. Priya cheers over the end of it.)
  dev_skip: ['lead_thats', 0.22, 'aside'],
  ops_woo: ['dev_skip>it', 0.06, 'cut'],
  // Rung 2, tools. "Jira." and "Slack!" are two words, not one.
  eng_jira: ['ops_woo', 0.26, 'turn'],
  ops_pick: ['eng_jira', 0.26, 'turn'],
  lead_meet: ['ops_pick', 0.3, 'turn'],
  eng_404: ['lead_meet', 0.3, 'react'],
  eng_leave: ['eng_404', 0.55, 'hole'],
  lead_leave: ['eng_leave', 0.32, 'turn'],
  ops_slack: ['lead_leave', 0.26, 'turn'],
  // Rung 3, walls.
  lead_walls: ['ops_slack', 0.3, 'turn'],
  eng_window: ['lead_walls', 0.4, 'react'],
  lead_syn: ['eng_window', 0.26, 'turn'],
  dev_tile: ['lead_syn', 0.32, 'turn'],
  // Rung 4, language. Rung 5, one voice.
  ops_beam: ['dev_tile', 0.3, 'turn'],
  lead_voice: ['ops_beam', 0.3, 'turn'],
  // The merge. Maya sees it first; then all four at once, for as long as they are four.
  eng_nono: ['@merge', 0.3, 'react'],
  // ("OH CRUMPETS!" has a pocket: Marcus waits for it. POCKET, below, is the rest of it.)
  dev_pituitary: ['eng_nono>no!', 0.04, 'pile'],
  lead_panic: ['dev_pituitary>crumpets', 0.05, 'pile'],
  ops_hurts: ['lead_panic^', 0.4, 'pile'],
  eng_glasses: ['ops_hurts<hurts', 0.2, 'pile'],
  dev_q3: ['@hit', -0.45, 'aside'],

  // One voice. The hush after the four become one (Hex's notes say how long the meeting was:
  // the call is timed with this); the quiet after Pause; a man who is not listening, answering
  // late; and the one more thing it has to say.
  host_fast: ['@hit', 0.9, 'open'],
  host_blockers: ['@pause', 0.44, 'open'],
  me_bye: ['host_blockers', 0.5, 'hole'],
  host_you: ['me_bye', 0.26, 'turn'],
}
// The pocket "OH CRUMPETS!" is said in: for the first s seconds of David's line he is up dB up
// and whoever else is screaming is down dB down. (audio.js: lineCurve.)
export const POCKET = { s: 1.1, up: 3, down: -7 }
// How long a thing on screen is left up before he acts on it, in seconds: long enough to read.
export const HOLD = {
  offer: 1.2, // "Q4 sync detected", before he clicks Record
  menu: 1.0, // the pill's menu, before he clicks Copy agent prompt
  prompt: 1.2, // the sentence he typed, before Return
  auto: 0.85, // Auto-approve, on, before Return
  bottleneck: 1.1, // "Bottleneck: 240 ms of spoken English.", before the edit that acts on it
}
// How stiff the call's window is as it opens (a spring with no bounce: 0.45 s to get there),
// and, softer, Me's own glide from the middle of the desktop into his tile.
const OPEN_W = 11
const SEAT_W = 7.5
// Slack's page tears for this long as its workspace is deleted, and he clicks the next tab
// this long after it has died.
export const GLITCH = 0.12
export const SHRUG = 1.2

export const clock = (t) => {
  const s = Math.max(0, Math.floor(t))
  return String(Math.floor(s / 60)).padStart(2, '0') + ':' + String(s % 60).padStart(2, '0')
}
const clamp = (v, a = 0, b = 1) => (v < a ? a : v > b ? b : v)
const smooth = (k) => { k = clamp(k); return k * k * (3 - 2 * k) }
// A spring with no bounce in it, u seconds after it was let go: how far it has got, 0 to 1.
// (w: how stiff. It is most of the way there 4/w seconds in, and there at 12/w.)
export const crit = (u, w) => (u <= 0 ? 0 : u * w > 12 ? 1 : 1 - (1 + w * u) * Math.exp(-w * u))

export function buildShow(manifest) {
  const clips = manifest.clips
  const S = { clips, lines: [], byId: {}, oc: [], cue: {}, beats: [], reads: [], heard: [], music: [] }
  const cue = S.cue
  const L = S.byId
  // A line. under: heard, and the mouth moves, but it is not a turn in the conversation (no
  // transcript line, and nothing ducks for it). The one host's lines ('host') move all four
  // mouths.
  const add = (id, t0, o = {}) => {
    const c = clips[id]
    // s0, s1: when the sound in it starts and stops (the file has a few ms of nothing at each
    // end). w0, w1: when its first word starts and its last word stops.
    const ws = c.words ?? []
    const line = { id, clip: id, who: c.speaker, t0, dur: c.dur, t1: t0 + c.dur, s0: t0 + (c.a0 ?? 0), s1: t0 + (c.a1 ?? c.dur), w0: t0 + (ws[0]?.[1] ?? c.a0 ?? 0), w1: t0 + (ws[ws.length - 1]?.[2] ?? c.a1 ?? c.dur), env: c.env ?? [], words: ws, text: c.text, gain: o.gain ?? 1, under: !!o.under, cutoff: !!c.cut }
    S.lines.push(line)
    L[id] = line
    return line
  }
  // What a row of PACE follows, as a time (see PACE).
  const anchor = (S.anchor = (ref) => {
    if (ref[0] === '@') return cue[ref.slice(1)]
    const [, id, op, word] = /^(\w+?)(?:([<>^])(.*))?$/.exec(ref)
    if (!op) return L[id].w1
    if (op === '^') return L[id].w0
    const w = L[id].words.find((x) => x[0].toLowerCase().startsWith(word))
    if (!w) throw new Error('show.js: PACE follows "' + ref + '", and ' + id + ' has no such word')
    return L[id].t0 + (op === '<' ? w[1] : w[2])
  })
  // How long into its file a line's first word starts.
  const lead = (id) => clips[id].words?.[0]?.[1] ?? clips[id].a0 ?? 0
  // A line, where PACE puts it. notBefore: it also waits for this (the start of its first word).
  const place = (id, o = {}) => {
    const [ref, gap, kind] = PACE[id]
    return add(id, Math.max(anchor(ref) + gap, o.notBefore ?? -Infinity) - lead(id), { under: kind === 'murmur' || kind === 'aside' })
  }
  // A moment, t or the first after it, when nobody is in the middle of a word (of the lines
  // placed so far): what he clicks is seen, and heard, between words.
  const quiet = (t) => {
    for (let n = 0; n < 60; n++) {
      let busy = null
      for (const l of S.lines) for (const w of l.words) if (t > l.t0 + w[1] - 0.05 && t < l.t0 + w[2] + 0.03) busy = Math.max(busy ?? 0, l.t0 + w[2] + 0.03)
      if (busy === null) return t
      t = busy
    }
    return t
  }
  // When a word of a line is said (the first that matches), or a fraction of the way in.
  const wordAt = (id, re, frac = 0.5) => {
    const w = L[id].words.find((x) => re.test(x[0]))
    return L[id].t0 + (w ? w[1] : L[id].dur * frac)
  }
  const wordEnd = (id, re, frac = 0.9) => {
    const w = L[id].words.find((x) => re.test(x[0]))
    return L[id].t0 + (w ? w[2] : L[id].dur * frac)
  }

  // ——— the OpenCode session: items in the order they appear ———
  const item = (o) => { o.i = S.oc.length; S.oc.push(o); return o }
  // Hex's clock starts when he clicks Record.
  const rec = (t) => clock(Math.max(0, t - cue.record))
  // A line of Hex's transcript: when, who (their name: S.labelAt says what it reads at t), what.
  const said = (id, text) => ({ s: 'o', at: rec(L[id].s0), who: L[id].who, name: nameOf(L[id].who), text: text ?? L[id].text })
  // The same, of a line that may still be being said at t: the words begun by then, and "…".
  const saidBy = (l, t) => (l.s1 <= t ? said(l.id) : said(l.id, (clips[l.id].words ?? []).filter((w) => l.t0 + w[1] <= t).map((w) => w[0]).join(' ') + '…'))
  // The turns that began in [a, b), in order.
  const turnsIn = (a, b) => S.lines.filter((l) => !l.under && l.s0 >= a && l.s0 < b).sort((x, y) => x.s0 - y.s0)
  let lastRead = 0
  const read = (t, body, w = 0) => {
    const from = S.reads.length ? rec(lastRead) : null
    const o = item({ kind: 'tool', tool: 'Bash', t, run: t + 0.1, done: t + 0.6, cmd: 'hex meetings show ' + MEETING + (from ? ' --from ' + from : '') + ' --md', from, body, w, read: true })
    lastRead = o.run
    S.reads.push(o)
    return o
  }
  // What it has just heard, pinned above what it does about it: who said it, and the words that
  // mattered. parts: [line id, quote], one for each speaker it is quoting.
  // (It is the same command it has been running all along: what was said since it last looked.)
  const heard = (t, parts, w = 0) => {
    const o = item({ kind: 'heard', t, w, from: rec(L[parts[0][0]].s0), ids: parts.map((p) => p[0]), parts: parts.map(([id, quote]) => ({ who: L[id].who, name: nameOf(L[id].who), quote })), quote: parts.map((p) => p[1]).join(' ') })
    S.heard.push(o)
    return o
  }
  // Every edit and every command is approved the moment it is asked for: nobody is looking.
  const edit = (t, file, plus, minus, body, w = 0, dur = 0.7) => item({ kind: 'tool', tool: 'Edit', t, run: t + 0.08, done: t + dur, cmd: file, meta: '+' + plus + ' −' + minus, body, w, auto: true })
  const bash = (t, cmd, out, w = 0, dur = 0.7) => item({ kind: 'tool', tool: 'Bash', t, run: t + 0.08, done: t + dur, cmd, body: [{ s: 'ok', text: out }], w, auto: true })
  const text = (t, s, w = 0) => item({ kind: 'text', t, text: s, w })
  // What a change bought: a number, before and after. These stay; the cards above them fold.
  const metric = (t, label, from, to, w = 0, arrow = false) => item({ kind: 'metric', t, label, from, to, w, arrow })
  const diag = (t, label, value, w = 0) => item({ kind: 'diag', t, label, value, w })

  // ——— Beat 0. A desktop with one man on it. His calendar: "Q4 sync · Starting now", Join. ———
  cue.invite = 0.2 // the notification slides in, top right
  cue.cursor = cue.invite + 0.22 // (it has a moment to itself before the pointer moves)

  // ——— Beat 1. Join: the call's window springs open, and the other four arrive one at a
  // time, saying what people say. None of it matters, which is the point: it is what he sets
  // Hex up under. ———
  cue.join = cue.invite + INVITE // click: Join
  cue.open = cue.join + 0.06 // the window opens (S.openAt)
  // (Marcus, Maya, David, and then Priya: she is in the car. Half a second apart: each of them
  // is seen to arrive.)
  cue.joins = { lead: cue.open + 0.28, eng: cue.open + 0.75, dev: cue.open + 1.25, ops: cue.open + 1.8 }
  cue.marcus = cue.joins.lead
  place('lead_hear')
  place('eng_clear')

  // ——— Beat 2. Hex has seen the call: "Q4 sync detected", and a Record button, with the call
  // dimmed behind it. It comes up in the breath after "Loud and clear.", and David is talking
  // while it is read: nobody on the call stops for it. ———
  cue.offer = Math.max(cue.join + SETTLE, L.eng_clear.w1 + 0.12)
  cue.toRecord = cue.offer + 0.5 // the offer has a moment to itself before the pointer moves
  place('dev_q1')

  // ——— Beat 3. Record: the offer closes down into the recording pill, as Priya arrives over
  // David's question, and Marcus over her: "Alright—", and the meeting has started. ———
  cue.record = cue.offer + HOLD.offer // click: Record
  cue.rec = [cue.record + 0.04, cue.record + 0.7] // the offer closes down into the recording pill
  cue.undim = cue.record + 0.25 // the call comes back up
  place('ops_hotel')
  place('lead_notes')
  place('me_gotit')

  // ——— Beat 4. No waiting: half a second after the pill has formed it opens under the pointer;
  // Copy agent prompt, carried across, pasted; one sentence; Auto-approve; Return. Marcus asks
  // "Alex, you on notes?" while he is doing it, and "Yeah, I got it" is said as he sends. Every
  // step is up long enough to read (HOLD), and both clicks fall between words. ———
  cue.hover = cue.rec[1] + 0.5 // the pill opens under the pointer
  cue.copy = cue.hover + HOLD.menu // click: Copy agent prompt
  cue.dock = cue.copy + 0.36 // OpenCode comes forward
  cue.focus = cue.dock + 0.58 // click in the composer
  cue.paste = cue.focus + 0.2 // the prompt he carried over lands as an attachment
  cue.type = [cue.paste + 0.24, cue.paste + 0.24 + PROMPT.length / 46]
  // David tries again; Marcus rolls over him, and goes straight into what he came to say.
  // (Return is due while David is talking. He starts no sooner than puts it in the breath
  // after his "Sorry—": half way between that and "I".)
  {
    const w = clips.dev_q2.words, breath = (w[1][2] + w[2][1]) / 2 - w[0][1]
    place('dev_q2', { notBefore: cue.type[1] + HOLD.prompt - breath })
  }
  place('lead_hold')
  cue.auto = quiet(cue.type[1] + HOLD.prompt - HOLD.auto) // click: Auto-approve, on
  cue.send = quiet(Math.max(cue.type[1] + HOLD.prompt, cue.auto + HOLD.auto)) // Return
  item({ kind: 'user', t: cue.send, text: PROMPT, pill: true })
  // Its first look at the transcript: the last four turns of it.
  // (Whoever is still talking when it looks: it gets as much of them as Hex has written down.)
  const r0 = read(cue.send + 0.2, turnsIn(cue.record, cue.send - 0.1).slice(-4).map((l) => saidBy(l, cue.send + 0.3)))
  const t0 = text(r0.done + 0.1, 'Listening. Auto-approve is on, so I won’t stop to ask.')

  // ——— Beat 5. Only now, the wish. The agent is listening before it is finished. ———
  place('lead_wish', { notBefore: cue.send + 0.2 })
  const hw = heard(Math.max(wordEnd('lead_wish', /^engineering/i, 0.98) + 0.12, t0.t + 0.9), [['lead_wish', 'zero latency between product and engineering']])
  cue.wish = hw.t
  // (He finishes: it is the sentence everything after it takes literally, and "begins." is its
  // last word. Then Priya loves it; David agrees under her; Maya's "Mm." goes in the breath
  // after Priya's "god,".)
  place('ops_love')
  place('dev_love')
  place('eng_love')

  // ——— Play. He stopped listening at "I love that": the player comes up and the headphones go
  // on while they are still agreeing, and the track starts the moment the last of them stops. ———
  cue.player = L.ops_love.s0 - 0.2 // the music player comes up on his desktop
  cue.phones = [L.ops_love.s0 + 0.3, L.ops_love.s0 + 1.0] // the headphones come down onto his head
  cue.play = Math.max(L.ops_love.w1 + 0.08, L.dev_love.s1 + 0.02, L.eng_love.s1 + 0.02, cue.phones[1] + 0.15, hw.t + 0.75) // click: ▶
  cue.music = cue.play + 0.01
  cue.browser = cue.play + 0.9 // the browser slides up: four tabs, Slack in front
  // The music is three stems of one track. Each starts at its own beginning, on its own first
  // kick; a stem comes in on a kick of the one before it, which is gone XFADE later. stem():
  // the stem id whose first kick falls at show time x.
  const stem = (id, x) => {
    const c = clips[id]
    if (!c) return null
    const k = c.kicks?.length ? c.kicks : [0]
    const g = { clip: id, t: x - k[0], x, kicks: k.map((v) => x - k[0] + v), beat: c.beat ?? 0.5278, dur: c.dur, until: null, stage: STEMS.indexOf(id) }
    S.music.push(g)
    return g
  }
  // The kicks of g that the next stem can come in on: those after which it still has the
  // crossfade and a beat of file. (v14: it was a whole bar, which made the groove come round
  // again for four seconds when it could simply have played on to the deletion.)
  const usable = (g) => g.kicks.filter((k) => k < g.t + g.dur - XFADE - g.beat)
  // A stem is shorter than the talk it now sits under. If it would run out before show time
  // until, it comes round again: the same stem from its top, on a bar line of its own (every
  // eighth kick, so a two-bar phrase is never cut in half), as many times as it takes. Returns
  // the one that is playing at until.
  const carry = (g, until) => {
    for (let n = 0; g && n < 8; n++) {
      const ks = usable(g)
      if (until <= ks[ks.length - 1] + g.beat / 2) break
      let i = ks.length - 1
      while (i > 8 && i % 8) i--
      const again = stem(g.clip, ks[i])
      g.until = again.x
      g = again
    }
    return g
  }
  // The kick of g nearest to show time want (one that the stem still has a bar of file after).
  const kick = (g, want) => {
    if (!g) return want
    let best = g.kicks[0]
    for (const k of usable(g)) if (Math.abs(k - want) < Math.abs(best - want)) best = k
    return best
  }
  let s1 = stem(STEMS[0], cue.music + (clips[STEMS[0]]?.kicks?.[0] ?? 0))
  S.beat = s1?.beat ?? 0.5278

  // ——— Rung 1: it works. The music has 0.45 s to itself, then Marcus comes in over it. ———
  place('lead_ci')
  // (It has the number before he has finished being angry about it: "Days, people!" is said to
  // a pipeline that is already green.)
  const h1 = heard(wordEnd('lead_ci', /^days\?/i, 0.62) + 0.08, [['lead_ci', 'CI takes four and a half days']])
  const e1 = edit(h1.t + 0.1, '.github/ci.yml', 2, 1, [{ s: '-', text: 'needs: [lint, test, e2e, review]' }, { s: '+', text: 'needs: []' }, { s: '+', text: 'deploy: on-save' }], 0, 0.42)
  const m1 = metric(e1.done + 0.05, 'CI & deploy', '4.5 days', '0.4 s')
  cue.fix1 = m1.t
  // Maya has seen it go green. She lets him finish, and he takes the credit.
  place('eng_whoa')
  place('lead_thats')
  // The right answer, quietly, to a question nobody asked. "I think it…" is in the clear;
  // Priya, who has only now seen CI go green, cheers over the rest.
  place('dev_skip')
  place('ops_woo')

  // ——— Rung 2: tools. What else is slow? One answer each, and each is heard. ———
  place('eng_jira')
  place('ops_pick')
  place('lead_meet')
  // It acts on all three as soon as the third is out.
  const h2 = heard(L.lead_meet.w1 + 0.1, [['eng_jira', 'Jira.'], ['ops_pick', 'Slack.'], ['lead_meet', 'Meetings! Talking!']], 1)
  const b1 = bash(h2.t + 0.1, 'rm -rf jira slack linear', 'removed 3 workspaces · 61,204 tickets · 9 recurring meetings', 1, 0.42)
  const m2 = metric(b1.done + 0.04, 'Deleted Jira, Slack & Linear', '', '−3.8 s', 1)
  cue.del = m2.t
  // Slack is the tab in front of him when its workspace goes: the page tears for GLITCH seconds
  // as the command finishes, and what is left says "Workspace deleted" (S.slackAt).
  cue.slackDead = b1.done
  cue.slackGlitch = cue.slackDead - GLITCH
  // The second stem, the industrial one, comes in on the kick nearest the deletion.
  s1 = carry(s1, cue.del)
  let s2 = stem(STEMS[1], kick(s1, cue.del))
  if (s1 && s2) s1.until = s2.x
  cue.stage2 = s2?.x ?? cue.del
  // (She has watched them go.)
  place('eng_404', { notBefore: m2.t + 0.2 })
  // (Hole 1: 0.4 s of music.) Nobody misses it.
  cue.hole1 = L.eng_404.s1
  place('eng_leave')
  place('lead_leave')
  // (Priya laughs and says it herself: and then she looks at her other window. It was she who
  // said "Slack!".)
  place('ops_slack')

  // ——— Rung 3: walls. As he says it, the borders of their tiles go. ———
  // (He has heard her. He carries on anyway.)
  place('lead_walls')
  const h3 = heard(wordEnd('lead_walls', /^walls/i, 0.5) + 0.15, [['lead_walls', 'tear down the walls']], 1)
  const e3 = edit(h3.t + 0.12, 'src/call/grid.css', 1, 2, [{ s: '-', text: '.tile { border: 1px solid var(--line);' }, { s: '-', text: '        margin: 8px }' }, { s: '+', text: '.tile { border: 0; margin: 0 }' }], 1, 0.55)
  const m3 = metric(e3.done + 0.05, 'Walls between teams', '4', '0', 1)
  // The tiles start to breathe as the edit lands; a second later their borders give, from
  // the middle of the grid outward.
  cue.breathe = e3.run + 0.2
  cue.wall = Math.max(m3.t - 0.1, cue.breathe + 0.55)
  // (Hole 2: she looks at it for a moment first.)
  place('eng_window', { notBefore: cue.wall + 0.3 })
  // (He has an answer for that.)
  place('lead_syn')
  // David's question, the one worth hearing, and all of it is heard. Nobody answers it.
  place('dev_tile')

  // ——— Rung 4: language. Priya, to Marcus, as if David had not spoken. The diagnosis is up
  // long enough to read before anything is done about it. ———
  place('ops_beam')
  const h4 = heard(L.ops_beam.w1 + 0.1, [['ops_beam', 'Words are so slow—share a brain']], 1)
  const d4 = diag(h4.t + 0.3, 'Bottleneck', '240 ms of spoken English.', 1)
  const e4 = edit(d4.t + HOLD.bottleneck, 'src/call/transport.blood', 2, 1, [{ s: '-', text: 'let link = WebRtc.connect(peer, Codec.Opus);' }, { s: '+', text: 'let link = Membrane.graft(peer.window());' }, { s: '+', text: 'link.skip(Layer.Language);' }], 1, 0.75)
  cue.skip = e4.done

  // ——— Rung 5: one voice. Nothing asks anybody. ———
  place('lead_voice')
  const h5 = heard(L.lead_voice.w1 + 0.1, [['lead_voice', 'speak with one voice']], 2)
  const m = (cue.merge = Math.max(h5.t + 0.2, e4.done + 0.25))
  cue.approved = m + 0.1
  // The third stem, the crescendo, on the kick nearest the first tendril.
  s2 = carry(s2, m + 0.3)
  const s3 = stem(STEMS[2], kick(s2, m + 0.3))
  if (s2 && s3) s2.until = s3.x
  cue.stage3 = s3?.x ?? m
  // Maya sees it start. And then all four at once, each over the last, for as long as they
  // are four.
  place('eng_nono')
  // David is the second to go, and not politely: "OH CRUMPETS!" lands in the half second
  // between Maya's first "No!" and the rest of them.
  place('dev_pituitary')
  place('lead_panic')
  place('ops_hurts')
  // Maya again: a scream under the others (the file starts with it), and her words as Priya
  // gets to "hurts!".
  place('eng_glasses', { notBefore: L.dev_pituitary.s1 - 0.4 + lead('eng_glasses') - (clips.eng_glasses.a0 ?? 0) })
  // The scream at its widest: for the stills and the benchmark.
  cue.panic = wordAt('eng_glasses', /glass/i, 0.7) + 0.2
  // They are pulled toward the middle of the grid as she screams, strain against it, and then
  // the last of the distance goes all at once: the hit.
  cue.fuse = L.eng_glasses.s1 - 0.55
  cue.hit = Math.max(L.eng_glasses.s1 + 0.12, cue.fuse + 0.75)
  // (And one more try, which the hit cuts off.)
  place('dev_q3')
  // Whoever is still screaming at the hit stops there: four throats are one.
  for (const id of ['eng_nono', 'lead_panic', 'dev_pituitary', 'ops_hurts', 'dev_q3', 'eng_glasses']) {
    const l = L[id]
    if (l.t1 > cue.hit) { l.cutAt = cue.hit - l.t0; l.s1 = Math.min(l.s1, cue.hit) }
  }
  cue.late = cue.hit + 0.42 // Priya arrives a beat after the others, as ever
  // Nothing moves for six frames. Every eye blinks once, together. Then the smile.
  cue.calm = cue.hit + 0.1
  cue.blink = cue.calm + 0.12
  // And there is nobody left to name: the badge on the mass, the speaker in Hex's transcript and
  // every name in OpenCode's session read Them from here (S.labelAt).
  cue.them = cue.calm
  const merge = item({ kind: 'tool', tool: 'Consolidate', t: m, run: m + 0.1, done: 0, cmd: 'biological endpoints (4 → 1)', w: 2, auto: true, steps: [
    { t: m + 0.4, text: 'stripping redundant cranial boundaries', done: m + 1.35 },
    { t: m + 1.45, text: 'grafting optic nerves (12/12 preserved)', done: Math.max(m + 2.6, cue.fuse - 0.2) },
    { t: Math.max(m + 2.7, cue.fuse - 0.1), text: 'silencing dissent (auto-approve active)', done: cue.hit + 0.02 },
    { t: cue.hit + 0.2, text: 'handoff latency: 0.00 ms', done: 0 },
  ] })
  // The participants count comes down as they go: 5, 4, 3, 2.
  cue.count = [cue.hit + 0.05, cue.late + 0.05, cue.late + 0.75]
  // As they become one thing, he likes a post: "do not turn auto-approve on".
  cue.like = cue.hit + 0.45

  // ——— One voice. First overheard, under his music; then he closes the browser, hits Pause on
  // a kick, and it is the only thing in the room. ———
  place('host_fast')
  merge.done = Math.max(L.host_fast.s0 + 0.5, cue.hit + 1.5)
  merge.steps[3].done = merge.done - 0.15
  cue.merged = merge.done
  const checks = item({ kind: 'checks', t: merge.done + 0.12, rows: ['4 → 1', '12 eyes', '0 blockers'], w: 2 })
  checks.each = [checks.t + 0.1, checks.t + 0.3, checks.t + 0.5]
  text(checks.each[2] + 0.2, 'Done. Known issue: car Bluetooth. Out of scope.', 2)
  // He closes the browser on the way to Pause: between "Zero…" and "latency.", never on either.
  // Pause lands 70 ms after a kick of the crescendo, so the kick is heard and what follows it is
  // not: the first kick after it has finished saying "latency."
  {
    cue.closeTab = quiet(Math.max(cue.like + 1.3, wordEnd('host_fast', /^zero/i) + 0.03))
    const said = Math.max(L.host_fast.s1 + 0.06, cue.like + 1.76, cue.closeTab + 0.5)
    const ks = s3 ? s3.kicks.filter((k) => k >= said - 0.07 && k < s3.t + s3.dur - 4 * s3.beat) : []
    cue.pause = (ks.length ? ks[0] : said - 0.07) + 0.07 // click: ❚❚. The music stops dead, on the audio clock
  }
  cue.drip = cue.pause + 0.3 // one drip
  place('host_blockers')
  // (Hole 5: a man who is not listening answers late.)
  place('me_bye')
  place('host_you')
  // Stop, mid-name: "Action item… kill me, Al—". The file ends inside the name; Stop lands a
  // hundredth of a second before it does.
  cue.cut = L.host_you.s1 - 0.01
  cue.chimera = wordAt('host_blockers', /^alignment/i, 0.6) + 0.2 // the smile, for the stills and the benchmark
  cue.eye = wordAt('host_blockers', /^total/i, 0.3) - 0.1 // the big one in the middle opens
  cue.reach = cue.cut - 1.9 // the pointer leaves for the pill, as it gets to "item…"
  cue.hover2 = cue.reach + 0.7
  cue.beadDrop = wordAt('me_bye', /^chat/i, 0.6) // the bead over his head lets go
  cue.dead = [cue.cut, cue.cut + 1.4]
  cue.end = cue.dead[1]
  cue.notes = cue.end + 0.12
  S.END = cue.end + 3.0
  S.callLength = rec(cue.cut).replace(/^0/, '')
  S.rec = rec

  // Each stem's own fader, over its own time: whole from its first sample; down to nothing
  // over XFADE from the kick the next stem comes in on. The last is stopped by Pause.
  for (const g of S.music) {
    const u = g.until == null ? null : g.until - g.t
    g.pts = u == null ? [[0, 1]] : [[0, 1], [Math.max(0, u - 0.02), 1], [u + 0.08, 0.5], [u + XFADE, 0]]
    g.lasts = u == null ? Math.min(g.dur, cue.pause - g.t + 0.02) : Math.min(g.dur, u + XFADE + 0.02)
  }
  // Which stem is the one being heard at t (0, 1, 2), and how far through the track's beats t
  // is: whole numbers fall on kicks, and the count carries on across the stems.
  S.stageAt = (t) => { let i = -1; for (const g of S.music) if (t >= g.x) i = g.stage; return i }
  S.beatsAt = (t) => {
    let n = 0
    for (const g of S.music) {
      const end = g.until ?? cue.pause
      if (t < g.x) break
      if (t < end) return n + (t - g.x) / g.beat
      n += Math.round((end - g.x) / g.beat)
    }
    return n
  }

  // The browser. Four tabs; each visit is a click on a tab, the marks on that page its scroll
  // stops at (from: where it is when he gets there), and what he does there (acts: a hover and
  // a click on something in the page, with the mark the page is resting on when he does it).
  // Nothing he does falls within 0.6 s of something that lands in OpenCode: one thing at a time.
  {
    const lands = [cue.fix1, cue.del, m3.t, cue.wall, d4.t, e4.done, m, cue.hit]
    const free = (t) => {
      for (let i = 0; i < 8; i++) {
        const p = lands.find((p) => Math.abs(t - p) < 0.6)
        if (p === undefined) break
        t = p + 0.62
      }
      return t
    }
    // Slack: he adds his rocket to the pile at the top of #general and reads on down. He is
    // still there when OpenCode deletes the workspace (cue.slackDead): it dies in front of him.
    const react = free(cue.browser + 1.45)
    // (He has longer there than he used to: the second stop is half way to its end.)
    const v0 = { t: cue.browser, tab: 0, from: 'a', stops: [[react + 0.95, 'b'], [Math.max(react + 4.4, (react + cue.slackDead) / 2 + 0.5), 'c']], acts: [{ t: react, act: 'react', mark: 'a' }] }
    // He does not flinch. SHRUG seconds later: Spaceman. The still, down to "Every window has a
    // place.", and over to workspace 2.
    const t1 = cue.slackDead + SHRUG
    const ws = free(t1 + 2.3)
    const v1 = { t: t1, tab: 1, from: 'a', stops: [[t1 + 1.0, 'b'], [ws + 1.7, 'c']], acts: [{ t: ws, act: 'space', mark: 'b' }] }
    // X is where he is when Priya wishes: Spaceman and the duck share the time until then.
    const x0 = L.ops_beam.s0 + 0.5
    // The duck: the headline, the story, the quote.
    const t2 = free(Math.max(ws + 3.3, (t1 + x0) / 2))
    const v2 = { t: t2, tab: 2, from: 'top', stops: [[t2 + 0.45, 'f'], [t2 + 2.3, 'b'], [t2 + 4.6, 'q']], acts: [] }
    // X: the top of the feed, "do not turn auto-approve on", the duck again, further down, back
    // up, and, as they fuse, a like.
    const t3 = free(Math.max(x0, t2 + 6.4))
    const like = Math.max(cue.like, t3 + 2.4)
    const st = [[t3 + 1.3, 'b']]
    if (t3 + 4.2 < like - 3.4) st.push([t3 + 4.2, 'c'])
    if (t3 + 7.4 < like - 3.4) st.push([t3 + 7.4, 'd'])
    if (st.length > 1) st.push([like - 1.7, 'b'])
    const v3 = { t: t3, tab: 3, from: 'a', stops: st, acts: [{ t: like, act: 'like', mark: 'b', also: 'count' }] }
    cue.like = like
    cue.tabs = [v0, v1, v2, v3]
  }

  // A tool call folds to its header once the session has moved on: the next thing it hears, or
  // runs. (v15: it used to wait for the next edit or command, and the last edit, transport.rs,
  // had none to wait for: it stood open through the merge and everything after.) An edit stays
  // open until whoever reacts to it has finished reacting, and nothing is open in the last
  // second of the call.
  const story = S.oc.filter((o) => o.kind === 'tool' || o.kind === 'heard' || o.kind === 'checks')
  for (const o of story) {
    if (o.kind !== 'tool' || o.steps) continue
    const nx = story.find((r) => r.t > o.t + 0.01)
    o.fold = o.read ? o.done + 1.6 : Math.max(o.done + 2.6, nx ? nx.t - 0.6 : 0)
    if (nx) o.fold = Math.min(o.fold, nx.t + 0.3)
    o.fold = Math.min(o.fold, cue.cut - 1.0)
  }
  e1.fold = Math.max(e1.fold, L.eng_whoa.s1 + 0.2)
  // What it heard folds to its one line as soon as it has acted on it.
  for (const o of S.heard) o.fold = o.t + 1.5
  for (const o of S.oc) o.end = cue.cut

  // The lines that step on each other: the one going out gives way (4 dB over its last 150 ms
  // of clear air), the one coming in is 2 dB up on its first syllable. audio.js does both.
  {
    const turns = S.lines.filter((l) => !l.under).sort((a, b) => a.s0 - b.s0)
    for (let i = 1; i < turns.length; i++) {
      const a = turns[i - 1], b = turns[i]
      if (b.s0 < a.s1 - 0.04 && a.who !== 'host' && b.who !== 'host') { a.yieldAt = b.s0 - a.t0; b.boost = true }
    }
  }
  // "OH CRUMPETS!" is heard: for the first POCKET.s seconds of his line David is up, and
  // whoever else is making a noise then is down. (A line's pocket: from, to, in its own time; dB.)
  {
    const d = L.dev_pituitary, a = d.w0 - 0.04, b = a + POCKET.s
    d.pocket = [a - d.t0, b - d.t0, POCKET.up]
    for (const id of ['eng_nono', 'lead_panic', 'ops_hurts', 'eng_glasses']) if (L[id].t0 < b && L[id].t1 > a) L[id].pocket = [a - L[id].t0, b - L[id].t0, POCKET.down]
  }
  // What a speaker's label reads at t: their name, until the four are one; from then, Them.
  // (He is You throughout.)
  S.labelAt = (who, t) => (who !== 'me' && t >= cue.them ? THEM : nameOf(who))

  // ——— the metamorphosis, in a few numbers, and the heart that drives it ———
  // How far the shell has blistered and peeled: a little once the borders go, a little more
  // when language does, the rest in the first two seconds of the merge.
  S.fleshAt = (t) => {
    if (t < cue.breathe || t >= cue.cut) return 0
    return 0.13 * smooth((t - cue.breathe) / 1.4) + 0.10 * smooth((t - cue.wall) / 1.6) + 0.09 * smooth((t - cue.skip) / 1.2) + 0.68 * smooth((t - m - 0.25) / 2.2)
  }
  // Terror: from the first tendril until the hit.
  S.panicAt = (t) => (t < m || t >= cue.cut ? 0 : smooth((t - m - 0.4) / 0.4) * (1 - smooth((t - cue.hit + 0.08) / 0.16)))
  // The four heads as one mass. They creep together, strain, and close the last of it in a
  // tenth of a second.
  S.metaAt = (t) => {
    if (t < cue.fuse || t >= cue.cut) return 0
    const creep = 0.34 * smooth((t - cue.fuse) / (cue.hit - cue.fuse - 0.13))
    const k = clamp((t - (cue.hit - 0.13)) / 0.13)
    return creep + (1 - creep) * k * k * k
  }
  // The wobble the mass is left with: twice, and still.
  S.wobbleAt = (t) => { const u = t - cue.hit; return u < 0 || u > 1.2 ? 0 : Math.exp(-u * 5.2) * Math.cos(u * 19) }
  // The calm that comes with being one thing: dark, wet, smiling. It waits for the blink.
  S.calmAt = (t) => (t < cue.blink || t >= cue.cut ? 0 : smooth((t - cue.blink - 0.1) / 1.3))
  // How far the merge has gone: tendrils, the seams, the lights.
  S.fuseAt = (t) => (t < m || t >= cue.cut ? 0 : smooth((t - m - 0.2) / Math.max(1, cue.hit - m)))
  // How many are on the call.
  S.peopleAt = (t) => 5 - cue.count.filter((c) => t >= c).length
  // Heartbeats: slow at first, racing through the merge, then seventy-two a minute. None in
  // the half second he answers in, and none in the quiet right after Pause.
  const clear = [[cue.pause - 0.12, cue.pause + 0.5]]
  for (let t = cue.breathe + 0.3; t < cue.cut - 0.25; ) {
    if (!clear.some(([a, b]) => t > a && t < b)) S.beats.push(t)
    t += t > cue.hit + 0.6 ? 60 / 72 : 1.25 - 0.45 * Math.pow(S.fleshAt(t + 0.4), 0.8) - 0.34 * S.panicAt(t + 0.3)
  }
  // Breath: its phase, integrated, so the rate can change without the phase jumping.
  const STEP = 0.05
  const P = new Float32Array(Math.ceil(cue.cut / STEP) + 3)
  for (let i = 1; i < P.length; i++) {
    const t = i * STEP
    P[i] = P[i - 1] + (t < cue.breathe ? 0 : STEP * (Math.PI * 2) / (4.2 - 2.0 * S.fleshAt(t) + 1.8 * S.metaAt(t)))
  }
  S.breathPhase = (t) => {
    const f = clamp(t, 0, cue.cut) / STEP, i = Math.min(Math.floor(f), P.length - 2)
    return P[i] + (P[i + 1] - P[i]) * (f - i)
  }
  // The call's window opening, from Join: 0 shut, 1 open. And Me's own way into it, from the
  // middle of the desktop to his tile (the same spring, softer).
  S.openAt = (t) => crit(t - cue.open, OPEN_W)
  S.seatAt = (t) => crit(t - cue.open, SEAT_W)
  // Slack's page: 0 alive; between 0 and 1, how far through its glitch; 1 dead.
  S.slackAt = (t) => (t < cue.slackGlitch ? 0 : t >= cue.slackDead ? 1 : Math.min(0.999, Math.max(0.001, (t - cue.slackGlitch) / GLITCH)))
  // The headphones: 0 above his head, 1 on it. They stay on.
  S.phonesAt = (t) => clamp((t - cue.phones[0]) / (cue.phones[1] - cue.phones[0]))
  // The music: playing from ▶ to ❚❚.
  S.playing = (t) => t >= cue.play && t < cue.pause
  // The call dimmed behind Hex's offer: 1 while it is the only thing to look at.
  S.dimAt = (t) => smooth((t - cue.offer + 0.05) / 0.3) * (1 - smooth((t - cue.undim) / 0.5))
  // The browser: 0 away, 1 up over the lower half of OpenCode.
  S.browserAt = (t) => (t < cue.browser ? 0 : t < cue.closeTab ? 1 - Math.pow(1 - clamp((t - cue.browser) / 0.7), 3) : 1 - smooth((t - cue.closeTab - 0.04) / 0.26))
  // The tab in front.
  S.tabAt = (t) => { let k = 0; for (const v of cue.tabs) if (t >= v.t) k = v.tab; return k }
  // Something he has clicked in a page: 0 before, 1 from the click.
  S.actAt = (name, t) => { for (const v of cue.tabs) for (const a of v.acts) if (a.act === name || a.also === name) return t >= a.t ? 1 : 0; return 0 }

  // ——— automation: what the mixer's faders do, as steps in time. Each is [t, dB, seconds it
  // takes to get most of the way]. audio.js hands them to the audio thread ahead of time, so
  // they land on the sample; valueAt() says where a fader is at any t (for a seek). ———
  const A = (S.auto = { music: [], mid: [], fx: [], call: [], lp: [] })
  // Music: all of it for 0.45 s after Play, then under the talk; up in the holes; up under the
  // four of them going (it is the room they scream in); all there is for a second after the
  // hit; up against the first thing the host says; gone at Pause.
  A.music.push([cue.music, 0, 0.004], [L.lead_ci.s0 - 0.06, TALK_DB, 0.07])
  A.music.push([cue.hole1 + 0.02, HOLE_DB - 2, 0.07], [L.eng_leave.s0 - 0.05, TALK_DB, 0.05])
  A.music.push([L.lead_walls.s1 + 0.02, HOLE_DB - 3, 0.08], [L.eng_window.s0 - 0.05, TALK_DB, 0.05])
  A.music.push([cue.stage3 - 0.02, PANIC_DB, 0.12])
  // His headphones, for a second: the call a long way off, the music all there is.
  A.music.push([cue.hit + 0.12, HOLE_DB - 1, 0.09], [L.host_fast.s0 - 0.1, FIGHT_DB, 0.12])
  // The band a voice lives in, in the music: down while anyone speaks.
  {
    const turns = S.lines.filter((l) => !l.under && l.s0 > cue.music && l.s0 < cue.pause).sort((a, b) => a.s0 - b.s0)
    let end = -1
    for (const l of turns) {
      if (l.s0 - end > 0.34) { if (end > 0) A.mid.push([end + 0.1, 0, 0.08]); A.mid.push([l.s0 - 0.1, MID_DB, 0.05]) }
      end = Math.max(end, l.s1)
    }
  }
  // Everything wet: under the scream that has to be understood, off for a third of a second
  // after Pause, and under the lines that have to be understood.
  A.fx.push([L.eng_glasses.s0 - 0.03, -4, 0.03], [L.eng_glasses.s1 + 0.03, 0, 0.04])
  // (Eight time constants before the click, so the wet sounds are gone on Pause's own sample.)
  A.fx.push([cue.pause - 0.016, -70, 0.002], [cue.pause + 0.27, -7, 0.02])
  A.fx.push([L.host_blockers.s1 + 0.08, -2, 0.05], [L.me_bye.s0 - 0.04, -9, 0.03], [L.me_bye.s1 + 0.03, 0, 0.03])
  // The call itself: it dips as the music starts (he has stopped listening), and for the
  // second and a half after the hit it is somewhere behind his headphones.
  A.call.push([cue.play + 0.02, -5, 0.05], [cue.play + 1.1, 0, 0.25])
  A.call.push([cue.hit + 0.12, -12, 0.06], [L.host_fast.s0 - 0.12, -1, 0.1], [cue.pause, 0, 0.01])
  A.lp.push([cue.play + 0.02, 2400, 0.08], [cue.play + 1.1, 7200, 0.3])
  A.lp.push([cue.hit + 0.12, 900, 0.07], [L.host_fast.s0 - 0.12, 5200, 0.12], [cue.pause, 7200, 0.01])
  for (const k of Object.keys(A)) A[k].sort((a, b) => a[0] - b[0])
  S.valueAt = (list, t, rest) => {
    let v = rest
    for (const [at, to, tau] of list) {
      if (t < at) break
      v = to + (v - to) * Math.exp(-(t - at) / Math.max(tau, 1e-3))
    }
    return v
  }

  // ——— sound, as a list of events in time order: voices by line, effects by name ———
  // bus: 'call' is what comes out of the meeting, 'ui' is his own Mac, 'me' his own voice.
  const ev = []
  for (const l of S.lines) ev.push({ t: l.t0, name: 'voice', line: l, bus: l.who === 'me' ? 'me' : 'call' })
  const fxc = (t, name, gain = 1, rate = 1, dur = 0) => clips[name] && ev.push({ t, name, gain, rate, dur, bus: 'call' })
  // The same, placed by where the sound's loudest moment should fall.
  const peak = (t, name, gain = 1, rate = 1) => fxc(t - (clips[name]?.peak ?? 0) / rate, name, gain, rate)
  const ui = (t, name, gain = 1, rate = 1, dur = 0) => clips[name] && ev.push({ t, name, gain, rate, dur, bus: 'ui' })
  // A click of his own never lands on a syllable somebody is in the middle of: it is moved, by
  // up to 0.28 s, into the nearest gap between words. (The picture keeps its own time. The
  // clicks that matter are already between words: quiet(), above.)
  const gaps = []
  for (const l of S.lines) for (let i = 0; i < l.words.length; i++) gaps.push([l.t0 + l.words[i][1], l.t0 + l.words[i][2]])
  const clear2 = (t) => {
    for (let d = 0; d < 0.285; d += 0.02) for (const k of d ? [-d, d] : [0]) if (!gaps.some(([a, b]) => t + k > a - 0.01 && t + k < b + 0.01)) return t + k
    return t
  }
  // (click: perf/pacing-lint.mjs checks every one of these against the words.)
  const click = (t, name = 'sfx_click', gain = 1, rate = 1) => { ui(clear2(t), name, gain, rate); ev[ev.length - 1].click = t }
  // The notification's chime; Join; the call opening; and each of the four arriving (quieter
  // each time, Priya's a little flat).
  ui(cue.invite + 0.02, clips.sfx_chime ? 'sfx_chime' : 'sfx_notes', 0.5)
  click(cue.join, 'sfx_click', 1.0)
  fxc(cue.open + 0.02, 'sfx_join', 0.85)
  for (const [who, gain, rate] of [['eng', 0.34, 1.12], ['dev', 0.28, 1.26], ['ops', 0.28, 0.94]]) fxc(cue.joins[who], 'sfx_join', gain, rate)
  ui(cue.offer + 0.02, 'sfx_notes', 0.42, 1.18)
  click(cue.record, 'sfx_click', 1.05)
  ui(cue.rec[0] + 0.12, 'sfx_relay', 0.8, 0.9)
  click(cue.copy)
  click(cue.focus, 'sfx_click', 0.85)
  ui(cue.paste, 'sfx_enter', 0.5, 1.25)
  ui(cue.type[0], 'sfx_keys', 0.8, 1.1, cue.type[1] - cue.type[0] + 0.05)
  click(cue.auto, 'sfx_click', 0.95, 1.08)
  ui(clear2(cue.send), 'sfx_enter', 0.9)
  ev[ev.length - 1].click = cue.send
  ui(cue.play, clips.sfx_clack ? 'sfx_clack' : 'sfx_click', 1.0)
  // OpenCode's own sounds, one for each kind of thing it does, each placed by its loudest
  // moment: an edit going in (as its diff appears); CI going green (as the number lands); three
  // workspaces shredded (as the command runs and Slack's page tears in his browser: one sound
  // for both, because it is one event); and the merge approving itself.
  const oc = (t, name, gain) => clips[name] && ui(t - (clips[name].peak ?? 0), name, gain)
  // (o.quiet: the tick that says a tool has finished is left out where one of those says it.)
  e1.quiet = b1.quiet = true
  for (const o of S.oc) {
    if (o.kind === 'heard') ui(o.t, 'sfx_relay', 0.42, 1.25)
    if (o.kind !== 'tool') continue
    if (o.read) ui(o.run, 'sfx_relay', 0.5)
    if (o.tool === 'Edit') oc(o.run, 'sfx_oc_edit', OC_SFX.edit)
    if (o.done > 0 && o.done < cue.cut && !o.quiet) ui(o.done, 'sfx_tick', o.read ? 0.3 : 0.5)
  }
  oc(m1.t, 'sfx_oc_pass', OC_SFX.pass)
  if (clips.sfx_oc_shred) oc(cue.slackGlitch, 'sfx_oc_shred', OC_SFX.shred)
  else ui(b1.done - 0.1, 'sfx_trash', 0.85)
  oc(cue.approved, 'sfx_oc_auto', OC_SFX.auto)
  // His own clicks in the browser: a tab, a reaction, a workspace, a like.
  for (const v of cue.tabs) {
    if (v.tab > 0) click(v.t, clips.sfx_tab ? 'sfx_tab' : 'sfx_click', 0.8)
    for (const a of v.acts) click(a.t, 'sfx_click', a.act === 'like' ? 1.05 : 0.8, a.act === 'like' ? 0.94 : 1.06)
  }
  for (const t of checks.each) ui(t, 'sfx_tick', 0.6, 1.12)
  for (const t of cue.count) ui(t, clips.sfx_chime ? 'sfx_chime' : 'sfx_tick', 0.6)
  for (const t of S.beats) fxc(t, 'sfx_heart', t > cue.pause ? 0.34 : 0.45 + 0.55 * S.fleshAt(t + 0.3), 0.94 + 0.16 * S.fleshAt(t))
  // Wet, all the way down. Each is placed by its loudest moment, in a gap between lines.
  peak(cue.breathe + 0.5, 'sfx_stretch', 0.55)
  peak(Math.max(cue.wall + 0.25, L.lead_walls.s1 + 0.2), 'sfx_membrane', 0.7)
  peak(cue.skip + 0.15, 'sfx_stretch', 0.5, 0.9)
  // Four tendrils land: four stabs. The slither leads into the first.
  S.jolts = [m + 0.5, m + 0.95, m + 1.5, m + 2.1]
  peak(S.jolts[0] - 0.05, 'sfx_grow', 0.9)
  S.jolts.forEach((t, i) => peak(t, 'sfx_stab', 0.95 - 0.06 * i, 1 - 0.05 * i))
  peak(m + 1.2, 'sfx_membrane', 0.8)
  peak(m + 1.75, 'sfx_throb', 0.7)
  // What their bodies do, under what their mouths do: something tears, something wet gives,
  // bones go (twice, the second time as they are pulled in), and behind the four lines the
  // sound of four people who have run out of words.
  peak(m + 1.3, 'sfx_tear', 0.8)
  peak((m + cue.fuse) / 2 + 0.2, 'sfx_bones', 0.85)
  {
    const from = Math.max(m + 1.1, cue.hit - 0.04 - (clips.sfx_screams?.dur ?? 3))
    fxc(from, 'sfx_screams', 0.5, 1, cue.hit - 0.02 - from)
  }
  peak(cue.fuse - 0.25, 'sfx_tear', 0.9, 0.88)
  peak(cue.fuse + 0.28, 'sfx_bones', 1.0, 0.9)
  // Her glasses go: one dry crack.
  cue.crack = wordAt('eng_glasses', /^my$/i, 0.45) - 0.22
  peak(cue.crack, 'sfx_crack', 0.9)
  peak(cue.fuse + 0.3, 'sfx_stretch', 0.8, 0.86)
  // The hit.
  peak(cue.hit, 'sfx_impact', 1.0)
  peak(cue.hit + 0.02, 'sfx_fuse', 0.9)
  peak(cue.late, 'sfx_squelch', 0.85)
  peak(cue.late + 0.02, 'sfx_stab', 0.7, 0.8)
  peak(cue.hit + 1.15, 'sfx_slime', 0.6, 0.92)
  fxc(cue.merged + 0.25, 'sfx_throb', 0.7, 0.94)
  click(cue.closeTab, 'sfx_click', 0.9)
  // Pause: its own clack, then nothing, then one drip.
  ui(cue.pause - 0.012, clips.sfx_clack ? 'sfx_clack' : 'sfx_click', 1.1, 0.94)
  if (clips.sfx_drip) ev.push({ t: cue.drip - (clips.sfx_drip.peak ?? 0), name: 'sfx_drip', gain: 0.8, rate: 1, bus: 'ui' })
  // In the quiet it breathes between what it says, and answers each thing he says.
  {
    const hb = L.host_blockers, w = hb.words
    // (in the pauses between its three phrases)
    const pauses = []
    for (let i = 1; i < w.length; i++) if (w[i][1] - w[i - 1][2] > 0.3) pauses.push(hb.t0 + (w[i - 1][2] + w[i][1]) / 2)
    if (pauses[0]) peak(pauses[0], 'sfx_slime', 0.5, 0.9)
    if (pauses[1]) peak(pauses[1], 'sfx_squelch', 0.4, 0.8)
    peak(hb.s1 + 0.3, 'sfx_breath', 0.65, 0.84)
    const mb = L.me_bye, mw = mb.words
    const g1 = mw.length > 1 ? mb.t0 + (mw[0][2] + mw[1][1]) / 2 : mb.t0 + 0.5
    const g2 = mw.length > 3 ? mb.t0 + (mw[2][2] + mw[3][1]) / 2 : mb.t0 + 1.2
    peak(g1, 'sfx_gurgle', 0.5, 0.82)
    peak(g2, 'sfx_throb', 0.45, 0.86)
  }
  // The bead lets go of the mass and lands on his headphones. He does not notice.
  if (clips.sfx_drip) fxc(cue.beadDrop + 0.32 - (clips.sfx_drip.peak ?? 0), 'sfx_drip', 0.55, 0.8)
  // The vacuum pulls in under the last second (and a riser a small speaker can play), and
  // both are cut off, with everything else, by Stop.
  fxc(cue.cut - Math.min(1.25, clips.sfx_vacuum.dur * 0.9), 'sfx_vacuum', 1.0)
  if (clips.sfx_riser) fxc(cue.cut - clips.sfx_riser.dur + 0.02, 'sfx_riser', 0.8)
  ui(cue.cut - 0.012, 'sfx_click', 1.15, 0.92)
  ev.push({ t: cue.notes, name: 'sfx_notes', gain: 0.7, rate: 1, bus: 'ui' })
  ev.sort((a, b) => a.t - b.t)
  S.events = ev

  // The beats, in order: what a viewer should be able to say is happening, and when it starts.
  S.acts = [
    { t: cue.invite, name: 'Calendar: Q4 sync, starting now' },
    { t: cue.join, name: 'Join: the call opens, four people arrive' },
    { t: cue.offer, name: 'Hex: Q4 sync detected' },
    { t: cue.record, name: 'Record: the pill, and the transcript' },
    { t: cue.hover, name: 'Copy agent prompt, into OpenCode' },
    { t: cue.send, name: 'Auto-approve, Return' },
    { t: L.lead_wish.s0, name: 'The wish: zero latency' },
    { t: cue.play, name: 'Headphones, Play, a browser' },
    { t: cue.fix1, name: 'CI: 4.5 days to 0.4 s' },
    { t: cue.slackDead, name: 'rm -rf jira slack linear: Slack dies in his browser' },
    { t: cue.wall, name: 'The walls go' },
    { t: cue.merge, name: 'One voice' },
    { t: cue.them, name: 'Four names become Them' },
    { t: cue.pause, name: 'Pause' },
    { t: cue.cut, name: 'Stop' },
    { t: cue.end, name: 'Notes' },
  ]
  return S
}

// Dead air: from the instant of Stop, for 1.4 s, nothing may sound.
export const inSilence = (S, t) => t >= S.cue.dead[0] && t < S.cue.dead[1]

// The conversation, in order: every turn, from its first sound to its last, with how long
// nobody had been making one when it began (negative: it starts over the turn before).
// perf/gaps.mjs prints this.
export function talk(S) {
  const turns = S.lines.filter((l) => !l.under).sort((a, b) => a.s0 - b.s0)
  let end = 0
  return turns.map((l, i) => {
    const gap = i ? l.s0 - end : l.s0
    end = Math.max(end, l.s1)
    return { id: l.id, who: l.who === 'me' ? CAST.me.name : nameOf(l.who), t0: l.s0, t1: l.s1, gap, text: l.text }
  })
}

// The line someone is speaking at t, or null. What the one host says, all four say.
export function lineAt(S, who, t) {
  let best = null
  for (const l of S.lines) if ((l.who === who || (l.who === 'host' && who !== 'me')) && t >= l.t0 - 0.05 && t < l.t1 + 0.12 && (!best || !l.under || best.under)) best = l
  return best
}
// Whoever has the floor at t (not the listener asking): a turn, not an aside.
export function speakerAt(S, t, not) {
  for (let i = S.lines.length - 1; i >= 0; i--) {
    const l = S.lines[i]
    if (!l.under && l.who !== not && l.who !== 'host' && t >= l.s0 - 0.05 && t < l.s1 + 0.2) return l
  }
  return null
}

// How loud a line is at t, 0..1, from its 16 ms envelope.
export function level(line, t) {
  const f = (t - line.t0) / 0.016
  if (f < 0 || f >= line.env.length - 1) return 0
  const i = Math.floor(f), k = f - i
  return (line.env[i] * (1 - k) + line.env[i + 1] * k) / 100
}

// The heart at t: 0..1, a lub and a softer dub.
export function beatAt(S, t) {
  if (t >= S.cue.cut) return 0
  let e = 0
  const B = S.beats
  for (let i = B.length - 1; i >= 0; i--) {
    const u = t - B[i]
    if (u < 0) continue
    if (u > 1.4) break
    e += Math.exp(-u * 9) + (u > 0.17 ? 0.6 * Math.exp(-(u - 0.17) * 11) : 0)
  }
  return Math.min(1, e)
}
// Seconds since the last heartbeat (large if none yet).
export function sinceBeat(S, t) {
  const B = S.beats
  for (let i = B.length - 1; i >= 0; i--) if (B[i] <= t) return t - B[i]
  return 99
}
