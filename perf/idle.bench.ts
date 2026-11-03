// Hex at rest, measured from outside (docs/perf.md): no input, no listener.
// Each run waits 8 s; strobe counts the process's CPU, thread wakeups, GPU
// and memory over it.
import { bench, native } from "@hona/strobe"

const quiet = ["cpu.wakes", "cpu.app.total", "gpu.process.3d", "memory.app.private"]

export default bench({
  targets: {
    // The Settings window open on General.
    window: native("hex", { bridge: false }),
    // A login launch: the tray only, no window.
    tray: native("hex", { args: ["--hidden"], bridge: false }),
  },
  // Startup work (fonts, GPU, first frames) finishes before any run.
  async setup({ page }) {
    await page.wait(5_000)
  },
  cases: {
    "window-open": {
      targets: ["window"],
      primary: "cpu.work",
      metrics: quiet,
      async run({ page }) {
        await page.wait(8_000)
      },
    },
    "window-closed": {
      targets: ["tray"],
      primary: "cpu.work",
      metrics: quiet,
      async run({ page }) {
        await page.wait(8_000)
      },
    },
  },
})
