import { pathToFileURL } from "node:url"
import { Cause, Deferred, Effect, Exit, Fiber } from "effect"
import { Hex, ToolCallError } from "./effect.js"
import type { EffectHandler, HexService } from "./effect.js"
import {
  openApplication,
  openPath,
  openUrl,
  press,
  PROTOCOL_VERSION,
  typeText,
} from "./model.js"
import type {
  Handler,
  HexCapabilities,
  HexConfig,
  Modifier,
  NativeAction,
  PromiseHex,
} from "./model.js"
import type { HostInput, HostOutput, Registration, RegistrationCommand } from "./protocol.js"

const MAX_COMMANDS = 512
const MAX_PHRASES_PER_COMMAND = 16
const MAX_PROTOCOL_PHRASES = 16
const MAX_REGISTRATION_BYTES = 256 * 1024
const MAX_FRAME_BYTES = 64 * 1024
const MAX_ID_BYTES = 128
const MAX_LABEL_BYTES = 1024
const MAX_VALUE_BYTES = 4096
const MAX_ERROR_BYTES = 4096
const MAX_PRESS_REPEAT = 100
const MAX_PENDING_TOOL_CALLS = 1024
const SHUTDOWN_TIMEOUT_MS = 2_000

type HostHandler = Handler | EffectHandler
type HostHandlerFunction = Exclude<HostHandler, Effect.Effect<void, unknown, Hex>>

const utf8Length = (value: string): number => Buffer.byteLength(value, "utf8")

const boundedString = (value: unknown, label: string, maxBytes: number): string => {
  if (typeof value !== "string" || value.length === 0 || utf8Length(value) > maxBytes) {
    throw new Error(`${label} must be a non-empty string no longer than ${maxBytes} UTF-8 bytes`)
  }
  return value
}

const record = (value: unknown): Record<string, unknown> | undefined =>
  typeof value === "object" && value !== null && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value))
    : undefined

const isFunction = (value: unknown): value is HostHandlerFunction =>
  typeof value === "function"

// Bound and normalize host input; Rust independently validates every registered action.
const validateNativeAction = (value: unknown, label: string): NativeAction | undefined => {
  const action = record(value)
  switch (action?.type) {
    case "openUrl": {
      if (typeof action.url !== "string") return undefined
      const url = boundedString(action.url, `${label}.url`, MAX_VALUE_BYTES)
      let parsed: URL
      try {
        parsed = new URL(url)
      } catch {
        throw new Error(`${label}.url must be an absolute URL`)
      }
      if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
        throw new Error(`${label}.url must use http or https`)
      }
      return openUrl(url)
    }
    case "openApplication":
      return typeof action.application === "string"
        ? openApplication(boundedString(action.application, `${label}.application`, MAX_VALUE_BYTES))
        : undefined
    case "openPath":
      return typeof action.path === "string"
        ? openPath(boundedString(action.path, `${label}.path`, MAX_VALUE_BYTES))
        : undefined
    case "typeText":
      return typeof action.text === "string"
        ? typeText(boundedString(action.text, `${label}.text`, MAX_VALUE_BYTES))
        : undefined
    case "press": {
      if (typeof action.key !== "string") return undefined
      const key = boundedString(action.key, `${label}.key`, 64)
      const modifiers = action.modifiers
      if (modifiers !== undefined && (
        !Array.isArray(modifiers)
        || modifiers.length > 5
        || !modifiers.every((modifier) =>
          modifier === "command"
          || modifier === "control"
          || modifier === "option"
          || modifier === "shift")
      )) throw new Error(`${label}.modifiers contains an unsupported modifier`)
      const repeat = action.repeat
      if (repeat !== undefined && (
        typeof repeat !== "number"
        || !Number.isInteger(repeat)
        || repeat < 1
        || repeat > MAX_PRESS_REPEAT
      )) throw new Error(`${label}.repeat must be an integer from 1 through ${MAX_PRESS_REPEAT}`)
      const namedKeys = new Set(["home", "end", "up", "down", "left", "right", "enter", "escape"])
      if (!namedKeys.has(key.toLowerCase()) && Array.from(key).length !== 1) {
        throw new Error(`${label}.key must be a character or supported named key`)
      }
      return press({
        key,
        ...(modifiers === undefined ? {} : { modifiers: modifiers as readonly Modifier[] }),
        ...(repeat === undefined ? {} : { repeat }),
      })
    }
    default:
      return undefined
  }
}

const adaptCapabilities = <Result>(
  dispatch: (action: NativeAction) => Result,
): HexCapabilities<Result> => ({
  openUrl: (url) => dispatch(openUrl(url)),
  openApplication: (application) => dispatch(openApplication(application)),
  openPath: (path) => dispatch(openPath(path)),
  press: (input) => dispatch(press(input)),
  typeText: (text) => dispatch(typeText(text)),
})

interface PreparedConfig {
  readonly registration: Registration
  readonly handlers: ReadonlyMap<string, HostHandler>
}

export const prepareConfig = (value: unknown): PreparedConfig => {
  const config = record(value)
  const commands = record(config?.commands)
  if (commands === undefined) throw new Error("The default export must be a HEX config with a commands object")
  const entries = Object.entries(commands)
  if (entries.length > MAX_COMMANDS) throw new Error(`A config may register at most ${MAX_COMMANDS} commands`)

  const handlers = new Map<string, HostHandler>()
  const rawDictation = config?.dictation
  let dictation: Registration["dictation"]
  if (rawDictation !== undefined) {
    const candidate = record(rawDictation)
    if (candidate === undefined) throw new Error("dictation must be an object")
    const validatePhrases = (control: "start" | "stop" | "send" | "cancel"): readonly string[] => {
      const phrases = candidate[control]
      if (!Array.isArray(phrases) || phrases.length === 0 || phrases.length > MAX_PROTOCOL_PHRASES) {
        throw new Error(`dictation.${control} must contain 1 through ${MAX_PROTOCOL_PHRASES} phrases`)
      }
      return phrases.map((phrase, index) =>
        boundedString(phrase, `dictation.${control}[${index}]`, 256))
    }
    dictation = {
      start: validatePhrases("start") as [string, ...string[]],
      stop: validatePhrases("stop") as [string, ...string[]],
      send: validatePhrases("send") as [string, ...string[]],
      cancel: validatePhrases("cancel") as [string, ...string[]],
    }
  }
  const registrationCommands = entries.map(([rawId, rawDefinition]): RegistrationCommand => {
    const id = boundedString(rawId, "command id", MAX_ID_BYTES)
    const definition = record(rawDefinition)
    if (definition === undefined) throw new Error(`commands.${id} must be an object`)
    const phrases = definition.phrases
    if (!Array.isArray(phrases) || phrases.length === 0 || phrases.length > MAX_PHRASES_PER_COMMAND) {
      throw new Error(`commands.${id}.phrases must contain 1 through ${MAX_PHRASES_PER_COMMAND} phrases`)
    }
    const validatedPhrases = phrases.map((phrase, index) =>
      boundedString(phrase, `commands.${id}.phrases[${index}]`, 256))
    const group = definition.group === undefined
      ? undefined
      : boundedString(definition.group, `commands.${id}.group`, MAX_LABEL_BYTES)
    const description = definition.description === undefined
      ? undefined
      : boundedString(definition.description, `commands.${id}.description`, MAX_LABEL_BYTES)
    const when = record(definition.when)
    let validatedWhen: RegistrationCommand["when"]
    if (when !== undefined) {
      const hasApplication = when.application !== undefined
      const hasBrowserHost = when.browserHost !== undefined
      if (hasApplication === hasBrowserHost) {
        throw new Error(`commands.${id}.when must contain exactly one context predicate`)
      }
      validatedWhen = hasApplication
        ? { application: boundedString(when.application, `commands.${id}.when.application`, 256) }
        : { browserHost: boundedString(when.browserHost, `commands.${id}.when.browserHost`, 253) }
    }

    const hasAction = definition.action !== undefined
    const hasRun = definition.run !== undefined
    if (hasAction === hasRun) throw new Error(`commands.${id} must contain exactly one of action or run`)
    const candidate = hasAction ? definition.action : definition.run
    const action = validateNativeAction(candidate, `commands.${id}.action`)
    if (action === undefined && !isFunction(candidate) && !Effect.isEffect(candidate)) {
      throw new Error(`commands.${id} has an unsupported execution value`)
    }
    if (isFunction(candidate)) {
      handlers.set(id, candidate)
    } else if (Effect.isEffect(candidate)) {
      handlers.set(id, candidate)
    }
    return {
      id,
      phrases: validatedPhrases,
      ...(group === undefined ? {} : { group }),
      ...(description === undefined ? {} : { description }),
      ...(validatedWhen === undefined ? {} : { when: validatedWhen }),
      execution: action === undefined
        ? { type: "handler" }
        : { type: "native", action },
    }
  })

  const registration: Registration = {
    type: "registration",
    protocolVersion: PROTOCOL_VERSION,
    ...(dictation === undefined ? {} : { dictation }),
    commands: registrationCommands,
  }
  if (utf8Length(JSON.stringify(registration)) > MAX_REGISTRATION_BYTES) {
    throw new Error(`The serialized registration exceeds ${MAX_REGISTRATION_BYTES} bytes`)
  }
  return { registration, handlers }
}

export const evaluateConfig = async (entrypoint: string): Promise<unknown> => {
  const module: unknown = await import(pathToFileURL(entrypoint).href)
  return record(module)?.default
}

const errorMessage = (error: unknown): string => {
  const message = error instanceof Error ? error.message : String(error)
  return utf8Length(message) <= MAX_ERROR_BYTES
    ? message
    : Buffer.from(message).subarray(0, MAX_ERROR_BYTES).toString("utf8")
}

const decodeInput = (line: string): HostInput => {
  let value: unknown
  try {
    value = JSON.parse(line)
  } catch {
    throw new Error("Received malformed NDJSON")
  }
  const input = record(value)
  switch (input?.type) {
    case "invoke":
      const context = record(input.context)
      if (context === undefined) throw new Error("invoke.context must be an object")
      const application = context.application === undefined
        ? undefined
        : boundedString(context.application, "invoke.context.application", MAX_VALUE_BYTES)
      const browserUrl = context.browserUrl === undefined
        ? undefined
        : boundedString(context.browserUrl, "invoke.context.browserUrl", MAX_VALUE_BYTES)
      const windowTitle = context.windowTitle === undefined
        ? undefined
        : boundedString(context.windowTitle, "invoke.context.windowTitle", MAX_VALUE_BYTES)
      const browserHost = context.browserHost === undefined
        ? browserUrl === undefined ? undefined : new URL(browserUrl).hostname
        : boundedString(context.browserHost, "invoke.context.browserHost", 253)
      return {
        type: "invoke",
        invocationId: boundedString(input.invocationId, "invoke.invocationId", MAX_ID_BYTES),
        commandId: boundedString(input.commandId, "invoke.commandId", MAX_ID_BYTES),
        context: {
          ...(application === undefined ? {} : { application }),
          ...(browserHost === undefined ? {} : { browserHost }),
          ...(browserUrl === undefined ? {} : { browserUrl }),
          ...(windowTitle === undefined ? {} : { windowTitle }),
        },
      }
    case "toolResult": {
      const result = record(input.result)
      const invocationId = boundedString(input.invocationId, "toolResult.invocationId", MAX_ID_BYTES)
      const toolCallId = boundedString(input.toolCallId, "toolResult.toolCallId", MAX_ID_BYTES)
      if (result?.type === "success") {
        return { type: "toolResult", invocationId, toolCallId, result: { type: "success" } }
      }
      if (result?.type === "failure") {
        const code = result.code === undefined
          ? undefined
          : boundedString(result.code, "toolResult.result.code", MAX_ID_BYTES)
        return {
          type: "toolResult",
          invocationId,
          toolCallId,
          result: {
            type: "failure",
            message: boundedString(result.message, "toolResult.result.message", MAX_ERROR_BYTES),
            ...(code === undefined ? {} : { code }),
          },
        }
      }
      throw new Error("toolResult.result must be success or failure")
    }
    case "shutdown":
      return { type: "shutdown" }
    default:
      throw new Error("Received an unsupported host message")
  }
}

const frames = async function* (input: AsyncIterable<string | Uint8Array>): AsyncGenerator<HostInput> {
  const decoder = new TextDecoder()
  let buffer = ""
  for await (const chunk of input) {
    buffer += typeof chunk === "string" ? chunk : decoder.decode(chunk, { stream: true })
    if (utf8Length(buffer) > MAX_FRAME_BYTES && !buffer.includes("\n")) {
      throw new Error(`Input frame exceeds ${MAX_FRAME_BYTES} bytes`)
    }
    let newline = buffer.indexOf("\n")
    while (newline >= 0) {
      const line = buffer.slice(0, newline)
      buffer = buffer.slice(newline + 1)
      if (utf8Length(line) > MAX_FRAME_BYTES) throw new Error(`Input frame exceeds ${MAX_FRAME_BYTES} bytes`)
      if (line.trim().length > 0) yield decodeInput(line)
      newline = buffer.indexOf("\n")
    }
  }
  buffer += decoder.decode()
  if (buffer.trim().length > 0) {
    if (utf8Length(buffer) > MAX_FRAME_BYTES) throw new Error(`Input frame exceeds ${MAX_FRAME_BYTES} bytes`)
    yield decodeInput(buffer)
  }
}

export interface HostOptions {
  readonly config: HexConfig | unknown
  readonly input: AsyncIterable<string | Uint8Array>
  readonly write: (frame: HostOutput) => void | Promise<void>
}

interface PendingToolCall {
  readonly invocationId: string
  readonly deferred: Deferred.Deferred<void, ToolCallError>
}

export const runHost = async ({ config, input, write }: HostOptions): Promise<void> => {
  const prepared = prepareConfig(config)
  let writes = Promise.resolve()
  let rejectOutput: (error: unknown) => void = () => undefined
  const outputFailed = new Promise<never>((_, reject) => { rejectOutput = reject })
  let closing = false
  const send = (frame: HostOutput): Promise<void> => {
    const encoded = JSON.stringify(frame)
    if (utf8Length(encoded) > MAX_FRAME_BYTES && frame.type !== "registration") {
      return Promise.reject(new Error(`Output frame exceeds ${MAX_FRAME_BYTES} bytes`))
    }
    writes = writes.then(() => write(frame)).catch((error) => {
      rejectOutput(error)
      throw error
    })
    return writes
  }

  await send(prepared.registration)
  const fibers = new Map<string, Fiber.Fiber<void, unknown>>()
  const pendingTools = new Map<string, PendingToolCall>()
  let nextToolCallId = 0

  const callTool = (invocationId: string, action: NativeAction): Effect.Effect<void, ToolCallError> =>
    Effect.gen(function* () {
      if (pendingTools.size >= MAX_PENDING_TOOL_CALLS) {
        return yield* Effect.fail(new ToolCallError({ message: "Too many tool calls are awaiting native results" }))
      }
      const toolCallId = String(++nextToolCallId)
      const deferred = yield* Deferred.make<void, ToolCallError>()
      pendingTools.set(toolCallId, { invocationId, deferred })
      return yield* Effect.gen(function* () {
        yield* Effect.tryPromise({
          try: () => send({ type: "toolCall", invocationId, toolCallId, action }),
          catch: (error) => new ToolCallError({ message: errorMessage(error) }),
        })
        return yield* Deferred.await(deferred)
      }).pipe(Effect.ensuring(Effect.sync(() => pendingTools.delete(toolCallId))))
    })

  const makeHex = (
    invocationId: string,
    context: Extract<HostInput, { readonly type: "invoke" }>["context"],
  ): HexService => ({
    context,
    ...adaptCapabilities((action) => callTool(invocationId, action)),
  })

  const makePromiseHex = (invocationId: string, signal: AbortSignal): {
    readonly hex: PromiseHex
    readonly awaitCalls: () => Promise<void>
  } => {
    const calls: Promise<void>[] = []
    const run = (action: NativeAction): Promise<void> => {
      const call = Effect.runPromise(callTool(invocationId, action), { signal })
      const observed = call.then(() => undefined)
      // Observe both promises immediately; awaitCalls still propagates failure
      // into the invocation even if user code ignores the returned promise.
      void observed.catch(() => undefined)
      calls.push(observed)
      return call
    }
    return {
      hex: adaptCapabilities(run),
      awaitCalls: () => Promise.all(calls).then(() => undefined),
    }
  }

  const invoke = (
    invocationId: string,
    commandId: string,
    context: Extract<HostInput, { readonly type: "invoke" }>["context"],
  ): void => {
    if (fibers.has(invocationId)) {
      void send({
        type: "invocationResult",
        invocationId,
        result: { type: "failure", message: "Invocation ID is already active" },
      }).catch(() => undefined)
      return
    }
    const handler = prepared.handlers.get(commandId)
    if (handler === undefined) {
      void send({
        type: "invocationResult",
        invocationId,
        result: { type: "failure", message: `Command ${commandId} has no host handler` },
      }).catch(() => undefined)
      return
    }
    const operation: Effect.Effect<void, unknown, Hex> = typeof handler !== "function"
      ? handler
      : Effect.acquireUseRelease(
        Effect.sync(() => new AbortController()),
        (controller) => Effect.suspend(() => {
          const promiseHex = makePromiseHex(invocationId, controller.signal)
          const result = handler({ hex: promiseHex.hex, context })
          if (Effect.isEffect(result)) {
            return result.pipe(
              Effect.andThen(Effect.tryPromise({
                try: promiseHex.awaitCalls,
                catch: (error) => error,
              })),
            )
          }
          return Effect.tryPromise({
            try: async () => {
              await result
              await promiseHex.awaitCalls()
            },
            catch: (error) => error,
          })
        }),
        (controller) => Effect.sync(() => controller.abort()),
      )
    const fiber = Effect.runFork(
      operation.pipe(Effect.provideService(Hex, Hex.of(makeHex(invocationId, context)))),
    )
    fibers.set(invocationId, fiber)
    fiber.addObserver((exit) => {
      fibers.delete(invocationId)
      if (closing) return
      const result = Exit.isSuccess(exit)
        ? { type: "success" as const }
        : { type: "failure" as const, message: errorMessage(Cause.squash(exit.cause)) }
      void send({ type: "invocationResult", invocationId, result }).catch(() => undefined)
    })
  }

  try {
    await Promise.race([
      (async () => {
        for await (const message of frames(input)) {
          if (message.type === "invoke") {
            invoke(message.invocationId, message.commandId, message.context)
          } else if (message.type === "toolResult") {
            const pending = pendingTools.get(message.toolCallId)
            if (pending === undefined || pending.invocationId !== message.invocationId) {
              throw new Error("toolResult does not match a pending tool call")
            }
            if (message.result.type === "success") {
              Effect.runFork(Deferred.succeed(pending.deferred, undefined))
            } else {
              Effect.runFork(Deferred.fail(pending.deferred, new ToolCallError({
                message: message.result.message,
                ...(message.result.code === undefined ? {} : { code: message.result.code }),
              })))
            }
          } else {
            break
          }
        }
      })(),
      outputFailed,
    ])
  } finally {
    closing = true
    for (const pending of pendingTools.values()) {
      Effect.runFork(Deferred.fail(pending.deferred, new ToolCallError({ message: "Command host stopped" })))
    }
    pendingTools.clear()
    const interrupt = Effect.runPromise(Fiber.interruptAll(fibers.values()))
    await Promise.race([
      interrupt,
      new Promise<void>((resolve) => setTimeout(resolve, SHUTDOWN_TIMEOUT_MS)),
    ])
    await Promise.race([
      writes.catch(() => undefined),
      new Promise<void>((resolve) => setTimeout(resolve, SHUTDOWN_TIMEOUT_MS)),
    ])
  }
}
