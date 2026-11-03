// Switching Settings panes (docs/perf.md). Driven through the UI toolkit's
// bridge (a build with the `strobe` feature).
import { bench, native } from "@hona/strobe"

export default bench({
  targets: {
    window: native("hex"),
  },
  async setup({ page }) {
    await page.waitFor("nav-general")
    await page.idle()
  },
  cases: {
    "pane-switch": {
      primary: "ui.work",
      metrics: ["ui.cpu", "ui.view", "ui.layout", "ui.text", "ui.frames", "cpu.work"],
      async run({ page }) {
        await page.click("nav-output", { until: "idle" })
        await page.click("nav-history", { until: "history-pane" })
        await page.click("nav-annotate", { until: "idle" })
        await page.click("nav-general", { until: "idle" })
      },
    },
    // The window at rest: the 250 ms tick runs the window's poll and nothing else.
    "window-at-rest": {
      primary: "ui.cpu",
      metrics: ["cpu.work", "cpu.wakes"],
      async run({ page }) {
        await page.wait(8_000)
      },
    },
  },
})
