// `node:child_process` for scripts: Node's `execFile` and `exec`, with
// their callbacks, errors, and `util.promisify` forms. Hex adds one option,
// `input`, written to the program's stdin. `hex.d.ts` types the subset.

import io from "hex:io"

const CUSTOM = Symbol.for("nodejs.util.promisify.custom")

function abortError(reason) {
  const error = new Error("The operation was aborted", { cause: reason })
  error.name = "AbortError"
  error.code = "ABORT_ERR"
  return error
}

function failure(target, result) {
  if (result.spawnError !== null) {
    const error = new Error(`spawn ${target.file} ${result.spawnError}`)
    error.code = result.spawnError
    error.syscall = `spawn ${target.file}`
    error.path = target.file
    error.spawnargs = target.args
    error.cmd = target.cmd
    return error
  }
  if (result.maxBuffer !== null) {
    const error = new RangeError(`${result.maxBuffer} maxBuffer length exceeded`)
    error.code = "ERR_CHILD_PROCESS_STDIO_MAXBUFFER"
    return error
  }
  if (result.code === 0 && result.signal === null && !result.timedOut) return null
  const error = new Error(`Command failed: ${target.cmd}\n${result.stderr}`)
  error.code = result.code
  error.killed = result.timedOut
  error.signal = result.signal ?? (result.timedOut ? "SIGTERM" : null)
  error.cmd = target.cmd
  return error
}

function request(target, options) {
  if (options === null || typeof options !== "object") throw new TypeError("options must be an object")
  const env = {}
  for (const [name, value] of Object.entries(options.env ?? {})) {
    if (value !== undefined) env[name] = String(value)
  }
  if (options.input !== undefined && typeof options.input !== "string") {
    throw new TypeError("input must be a string")
  }
  return JSON.stringify({
    file: target.file,
    args: target.args,
    shell: target.shell,
    cwd: options.cwd === undefined ? null : String(options.cwd),
    env,
    timeout: options.timeout === undefined ? null : Number(options.timeout),
    maxBuffer: options.maxBuffer === undefined ? null : Number(options.maxBuffer),
    input: options.input ?? null,
  })
}

function run(target, options, callback) {
  if (callback !== undefined && typeof callback !== "function") throw new TypeError("callback must be a function")
  const spec = request(target, options)
  const signal = options.signal ?? null
  let done = false
  let id = 0
  const finish = (error, stdout, stderr) => {
    if (done) return
    done = true
    signal?.removeEventListener("abort", onAbort)
    callback?.(error, stdout, stderr)
  }
  const onAbort = () => {
    if (done) return
    io.cancel(id)
    finish(abortError(signal.reason), "", "")
  }
  const child = {
    pid: undefined,
    killed: false,
    kill() {
      if (done) return false
      io.cancel(id)
      child.killed = true
      finish(failure(target, { spawnError: null, maxBuffer: null, code: null, signal: "SIGTERM", stderr: "", timedOut: false }), "", "")
      return true
    },
  }
  if (signal?.aborted) {
    queueMicrotask(onAbort)
    return child
  }
  try {
    id = io.start(
      "exec",
      spec,
      (json) => {
        const result = JSON.parse(json)
        finish(failure(target, result), result.stdout, result.stderr)
      },
      (message) => finish(new Error(message), "", ""),
    )
  } catch (error) {
    queueMicrotask(() => finish(error, "", ""))
    return child
  }
  signal?.addEventListener("abort", onAbort)
  return child
}

/** Node's `execFile(file[, args][, options][, callback])`: no shell. */
export function execFile(file, args, options, callback) {
  if (typeof args === "function") {
    callback = args
    args = undefined
    options = undefined
  } else if (args != null && !Array.isArray(args)) {
    callback = options
    options = args
    args = undefined
  }
  if (typeof options === "function") {
    callback = options
    options = undefined
  }
  if (typeof file !== "string" || file === "") throw new TypeError("file must be a non-empty string")
  const list = (args ?? []).map(String)
  const target = { file, args: list, shell: false, cmd: [file, ...list].join(" ") }
  return run(target, options ?? {}, callback)
}

/** Node's `exec(command[, options][, callback])`: `/bin/sh -c` or `cmd.exe /d /s /c`. */
export function exec(command, options, callback) {
  if (typeof options === "function") {
    callback = options
    options = undefined
  }
  if (typeof command !== "string" || command === "") throw new TypeError("command must be a non-empty string")
  const target = { file: command, args: [], shell: true, cmd: command }
  return run(target, options ?? {}, callback)
}

function promised(start) {
  return (...args) => {
    let child
    const promise = new Promise((resolve, reject) => {
      child = start(...args, (error, stdout, stderr) => {
        if (error) {
          error.stdout = stdout
          error.stderr = stderr
          reject(error)
        } else {
          resolve({ stdout, stderr })
        }
      })
    })
    promise.child = child
    return promise
  }
}

Object.defineProperty(execFile, CUSTOM, { value: promised(execFile) })
Object.defineProperty(exec, CUSTOM, { value: promised(exec) })

export default { exec, execFile }
