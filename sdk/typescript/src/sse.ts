import { HexError } from "./errors.js"

const MAX_SSE_EVENT_CHARS = 64 * 1024

/** Consume bounded SSE data events independently of transport chunk boundaries.
 * Returning true from onData ends consumption and releases the response body.
 */
export const consumeSse = async (
  response: Response,
  signal: AbortSignal,
  description: string,
  onData: (data: string) => boolean | void,
): Promise<void> => {
  if (!response.headers.get("content-type")?.toLowerCase().startsWith("text/event-stream")) {
    throw new HexError("invalid-response", `HEX returned an invalid ${description} content type`)
  }
  if (response.body === null) throw new HexError("invalid-response", `HEX returned no ${description} stream`)

  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader()
  let line = ""
  let swallowLineFeed = false
  let data: string[] = []
  let eventChars = 0

  const dispatch = (): boolean => {
    if (data.length === 0) return false
    const value = data.join("\n")
    data = []
    eventChars = 0
    signal.throwIfAborted()
    const done = onData(value) === true
    signal.throwIfAborted()
    return done
  }
  const acceptLine = (): boolean => {
    const current = line
    line = ""
    if (current === "") return dispatch()
    if (!current.startsWith(":")) {
      const separator = current.indexOf(":")
      const field = separator < 0 ? current : current.slice(0, separator)
      let value = separator < 0 ? "" : current.slice(separator + 1)
      if (value.startsWith(" ")) value = value.slice(1)
      if (field === "data") {
        eventChars += value.length + 1
        if (eventChars > MAX_SSE_EVENT_CHARS) {
          throw new HexError("invalid-response", `HEX ${description} event exceeded its byte limit`)
        }
        data.push(value)
      }
    }
    return false
  }

  try {
    while (true) {
      signal.throwIfAborted()
      const chunk = await reader.read()
      signal.throwIfAborted()
      if (chunk.done) {
        if (line !== "" && acceptLine()) return
        dispatch()
        return
      }
      for (const character of chunk.value) {
        signal.throwIfAborted()
        if (swallowLineFeed) {
          swallowLineFeed = false
          if (character === "\n") continue
        }
        if (character === "\r") {
          swallowLineFeed = true
          if (acceptLine()) return
        } else if (character === "\n") {
          if (acceptLine()) return
        } else {
          line += character
          if (line.length > MAX_SSE_EVENT_CHARS) {
            throw new HexError("invalid-response", `HEX ${description} line exceeded its byte limit`)
          }
        }
      }
    }
  } finally {
    // Also cancel on a terminal event, decoder error, or callback failure.
    await reader.cancel().catch(() => {})
    reader.releaseLock()
  }
}
