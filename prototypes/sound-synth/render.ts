// Renders every preset with the reference JS synth.
//   bun prototypes/sound-synth/render.ts fixtures   rewrites tests/fixtures/sound_synth (Rust parity test)
//   bun prototypes/sound-synth/render.ts <dir>      also writes 16-bit WAVs to <dir> for offline checks
import { mkdirSync, readFileSync, writeFileSync } from 'fs'
import { join } from 'path'
import { CUES, SAMPLE_RATE, render } from './synth.js'

const root = join(import.meta.dir, '../..')
const presets = JSON.parse(readFileSync(join(root, 'resources/sounds/presets.json'), 'utf8'))
const fixtures = join(root, 'tests/fixtures/sound_synth')
const target = process.argv[2] ?? 'fixtures'

function wav(path: string, samples: Float32Array) {
  const data = Buffer.alloc(samples.length * 2)
  samples.forEach((v, i) => data.writeInt16LE(Math.round(Math.max(-1, Math.min(1, v)) * 32767), i * 2))
  const h = Buffer.alloc(44)
  h.write('RIFF', 0); h.writeUInt32LE(36 + data.length, 4); h.write('WAVE', 8); h.write('fmt ', 12); h.writeUInt32LE(16, 16)
  h.writeUInt16LE(1, 20); h.writeUInt16LE(1, 22); h.writeUInt32LE(SAMPLE_RATE, 24); h.writeUInt32LE(SAMPLE_RATE * 2, 28)
  h.writeUInt16LE(2, 32); h.writeUInt16LE(16, 34); h.write('data', 36); h.writeUInt32LE(data.length, 40)
  writeFileSync(path, Buffer.concat([h, data]))
}

mkdirSync(fixtures, { recursive: true })
const summary: Record<string, Record<string, { frames: number; sum: number; energy: number; peak: number }>> = {}
for (const preset of presets) {
  summary[preset.id] = {}
  for (const cue of CUES) {
    const samples = render(preset, cue)
    let sum = 0, energy = 0, peak = 0
    for (const v of samples) { sum += v; energy += v * v; peak = Math.max(peak, Math.abs(v)) }
    summary[preset.id][cue] = { frames: samples.length, sum, energy, peak }
    if (preset.id === 'breath_ping') writeFileSync(join(fixtures, `${preset.id}-${cue}.f32`), Buffer.from(samples.buffer))
    if (target !== 'fixtures') { mkdirSync(join(target, preset.id), { recursive: true }); wav(join(target, preset.id, `${cue}.wav`), samples) }
  }
}
writeFileSync(join(fixtures, 'summary.json'), JSON.stringify(summary, null, 2) + '\n')
console.log(`rendered ${presets.length} presets`)
