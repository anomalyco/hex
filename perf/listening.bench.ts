// Hex listening at rest (docs/perf.md). The copy turns its listener on: it
// installs the keyboard hook and loads the default English model, copied
// from this machine's own Hex install (seeds/listening.ts), but records
// nothing. The profiles move its shortcuts to F19 and F18, so keys pressed
// on a machine in use start nothing; the hooks still see every key and
// mouse move, so runs on an idle machine read lowest.
// - `released`: the microphone released while idle (`hex-listening`).
// - `warm`: the microphone open, the default (`hex-listening-warm`); this
//   opens the machine's real microphone.
import { bench, native } from "@hona/strobe"
import { existsSync, readFileSync } from "node:fs"
import { join } from "node:path"
import { listeningSeed } from "./seeds/listening"

if (!listeningSeed()) throw new Error("listening.bench.ts needs Hex's English model installed on this machine")

const home = "seeds/.generated/listening"
const quiet = ["cpu.wakes", "cpu.app.total", "memory.app.private"]

/** Resolves once the copy's event log (`live.ndjson`) shows its listener
 * started. Windows and Linux write `session_started` after the model has
 * loaded and warmed; macOS writes it before. */
async function listenerStarted(ctx: { home: string | null; page: { wait(ms: number): Promise<unknown> } }) {
  const events = join(ctx.home!, "hex-data", "logs", "live.ndjson")
  const deadline = Date.now() + 60_000
  while (!(existsSync(events) && readFileSync(events, "utf8").includes('"kind":"session_started"'))) {
    if (Date.now() > deadline) throw new Error("the listener did not start in 60 s")
    await ctx.page.wait(100)
  }
}

export default bench({
  targets: {
    released: native("hex-listening", { args: ["--hidden"], home, window: false }),
    warm: native("hex-listening-warm", { args: ["--hidden"], home, window: false }),
  },
  // The model loads and the listener settles first: at least 10 s, and 2 s
  // after the listener started. A new build's first launch compiles the GPU
  // driver's shaders while the model loads, 5 s of CPU and more on a busy
  // machine; with a fixed wait, that ran into the measured runs.
  async setup(ctx) {
    const started = Date.now()
    await listenerStarted(ctx)
    await ctx.page.wait(Math.max(2_000, 10_000 - (Date.now() - started)))
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
