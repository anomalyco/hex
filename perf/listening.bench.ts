// Hex listening at rest (docs/perf.md). The copy turns its listener on: it
// installs the keyboard hook and loads the default English model, copied
// from this machine's own Hex install (seeds/listening.ts), but records
// nothing. Leave the dictation shortcut alone while it runs.
// - `released`: the microphone released while idle (`hex-listening`).
// - `warm`: the microphone open, the default (`hex-listening-warm`); this
//   opens the machine's real microphone.
import { bench, native } from "@hona/strobe"
import { listeningSeed } from "./seeds/listening"

if (!listeningSeed()) throw new Error("listening.bench.ts needs Hex's English model installed on this machine")

const home = "seeds/.generated/listening"
const quiet = ["cpu.wakes", "cpu.app.total", "memory.app.private"]

export default bench({
  targets: {
    released: native("hex-listening", { args: ["--hidden"], home, window: false }),
    warm: native("hex-listening-warm", { args: ["--hidden"], home, window: false }),
  },
  // The model loads and the listener settles first.
  async setup({ page }) {
    await page.wait(10_000)
  },
  cases: {
    "listening-at-rest": {
      primary: "cpu.work",
      metrics: quiet,
      async run({ page }) {
        await page.wait(8_000)
      },
    },
  },
})
