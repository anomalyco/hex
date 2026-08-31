import { Deferred, Effect, Fiber, Stream } from "effect"
import { watch } from "node:fs"
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { describe, expect, it } from "vitest"
import * as Hex from "../src/effect.js"
import { options, processIsAlive } from "./support.js"

describe("Effect client", () => {
  it("creates a ready transcriber with scope-owned cleanup", async () => {
    let pid = 0
    const progress: string[] = []
    const result = await Effect.runPromise(Effect.scoped(Effect.gen(function* () {
      const transcriber = yield* Hex.create({
        ...options(), model: "parakeet_v2", language: "en",
        onProgress: (event) => { progress.push(event.type) },
      })
      pid = transcriber.pid
      expect(transcriber.language).toBe("en")
      return yield* transcriber.transcribe(new Uint8Array([1, 2, 3]))
    })))
    expect(result.transcript).toBe("hello from hex")
    expect(progress).toEqual(["downloading", "verifying", "loading"])
    expect(processIsAlive(pid)).toBe(false)
  })

  it("interrupts model preparation and waits for helper cleanup", async () => {
    const directory = await mkdtemp(join(tmpdir(), "hex-effect-ready-"))
    const pidPath = join(directory, "pid")
    let markStarted = () => {}
    const started = new Promise<void>((resolve) => { markStarted = resolve })
    try {
      await Effect.runPromise(Effect.scoped(Effect.gen(function* () {
        const fiber = yield* Hex.create({
          ...options(), model: "parakeet_v2",
          env: { HEX_FAKE_PID_PATH: pidPath, HEX_FAKE_PREPARE_HANG: "1" },
          shutdownTimeoutMs: 100,
          onProgress: markStarted,
        }).pipe(Effect.forkScoped)
        yield* Effect.promise(() => started)
        yield* Fiber.interrupt(fiber)
        const pid = yield* Effect.promise(() => readFile(pidPath, "utf8"))
        expect(processIsAlive(Number(pid))).toBe(false)
      })))
    } finally {
      await rm(directory, { recursive: true, force: true })
    }
  })

  it("waits for interrupted inference to stop even while the owning scope stays open", async () => {
    let markStarted = () => {}
    const started = new Promise<void>((resolve) => { markStarted = resolve })
    await Effect.runPromise(Effect.scoped(Effect.gen(function* () {
      const transcriber = yield* Hex.create({
        ...options(), model: "parakeet_v2", shutdownTimeoutMs: 100,
        env: { HEX_FAKE_TRANSCRIBE_HANG: "1" },
        fetch: async (url, init) => {
          const response = await fetch(url, init)
          if (String(url).includes("/transcriptions")) markStarted()
          return response
        },
      })
      const fiber = yield* transcriber.transcribe(new Uint8Array(4)).pipe(Effect.forkScoped)
      yield* Effect.promise(() => started)
      yield* Fiber.interrupt(fiber)
      expect(processIsAlive(transcriber.pid)).toBe(false)
    })))
  })

  it("cleans up pending startup interruption before returning to an open parent scope", async () => {
    const directory = await mkdtemp(join(tmpdir(), "hex-effect-start-"))
    const pidPath = join(directory, "pid")
    const gate = join(directory, "handshake")
    const watcher = watch(directory)
    const started = new Promise<void>((resolve) => {
      watcher.on("change", (_event, file) => {
        if (file === "pid") resolve()
      })
    })
    try {
      await Effect.runPromise(Effect.scoped(Effect.gen(function* () {
        const fiber = yield* Hex.create({
          ...options(), model: "parakeet_v2",
          env: { HEX_FAKE_PID_PATH: pidPath, HEX_FAKE_HANDSHAKE_GATE: gate },
        }).pipe(Effect.forkScoped)
        yield* Effect.promise(() => started)
        fiber.interruptUnsafe()
        yield* Effect.promise(() => writeFile(gate, "ready"))
        yield* Fiber.interrupt(fiber)
        const pid = yield* Effect.promise(() => readFile(pidPath, "utf8"))
        expect(processIsAlive(Number(pid))).toBe(false)
      })))
    } finally {
      watcher.close()
      await rm(directory, { recursive: true, force: true })
    }
  })

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
    let requestSignal: AbortSignal | null | undefined
    await Effect.runPromise(Effect.scoped(Effect.gen(function* () {
      const host = yield* Hex.create({
        ...options(),
        env: { HEX_FAKE_PREPARE_HANG: "1" },
        fetch: (url, init) => {
          if (String(url).includes("/prepare")) requestSignal = init?.signal
          return fetch(url, init)
        },
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
      expect(requestSignal?.aborted).toBe(true)
      expect(processIsAlive(host.pid)).toBe(true)
      expect((yield* host.client.health()).apiVersion).toBe("2")
    })))

    expect(processIsAlive(pid)).toBe(false)
  })

  it("preserves typed preparation failures through the progress stream", async () => {
    await Effect.runPromise(Effect.scoped(Effect.gen(function* () {
      const host = yield* Hex.create({ ...options(), env: { HEX_FAKE_PREPARE_ERROR: "1" } })
      const error = yield* host.client.models.prepare("parakeet_v2").pipe(Stream.runDrain, Effect.flip)
      expect(error).toMatchObject({
        _tag: "Hex.ModelPreparationError", code: "model-prepare-failed", remoteCode: "load-failed",
      })
      expect(processIsAlive(host.pid)).toBe(true)
    })))
  })
})
