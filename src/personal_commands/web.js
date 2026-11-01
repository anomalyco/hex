// The web globals Hex gives scripts: fetch with Headers and Response,
// AbortController and AbortSignal, DOMException, setTimeout and
// clearTimeout, and queueMicrotask. Hex's native side does the work
// (`hex:io`); `hex.d.ts` types the supported subset.

import io from "hex:io"

const INTERNAL = Symbol("internal")
const ABORT = Symbol("abort")

const DOMException =
  typeof globalThis.DOMException === "function"
    ? globalThis.DOMException
    : class DOMException extends Error {
        constructor(message = "", name = "Error") {
          super(message)
          Object.defineProperty(this, "name", { value: String(name), configurable: true, writable: true })
        }
      }

const abortError = () => new DOMException("This operation was aborted", "AbortError")

class AbortSignal {
  #aborted = false
  #reason = undefined
  #listeners = []
  onabort = null

  constructor(key) {
    if (key !== INTERNAL) throw new TypeError("Illegal constructor")
  }

  get aborted() {
    return this.#aborted
  }

  get reason() {
    return this.#reason
  }

  throwIfAborted() {
    if (this.#aborted) throw this.#reason
  }

  addEventListener(type, listener, options) {
    if (type !== "abort" || listener == null) return
    if (this.#listeners.some((entry) => entry.listener === listener)) return
    this.#listeners.push({ listener, once: typeof options === "object" && options?.once === true })
  }

  removeEventListener(type, listener) {
    if (type !== "abort") return
    this.#listeners = this.#listeners.filter((entry) => entry.listener !== listener)
  }

  [ABORT](reason) {
    if (this.#aborted) return
    this.#aborted = true
    this.#reason = reason === undefined ? abortError() : reason
    const event = Object.freeze({ type: "abort", target: this, currentTarget: this })
    if (typeof this.onabort === "function") this.onabort.call(this, event)
    const listeners = this.#listeners
    this.#listeners = listeners.filter((entry) => !entry.once)
    for (const { listener } of listeners) {
      if (typeof listener === "function") listener.call(this, event)
      else listener.handleEvent(event)
    }
  }

  static abort(reason) {
    const signal = new AbortSignal(INTERNAL)
    signal[ABORT](reason)
    return signal
  }

  static timeout(milliseconds) {
    const signal = new AbortSignal(INTERNAL)
    setTimeout(() => signal[ABORT](new DOMException("The operation timed out.", "TimeoutError")), milliseconds)
    return signal
  }

  static any(signals) {
    const signal = new AbortSignal(INTERNAL)
    for (const source of signals) {
      if (source.aborted) {
        signal[ABORT](source.reason)
        return signal
      }
    }
    for (const source of signals) source.addEventListener("abort", () => signal[ABORT](source.reason), { once: true })
    return signal
  }
}

class AbortController {
  #signal = new AbortSignal(INTERNAL)

  get signal() {
    return this.#signal
  }

  abort(reason) {
    this.#signal[ABORT](reason)
  }
}

const TOKEN = /^[!#$%&'*+.^_`|~0-9a-z-]+$/

function headerName(name) {
  const normalized = String(name).toLowerCase()
  if (!TOKEN.test(normalized)) throw new TypeError(`Invalid header name: ${name}`)
  return normalized
}

const headerValue = (value) => String(value).replace(/^[\t\n\r ]+|[\t\n\r ]+$/g, "")

class Headers {
  #map = new Map()

  constructor(init) {
    if (init == null) return
    if (typeof init !== "object" && typeof init !== "function") throw new TypeError("Headers needs an object or pairs")
    if (typeof init[Symbol.iterator] === "function") {
      for (const pair of init) {
        const entry = [...pair]
        if (entry.length !== 2) throw new TypeError("Each header pair needs a name and a value")
        this.append(entry[0], entry[1])
      }
    } else {
      for (const [name, value] of Object.entries(init)) this.append(name, value)
    }
  }

  append(name, value) {
    const key = headerName(name)
    const current = this.#map.get(key)
    this.#map.set(key, current === undefined ? headerValue(value) : `${current}, ${headerValue(value)}`)
  }

  set(name, value) {
    this.#map.set(headerName(name), headerValue(value))
  }

  get(name) {
    return this.#map.get(headerName(name)) ?? null
  }

  has(name) {
    return this.#map.has(headerName(name))
  }

  delete(name) {
    this.#map.delete(headerName(name))
  }

  forEach(callback, thisArg) {
    for (const [name, value] of this) callback.call(thisArg, value, name, this)
  }

  *entries() {
    yield* [...this.#map.entries()].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
  }

  *keys() {
    for (const [name] of this.entries()) yield name
  }

  *values() {
    for (const [, value] of this.entries()) yield value
  }

  [Symbol.iterator]() {
    return this.entries()
  }
}

class Response {
  #body
  #used = false
  #status
  #statusText
  #headers
  #url

  constructor(body = null, init = {}) {
    if (body === INTERNAL) {
      this.#status = init.status
      this.#statusText = init.statusText
      this.#headers = new Headers(init.headers)
      this.#url = init.url
      this.#body = init.body
      return
    }
    if (body !== null && typeof body !== "string") throw new TypeError("Hex's Response accepts only string bodies")
    this.#status = init.status ?? 200
    this.#statusText = init.statusText ?? ""
    this.#headers = new Headers(init.headers)
    this.#url = ""
    this.#body = body === null ? new ArrayBuffer(0) : io.encode(body)
  }

  get ok() {
    return this.#status >= 200 && this.#status <= 299
  }

  get status() {
    return this.#status
  }

  get statusText() {
    return this.#statusText
  }

  get headers() {
    return this.#headers
  }

  get url() {
    return this.#url
  }

  get bodyUsed() {
    return this.#used
  }

  async arrayBuffer() {
    if (this.#used) throw new TypeError("Body is unusable: Body has already been read")
    this.#used = true
    return this.#body
  }

  async text() {
    return io.decode(await this.arrayBuffer())
  }

  async json() {
    return JSON.parse(await this.text())
  }
}

function fetch(input, init = {}) {
  return new Promise((resolve, reject) => {
    const url = typeof input === "object" && input !== null && typeof input.url === "string" ? input.url : String(input)
    const method = String(init.method ?? "GET").toUpperCase()
    const headers = new Headers(init.headers)
    const body = init.body ?? null
    if (body !== null) {
      if (method === "GET" || method === "HEAD") throw new TypeError("Request with GET/HEAD method cannot have body.")
      if (typeof body !== "string") throw new TypeError("Hex's fetch accepts only string bodies; use JSON.stringify")
      if (!headers.has("content-type")) headers.set("content-type", "text/plain;charset=UTF-8")
    }
    const signal = init.signal ?? null
    signal?.throwIfAborted()
    let settled = false
    let id = 0
    const settle = () => {
      settled = true
      signal?.removeEventListener("abort", onAbort)
    }
    const onAbort = () => {
      if (settled) return
      settle()
      io.cancel(id)
      reject(signal.reason)
    }
    id = io.start(
      "fetch",
      JSON.stringify({ url, method, headers: [...headers], body }),
      (raw) => {
        if (settled) return
        settle()
        resolve(new Response(INTERNAL, raw))
      },
      (message) => {
        if (settled) return
        settle()
        reject(new TypeError(`fetch failed: ${message}`))
      },
    )
    signal?.addEventListener("abort", onAbort)
  })
}

function setTimeout(callback, delay = 0, ...args) {
  if (typeof callback !== "function") throw new TypeError("setTimeout needs a function")
  return io.start("timer", JSON.stringify(Number(delay) || 0), () => callback(...args), () => {})
}

function clearTimeout(id) {
  if (typeof id === "number") io.cancel(id)
}

const queueMicrotask = (callback) => {
  Promise.resolve().then(callback)
}

for (const [name, value] of Object.entries({
  AbortController,
  AbortSignal,
  DOMException,
  Headers,
  Response,
  clearTimeout,
  fetch,
  queueMicrotask,
  setTimeout,
})) {
  Object.defineProperty(globalThis, name, { value, writable: true, configurable: true })
}
