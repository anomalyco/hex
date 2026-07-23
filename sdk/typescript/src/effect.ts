import { Cause, Context, Effect, Layer, Queue, Schema, Scope, Stream } from "effect"
import { HexError as PromiseHexError } from "./errors.js"
import { create as createPromiseHost } from "./host.js"
import type {
  Capabilities,
  CreateOptions as PromiseCreateOptions,
  Health,
  ModelId,
  ModelInfo,
  ModelProgress,
  TranscriptionRequest,
  TranscriptionResult,
} from "./types.js"

const ErrorFields = {
  code: Schema.String,
  message: Schema.String,
  cause: Schema.optionalKey(Schema.Defect()),
}

export class StartupError extends Schema.TaggedErrorClass<StartupError>()(
  "Hex.StartupError",
  ErrorFields,
) {}

export class ProtocolError extends Schema.TaggedErrorClass<ProtocolError>()(
  "Hex.ProtocolError",
  ErrorFields,
) {}

export class RequestError extends Schema.TaggedErrorClass<RequestError>()(
  "Hex.RequestError",
  {
    ...ErrorFields,
    status: Schema.optionalKey(Schema.Number),
    remoteCode: Schema.optionalKey(Schema.String),
  },
) {}

export class ModelPreparationError extends Schema.TaggedErrorClass<ModelPreparationError>()(
  "Hex.ModelPreparationError",
  {
    ...ErrorFields,
    remoteCode: Schema.optionalKey(Schema.String),
  },
) {}

export class CancellationError extends Schema.TaggedErrorClass<CancellationError>()(
  "Hex.CancellationError",
  ErrorFields,
) {}

export type HexError =
  | StartupError
  | ProtocolError
  | RequestError
  | ModelPreparationError
  | CancellationError

export type CreateOptions = Omit<PromiseCreateOptions, "signal">

export interface PrepareModelOptions {
  readonly language?: string
}

export interface ListModelsOptions {
  readonly language?: string
}

export interface Interface {
  readonly health: () => Effect.Effect<Health, HexError>
  readonly capabilities: () => Effect.Effect<Capabilities, HexError>
  readonly models: {
    readonly list: (options?: ListModelsOptions) => Effect.Effect<readonly ModelInfo[], HexError>
    readonly prepare: (id: ModelId, options?: PrepareModelOptions) => Stream.Stream<ModelProgress, HexError>
  }
  readonly transcribe: (
    request: Omit<TranscriptionRequest, "signal">,
  ) => Effect.Effect<TranscriptionResult, HexError>
}

export interface Host {
  readonly pid: number
  readonly client: Interface
}

export class Service extends Context.Service<Service, Interface>()("@hex-ai/client/Hex") {}

const causeFields = (cause: unknown): { readonly cause?: unknown } =>
  cause === undefined ? {} : { cause }

const toHexError = (error: unknown): HexError => {
  if (!(error instanceof PromiseHexError)) {
    return new RequestError({ code: "unknown", message: "HEX operation failed", cause: error })
  }
  const fields = {
    code: error.code,
    message: error.message,
    ...(error.remoteCode === undefined ? {} : { remoteCode: error.remoteCode }),
    ...causeFields(error.cause),
  }
  switch (error.code) {
    case "startup-failed":
    case "startup-timeout":
    case "service-exited":
    case "shutdown-failed":
      return new StartupError(fields)
    case "cancelled":
      return new CancellationError(fields)
    case "invalid-handshake":
    case "invalid-response":
      return new ProtocolError(fields)
    case "model-prepare-failed":
      return new ModelPreparationError(fields)
    case "request-failed":
      return new RequestError({
        ...fields,
        ...(error.status === undefined ? {} : { status: error.status }),
      })
  }
}

const fromPromise = <A>(
  operation: (signal: AbortSignal) => Promise<A>,
): Effect.Effect<A, HexError> => Effect.tryPromise({ try: operation, catch: toHexError })

const makeClient = (client: Awaited<ReturnType<typeof createPromiseHost>>["client"]): Interface => ({
  health: Effect.fn("Hex.health")(function* () {
    return yield* fromPromise((signal) => client.health({ signal }))
  }),
  capabilities: Effect.fn("Hex.capabilities")(function* () {
    return yield* fromPromise((signal) => client.capabilities({ signal }))
  }),
  models: {
    list: Effect.fn("Hex.models.list")(function* (options) {
      return yield* fromPromise((signal) => client.models.list({
        ...(options?.language === undefined ? {} : { language: options.language }),
        signal,
      }))
    }),
    prepare: (id, options) => Stream.callback<ModelProgress, HexError>(
      (queue) =>
        Effect.acquireRelease(
          Effect.sync(() => {
            const controller = new AbortController()
            void client.models.prepare(id, {
              ...(options?.language === undefined ? {} : { language: options.language }),
              signal: controller.signal,
              onProgress: (progress) => {
                Queue.offerUnsafe(queue, progress)
              },
            }).then(
              () => {
                Queue.endUnsafe(queue)
              },
              (error: unknown) => {
                Queue.failCauseUnsafe(queue, Cause.fail(toHexError(error)))
              },
            )
            return controller
          }),
          (controller) => Effect.sync(() => controller.abort()),
        ),
      { bufferSize: 32, strategy: "sliding" },
    ),
  },
  transcribe: Effect.fn("Hex.transcribe")(function* (request) {
    return yield* fromPromise((signal) => client.transcribe({ ...request, signal }))
  }),
})

export const create = (options: CreateOptions = {}): Effect.Effect<Host, HexError, Scope.Scope> =>
  Effect.acquireRelease(
    fromPromise((signal) => createPromiseHost({ ...options, signal })),
    (host) =>
      Effect.tryPromise({ try: () => host.close(), catch: toHexError }).pipe(
        Effect.catch((error) => Effect.logError(error)),
      ),
  ).pipe(Effect.map((host) => ({ pid: host.pid, client: makeClient(host.client) })))

export const layer = (options: CreateOptions = {}): Layer.Layer<Service, HexError> =>
  Layer.effect(Service, create(options).pipe(Effect.map((host) => Service.of(host.client))))
