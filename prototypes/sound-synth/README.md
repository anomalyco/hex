# Sound synth

`synth.js` is the reference implementation of Hex's procedural feedback tones;
`src/sound_synth.rs` mirrors it step for step. Recipes live in
`resources/sounds/presets.json`; the tuner's "Copy preset" output is one entry.

- `bun prototypes/sound-synth/render.ts` rewrites `tests/fixtures/sound_synth`
  after a change to the algorithm or the presets (the Rust parity test reads it).
- `bun prototypes/sound-synth/render.ts <dir>` also writes WAVs for offline checks.

The tuner page (`index.html`, `main.js`) runs on the Problems viewer kit. To
publish it, stage a folder with these files plus `kit/` (copied from the
team-board repo), `presets.js` (`export default` + `presets.json`) and
`sounds/classic/{start,stop,cancel}.mp3`, then `board.ts publish <dir> --include sounds`.
