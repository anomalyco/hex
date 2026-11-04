// Using the Settings window (docs/perf.md), driven as a person would: strobe
// finds each control in Hex's accessibility tree by its key (UI Automation's
// AutomationId) and posts mouse and key input to the window. The home holds
// a full History (seeds/history.ts). Needs an unlocked session: pointer input
// and presented frames don't reach a locked one.
import { bench, native } from "@hona/strobe"
import { historySeed } from "./seeds/history"

const home = historySeed()
// The UI thread's CPU decides "does this cost less"; what reached the screen
// and how soon are the person's view of it.
const ui = ["interaction.p50", "interaction.p95", "frames.dropped", "cpu.work", "memory.app.private"]

export default bench({
  targets: {
    window: native("hex", { home, viewport: { width: 980, height: 760 } }),
  },
  // Fonts, the GPU and the first frames settle, then each case starts from
  // its pane.
  async setup({ page, case: name }) {
    await page.waitFor("basics", { timeout: 30_000 })
    await page.idle()
    if (name !== "switch-panes" && name !== "model-picker") {
      await page.click("nav-history", { until: "history-list" })
      await page.idle()
    }
  },
  cases: {
    // General → History (200 rows of 10,000) → Output → General.
    "switch-panes": {
      primary: "ui.cpu",
      metrics: ui,
      async run({ page }) {
        await page.click("nav-history", { until: "history-list" })
        await page.click("nav-output", { until: "output-content" })
        await page.click("nav-general", { until: "basics" })
      },
    },
    // Down History's list and back, at a trackpad's pace.
    "history-scroll": {
      primary: "ui.cpu",
      metrics: ["scroll.latency.p50", "scroll.latency.p95", "frames.dropped", "cpu.work"],
      async run({ page, phase }) {
        await phase("down", () => page.scroll("history-scroll", { by: 6_000, speed: 3_000 }))
        await phase("up", () => page.scroll("history-scroll", { by: -6_000, speed: 3_000 }))
      },
    },
    // A search typed into History: each key searches all 10,000 entries.
    "history-search": {
      primary: "ui.cpu",
      metrics: ui,
      run: ({ page }) => page.type("history-search", "cache key", { delay: 120 }),
      async reset({ page }) {
        await page.press("Control+a")
        await page.press("Backspace")
        await page.idle()
      },
    },
    // The speech model picker: open it, its list of every model, back to the
    // comparison, and close. Choosing a model would download or load one,
    // which is the network's and the model's cost, not the window's.
    "model-picker": {
      primary: "ui.cpu",
      metrics: ui,
      async run({ page }) {
        await page.click("transcription-control", { until: "transcription-picker" })
        await page.click("more-models", { until: "all-view" })
        await page.click("picker-back", { until: "compare-view" })
        await page.click("close-picker", { until: "transcription-control" })
      },
    },
  },
})
