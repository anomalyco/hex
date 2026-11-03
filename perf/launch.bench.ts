// Cold launch to the first painted Settings window (docs/perf.md): process
// start, fonts, GPU device and pipelines, and the first frames.
import { bench, native } from "@hona/strobe"

export default bench({
  targets: {
    window: native("hex", { bridge: false }),
  },
  cases: {
    "cold-launch": {
      primary: "launch.ready",
      metrics: ["launch.settled", "launch.api", "cpu.work", "memory.app.private"],
      launch: {
        until: { window: "painted" },
        milestones: {
          // The local API starts on its own thread while the window opens.
          api: { match: "local API started" },
        },
      },
    },
  },
})
