import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process"
import { readFile } from "node:fs/promises"
import { homedir } from "node:os"
import { join } from "node:path"
import { HexError } from "./errors.js"
import { makeClient } from "./client.js"
import { resolveCommand } from "./helper.js"
import { decodeEndpoint } from "./protocol.js"
import type { ConnectOptions, CreateOptions, HexClient, HexHost, Transcriber, TranscriberOptions } from "./types.js"

const DEFAULT_STARTUP_TIMEOUT_MS = 10_000
const DEFAULT_SHUTDOWN_TIMEOUT_MS = 5_500
const MAX_HANDSHAKE_BYTES = 16 * 1024
const MAX_STDERR_BYTES = 16 * 1024

const waitForExit = async (child: ChildProcessWithoutNullStreams, timeoutMs: number): Promise<boolean> => {
  if (child.exitCode !== null || child.signalCode !== null) return true
  return new Promise((resolve) => {
    let settled = false
    const finish = (exited: boolean) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      child.off("exit", onExit)
      resolve(exited)
    }
    const onExit = () => finish(true)
    const timer = setTimeout(() => finish(false), timeoutMs)
    child.once("exit", onExit)
    if (child.exitCode !== null || child.signalCode !== null) finish(true)
  })
}

const stop = async (child: ChildProcessWithoutNullStreams, timeoutMs: number): Promise<void> => {
  if (child.exitCode !== null || child.signalCode !== null) return
  child.stdin.end()
  if (await waitForExit(child, timeoutMs)) return
  try {
    child.kill("SIGTERM")
  } catch (cause) {
    throw new HexError("shutdown-failed", "Could not terminate the embedded HEX service", { cause })
  }
  if (await waitForExit(child, 1_000)) return
  try {
    child.kill("SIGKILL")
  } catch (cause) {
    throw new HexError("shutdown-failed", "Could not kill the embedded HEX service", { cause })
  }
  if (!await waitForExit(child, 1_000)) {
    throw new HexError("shutdown-failed", "Embedded HEX remained alive after SIGKILL")
  }
}

const readHandshake = (
  child: ChildProcessWithoutNullStreams,
  timeoutMs: number,
  signal?: AbortSignal,
): Promise<string> => new Promise((resolve, reject) => {
  let settled = false
  let buffer = Buffer.alloc(0)
  let stderr = ""
  const finish = (result: { readonly line: string } | { readonly error: HexError }) => {
    if (settled) return
    settled = true
    clearTimeout(timeout)
    child.stdout.off("data", onData)
    child.stderr.off("data", onStderr)
    child.off("error", onError)
    child.off("exit", onExit)
    signal?.removeEventListener("abort", onAbort)
    if ("line" in result) resolve(result.line)
    else reject(result.error)
  }
  const onData = (chunk: Buffer) => {
    buffer = Buffer.concat([buffer, chunk])
    if (buffer.length > MAX_HANDSHAKE_BYTES) {
      finish({ error: new HexError("invalid-handshake", "HEX startup handshake exceeded its byte limit") })
      return
    }
    const newline = buffer.indexOf(0x0a)
    if (newline >= 0) finish({ line: buffer.subarray(0, newline).toString("utf8") })
  }
  const onStderr = (chunk: Buffer) => {
    stderr = `${stderr}${chunk.toString("utf8")}`.slice(-MAX_STDERR_BYTES)
  }
  const onError = (cause: Error) => {
    finish({ error: new HexError("startup-failed", "Could not start the HEX helper", { cause }) })
  }
  const onExit = (code: number | null, exitSignal: NodeJS.Signals | null) => {
    const detail = stderr.trim()
    finish({
      error: new HexError(
        "service-exited",
        `HEX exited before becoming ready (${code ?? exitSignal ?? "unknown"})${detail ? `: ${detail}` : ""}`,
      ),
    })
  }
  const onAbort = () => {
    finish({ error: new HexError("cancelled", "HEX startup was cancelled", { cause: signal?.reason }) })
  }
  const timeout = setTimeout(() => {
    finish({ error: new HexError("startup-timeout", `HEX did not become ready within ${timeoutMs}ms`) })
  }, timeoutMs)
  child.stdout.on("data", onData)
  child.stderr.on("data", onStderr)
  child.once("error", onError)
  child.once("exit", onExit)
  signal?.addEventListener("abort", onAbort, { once: true })
  if (signal?.aborted === true) onAbort()
})

export const startHost = async (options: Omit<CreateOptions, "model"> = {}): Promise<HexHost> => {
  if (options.signal?.aborted) {
    throw new HexError("cancelled", "HEX startup was cancelled", { cause: options.signal.reason })
  }
  const [executable, ...arguments_] = resolveCommand(options)
  const child = spawn(executable, arguments_, {
    cwd: options.cwd,
    env: options.env === undefined ? process.env : { ...process.env, ...options.env },
    stdio: ["pipe", "pipe", "pipe"],
  })
  const lifetime = new AbortController()
  let closed: Promise<void> | undefined
  const close = (): Promise<void> => {
    closed ??= (async () => {
      lifetime.abort(new HexError("service-exited", "HEX host closed"))
      await stop(child, options.shutdownTimeoutMs ?? DEFAULT_SHUTDOWN_TIMEOUT_MS)
    })()
    return closed
  }
  try {
    const line = await readHandshake(child, options.startupTimeoutMs ?? DEFAULT_STARTUP_TIMEOUT_MS, options.signal)
    const endpoint = decodeEndpoint(line)
    if (endpoint.pid !== child.pid) {
      throw new HexError("invalid-handshake", "HEX reported a different process identifier")
    }
    child.stderr.resume()
    child.stdout.resume()
    child.once("exit", (code, exitSignal) => {
      lifetime.abort(new HexError(
        "service-exited",
        `Embedded HEX exited (${code ?? exitSignal ?? "unknown"})`,
      ))
    })
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new HexError("service-exited", "HEX exited after its startup handshake")
    }
    const client = makeClient(endpoint, options.fetch ?? globalThis.fetch, lifetime.signal)
    return { pid: endpoint.pid, client, close }
  } catch (error) {
    await close()
    throw error
  }
}

export const prepareTranscriber = async (host: HexHost, options: TranscriberOptions): Promise<Transcriber> => {
  const { model, language = "en", signal, onProgress } = options
  try {
    await host.client.models.prepare(model, {
      language,
      ...(signal === undefined ? {} : { signal }),
      ...(onProgress === undefined ? {} : { onProgress }),
    })
    if (signal?.aborted) throw new HexError("cancelled", "HEX preparation was cancelled", { cause: signal.reason })
  } catch (error) {
    await host.close()
    throw error
  }
  return {
    pid: host.pid,
    model,
    language,
    close: host.close,
    async transcribe(audio, { signal } = {}) {
      if (signal?.aborted) {
        throw new HexError("cancelled", "HEX transcription was cancelled", { cause: signal.reason })
      }
      try {
        const result = await host.client.transcribe({
          audio: { data: audio, contentType: "audio/wav" }, model, language,
          ...(signal === undefined ? {} : { signal }),
        })
        if (signal?.aborted) throw new HexError("cancelled", "HEX transcription was cancelled", { cause: signal.reason })
        return result
      } catch (error) {
        // Socket cancellation alone cannot stop native inference. Wait for its owner to exit.
        if (signal?.aborted || error instanceof HexError && error.code === "service-exited") {
          await host.close()
        }
        throw error
      }
    },
  }
}

export function create(options: TranscriberOptions): Promise<Transcriber>
export function create(options?: CreateOptions): Promise<HexHost>
export async function create(options: CreateOptions | TranscriberOptions = {}): Promise<HexHost | Transcriber> {
  if (options.model !== undefined) {
    const snapshot = { ...options }
    return prepareTranscriber(await startHost(snapshot), snapshot)
  }
  return startHost(options)
}

export const connect = async (options: ConnectOptions = {}): Promise<HexClient> => {
  const discoveryPath = options.discoveryPath
    ?? join(process.env.HEX_APPLICATION_SUPPORT_DIR ?? join(homedir(), "Library", "Application Support", "voice-control"), "local-api.json")
  let value: unknown
  try {
    value = JSON.parse(await readFile(discoveryPath, "utf8"))
  } catch (cause) {
    throw new HexError("startup-failed", "Could not read the running HEX endpoint", { cause })
  }
  if (typeof value !== "object" || value === null) {
    throw new HexError("invalid-handshake", "HEX discovery contained an invalid endpoint")
  }
  const input = value as Record<string, unknown>
  const endpoint = decodeEndpoint(JSON.stringify({
    type: "ready",
    url: `http://127.0.0.1:${String(input.port)}`,
    token: input.token,
    apiVersion: input.apiVersion,
    pid: input.pid,
  }))
  const lifetime = options.signal ?? new AbortController().signal
  const client = makeClient(endpoint, options.fetch ?? globalThis.fetch, lifetime)
  await client.health(options.signal === undefined ? undefined : { signal: options.signal })
  return client
}
