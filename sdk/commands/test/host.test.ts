import { Deferred, Effect } from "effect"
import { describe, expect, it } from "vitest"
import { Hex } from "../src/effect.js"
import { openUrl } from "../src/index.js"
import type { HandlerArguments } from "../src/index.js"
import { prepareConfig, runHost } from "../src/host.js"
import type { HostOutput } from "../src/protocol.js"

async function* messages(lines: readonly unknown[]): AsyncGenerator<string> {
  for (const line of lines) yield `${JSON.stringify(line)}\n`
}

describe("command host", () => {
  it("emits a bounded serializable registration without handlers", () => {
    const prepared = prepareConfig({
      dictation: {
        start: ["begin note"],
        stop: ["finish note"],
        send: ["send note"],
        cancel: ["discard note"],
      },
      commands: {
        native: { phrases: ["open example"], action: openUrl("https://example.com") },
        handled: { phrases: ["do work"], group: "Work", run: () => undefined },
      },
    })
    expect(prepared.registration).toEqual({
      type: "registration",
      protocolVersion: 1,
      dictation: {
        start: ["begin note"],
        stop: ["finish note"],
        send: ["send note"],
        cancel: ["discard note"],
      },
      commands: [
        {
          id: "native",
          phrases: ["open example"],
          execution: { type: "native", action: { type: "openUrl", url: "https://example.com" } },
        },
        {
          id: "handled",
          phrases: ["do work"],
          group: "Work",
          execution: { type: "handler" },
        },
      ],
    })
    expect(() => JSON.stringify(prepared.registration)).not.toThrow()
  })

  it("adapts vanilla handlers and round-trips tool calls", async () => {
    const output: HostOutput[] = []
    let observedApplication: string | undefined
    await runHost({
      config: {
        commands: {
          example: {
            phrases: ["open example"],
            run: async ({ hex, context }: HandlerArguments) => {
              observedApplication = context.application
              await hex.openUrl("https://example.com")
            },
          },
        },
      },
      input: messages([
        {
          type: "invoke",
          invocationId: "inv-1",
          commandId: "example",
          context: { application: "Brave Browser", browserUrl: "https://example.com/page" },
        },
        {
          type: "toolResult",
          invocationId: "inv-1",
          toolCallId: "1",
          result: { type: "success" },
        },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })
    expect(output.map((frame) => frame.type)).toEqual([
      "registration",
      "toolCall",
      "invocationResult",
    ])
    expect(output[1]).toEqual({
      type: "toolCall",
      invocationId: "inv-1",
      toolCallId: "1",
      action: { type: "openUrl", url: "https://example.com" },
    })
    expect(observedApplication).toBe("Brave Browser")
  })

  it("runs Effect handlers concurrently", async () => {
    const firstStarted = await Effect.runPromise(Deferred.make<void>())
    const releaseFirst = await Effect.runPromise(Deferred.make<void>())
    const secondToolCalled = await Effect.runPromise(Deferred.make<void>())
    const output: HostOutput[] = []
    async function* input(): AsyncGenerator<string> {
      yield `${JSON.stringify({ type: "invoke", invocationId: "first", commandId: "first", context: {} })}\n`
      await Effect.runPromise(Deferred.await(firstStarted))
      yield `${JSON.stringify({ type: "invoke", invocationId: "second", commandId: "second", context: {} })}\n`
      await Effect.runPromise(Deferred.await(secondToolCalled))
      yield `${JSON.stringify({ type: "toolResult", invocationId: "second", toolCallId: "1", result: { type: "success" } })}\n`
      await Effect.runPromise(Deferred.succeed(releaseFirst, undefined))
      yield `${JSON.stringify({ type: "shutdown" })}\n`
    }
    await runHost({
      config: {
        commands: {
          first: {
            phrases: ["first"],
            run: Effect.gen(function* () {
              yield* Deferred.succeed(firstStarted, undefined)
              yield* Deferred.await(releaseFirst)
            }),
          },
          second: {
            phrases: ["second"],
            run: Effect.gen(function* () {
              const hex = yield* Hex
              yield* hex.typeText("second")
            }),
          },
        },
      },
      input: input(),
      write: (frame) => {
        output.push(frame)
        if (frame.type === "toolCall" && frame.invocationId === "second") {
          Effect.runFork(Deferred.succeed(secondToolCalled, undefined))
        }
      },
    })
    expect(output.some((frame) => frame.type === "toolCall" && frame.invocationId === "second")).toBe(true)
    expect(output.filter((frame) => frame.type === "invocationResult")).toHaveLength(2)
  })

  it("validates native actions from both execution fields", () => {
    expect(() => prepareConfig({ commands: { bad: { phrases: [], run: () => undefined } } })).toThrow(
      "phrases must contain",
    )
    expect(() => prepareConfig({
      commands: { bad: { phrases: ["bad"], action: { type: "press", key: "x", repeat: 101 } } },
    })).toThrow("repeat must be")
    expect(() => prepareConfig({
      commands: { bad: { phrases: ["bad"], run: { type: "openUrl", url: "file:///tmp/example" } } },
    })).toThrow("must use http or https")
    expect(() => prepareConfig({
      commands: { bad: { phrases: ["bad"], action: { type: "press", key: "x", modifiers: ["meta"] } } },
    })).toThrow("unsupported modifier")
  })

  it("bounds every dictation protocol phrase list", () => {
    expect(() => prepareConfig({
      dictation: { start: [], stop: ["stop"], send: ["send"], cancel: ["cancel"] },
      commands: {},
    })).toThrow("dictation.start must contain")
    expect(() => prepareConfig({
      dictation: { start: ["start"], stop: ["stop"], send: ["send"], cancel: [""] },
      commands: {},
    })).toThrow("dictation.cancel[0] must be a non-empty string")
  })

  it("waits for vanilla tool calls even when the handler forgets await", async () => {
    const output: HostOutput[] = []
    await runHost({
      config: {
        commands: {
          example: {
            phrases: ["open example"],
            run: ({ hex }: HandlerArguments) => {
              hex.openUrl("https://example.com")
            },
          },
        },
      },
      input: messages([
        { type: "invoke", invocationId: "inv-1", commandId: "example", context: {} },
        { type: "toolResult", invocationId: "inv-1", toolCallId: "1", result: { type: "success" } },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })

    expect(output.map((frame) => frame.type)).toEqual([
      "registration",
      "toolCall",
      "invocationResult",
    ])
  })

  it("runs Effect-producing handlers and exposes browserHost", async () => {
    const output: HostOutput[] = []
    await runHost({
      config: {
        commands: {
          example: {
            phrases: ["show host"],
            run: ({ context }: { context: { browserHost?: string } }) => Effect.gen(function* () {
              const hex = yield* Hex
              yield* hex.typeText(context.browserHost ?? "missing")
            }),
          },
        },
      },
      input: messages([
        {
          type: "invoke",
          invocationId: "inv-1",
          commandId: "example",
          context: { browserUrl: "https://example.com/page" },
        },
        { type: "toolResult", invocationId: "inv-1", toolCallId: "1", result: { type: "success" } },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })

    expect(output[1]).toMatchObject({
      type: "toolCall",
      action: { type: "typeText", text: "example.com" },
    })
  })

  it("terminates and cleans active work when output fails", async () => {
    await expect(runHost({
      config: {
        commands: {
          example: {
            phrases: ["open example"],
            run: ({ hex }: HandlerArguments) => hex.openUrl("https://example.com"),
          },
        },
      },
      input: messages([
        { type: "invoke", invocationId: "inv-1", commandId: "example", context: {} },
      ]),
      write: (frame) => {
        if (frame.type === "toolCall") throw new Error("broken output")
      },
    })).rejects.toThrow("broken output")
  })

  it("turns an unawaited tool failure into an invocation failure", async () => {
    const output: HostOutput[] = []
    await runHost({
      config: {
        commands: {
          example: {
            phrases: ["open example"],
            run: ({ hex }: HandlerArguments) => {
              hex.openUrl("https://example.com")
            },
          },
        },
      },
      input: messages([
        { type: "invoke", invocationId: "inv-1", commandId: "example", context: {} },
        {
          type: "toolResult",
          invocationId: "inv-1",
          toolCallId: "1",
          result: { type: "failure", message: "native action failed" },
        },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })

    expect(output.at(-1)).toEqual({
      type: "invocationResult",
      invocationId: "inv-1",
      result: { type: "failure", message: "native action failed" },
    })
  })

  it("interrupts pending invocations on shutdown", async () => {
    let releaseShutdown: (() => void) | undefined
    const toolWritten = new Promise<void>((resolve) => { releaseShutdown = resolve })
    async function* input(): AsyncGenerator<string> {
      yield `${JSON.stringify({ type: "invoke", invocationId: "inv-1", commandId: "example", context: {} })}\n`
      await toolWritten
      yield `${JSON.stringify({ type: "shutdown" })}\n`
    }

    await runHost({
      config: {
        commands: {
          example: {
            phrases: ["open example"],
            run: ({ hex }: HandlerArguments) => hex.openUrl("https://example.com"),
          },
        },
      },
      input: input(),
      write: (frame) => {
        if (frame.type === "toolCall") releaseShutdown?.()
      },
    })
  })
})
