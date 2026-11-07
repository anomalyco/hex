// Hex at rest, measured from outside (docs/perf.md): no input, no listener.
// Each run waits 8 s; strobe counts the process's CPU, thread wakeups and
// memory over it.
import { bench, native, type Page } from "@hona/strobe"

// General fills in its switches after the first frames: Launch at login once
// the OS answers for the login item ("Checking…" until then), and on macOS
// the first reads of the tree can miss them. strobe compares every run's
// start with run 1's (ResetIncomplete), so the window's runs start only once
// the switches have held still for 1.5 s.
async function generalSettled(page: Page) {
  await page.waitFor("basics", { timeout: 30_000 })
  let last = ""
  for (let same = 0, tries = 0; same < 3 && tries < 60; tries++) {
    const checking = (await page.query("login-item-status")).some((e) => e.name.includes("Checking"))
    const toggles = [...(await page.query("role=switch")), ...(await page.query("role=checkbox"))]
    const now = JSON.stringify(toggles.map((e) => [e.path, e.checked]))
    same = !checking && now === last ? same + 1 : 0
    last = now
    await page.wait(500)
  }
}

export default bench({
  targets: {
    // The Settings window open on General. Needs an unlocked session. A block
    // on another screen (a first-run dialog, another pane) is refused.
    window: native("hex", { expect: { title: "Hex", heading: "General" } }),
    // A login launch: the tray only, no window. Runs on a locked session too.
    tray: native("hex", { args: ["--hidden"], window: false }),
  },
  // Startup work (fonts, GPU, first frames) finishes before any run.
  async setup({ page, target }) {
    if (target === "window") await generalSettled(page)
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
    // The same tray, judged by what it holds. The capture capsule, picker,
    // and ink wait hidden from launch; a GPU device, and their swapchains,
    // count here and in gpu.vram. memory.app.host is Hex's own memory
    // without the VRAM a driver maps into the process.
    "window-closed-memory": {
      targets: ["tray"],
      primary: "memory.app.private",
      metrics: ["cpu.wakes", "cpu.work", "gpu.vram", "memory.app.host"],
      async run({ page }) {
        await page.wait(8_000)
      },
    },
  },
})
