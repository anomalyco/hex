import { Deferred, Effect, Fiber, Stream } from "effect"
import { describe, expect, it } from "vitest"
import * as Hex from "../src/effect.js"
import { options, processIsAlive } from "./support.js"

describe("Effect client", () => {
  it.each([
    Hex.StartupError,
    Hex.ProtocolError,
    Hex.RequestError,
    Hex.ModelPreparationError,
    Hex.CancellationError,
  ])("preserves tagged error fields and yieldable failures for %s", async (ErrorClass) => {
    const cause = new Error("underlying failure")
    const error = new ErrorClass({ code: "test-failure", message: "operation failed", cause })
    expect(error).toBeInstanceOf(Error)
    expect(error._tag).toBe(`Hex.${ErrorClass.name}`)
    expect(error.message).toBe("operation failed")
    expect(error.cause).toBe(cause)
    const failure = await Effect.runPromise(Effect.gen(function* () {
      return yield* error
    }).pipe(Effect.flip))
    expect(failure).toBe(error)
  })

  it("scopes the helper and exposes Effect and Stream operations", async () => {
    let pid = 0
    const result = await Effect.runPromise(Effect.scoped(Effect.gen(function* () {
      const host = yield* Hex.create(options())
      pid = host.pid
      const health = yield* host.client.health()
      const progress = yield* host.client.models.prepare("parakeet_v2", { language: "en" }).pipe(
        Stream.runCollect,
      )
      const transcript = yield* host.client.transcribe({
        audio: { data: new Uint8Array([1, 2, 3]), contentType: "audio/wav" },
        model: "parakeet_v2",
      })
      return { health, progress: Array.from(progress, (event) => event.type), transcript }
    })))

    expect(result).toEqual({
      health: { version: "test", apiVersion: "2" },
      progress: ["downloading", "verifying", "loading"],
      transcript: { transcript: "hello from hex", durationMs: 750 },
    })
    expect(processIsAlive(pid)).toBe(false)
  })

  it("provides the client as a scoped service layer", async () => {
    const health = await Effect.runPromise(
      Effect.gen(function* () {
        const hex = yield* Hex.Service
        return yield* hex.health()
      }).pipe(Effect.provide(Hex.layer(options()))),
    )

    expect(health.version).toBe("test")
  })

  it("interrupts an in-flight preparation when its stream scope closes", async () => {
    let pid = 0
    await Effect.runPromise(Effect.scoped(Effect.gen(function* () {
      const host = yield* Hex.create({
        ...options(),
        env: { HEX_FAKE_PREPARE_HANG: "1" },
      })
      pid = host.pid
      const progress = yield* Deferred.make<void>()
      const fiber = yield* host.client.models.prepare("parakeet_v2").pipe(
        Stream.tap(() => Deferred.succeed(progress, undefined)),
        Stream.runDrain,
        Effect.forkScoped,
      )
      yield* Deferred.await(progress)
      yield* Fiber.interrupt(fiber)
    })))

    expect(processIsAlive(pid)).toBe(false)
  })
})
