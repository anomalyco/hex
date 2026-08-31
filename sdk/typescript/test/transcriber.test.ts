import { mkdtemp, readFile, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { describe, expect, it } from "vitest"
import { create, type ModelProgress } from "../src/index.js"
import { options, processIsAlive } from "./support.js"

describe("ready transcriber", () => {
  it("prepares once and binds the model and language across recordings", async () => {
    const progress: ModelProgress[] = []
    const requests: string[] = []
    const input = {
      ...options(), model: "parakeet_v2" as const, language: "en",
      onProgress: (event: ModelProgress) => { progress.push(event) },
      fetch: async (url: string | URL | Request, init?: RequestInit) => {
        requests.push(String(url))
        return fetch(url, init)
      },
    }
    const creating = create(input)
    input.language = "fr"
    const transcriber = await creating
    try {
      expect(progress.map((event) => event.type)).toEqual(["downloading", "verifying", "loading"])
      expect(transcriber.model).toBe("parakeet_v2")
      expect(transcriber.language).toBe("en")
      for (const audio of [new Uint8Array([1, 2, 3]), new ArrayBuffer(4)]) {
        expect(await transcriber.transcribe(audio)).toEqual({ transcript: "hello from hex", durationMs: 750 })
      }
      expect(requests.filter((url) => url.includes("/prepare"))).toHaveLength(1)
      expect(requests.filter((url) => url.includes("/transcriptions")).map((url) => new URL(url).search))
        .toEqual(["?model=parakeet_v2&language=en", "?model=parakeet_v2&language=en"])
    } finally {
      await transcriber.close()
    }
    await transcriber.close()
    expect(processIsAlive(transcriber.pid)).toBe(false)
    await expect(transcriber.transcribe(new Uint8Array(4))).rejects.toMatchObject({ code: "service-exited" })
  })

  it.each(["error", "cancel", "callback"])("closes the helper before failed creation settles: %s", async (mode) => {
    const directory = await mkdtemp(join(tmpdir(), "hex-ready-"))
    const pidPath = join(directory, "pid")
    const controller = new AbortController()
    try {
      await expect(create({
        ...options(), model: "parakeet_v2",
        env: { HEX_FAKE_PID_PATH: pidPath, HEX_FAKE_PREPARE_ERROR: "1" },
        signal: controller.signal,
        onProgress: () => {
          if (mode === "cancel") controller.abort()
          if (mode === "callback") throw new Error("UI callback failed")
        },
      })).rejects.toMatchObject({
        code: mode === "error" ? "model-prepare-failed" : mode === "cancel" ? "cancelled" : "request-failed",
      })
      expect(processIsAlive(Number(await readFile(pidPath, "utf8")))).toBe(false)
    } finally {
      await rm(directory, { recursive: true, force: true })
    }
  })

  it("does not close a ready transcriber for an already-aborted request", async () => {
    const transcriber = await create({ ...options(), model: "parakeet_v2" })
    try {
      await expect(transcriber.transcribe(new Uint8Array(4), { signal: AbortSignal.abort() }))
        .rejects.toMatchObject({ code: "cancelled" })
      expect(processIsAlive(transcriber.pid)).toBe(true)
      expect((await transcriber.transcribe(new Uint8Array(4))).transcript).toBe("hello from hex")
    } finally {
      await transcriber.close()
    }
  })

  it.each(["abort", "close", "replaced-signal"])("waits for native ownership to end on %s", async (mode) => {
    let markStarted = () => {}
    const started = new Promise<void>((resolve) => { markStarted = resolve })
    const controller = new AbortController()
    const transcriber = await create({
      ...options(), model: "parakeet_v2",
      env: { HEX_FAKE_TRANSCRIBE_HANG: "1" },
      fetch: async (url, init) => {
        const response = await fetch(url, init)
        if (String(url).includes("/transcriptions")) markStarted()
        return response
      },
    })
    try {
      const request = { signal: controller.signal }
      const result = expect(transcriber.transcribe(new Uint8Array(4), request))
        .rejects.toMatchObject({ code: mode === "close" ? "service-exited" : "cancelled" })
      await started
      if (mode === "replaced-signal") request.signal = new AbortController().signal
      if (mode === "close") void transcriber.close()
      else controller.abort()
      await result
      expect(processIsAlive(transcriber.pid)).toBe(false)
    } finally {
      await transcriber.close()
    }
  })

  it("keeps the helper alive after an ordinary request failure", async () => {
    const transcriber = await create({
      ...options(), model: "parakeet_v2", env: { HEX_FAKE_TRANSCRIBE_ERROR: "1" },
    })
    try {
      await expect(transcriber.transcribe(new Uint8Array(4)))
        .rejects.toMatchObject({ code: "request-failed", remoteCode: "model-not-ready" })
      expect(processIsAlive(transcriber.pid)).toBe(true)
    } finally {
      await transcriber.close()
    }
  })

  it("settles every pending request when one cancellation closes their helper", async () => {
    let markStarted = () => {}
    const started = new Promise<void>((resolve) => { markStarted = resolve })
    let uploads = 0
    const controller = new AbortController()
    const transcriber = await create({
      ...options(), model: "parakeet_v2", env: { HEX_FAKE_TRANSCRIBE_HANG: "1" },
      fetch: async (url, init) => {
        const response = await fetch(url, init)
        if (String(url).includes("/transcriptions") && ++uploads === 2) markStarted()
        return response
      },
    })
    try {
      const first = expect(transcriber.transcribe(new Uint8Array(4), { signal: controller.signal }))
        .rejects.toMatchObject({ code: "cancelled" })
      const second = expect(transcriber.transcribe(new Uint8Array(4)))
        .rejects.toMatchObject({ code: "service-exited" })
      await started
      controller.abort()
      await Promise.all([first, second])
      expect(processIsAlive(transcriber.pid)).toBe(false)
    } finally {
      await transcriber.close()
    }
  })
})
