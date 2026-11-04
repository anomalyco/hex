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
import { closeSync, openSync, readFileSync, readSync, statSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { pathToFileURL } from "node:url"
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

/** The browser started by the `browser` case's setup. */
let browser: ReturnType<typeof Bun.spawn> | undefined

/** Puts a browser in front, as when someone dictates into a web page: a
 * browser with its own profile in the copy's home, on a long local page,
 * brought to the foreground. Windows: Microsoft Edge, on an unlocked
 * session (a locked one keeps the lock screen in front). Linux: Chromium
 * on X11 with its page in the accessibility tree (Chromium exposes pages
 * to AT-SPI only with renderer accessibility on). */
async function browserInFront(ctx: Ctx): Promise<void> {
  const page = join(ctx.home, "long-page.html")
  const rows = Array.from({ length: 3_000 }, (_, i) => `<p>Paragraph ${i} with a <a href="#${i}">link</a> and <button>button ${i}</button></p>`)
  writeFileSync(page, `<!doctype html><title>Long page</title><body>${rows.join("")}</body>`)
  if (process.platform === "linux") {
    const root = process.getuid?.() === 0 ? ["--no-sandbox"] : []
    const profile = `--user-data-dir=${join(ctx.home, "chromium")}`
    browser = Bun.spawn(["chromium", profile, ...root, "--no-first-run", "--no-default-browser-check", "--force-renderer-accessibility", "--new-window", pathToFileURL(page).href])
    await ctx.page.wait(5_000)
    Bun.spawnSync(["xdotool", "search", "--sync", "--name", "Long page", "windowactivate", "--sync"])
    const pid = Bun.spawnSync(["xdotool", "getactivewindow", "getwindowpid"]).stdout.toString().trim()
    const front = pid ? readFileSync(`/proc/${pid}/comm`, "utf8").trim() : "nothing"
    if (!front.startsWith("chromium")) throw new Error(`the browser case needs Chromium in front on X11; in front: ${front}`)
    return
  }
  const edge = "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"
  browser = Bun.spawn([edge, `--user-data-dir=${join(ctx.home, "edge")}`, "--no-first-run", "--no-default-browser-check", "--new-window", pathToFileURL(page).href])
  await ctx.page.wait(5_000)
  // A background process may take the foreground once a key went to it.
  const script = `
Add-Type -Name W -Namespace F -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetForegroundWindow(System.IntPtr h); [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, uint f, System.UIntPtr e); [DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow(); [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr h, out uint p);'
$w = Get-Process msedge | Where-Object { $_.MainWindowTitle -like 'Long page*' } | Select-Object -First 1
if (-not $w) { 'no window'; exit 1 }
[F.W]::keybd_event(0x12, 0, 0, [System.UIntPtr]::Zero); [F.W]::keybd_event(0x12, 0, 2, [System.UIntPtr]::Zero)
[void][F.W]::SetForegroundWindow($w.MainWindowHandle)
Start-Sleep -Milliseconds 500
$p = 0; [void][F.W]::GetWindowThreadProcessId([F.W]::GetForegroundWindow(), [ref]$p)
(Get-Process -Id $p).ProcessName`
  const front = Bun.spawnSync(["powershell", "-NoProfile", "-Command", script]).stdout.toString().trim()
  if (front !== "msedge") throw new Error(`the browser case needs Edge in front on an unlocked session; in front: ${front}`)
}

async function printed(reply: Reply): Promise<string> {
  if (reply.status !== "transcript" || !reply.text) throw new Error(`stop --print answered ${JSON.stringify(reply)}`)
  return reply.text
}

// Windows and Linux open no window with `--hidden`; the macOS app opens none
// once set up, and `--device` makes a build without the fixture fail to open
// the microphone instead of recording it.
const args = process.platform === "darwin" ? ["--device", "Audio fixture"] : ["--hidden"]

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
    if (ctx.case === "browser") await browserInFront(ctx)
  },
  teardown() {
    browser?.kill()
    browser = undefined
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
    // Release to text with a browser in front (Windows unlocked, Linux on
    // X11): the dictation case's stop, plus the page read the foreground
    // context starts for a browser. Nothing set up here uses the page.
    ...(process.platform !== "darwin" && {
      browser: {
        targets: ["warm"],
        primary: "phase.finish",
        metrics: ["cpu.work", "cpu.wakes"],
        async run(ctx) {
          await start(ctx)
          await ctx.page.wait(STOP_AFTER_MS)
          await printed(await ctx.phase("finish", () => stop(ctx)))
        },
      },
    }),
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
