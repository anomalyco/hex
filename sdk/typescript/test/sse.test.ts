import { describe, expect, it, vi } from "vitest"
import { makeClient } from "../src/client.js"
import { consumeSse } from "../src/sse.js"

const response = (chunks: readonly Uint8Array[], cancel = () => {}, close = true) =>
  new Response(new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(chunk)
      if (close) controller.close()
    },
    cancel,
  }), { headers: { "content-type": "text/event-stream" } })

const encoded = (text: string) => new TextEncoder().encode(text)

describe("shared SSE framing", () => {
  it.each(["\n", "\r\n", "\r"])("decodes %j separators independently of byte chunks", async (separator) => {
    const bytes = encoded([
      ": comment", "event: ignored", "data: café", "data:  second", "",
      "data: tail",
    ].join(separator))
    for (const chunks of [[bytes], Array.from(bytes, (byte) => new Uint8Array([byte]))]) {
      const values: string[] = []
      await consumeSse(response(chunks), new AbortController().signal, "fixture", (data) => { values.push(data) })
      expect(values).toEqual(["café\n second", "tail"])
    }
  })

  it.each([
    ["line", `:${"x".repeat(64 * 1024)}\n\n`],
    ["event", `data: ${"x".repeat(32 * 1024)}\ndata: ${"x".repeat(32 * 1024)}\n\n`],
  ])("bounds an individual %s and cancels its body on failure", async (_kind, text) => {
    const cancel = vi.fn()
    await expect(consumeSse(
      response([encoded(text)], cancel, false), new AbortController().signal, "fixture", () => {},
    )).rejects.toMatchObject({ code: "invalid-response" })
    expect(cancel).toHaveBeenCalledTimes(1)
  })

  it("cancels on terminal completion without decoding trailing data", async () => {
    const cancel = vi.fn()
    const values: string[] = []
    await consumeSse(response([encoded("data: done\n\ndata: ignored\n\n")], cancel, false),
      new AbortController().signal, "fixture", (data) => {
        values.push(data)
        return true
      })
    expect(values).toEqual(["done"])
    expect(cancel).toHaveBeenCalledTimes(1)
  })

  it("preserves callback failure and releases the underlying body", async () => {
    const cancel = vi.fn()
    const failure = new Error("callback failed")
    await expect(consumeSse(response([encoded("data: event\n\n")], cancel, false),
      new AbortController().signal, "fixture", () => { throw failure }))
      .rejects.toBe(failure)
    expect(cancel).toHaveBeenCalledTimes(1)
  })
})

describe("SDK SSE consumers", () => {
  const endpoint = { type: "ready", url: "http://127.0.0.1:1", token: "fixture", apiVersion: "2", pid: 1 } as const

  it("dispatches CR-only events and completes preparation without another byte or EOF", async () => {
    const cancel = vi.fn()
    const transport: typeof fetch = async () => response([
      encoded('data: {"type":"verifying"}\r\rdata: {"type":"ok"}\r\r'),
    ], cancel, false)
    const client = makeClient(endpoint, transport, new AbortController().signal)
    const onProgress = vi.fn()
    await client.models.prepare("parakeet_v2", { onProgress })
    expect(onProgress).toHaveBeenCalledExactlyOnceWith({ type: "verifying" })
    expect(cancel).toHaveBeenCalledTimes(1)
  }, 1_000)

  it("accepts one large chunk of small progress and level events while bounding observations", async () => {
    const count = 3_000
    const progress = "data: {\"type\":\"verifying\"}\n\n".repeat(count) + "data: {\"type\":\"ok\"}\n\n"
    const levels = Array.from({ length: count }, (_, index) =>
      `data: ${JSON.stringify({ rmsDb: index, peakDb: index })}\n\n`).join("")
    const transport: typeof fetch = async (url) => {
      if (String(url).includes("/prepare")) return response([encoded(progress)])
      if (String(url).endsWith("/levels")) return response([encoded(levels)])
      if (String(url).endsWith("/dictations")) {
        return Response.json({ id: 1, ownerToken: "x".repeat(32), sampleRate: 16_000 })
      }
      return new Response(null, { status: 204 })
    }
    const client = makeClient(endpoint, transport, new AbortController().signal)
    const onProgress = vi.fn()
    await client.models.prepare("parakeet_v2", { onProgress })
    expect(onProgress).toHaveBeenCalledTimes(count)
    const recording = await client.dictation.start({ source: "fixture" })
    try {
      const values = []
      for await (const level of recording.levels) values.push(level)
      expect(values.length).toBeLessThanOrEqual(33) // one waiting reader, then the 32-value sliding buffer
      expect(values.at(-1)).toEqual({ rmsDb: count - 1, peakDb: count - 1 })
    } finally {
      await recording.cancel()
    }
  })

  it.each(["throw", "abort"])("preserves %s from a progress callback ahead of buffered completion", async (operation) => {
    const cancel = vi.fn()
    const controller = new AbortController()
    const transport: typeof fetch = async () => response([
      encoded('data: {"type":"verifying"}\n\ndata: {"type":"ok"}\n\n'),
    ], cancel, false)
    const client = makeClient(endpoint, transport, controller.signal)
    await expect(client.models.prepare("parakeet_v2", {
      onProgress: () => {
        if (operation === "abort") controller.abort("fixture cancellation")
        else throw new Error("fixture failure")
      },
    })).rejects.toMatchObject({ code: operation === "abort" ? "cancelled" : "request-failed" })
    expect(cancel).toHaveBeenCalledTimes(1)
  })
})
