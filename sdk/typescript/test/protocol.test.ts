import { describe, expect, it } from "vitest"
import { decodeCapabilities, decodeEndpoint, decodeEndpointValue, decodeTranscription } from "../src/protocol.js"

const endpoint = {
  type: "ready",
  url: "http://127.0.0.1:1234",
  token: "",
  apiVersion: "2",
  pid: 1,
}

describe("capabilities decoding", () => {
  const capabilities = {
    audioFormats: ["audio/wav"],
    partialTranscripts: false,
    serviceCapture: false,
  } as const

  it("keeps output processing optional and typed", () => {
    expect(decodeCapabilities(capabilities)).toEqual(capabilities)
    expect(decodeCapabilities({ ...capabilities, outputProcessing: true })).toEqual({
      ...capabilities,
      outputProcessing: true,
    })
    expect(decodeCapabilities({ ...capabilities, outputProcessing: false })).toEqual({
      ...capabilities,
      outputProcessing: false,
    })
    expect(() => decodeCapabilities({ ...capabilities, outputProcessing: "yes" }))
      .toThrowError(expect.objectContaining({ code: "invalid-response" }))
  })
})

describe("transcription decoding", () => {
  const untimed = { transcript: "hello there", durationMs: 750 }

  it("keeps an untimed response as before", () => {
    expect(decodeTranscription(untimed)).toEqual(untimed)
  })

  it("decodes native segments and words", () => {
    const segments = [{ text: "hello there", startMs: 0, endMs: 750 }]
    const words = [
      { text: "hello", startMs: 0, endMs: 300 },
      { text: "there", startMs: 320, endMs: 750 },
    ]
    expect(decodeTranscription({ ...untimed, timestampGranularity: "segment", segments }))
      .toEqual({ ...untimed, timestampGranularity: "segment", segments })
    expect(decodeTranscription({ ...untimed, timestampGranularity: "word", segments, words }))
      .toEqual({ ...untimed, timestampGranularity: "word", segments, words })
    expect(decodeTranscription({ ...untimed, timestampGranularity: "segment", segments: [] }))
      .toEqual({ ...untimed, timestampGranularity: "segment", segments: [] })
  })

  it.each([
    [{ timestampGranularity: "none" }],
    [{ timestampGranularity: "words", segments: [] }],
    [{ segments: [{ text: "hello", startMs: 0, endMs: 750 }] }],
    [{ timestampGranularity: "segment" }],
    [{ timestampGranularity: "segment", segments: [{ text: "hello", startMs: 5 }] }],
    [{ timestampGranularity: "segment", segments: [{ text: "hello", startMs: -1, endMs: 5 }] }],
    [{ timestampGranularity: "word", segments: [], words: [{ text: "hi", startMs: 9, endMs: 4 }] }],
    [{ timestampGranularity: "word", segments: [] }],
  ])("rejects a malformed timed response for %j", (timed) => {
    expect(() => decodeTranscription({ ...untimed, ...timed }))
      .toThrowError(expect.objectContaining({ code: "invalid-response" }))
  })
})

describe("endpoint decoding", () => {
  it("accepts the same endpoint from startup JSON and an already parsed value", () => {
    expect(decodeEndpoint(JSON.stringify(endpoint))).toEqual(endpoint)
    expect(decodeEndpointValue(endpoint)).toEqual(endpoint)
  })

  it("retains the startup JSON parse error and its cause", () => {
    expect(() => decodeEndpoint("{")).toThrowError(expect.objectContaining({
      code: "invalid-handshake",
      message: "HEX returned malformed startup JSON",
      cause: expect.any(SyntaxError),
    }))
  })

  it.each([
    [null, "invalid-handshake", "HEX returned an invalid startup handshake"],
    [{ ...endpoint, apiVersion: undefined }, "invalid-handshake", "HEX returned an invalid startup handshake"],
    [{ ...endpoint, apiVersion: "1", pid: 0 }, "incompatible-api", "HEX local API 1 is incompatible with client API 2; install a matching HEX app"],
    [{ ...endpoint, pid: 1.5 }, "invalid-handshake", "HEX returned an invalid startup handshake"],
    [{ ...endpoint, token: null }, "invalid-handshake", "HEX returned an invalid startup handshake"],
    [{ ...endpoint, url: "invalid" }, "invalid-handshake", "HEX returned an invalid service URL"],
    [{ ...endpoint, url: "http://user@127.0.0.1:1234" }, "invalid-handshake", "HEX service URL is not an authenticated loopback endpoint"],
  ])("preserves validation errors for %j", (value, code, message) => {
    for (const decode of [() => decodeEndpointValue(value), () => decodeEndpoint(JSON.stringify(value))]) {
      expect(decode).toThrowError(expect.objectContaining({ code, message }))
    }
  })
})
