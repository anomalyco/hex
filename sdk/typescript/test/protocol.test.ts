import { describe, expect, it } from "vitest"
import { decodeEndpoint, decodeEndpointValue } from "../src/protocol.js"

const endpoint = {
  type: "ready",
  url: "http://127.0.0.1:1234",
  token: "",
  apiVersion: "2",
  pid: 1,
}

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
