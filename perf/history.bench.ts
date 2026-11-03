// The History pane over a seeded store (docs/perf.md). Driven through the
// UI toolkit's bridge (a build with the `strobe` feature).
import { bench, native } from "@hona/strobe"
import { historySeed } from "./seeds/history"

const sizes = { run: [10_000], more: [200, 2_000] } as const
for (const count of [...sizes.run, ...sizes.more]) historySeed(count)

export default bench({
  targets: {
    window: native("hex", { home: "seeds/.generated/history-{entries}" }),
  },
  params: { entries: sizes },
  // Every case starts on History; `searching-at-rest` with a query that
  // matches nothing, so each search scans every entry.
  async setup({ page, case: name }) {
    await page.click("nav-history", { until: "history-search" })
    if (name === "searching-at-rest") {
      await page.type("history-search", "zzzz", { until: "idle" })
    }
    await page.idle()
  },
  cases: {
    // The pane open and still: its poll runs on the window's 250 ms tick.
    "open-at-rest": {
      primary: "ui.cpu",
      metrics: ["cpu.work", "cpu.wakes", "ui.work"],
      async run({ page }) {
        await page.wait(8_000)
      },
    },
    "searching-at-rest": {
      primary: "ui.cpu",
      metrics: ["cpu.work", "ui.work"],
      async run({ page }) {
        await page.wait(8_000)
      },
    },
    "type-search": {
      primary: "ui.work",
      metrics: ["ui.cpu", "ui.frames", "ui.interval.p95"],
      async run({ page }) {
        await page.type("history-search", "cache key", { delay: 60, until: "idle" })
      },
      async reset({ page }) {
        await page.click("history-search")
        await page.press("ControlOrMeta+a")
        await page.press("Backspace", { until: "idle" })
      },
    },
    scroll: {
      primary: "ui.work",
      metrics: ["ui.cpu", "ui.frames", "ui.interval.p95", "ui.interval.max"],
      async run({ page, phase }) {
        await phase("down", () => page.scroll("history-scroll", { by: 6_000, speed: 3_000 }, { until: "idle" }))
        await phase("up", () => page.scroll("history-scroll", { by: -6_000, speed: 3_000 }, { until: "idle" }))
      },
    },
  },
})
