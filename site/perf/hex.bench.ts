import { bench, web } from "@hona/strobe-bench"

// The landing page as a desktop visitor sees it: the prerendered page, then the show engine (WebGPU) takes over the
// head once the browser is idle. data-live goes on the page once the engine has drawn the head.
const url = "/hex/"
const live = "main[data-live]"

export default bench({
  targets: {
    web: web({
      viewport: { width: 1440, height: 900 },
      url,
      prepare: ({ playwright }) =>
        playwright.page.addInitScript(() => {
          // The show's own frames: one swapchain texture per drawn frame, as count.frame.
          if (typeof GPUCanvasContext === "undefined") return
          const take = GPUCanvasContext.prototype.getCurrentTexture
          GPUCanvasContext.prototype.getCurrentTexture = function () {
            ;(globalThis as { __strobeCount?: (name: string) => void }).__strobeCount?.("frame")
            return take.call(this)
          }
        }),
    }),
  },
  cases: {
    // A returning visit, from navigation until the engine has drawn the head.
    "load to live head": {
      primary: "run.duration",
      metrics: ["main.work"],
      run: ({ page }) => page.goto(url, { until: live }),
    },
    // The idle head breathing before anyone presses the key.
    "idle head 8 s": {
      primary: "main.busy",
      metrics: ["main.work", "main.gc", "count.frame", "cpu.app.total", "gpu.process.3d", "frames.dropped.pct"],
      run: ({ page }) => page.wait(8000),
    },
    // Press the option key: the first 20 s of the show (calm, the first OpenCode prompts).
    "show opening 20 s": {
      primary: "main.busy",
      metrics: ["main.work", "main.gc", "count.frame", "cpu.app.total", "gpu.process.3d"],
      run: async ({ page }) => {
        await page.click(".keycap")
        await page.wait(20_000)
      },
      reset: ({ page }) => page.goto(url, { until: live }),
    },
  },
  async setup({ page }) {
    await page.waitFor(live, { timeout: 30_000 })
  },
})
