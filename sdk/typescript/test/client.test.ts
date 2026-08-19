import { describe, expect, it } from "vitest"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { connect, create, HexError } from "../src/index.js"
import { defaultDiscoveryPath } from "../src/host.js"
import { helper, options, processIsAlive } from "./support.js"

describe("Promise client", () => {
  it("resolves per-user discovery paths on each desktop platform", () => {
    expect(defaultDiscoveryPath("darwin", {}, "/Users/hex")).toBe(
      join("/Users/hex", "Library", "Application Support", "voice-control", "local-api.json"),
    )
    expect(defaultDiscoveryPath("win32", { APPDATA: "C:\\Users\\hex\\AppData\\Roaming" }, "C:\\Users\\hex")).toBe(
      join("C:\\Users\\hex\\AppData\\Roaming", "voice-control", "local-api.json"),
    )
    expect(defaultDiscoveryPath("linux", { XDG_DATA_HOME: "/data/hex" }, "/home/hex")).toBe(
      join("/data/hex", "voice-control", "local-api.json"),
    )
    expect(defaultDiscoveryPath("win32", { HEX_APPLICATION_SUPPORT_DIR: "D:\\hex-data" }, "C:\\Users\\hex")).toBe(
      join("D:\\hex-data", "local-api.json"),
    )
  })

  it("owns the helper and exercises the complete protocol", async () => {
    const host = await create(options())
    const progress: Array<string> = []

    expect(await host.client.health()).toEqual({ version: "test", apiVersion: "2" })
    expect(await host.client.capabilities()).toEqual({
      audioFormats: ["audio/wav"],
      partialTranscripts: false,
      serviceCapture: false,
    })
    expect(await host.client.models.list()).toMatchObject([{ id: "parakeet_v2", installed: true }])
    await host.client.models.prepare("parakeet_v2", {
      language: "en",
      onProgress: (event) => progress.push(event.type),
    })
    expect(progress).toEqual(["downloading", "verifying", "loading"])
    expect(await host.client.transcribe({
      audio: { data: new Uint8Array([1, 2, 3]), contentType: "audio/wav" },
      model: "parakeet_v2",
      language: "en",
    })).toEqual({ transcript: "hello from hex", durationMs: 750 })

    const pid = host.pid
    expect(processIsAlive(pid)).toBe(true)
    await host.close()
    await host.close()
    expect(processIsAlive(pid)).toBe(false)
  })

  it("rejects an endpoint outside loopback and cleans up the child", async () => {
    const result = create({
      command: [process.execPath, helper],
      env: { HEX_FAKE_MODE: "bad-handshake" },
    })

    await expect(result).rejects.toMatchObject({ code: "invalid-handshake" } satisfies Partial<HexError>)
  })

  it("reports an early helper exit", async () => {
    const result = create({
      command: [process.execPath, helper],
      env: { HEX_FAKE_MODE: "exit-before-ready" },
    })

    await expect(result).rejects.toMatchObject({ code: "service-exited" } satisfies Partial<HexError>)
  })

  it("preserves cancellation after SSE response headers", async () => {
    const host = await create({
      ...options(),
      env: { HEX_FAKE_PREPARE_HANG: "1" },
    })
    const controller = new AbortController()
    try {
      const preparation = host.client.models.prepare("parakeet_v2", {
        signal: controller.signal,
        onProgress: () => controller.abort("test cancellation"),
      })
      await expect(preparation).rejects.toMatchObject({ code: "cancelled" } satisfies Partial<HexError>)
    } finally {
      await host.close()
    }
  })

  it("owns a running-app dictation handle with live levels and raw completion", async () => {
    const directory = await mkdtemp(join(tmpdir(), "hex-client-"))
    const discoveryPath = join(directory, "local-api.json")
    const host = await create({
      ...options(),
      env: {
        HEX_FAKE_SERVICE_CAPTURE: "1",
        HEX_FAKE_DISCOVERY_PATH: discoveryPath,
        HEX_FAKE_FINISH_RETRY: "1",
      },
    })
    try {
      const hex = await connect({ discoveryPath })
      expect(await hex.capabilities()).toMatchObject({ serviceCapture: true })
      const recording = await hex.dictation.start({ source: "tp7" })
      expect(recording.ownerToken).toMatch(/^hex_capture_/)
      expect(recording.sampleRate).toBe(48_000)
      const levelIterator = recording.levels[Symbol.asyncIterator]()
      const first = await levelIterator.next()
      expect(first).toEqual({ value: { rmsDb: -24.5, peakDb: -8 }, done: false })
      const audioIterator = recording.audio[Symbol.asyncIterator]()
      const audio = await audioIterator.next()
      expect(audio.done).toBe(false)
      expect(Array.from(audio.value ?? [])).toEqual([0.25, -0.5])
      await levelIterator.return?.()
      await audioIterator.return?.()
      await expect(recording.finish()).rejects.toMatchObject({
        code: "request-failed",
        status: 503,
      } satisfies Partial<HexError>)
      expect(await recording.finish()).toEqual({
        transcript: "running app text",
        durationMs: 1234,
      })
      const cancelled = await hex.dictation.start({ source: "tp7" })
      await cancelled.cancel()
      await cancelled.cancel()
    } finally {
      await host.close()
      await rm(directory, { recursive: true, force: true })
    }
  })

  it("rejects a legacy running app before sending a request", async () => {
    const directory = await mkdtemp(join(tmpdir(), "hex-client-legacy-"))
    const discoveryPath = join(directory, "local-api.json")
    let requested = false
    try {
      await writeFile(discoveryPath, JSON.stringify({
        port: 1,
        token: "a".repeat(64),
        apiVersion: "1",
        pid: process.pid,
      }))
      await expect(connect({
        discoveryPath,
        fetch: async () => {
          requested = true
          throw new Error("legacy app should not receive a request")
        },
      })).rejects.toMatchObject({ code: "incompatible-api" } satisfies Partial<HexError>)
      expect(requested).toBe(false)
    } finally {
      await rm(directory, { recursive: true, force: true })
    }
  })

  it("does not let a buffered terminal event overtake cancellation", async () => {
    const host = await create({
      ...options(),
      env: { HEX_FAKE_PREPARE_BATCH: "1" },
    })
    const controller = new AbortController()
    try {
      const preparation = host.client.models.prepare("parakeet_v2", {
        signal: controller.signal,
        onProgress: () => controller.abort("test cancellation"),
      })
      await expect(preparation).rejects.toMatchObject({ code: "cancelled" } satisfies Partial<HexError>)
    } finally {
      await host.close()
    }
  })

  it("bounds an unconsumed observation stream and cancels it through return", async () => {
    const host = await create({
      ...options(),
      env: { HEX_FAKE_SERVICE_CAPTURE: "1", HEX_FAKE_LEVEL_BURST: "1" },
    })
    try {
      const recording = await host.client.dictation.start({ source: "buffer-test" })
      const iterator = recording.levels[Symbol.asyncIterator]()
      await new Promise((resolve) => setTimeout(resolve, 20))
      expect(await iterator.next()).toEqual({
        value: { rmsDb: 8, peakDb: 8 },
        done: false,
      })
      await iterator.return?.()
      await recording.cancel()
    } finally {
      await host.close()
    }
  })

  it("preserves remote HTTP and SSE error codes", async () => {
    const httpHost = await create({
      ...options(),
      env: { HEX_FAKE_TRANSCRIBE_ERROR: "1" },
    })
    try {
      const transcription = httpHost.client.transcribe({
        audio: { data: new Uint8Array([1]), contentType: "audio/wav" },
        model: "parakeet_v2",
      })
      await expect(transcription).rejects.toMatchObject({
        code: "request-failed",
        status: 409,
        remoteCode: "model-not-ready",
      } satisfies Partial<HexError>)
    } finally {
      await httpHost.close()
    }

    const sseHost = await create({
      ...options(),
      env: { HEX_FAKE_PREPARE_ERROR: "1" },
    })
    try {
      await expect(sseHost.client.models.prepare("parakeet_v2")).rejects.toMatchObject({
        code: "model-prepare-failed",
        remoteCode: "load-failed",
      } satisfies Partial<HexError>)
    } finally {
      await sseHost.close()
    }
  })

  it("rejects a non-SSE model preparation response", async () => {
    const host = await create(options())
    try {
      await expect(host.client.models.prepare("parakeet_v3")).rejects.toMatchObject({
        code: "invalid-response",
      } satisfies Partial<HexError>)
    } finally {
      await host.close()
    }
  })
})
