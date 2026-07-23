import { describe, expect, it } from "vitest"
import { create, HexError } from "../src/index.js"
import { helper, options, processIsAlive } from "./support.js"

describe("Promise client", () => {
  it("owns the helper and exercises the complete protocol", async () => {
    const host = await create(options())
    const progress: Array<string> = []

    expect(await host.client.health()).toEqual({ version: "test", apiVersion: "1" })
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
