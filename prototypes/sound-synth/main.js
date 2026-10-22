import { viewer } from './kit/viewer.js'
import { CUES, SAMPLE_RATE, render } from './synth.js'
import PRESETS from './presets.js'

const NOTES = {
  classic: 'The recorded sounds Hex plays now, for reference. Knobs don’t apply.',
  breath_ping: 'Sine ping over a soft breath, answered by a lower pong. Kit’s pick on D-2.',
  soft_sine: 'A pure sine with a wisp of air; the least noticeable.',
  felt_marimba: 'A heavily muted bar: a soft knock with no ring. Cancel is two falling taps.',
  air_bubble: 'A sine that rises on start and sinks on stop.',
  hum_dot: 'A short, round, low-passed triangle dot; the warmest and lowest.',
  whisper: 'Mostly breath with only a hint of pitch.',
}
const OPTIONS = [{ id: 'classic', name: 'Today' }, ...PRESETS.map((p) => ({ id: p.id, name: p.name }))]
const KNOBS = [
  { key: 'pitch', label: 'Pitch', min: 300, max: 1600, step: 1, fmt: (v) => `${Math.round(v)} Hz` },
  { key: 'interval', label: 'Interval', min: -12, max: 12, step: 0.1, fmt: (v) => `${v.toFixed(1)} st` },
  { key: 'attack', label: 'Attack', min: 1, max: 15, step: 0.5, fmt: (v) => `${v} ms` },
  { key: 'decay', label: 'Decay', min: 15, max: 120, step: 1, fmt: (v) => `${v} ms` },
  { key: 'breath', label: 'Breath', min: 0, max: 3, step: 0.05, fmt: (v) => v.toFixed(2) },
  { key: 'brightness', label: 'Brightness', min: 800, max: 6000, step: 50, fmt: (v) => `${(v / 1000).toFixed(2)} kHz` },
  { key: 'volume', label: 'Volume', min: -45, max: -24, step: 0.5, fmt: (v) => `${v} LUFS` },
]

const st = { option: 'breath_ping', theme: 'dark', params: Object.fromEntries(PRESETS.map((p) => [p.id, structuredClone(p)])) }
const $ = (s) => document.querySelector(s)
const preset = () => st.params[st.option]

// ── Audio (Web Audio; plays only on tap)
try { if (navigator.audioSession) navigator.audioSession.type = 'playback' } catch {}
let ctx
const audio = () => (ctx ??= new (window.AudioContext || window.webkitAudioContext)())
let rendered = {}
const classic = {}
function classicBuffer(cue) {
  classic[cue] ??= fetch(new URL(`./sounds/classic/${cue}.mp3`, import.meta.url)).then((r) => r.arrayBuffer()).then((b) => audio().decodeAudioData(b)).catch(() => null)
  return classic[cue]
}
async function play(cue) {
  const c = audio()
  if (c.state !== 'running') await c.resume().catch(() => {})
  let buffer
  if (st.option === 'classic') buffer = await classicBuffer(cue)
  else {
    const samples = rendered[cue]
    buffer = c.createBuffer(1, samples.length, SAMPLE_RATE)
    buffer.copyToChannel(samples, 0)
  }
  if (!buffer) return
  const src = c.createBufferSource()
  src.buffer = buffer
  src.connect(c.destination)
  src.start()
  flash(cue)
}
function flash(cue) { const b = $(`.pad[data-cue="${cue}"]`); b.classList.add('hit'); setTimeout(() => b.classList.remove('hit'), 120) }

// ── Rendering and drawing
function draw(canvas, samples) {
  const dpr = devicePixelRatio || 1, w = canvas.clientWidth * dpr, h = canvas.clientHeight * dpr
  canvas.width = w; canvas.height = h
  const g = canvas.getContext('2d')
  g.clearRect(0, 0, w, h)
  if (!samples) return
  g.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--wave')
  const span = Math.round(0.25 * SAMPLE_RATE) // every pad shares a 250 ms axis
  let peak = 0; for (const v of samples) peak = Math.max(peak, Math.abs(v))
  const scale = (h / 2) * 0.9 / Math.max(peak, 1e-6)
  for (let x = 0; x < w; x++) {
    const a = Math.floor((x / w) * span), b = Math.floor(((x + 1) / w) * span)
    let m = 0; for (let i = a; i < b && i < samples.length; i++) m = Math.max(m, Math.abs(samples[i]))
    g.fillRect(x, h / 2 - m * scale, 1, Math.max(1, 2 * m * scale))
  }
}
function renderAll() {
  const off = st.option === 'classic'
  rendered = off ? {} : Object.fromEntries(CUES.map((cue) => [cue, render(preset(), cue)]))
  for (const cue of CUES) {
    const pad = $(`.pad[data-cue="${cue}"]`), s = rendered[cue]
    draw(pad.querySelector('canvas'), s)
    pad.querySelector('.meta').textContent = s ? `${Math.round((s.length / SAMPLE_RATE) * 1000)} ms` : 'recorded'
  }
}

// ── Knobs
function buildKnobs() {
  const card = $('#knobs')
  card.innerHTML = ''
  for (const k of KNOBS) {
    const row = document.createElement('div')
    row.className = 'knob'
    row.innerHTML = `<label for="k-${k.key}">${k.label}</label><input id="k-${k.key}" type="range" min="${k.min}" max="${k.max}" step="${k.step}"><output></output>`
    const input = row.querySelector('input'), out = row.querySelector('output')
    input.addEventListener('input', () => { preset()[k.key] = +input.value; out.textContent = k.fmt(+input.value); renderAll() })
    card.append(row)
  }
}
function syncKnobs() {
  const off = st.option === 'classic'
  $('#knobs').classList.toggle('off', off)
  const p = off ? PRESETS[0] : preset()
  for (const k of KNOBS) {
    const input = $(`#k-${k.key}`)
    input.value = p[k.key]
    input.nextElementSibling.textContent = k.fmt(p[k.key])
  }
  $('#copy').disabled = off; $('#reset').disabled = off
}

function update() {
  const opt = OPTIONS.find((o) => o.id === st.option)
  document.documentElement.dataset.theme = st.theme
  $('#name').textContent = opt.name
  $('#note').textContent = NOTES[st.option] ?? ''
  $('#json').hidden = true
  syncKnobs()
  renderAll()
}

buildKnobs()
document.querySelectorAll('.pad').forEach((b) => b.addEventListener('pointerdown', () => play(b.dataset.cue)))
$('#sequence').addEventListener('click', () => { play('start'); setTimeout(() => play('stop'), 1000) })
$('#reset').addEventListener('click', () => { st.params[st.option] = structuredClone(PRESETS.find((p) => p.id === st.option)); update() })
$('#copy').addEventListener('click', async () => {
  const json = JSON.stringify(preset(), null, 2)
  const area = $('#json')
  area.value = json; area.hidden = false
  let copied = false
  try { await navigator.clipboard.writeText(json); copied = true } catch { area.select(); try { copied = document.execCommand('copy') } catch {} }
  pv.toast?.(copied ? 'Preset copied' : 'Select the JSON to copy it')
  const changed = KNOBS.filter((k) => preset()[k.key] !== PRESETS.find((p) => p.id === st.option)[k.key]).map((k) => `${k.label} ${k.fmt(preset()[k.key])}`)
  pv.report({ kind: 'preset', summary: `${preset().name}${changed.length ? `: ${changed.join(', ')}` : ' (unchanged)'}`, data: preset() })
})

const pv = viewer({
  key: 'hex-sound-tuner-1', title: 'Hex Sound Tuner', feature: 'HEX-2',
  stage: '#stage', fit: 'width', width: 440, desktop: 'canvas',
  theme: () => st.theme,
  background: () => (st.theme === 'dark' ? '#0b0b0c' : '#f5f5f7'),
  options: OPTIONS.map((o, i) => ({ id: o.id, letter: String.fromCharCode(65 + i), name: o.name, note: NOTES[o.id] })),
  current: () => st.option,
  select: (id) => { st.option = id; update() },
  keyGuard: () => document.activeElement?.type !== 'range',
  quick: { icon: 'play', label: 'Play start · stop', run: () => $('#sequence').click() },
  settings: [{ title: 'Display', items: [
    { label: 'Appearance', choices: [{ id: 'dark', label: 'Dark' }, { id: 'light', label: 'Light' }], value: () => st.theme, set: (id) => { st.theme = id; update() } },
  ] }],
  about: 'Procedural Hex sounds: each set is a small synth recipe (pitch, glide, envelope, filtered-noise breath, low-pass), rendered by the same algorithm Hex runs in Rust. Tap a pad to hear it, turn the knobs, then Copy preset.',
})
update()
window.__tuner = { st, pv, render }
