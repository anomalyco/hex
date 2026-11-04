// Dictation end to end (docs/perf.md). The copy hears `speech.wav`, one
// spoken sentence and then silence, instead of a microphone
// (HEX_AUDIO_FIXTURE, seeds/dictation.ts), so nobody is recorded. The bench
// drives it through its local API as `hex dictation` does: `start` as the
// sentence begins, then `stop --print` in the silence after it, which
// returns the processed text instead of pasting it. Nothing reaches another
// app. Only bench builds that have HEX_AUDIO_FIXTURE (#1230): an
// older one opens the real microphone, which setup notices too late.
// - `warm`: the microphone open while idle, the default (`hex-dictation`).
// - `released`: Release microphone while idle (`hex-dictation-released`).
import { bench, native } from "@hona/strobe"
import { closeSync, openSync, readFileSync, readSync, statSync } from "node:fs"
import { join } from "node:path"
import { dictationSeed, SILENCE_MS, speechMs } from "./seeds/dictation"

const home = dictationSeed()
if (!home) throw new Error("dictation.bench.ts needs Hex's English model installed and a text-to-speech voice")

const SPEECH_MS = speechMs(home)
/** The fixture's length: it loops. */
const PERIOD_MS = SPEECH_MS + SILENCE_MS
/** Each dictation stops this long into the silence after the sentence. */
const STOP_AFTER_MS = SPEECH_MS + 500

type Ctx = { home: string; target: string; page: { wait(ms: number): Promise<unknown> } }
type Reply = { status: string; text?: string; code?: string; message?: string }

/** One `POST /control` to the copy, as the `hex` CLI sends it (docs/cli.md). */
async function control(ctx: Ctx, command: Record<string, unknown>): Promise<Reply> {
  const api = JSON.parse(readFileSync(join(ctx.home, "hex-data", "local-api.json"), "utf8"))
  const response = await fetch(`http://127.0.0.1:${api.port}/control`, {
    method: "POST",
    headers: { authorization: `Bearer ${api.token}`, "content-type": "application/json" },
    body: JSON.stringify({ version: 4, ...command }),
  })
  return (await response.json()) as Reply
}

const stop = (ctx: Ctx) => control(ctx, { command: "dictation", action: "stop", print: true })

/** When the fixture last opened (epoch ms), from the copy's log: it plays from its start at each open. */
function fixtureOpened(ctx: Ctx): number {
  const log = readFileSync(join(ctx.home, "hex-data", "logs", "process.log"), "utf8")
  const opened = [...log.matchAll(/^\S*?(\d{4}-\d\d-\d\dT[\d:.]+Z).*HEX_AUDIO_FIXTURE replaces the microphone/gm)]
  if (!opened.length) throw new Error("the copy never opened its audio fixture")
  return Date.parse(opened[opened.length - 1][1])
}

/** Starts a dictation as the sentence begins. A released microphone opens
 * the fixture at its start; an open one waits for the loop to come round. */
async function start(ctx: Ctx): Promise<Reply> {
  if (ctx.target === "warm") {
    const into = (Date.now() - fixtureOpened(ctx)) % PERIOD_MS
    await ctx.page.wait(PERIOD_MS - into)
  }
  return control(ctx, { command: "dictation", action: "start" })
}

/** The copy's event log (`live.ndjson`), whose states say when a recording began. */
const events = (ctx: Ctx) => join(ctx.home, "hex-data", "logs", "live.ndjson")

/** Resolves once the event log, read from byte `from` on, shows the
 * recording began. A released microphone's start reports `dictating` twice:
 * at the command, and at the opened microphone's first chunk, where the
 * recording begins. The log is written as each event happens; this reads it
 * every millisecond. */
async function recordingBegan(ctx: Ctx, from: number): Promise<void> {
  const deadline = Date.now() + 5_000
  const fd = openSync(events(ctx), "r")
  try {
    let text = ""
    for (;;) {
      const chunk = Buffer.alloc(64 * 1024)
      const read = readSync(fd, chunk, 0, chunk.length, from + Buffer.byteLength(text))
      text += chunk.toString("utf8", 0, read)
      if ((text.match(/"state":"dictating"/g) ?? []).length >= 2) return
      if (Date.now() > deadline) throw new Error("the recording never began")
      await Bun.sleep(1)
    }
  } finally {
    closeSync(fd)
  }
}

async function printed(reply: Reply): Promise<string> {
  if (reply.status !== "transcript" || !reply.text) throw new Error(`stop --print answered ${JSON.stringify(reply)}`)
  return reply.text
}

// Windows opens no window with `--hidden`; the macOS app opens none once set
// up, and `--device` makes a build without the fixture fail to open the
// microphone instead of recording it.
const args = process.platform === "win32" ? ["--hidden"] : ["--device", "Audio fixture"]

export default bench({
  targets: {
    warm: native("hex-dictation", { args, home, window: false }),
    released: native("hex-dictation-released", { args, home, window: false }),
    // Released and idle between dictations. ggml keeps a Metal model's
    // memory wired for 3 minutes after its last use; 2 s lets a run see what
    // an idle Hex does once that has run out.
    idle: native("hex-dictation-released", { args, home, window: false, env: { GGML_METAL_RESIDENCY_KEEP_ALIVE_S: "2" } }),
  },
  // The model loads and the listener starts; one dictation then warms the
  // whole path (stream, model, output).
  async setup(ctx) {
    const deadline = Date.now() + 60_000
    for (;;) {
      const reply = await control(ctx, { command: "dictation", action: "start" }).catch((error) => ({ status: "error", message: String(error) }) as Reply)
      if (reply.status === "done") break
      if (Date.now() > deadline) throw new Error(`dictation start answered ${JSON.stringify(reply)}`)
      await ctx.page.wait(500)
    }
    await ctx.page.wait(STOP_AFTER_MS)
    await printed(await stop(ctx))
    fixtureOpened(ctx)
    if (ctx.case === "recording") await start(ctx)
  },
  cases: {
    // Listening between dictations, microphone released: what the loaded
    // model and the listener cost while nothing records.
    listening: {
      targets: ["idle"],
      primary: "cpu.wakes",
      metrics: ["cpu.work", "memory.app.host"],
      async run({ page }) {
        await page.wait(8_000)
      },
    },
    // Start to capture with the microphone released: the start command
    // reaching the idle listener, the microphone opening, and its first
    // chunk recorded.
    start: {
      targets: ["released"],
      primary: "phase.start",
      metrics: ["cpu.work", "cpu.wakes"],
      async run(ctx) {
        // The listener settles into its idle wait after the last dictation.
        await ctx.page.wait(1_000)
        const from = statSync(events(ctx)).size
        await ctx.phase("start", async () => {
          await start(ctx)
          await recordingBegan(ctx, from)
        })
        await ctx.page.wait(STOP_AFTER_MS)
        await printed(await stop(ctx))
      },
    },
    // Release to text: the stop's wait for the transcript (the 200 ms
    // release tail, the live stream's finish, processing, and History).
    dictation: {
      targets: ["warm", "released"],
      primary: "phase.finish",
      metrics: ["cpu.work", "cpu.wakes", "memory.app.host"],
      async run(ctx) {
        await start(ctx)
        await ctx.page.wait(STOP_AFTER_MS)
        await printed(await ctx.phase("finish", () => stop(ctx)))
      },
    },
    // While a dictation records: the audio path, the live stream's
    // inference, and the HUD meter's plumbing. Each run is 6 s of one
    // recording; reset finishes it and starts the next.
    recording: {
      targets: ["warm", "released"],
      primary: "cpu.wakes",
      metrics: ["cpu.work", "memory.app.host"],
      async run({ page }) {
        await page.wait(6_000)
      },
      async reset(ctx) {
        await printed(await stop(ctx))
        await start(ctx)
      },
    },
  },
})
