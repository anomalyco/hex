// Hex at rest, measured from outside (docs/perf.md): no input, no listener.
// Each run waits 8 s; strobe counts the process's CPU, thread wakeups and
// memory over it.
import { bench, native } from "@hona/strobe"

export default bench({
  targets: {
    // The Settings window open on General. Needs an unlocked session.
    window: native("hex", { bridge: false }),
    // A login launch: the tray only, no window. Runs on a locked session too.
    tray: native("hex", { args: ["--hidden"], window: false, bridge: false }),
  },
  // Startup work (fonts, GPU, first frames) finishes before any run.
  async setup({ page }) {
    await page.wait(5_000)
  },
  cases: {
    "window-open": {
      targets: ["window"],
      primary: "cpu.work",
      metrics: ["cpu.wakes", "cpu.app.total", "gpu.process.3d", "memory.app.private"],
      async run({ page }) {
        await page.wait(8_000)
      },
    },
    "window-closed": {
      targets: ["tray"],
      primary: "cpu.work",
      metrics: ["cpu.wakes", "cpu.app.total", "memory.app.private"],
      async run({ page }) {
        await page.wait(8_000)
      },
    },
    // The same tray, judged by what it holds: the GPU device and the
    // surfaces of the overlays that wait hidden (the capture capsule and
    // ink are made at launch, and the picker too except on Windows).
    // `gpu.vram` is the surfaces' dedicated GPU memory.
    "window-closed-memory": {
      targets: ["tray"],
      primary: "memory.app.private",
      metrics: ["gpu.vram", "cpu.wakes", "cpu.work"],
      async run({ page }) {
        await page.wait(8_000)
      },
    },
  },
})
