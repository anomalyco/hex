import { HexError } from "./errors.js"
import {
  decodeCapabilities,
  decodeHealth,
  decodeModels,
  decodeProgress,
  decodeTranscription,
  type EmbeddedEndpoint,
} from "./protocol.js"
import type {
  HexClient,
  ListModelsOptions,
  ModelId,
  PrepareModelOptions,
  RequestOptions,
  TranscriptionRequest,
} from "./types.js"

const MAX_SSE_EVENT_CHARS = 64 * 1024

interface ResponseContext {
  readonly response: Response
  readonly signal: AbortSignal
}

const abortError = (signal: AbortSignal): HexError => {
  if (signal.reason instanceof HexError) return signal.reason
  return new HexError("cancelled", "HEX operation was cancelled", { cause: signal.reason })
}

const boundaryError = (
  cause: unknown,
  signal: AbortSignal,
  code: "request-failed" | "invalid-response",
  message: string,
): HexError => {
  if (cause instanceof HexError) return cause
  if (signal.aborted) return abortError(signal)
  return new HexError(code, message, { cause })
}

const rejection = async (
  response: Response,
  signal: AbortSignal,
): Promise<{ readonly message: string; readonly remoteCode?: string }> => {
  try {
    const body: unknown = await response.json()
    if (typeof body === "object" && body !== null && "code" in body && typeof body.code === "string") {
      return { message: body.code, remoteCode: body.code }
    }
  } catch (cause) {
    if (signal.aborted) throw abortError(signal)
    if (cause instanceof HexError) throw cause
  }
  return { message: `HTTP ${response.status}` }
}

const json = async ({ response, signal }: ResponseContext): Promise<unknown> => {
  try {
    return await response.json()
  } catch (cause) {
    throw boundaryError(cause, signal, "invalid-response", "HEX returned malformed JSON")
  }
}

const combineSignals = (first: AbortSignal, second?: AbortSignal): AbortSignal => {
  if (second === undefined) return first
  if (first.aborted) return first
  if (second.aborted) return second
  return AbortSignal.any([first, second])
}

export const makeClient = (
  endpoint: EmbeddedEndpoint,
  transport: typeof globalThis.fetch,
  lifetime: AbortSignal,
): HexClient => {
  const request = async (
    path: string,
    init: RequestInit = {},
    callerSignal?: AbortSignal,
  ): Promise<ResponseContext> => {
    const signal = combineSignals(lifetime, callerSignal)
    let response: Response
    try {
      response = await transport(`${endpoint.url}${path}`, {
        ...init,
        headers: (() => {
          const headers = new Headers(init.headers)
          headers.set("authorization", `Bearer ${endpoint.token}`)
          return headers
        })(),
        signal,
      })
    } catch (cause) {
      throw boundaryError(cause, signal, "request-failed", "Could not reach the embedded HEX service")
    }
    if (!response.ok) {
      const remote = await rejection(response, signal)
      throw new HexError("request-failed", `HEX rejected the request: ${remote.message}`, {
        status: response.status,
        ...(remote.remoteCode === undefined ? {} : { remoteCode: remote.remoteCode }),
      })
    }
    return { response, signal }
  }

  const health = async (options?: RequestOptions) =>
    decodeHealth(await json(await request("/health", {}, options?.signal)))

  const capabilities = async (options?: RequestOptions) =>
    decodeCapabilities(await json(await request("/capabilities", {}, options?.signal)))

  const list = async (options?: ListModelsOptions) => {
    const query = new URLSearchParams()
    if (options?.language !== undefined) query.set("language", options.language)
    const suffix = query.size === 0 ? "" : `?${query}`
    return decodeModels(await json(await request(`/models${suffix}`, {}, options?.signal)))
  }

  const prepare = async (id: ModelId, options?: PrepareModelOptions): Promise<void> => {
    const query = new URLSearchParams()
    if (options?.language !== undefined) query.set("language", options.language)
    const suffix = query.size === 0 ? "" : `?${query}`
    const context = await request(`/models/${id}/prepare${suffix}`, { method: "POST" }, options?.signal)
    const { response, signal } = context
    if (!response.headers.get("content-type")?.toLowerCase().startsWith("text/event-stream")) {
      throw new HexError("invalid-response", "HEX returned an invalid model progress content type")
    }
    if (response.body === null) throw new HexError("invalid-response", "HEX returned no model progress stream")

    const reader = response.body.pipeThrough(new TextDecoderStream()).getReader()
    let line = ""
    let pendingCarriageReturn = false
    let data: Array<string> = []
    let eventChars = 0
    let completed = false

    const dispatch = () => {
      if (data.length === 0 || completed) {
        data = []
        eventChars = 0
        return
      }
      let value: unknown
      try {
        value = JSON.parse(data.join("\n"))
      } catch (cause) {
        throw new HexError("invalid-response", "HEX returned malformed model progress", { cause })
      }
      data = []
      eventChars = 0
      const progress = decodeProgress(value)
      if (progress === "ok") {
        completed = true
        return
      }
      try {
        options?.onProgress?.(progress)
      } catch (cause) {
        throw new HexError("request-failed", "The model progress callback failed", { cause })
      }
      if (signal.aborted) throw abortError(signal)
    }

    const acceptLine = () => {
      if (line === "") {
        dispatch()
      } else if (!line.startsWith(":")) {
        const separator = line.indexOf(":")
        const field = separator < 0 ? line : line.slice(0, separator)
        let value = separator < 0 ? "" : line.slice(separator + 1)
        if (value.startsWith(" ")) value = value.slice(1)
        if (field === "data") {
          eventChars += value.length + 1
          if (eventChars > MAX_SSE_EVENT_CHARS) {
            throw new HexError("invalid-response", "HEX model progress event exceeded its byte limit")
          }
          data.push(value)
        }
      }
      line = ""
    }

    const accept = (chunk: string, end: boolean) => {
      for (const character of chunk) {
        if (signal.aborted) throw abortError(signal)
        if (completed) return
        if (pendingCarriageReturn) {
          pendingCarriageReturn = false
          acceptLine()
          if (character === "\n") continue
        }
        if (character === "\r") {
          pendingCarriageReturn = true
        } else if (character === "\n") {
          acceptLine()
        } else {
          line += character
          if (line.length > MAX_SSE_EVENT_CHARS) {
            throw new HexError("invalid-response", "HEX model progress line exceeded its byte limit")
          }
        }
      }
      if (end) {
        if (pendingCarriageReturn) {
          pendingCarriageReturn = false
          acceptLine()
        } else if (line !== "") {
          acceptLine()
        }
        dispatch()
      }
    }

    try {
      while (!completed) {
        const result = await reader.read()
        if (result.done) {
          accept("", true)
          break
        }
        accept(result.value, false)
      }
    } catch (cause) {
      throw boundaryError(cause, signal, "invalid-response", "HEX model progress stream failed")
    } finally {
      try {
        await reader.cancel()
      } catch {
        // The underlying stream may already be closed or aborted.
      }
      reader.releaseLock()
    }
    if (!completed) throw new HexError("model-prepare-failed", "HEX model preparation ended without a result")
  }

  const transcribe = async (input: TranscriptionRequest) => {
    const query = new URLSearchParams({ model: input.model })
    if (input.language !== undefined) query.set("language", input.language)
    let body: ArrayBuffer
    if (input.audio.data instanceof ArrayBuffer) {
      body = input.audio.data
    } else if (
      input.audio.data.buffer instanceof ArrayBuffer
      && input.audio.data.byteOffset === 0
      && input.audio.data.byteLength === input.audio.data.buffer.byteLength
    ) {
      body = input.audio.data.buffer
    } else {
      const copy = new Uint8Array(input.audio.data.byteLength)
      copy.set(input.audio.data)
      body = copy.buffer
    }
    const response = await request(`/transcriptions?${query}`, {
      method: "POST",
      headers: { "content-type": input.audio.contentType },
      body,
    }, input.signal)
    return decodeTranscription(await json(response))
  }

  return { health, capabilities, models: { list, prepare }, transcribe }
}
