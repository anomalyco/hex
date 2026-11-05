// Launches (docs/perf.md): process start, fonts, GPU device and pipelines,
// and the first frames.
import { bench, native } from "@hona/strobe"

const api = { match: "local API started" }
// The window's compositor logs its first frame and the backend it drew with.
const drawn = { match: "window draws" }

export default bench({
  targets: {
    // Needs an unlocked session: it ends on the window's first painted frame.
    // A returning user's launch: the GPU shader caches stay warm (strobe's
    // default), as after any earlier launch (docs/perf.md).
    window: native("hex"),
    // A login launch with no window. Runs on a locked session too.
    tray: native("hex", { args: ["--hidden"], window: false }),
  },
  cases: {
    "cold-launch": {
      targets: ["window"],
      primary: "launch.ready",
      metrics: ["launch.settled", "launch.api", "cpu.work", "memory.app.private"],
      launch: {
        until: { window: "painted" },
        // The local API starts on its own thread while the window opens.
        milestones: { api },
      },
    },
    // To the window's first frame drawn, from Hex's own log: needs no screen
    // capture (macOS refuses `painted` without Screen Recording). It covers
    // the GPU device and pipelines the window draws with.
    "first-frame": {
      targets: ["window"],
      primary: "launch.ready",
      metrics: ["launch.api", "cpu.work", "memory.app.private"],
      launch: {
        until: { milestone: "drawn" },
        milestones: { drawn, api },
      },
    },
    // To the local API answering: the runtime, tray, and overlays are up.
    "hidden-launch": {
      targets: ["tray"],
      primary: "launch.ready",
      metrics: ["cpu.work", "memory.app.private"],
      launch: {
        until: { milestone: "api" },
        milestones: { api },
      },
    },
  },
})
