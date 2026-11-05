// Using the Settings window (docs/perf.md), driven as a person would: strobe
// finds each control in Hex's accessibility tree by its key (UI Automation's
// AutomationId) and posts mouse and key input to the window. The home holds
// a full History (seeds/history.ts). Needs an unlocked session: pointer input
// and presented frames don't reach a locked one.
import { bench, native, type Page } from "@hona/strobe"
import { historySeed } from "./seeds/history"

const home = historySeed()
// A sidebar item, clicked.
const open = (page: Page, item: string, until: string) => page.click(item, { until })
// History's list `by` pixels at 3,000 px/s.
const scroll = (page: Page, by: number) => page.scroll("history-scroll", { by, speed: 3_000 })
// The UI thread's CPU decides "does this cost less"; what reached the screen
// and how soon are the person's view of it. The GPU's share says what each
// frame costs it (macOS reports it without the Screen Recording frames need).
const ui = ["interaction.p50", "interaction.p95", "frames.dropped", "cpu.work", "gpu.process.3d", "memory.app.private"]

export default bench({
  targets: {
    // Each case names the pane its blocks show before and after the runs; a
    // block on another screen (a first-run dialog) is refused, not timed.
    window: native("hex", { home, viewport: { width: 980, height: 760 }, expect: { title: "Hex" } }),
  },
  // Fonts, the GPU and the first frames settle, then each case starts from
  // its pane.
  async setup({ page, case: name }) {
    await page.waitFor("basics", { timeout: 30_000 })
    await page.idle()
    if (name !== "switch-panes" && name !== "model-picker") {
      await open(page, "nav-history", "history-list")
      await page.idle()
    }
  },
  cases: {
    // General → History (200 rows of 10,000) → Output → General.
    "switch-panes": {
      expect: { heading: "General" },
      primary: "ui.cpu",
      metrics: ui,
      async run({ page }) {
        await open(page, "nav-history", "history-list")
        await open(page, "nav-output", "output-content")
        await open(page, "nav-general", "basics")
      },
    },
    // Down History's list and back, at a trackpad's pace.
    "history-scroll": {
      expect: { heading: "History" },
      primary: "ui.cpu",
      metrics: ["scroll.latency.p50", "scroll.latency.p95", "frames.dropped", "cpu.work", "gpu.process.3d"],
      async run({ page, phase }) {
        await phase("down", () => scroll(page, 6_000))
        await phase("up", () => scroll(page, -6_000))
      },
    },
    // A search typed into History: each key searches all 10,000 entries.
    "history-search": {
      expect: { heading: "History" },
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
      expect: { heading: "General" },
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
