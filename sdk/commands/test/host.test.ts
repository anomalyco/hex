import { Deferred, Effect } from "effect"
import { describe, expect, it } from "vitest"
import { Hex } from "../src/effect.js"
import { choice, digit, letter, openUrl, text, union } from "../src/index.js"
import type { HandlerArguments, Letter } from "../src/index.js"
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
      protocolVersion: 2,
      dictation: {
        start: ["begin note"],
        stop: ["finish note"],
        send: ["send note"],
        cancel: ["discard note"],
      },
      transformations: [],
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

  it("registers and applies ordered transformations", async () => {
    const output: HostOutput[] = []
    await runHost({
      config: {
        transformations: {
          trim: { name: "Trim", transform: (text: string) => text.trim() },
          lowercase: {
            name: "Lowercase",
            description: "Use lowercase output",
            transform: async (text: string) => text.toLowerCase(),
          },
        },
        commands: {},
      },
      input: messages([
        {
          type: "transform",
          invocationId: "transform-1",
          transformationIds: ["trim", "lowercase"],
          text: "  HELLO  ",
          context: { application: "Messages" },
        },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })

    expect(output[0]).toMatchObject({
      type: "registration",
      transformations: [
        { id: "trim", name: "Trim" },
        { id: "lowercase", name: "Lowercase", description: "Use lowercase output" },
      ],
    })
    expect(output[1]).toEqual({
      type: "transformationResult",
      invocationId: "transform-1",
      result: { type: "success", text: "hello" },
    })
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

  it("passes bounded captures to vanilla and Effect handlers", async () => {
    const output: HostOutput[] = []
    const observedCaptures: Readonly<Record<string, string>>[] = []
    let observedEffectCaptures: Readonly<Record<string, string | number>> | undefined
    await runHost({
      config: {
        commands: {
          search: {
            phrases: ["search amazon for {query}"],
            run: async ({ captures }: HandlerArguments) => {
              observedCaptures.push(captures)
            },
          },
          note: {
            phrases: ["note {text}"],
            run: Effect.gen(function* () {
              const hex = yield* Hex
              observedEffectCaptures = hex.captures
            }),
          },
        },
      },
      input: messages([
        {
          type: "invoke",
          invocationId: "inv-1",
          commandId: "search",
          context: {},
          captures: { query: "wool socks" },
        },
        { type: "invoke", invocationId: "inv-2", commandId: "note", context: {}, captures: { text: "buy socks" } },
        { type: "invoke", invocationId: "inv-3", commandId: "search", context: {} },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })
    expect(output.filter((frame) => frame.type === "invocationResult")).toHaveLength(3)
    expect(observedCaptures).toEqual([{ query: "wool socks" }, {}])
    expect(observedEffectCaptures).toEqual({ text: "buy socks" })
  })

  it("registers typed capture schemas and passes numeric values", async () => {
    const output: HostOutput[] = []
    let observed: number | undefined
    await runHost({
      config: {
        commands: {
          control: {
            phrases: ["control {number}"],
            captures: { number: digit({ min: 1, max: 3 }) },
            run: ({ captures }: HandlerArguments<{ readonly number: number }>) => {
              observed = captures.number
            },
          },
        },
      },
      input: messages([
        { type: "invoke", invocationId: "inv-1", commandId: "control", context: {}, captures: { number: 2 } },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })

    expect(output[0]).toMatchObject({
      protocolVersion: 2,
      commands: [{ captures: { number: { type: "digit", min: 1, max: 3 } } }],
    })
    expect(observed).toBe(2)
  })

  it("rejects invocation values that do not match the registered schema", async () => {
    const output: HostOutput[] = []
    await runHost({
      config: {
        commands: {
          control: {
            phrases: ["control {number}"],
            captures: { number: digit({ min: 1, max: 3 }) },
            run: () => undefined,
          },
        },
      },
      input: messages([
        { type: "invoke", invocationId: "bad", commandId: "control", context: {}, captures: { number: 7 } },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })

    expect(output.at(-1)).toEqual({
      type: "invocationResult",
      invocationId: "bad",
      result: { type: "failure", message: "Command control received invalid captures" },
    })
  })

  it("normalizes choice aliases and passes only canonical values", async () => {
    const output: HostOutput[] = []
    let observed: { direction: "left" | "right"; edge: "top" | "bottom" } | undefined
    await runHost({
      config: {
        commands: {
          move: {
            phrases: ["move {direction} {edge}"],
            captures: {
              direction: choice({ left: ["LEFT!", "back"], right: ["right", "forward"] } as const),
              edge: choice(["top", "bottom"] as const),
            },
            run: ({ captures }: HandlerArguments<{
              readonly direction: "left" | "right"
              readonly edge: "top" | "bottom"
            }>) => { observed = captures },
          },
        },
      },
      input: messages([
        {
          type: "invoke",
          invocationId: "choice-1",
          commandId: "move",
          context: {},
          captures: { direction: "left", edge: "bottom" },
        },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })

    expect(output[0]).toMatchObject({
      protocolVersion: 2,
      commands: [{
        captures: {
          direction: {
            type: "choice",
            choices: { left: ["left", "back"], right: ["right", "forward"] },
          },
          edge: { type: "choice", choices: { top: ["top"], bottom: ["bottom"] } },
        },
      }],
    })
    expect(observed).toEqual({ direction: "left", edge: "bottom" })
  })

  it("rejects noncanonical choice invocation values", async () => {
    const output: HostOutput[] = []
    await runHost({
      config: {
        commands: {
          move: {
            phrases: ["move {direction}"],
            captures: { direction: choice({ left: ["left", "back"] } as const) },
            run: () => undefined,
          },
        },
      },
      input: messages([
        { type: "invoke", invocationId: "bad-choice", commandId: "move", context: {}, captures: { direction: "back" } },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })

    expect(output.at(-1)).toMatchObject({
      invocationId: "bad-choice",
      result: { type: "failure", message: "Command move received invalid captures" },
    })
  })

  it("registers letters and accepts only canonical lowercase invocation values", async () => {
    const output: HostOutput[] = []
    let observed: string | undefined
    await runHost({
      config: {
        commands: {
          control: {
            phrases: ["control {key}"],
            captures: { key: letter() },
            run: ({ captures }: HandlerArguments<{ readonly key: Letter }>) => { observed = captures.key },
          },
        },
      },
      input: messages([
        { type: "invoke", invocationId: "letter", commandId: "control", context: {}, captures: { key: "q" } },
        { type: "invoke", invocationId: "uppercase", commandId: "control", context: {}, captures: { key: "Q" } },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })

    expect(output[0]).toMatchObject({ commands: [{ captures: { key: { type: "letter" } } }] })
    expect(observed).toBe("q")
    expect(output.at(-1)).toMatchObject({
      invocationId: "uppercase",
      result: { type: "failure", message: "Command control received invalid captures" },
    })
  })

  it("registers flattened unions and validates each canonical runtime value", async () => {
    const output: HostOutput[] = []
    const observed: Array<string | number> = []
    const key = union(
      letter(),
      digit(),
      choice({ home: ["home"], escape: ["escape", "cancel"] } as const),
    )
    await runHost({
      config: {
        commands: {
          key: {
            phrases: ["key {key}"],
            captures: { key },
            run: ({ captures }: HandlerArguments<{ readonly key: Letter | number | "home" | "escape" }>) => {
              observed.push(captures.key)
            },
          },
        },
      },
      input: messages([
        { type: "invoke", invocationId: "letter", commandId: "key", context: {}, captures: { key: "q" } },
        { type: "invoke", invocationId: "digit", commandId: "key", context: {}, captures: { key: 2 } },
        { type: "invoke", invocationId: "choice", commandId: "key", context: {}, captures: { key: "escape" } },
        { type: "invoke", invocationId: "alias", commandId: "key", context: {}, captures: { key: "cancel" } },
        { type: "shutdown" },
      ]),
      write: (frame) => { output.push(frame) },
    })

    expect(output[0]).toMatchObject({
      commands: [{ captures: { key: { type: "union", members: [
        { type: "letter" },
        { type: "digit", min: 0, max: 9 },
        { type: "choice", choices: { home: ["home"], escape: ["escape", "cancel"] } },
      ] } } }],
    })
    expect(observed).toEqual(["q", 2, "escape"])
    expect(output.at(-1)).toMatchObject({
      invocationId: "alias",
      result: { type: "failure", message: "Command key received invalid captures" },
    })
  })

  it("validates typed capture schemas and every alias binding", () => {
    expect(prepareConfig({
      commands: {
        nested: {
          phrases: ["key {key}"],
          captures: { key: {
            type: "union",
            members: [
              { type: "letter" },
              { type: "union", members: [
                { type: "digit", min: 0, max: 9 },
                { type: "choice", choices: { home: ["home"] } },
              ] },
            ],
          } },
          run: () => undefined,
        },
      },
    }).registration.commands[0]?.captures).toMatchObject({
      key: { type: "union", members: [{ type: "letter" }, { type: "digit" }, { type: "choice" }] },
    })

    expect(() => prepareConfig({
      commands: {
        bad: {
          phrases: ["move {from} to {to}", "move {from}"],
          captures: { from: digit({ min: 1, max: 3 }), to: digit({ min: 4, max: 6 }) },
          run: () => undefined,
        },
      },
    })).toThrow("bind every declared capture exactly once")
    expect(() => prepareConfig({
      commands: {
        bad: {
          phrases: ["say {words} now"],
          captures: { words: text() },
          run: () => undefined,
        },
      },
    })).toThrow("text() capture must be trailing")
    expect(() => prepareConfig({
      commands: {
        bad: {
          phrases: ["control {number}"],
          captures: { number: digit({ min: 3, max: 1 }) },
          run: () => undefined,
        },
      },
    })).toThrow("digit range")
    expect(() => prepareConfig({
      commands: {
        bad: {
          phrases: ["control {number}"],
          captures: { number: digit({ min: 1, max: 3 }) },
          action: openUrl("https://example.com"),
        },
      },
    })).toThrow("require run")
    const oversizedUnion = {
      type: "union",
      members: [
        { type: "choice", choices: Object.fromEntries(Array.from({ length: 16 }, (_, value) => [
          `left${value}`,
          Array.from({ length: 16 }, (_, alias) => `left-${value}-${alias}-${"x".repeat(40)}`),
        ])) },
        { type: "choice", choices: Object.fromEntries(Array.from({ length: 16 }, (_, value) => [
          `right${value}`,
          Array.from({ length: 16 }, (_, alias) => `right-${value}-${alias}-${"x".repeat(40)}`),
        ])) },
      ],
    }
    for (const descriptor of [
      { type: "choice", choices: {} },
      { type: "choice", choices: { left: [] } },
      { type: "choice", choices: { left: ["two words"] } },
      { type: "choice", choices: { left: ["LEFT"], other: ["left!"] } },
      { type: "choice", choices: { "": ["left"] } },
      { type: "choice", choices: { left: ["left"] }, extra: true },
      { type: "letter", extra: true },
      choice(["left", "left"] as const),
      { type: "choice", choices: { ["x".repeat(129)]: ["left"] } },
      { type: "choice", choices: { left: ["x".repeat(65)] } },
      { type: "choice", choices: { left: Array.from({ length: 17 }, (_, index) => `word${index}`) } },
      {
        type: "choice",
        choices: Object.fromEntries(Array.from({ length: 65 }, (_, index) => [`value${index}`, [`word${index}`]])),
      },
      { type: "union", members: [{ type: "letter" }] },
      { type: "union", members: [{ type: "letter" }, { type: "text" }] },
      { type: "union", members: [{ type: "letter" }, { type: "choice", choices: { a: ["alpha"] } }] },
      { type: "union", members: [{ type: "digit", min: 0, max: 9 }, { type: "choice", choices: { two: ["2"] } }] },
      {
        type: "union",
        members: Array.from({ length: 17 }, (_, index) => ({
          type: "choice",
          choices: { [`key${index}`]: [`key${index}`] },
        })),
      },
      {
        type: "union",
        members: [
          { type: "letter" },
          { type: "union", members: [{ type: "digit", min: 0, max: 0 }, { type: "union", members: [
            { type: "choice", choices: { home: ["home"] } },
            { type: "union", members: [{ type: "choice", choices: { end: ["end"] } }, { type: "union", members: [
              { type: "choice", choices: { up: ["up"] } },
              { type: "choice", choices: { down: ["down"] } },
            ] }] },
          ] }] },
        ],
      },
      oversizedUnion,
    ]) {
      expect(() => prepareConfig({
        commands: {
          bad: {
            phrases: ["move {direction}"],
            captures: { direction: descriptor },
            run: () => undefined,
          },
        },
      }), JSON.stringify(descriptor)).toThrow()
    }
  })

  it("rejects unbounded captures", async () => {
    const output: HostOutput[] = []
    const oversized = "x".repeat(2048)
    await expect(runHost({
      config: {
        commands: {
          search: {
            phrases: ["search amazon for {query}"],
            run: () => undefined,
          },
        },
      },
      input: messages([
        {
          type: "invoke",
          invocationId: "inv-1",
          commandId: "search",
          context: {},
          captures: { query: oversized },
        },
      ]),
      write: (frame) => { output.push(frame) },
    })).rejects.toThrow("invoke.captures.query")
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
      commands: {
        bad: {
          phrases: ["control {number}"],
          captures: { number: digit() },
          run: openUrl("https://example.com"),
        },
      },
    })).toThrow("capture descriptors require a handler run")
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

  it("reports Effect timeouts without losing subsequent invocations", async () => {
    const timedOut = await Effect.runPromise(Deferred.make<void>())
    const completed = await Effect.runPromise(Deferred.make<void>())
    const output: HostOutput[] = []
    async function* input(): AsyncGenerator<string> {
      yield `${JSON.stringify({ type: "invoke", invocationId: "timeout", commandId: "timeout", context: {} })}\n`
      await Effect.runPromise(Deferred.await(timedOut))
      yield `${JSON.stringify({ type: "invoke", invocationId: "next", commandId: "next", context: {} })}\n`
      await Effect.runPromise(Deferred.await(completed))
      yield `${JSON.stringify({ type: "shutdown" })}\n`
    }

    await runHost({
      config: {
        commands: {
          timeout: { phrases: ["timeout"], run: Effect.never.pipe(Effect.timeout(0)) },
          next: { phrases: ["next"], run: Effect.void },
        },
      },
      input: input(),
      write: (frame) => {
        output.push(frame)
        if (frame.type === "invocationResult") {
          Effect.runFork(Deferred.succeed(frame.invocationId === "timeout" ? timedOut : completed, undefined))
        }
      },
    })

    expect(output.filter((frame) => frame.type === "invocationResult")).toEqual([
      { type: "invocationResult", invocationId: "timeout", result: { type: "failure", message: "TimeoutError" } },
      { type: "invocationResult", invocationId: "next", result: { type: "success" } },
    ])
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
