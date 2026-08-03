import { describe, expect, it } from "vitest"
import {
  choice,
  defineHexConfig,
  digit,
  letter,
  openApplication,
  openPath,
  openUrl,
  press,
  typeText,
  text,
  union,
} from "../src/index.js"

describe("command model", () => {
  it("builds every native descriptor", () => {
    expect(openUrl("https://example.com")).toEqual({ type: "openUrl", url: "https://example.com" })
    expect(openApplication("Slack")).toEqual({ type: "openApplication", application: "Slack" })
    expect(openPath("/tmp/example")).toEqual({ type: "openPath", path: "/tmp/example" })
    expect(press("escape")).toEqual({ type: "press", key: "escape" })
    expect(press({ key: "p", modifiers: ["command", "shift"], repeat: 2 })).toEqual({
      type: "press",
      key: "p",
      modifiers: ["command", "shift"],
      repeat: 2,
    })
    expect(typeText("fixed text")).toEqual({ type: "typeText", text: "fixed text" })
  })

  it("preserves normal TypeScript record composition", () => {
    const navigation = {
      home: { phrases: ["open home"] as const, action: openUrl("https://example.com") },
    }
    const config = defineHexConfig({
      transformations: {
        lowercase: {
          name: "Lowercase",
          transform: (text) => text.toLowerCase(),
        },
      },
      dictation: {
        start: ["begin note"],
        stop: ["finish note"],
        send: ["send note"],
        cancel: ["discard note"],
      },
      commands: {
        ...navigation,
        slack: { phrases: ["open slack"], run: openApplication("Slack") },
      },
    })
    expect(Object.keys(config.commands)).toEqual(["home", "slack"])
    expect(config.dictation.send).toEqual(["send note"])
    expect(config.transformations.lowercase.transform("HELLO")).toBe("hello")
  })

  it("contextually types Promise handlers through the shared config model", () => {
    const config = defineHexConfig({
      commands: {
        contextual: {
          phrases: ["show context"],
          run: ({ hex, context }) => hex.typeText(context.browserHost ?? "none"),
        },
      },
    })

    expect(config.commands.contextual!.phrases).toEqual(["show context"])
  })

  it("infers exact typed capture values from descriptors", () => {
    const config = defineHexConfig({
      commands: {
        range: {
          phrases: ["move {column} to {row} then {note}"],
          captures: {
            column: digit({ min: 1, max: 3 }),
            row: digit({ min: 4, max: 9 }),
            note: text(),
          },
          run: ({ captures }) => {
            expectTypeOf(captures).toEqualTypeOf<Readonly<{
              column: number
              row: number
              note: string
            }>>()
          },
        },
      },
    })

    expect(config.commands.range!.captures!.column).toEqual({ type: "digit", min: 1, max: 3 })
    expect(config.commands.range!.captures!.note).toEqual({ type: "text" })
  })

  it("defaults digit captures to zero through nine", () => {
    expect(digit()).toEqual({ type: "digit", min: 0, max: 9 })
  })

  it("infers the exact lowercase letter union", () => {
    const config = defineHexConfig({
      commands: {
        control: {
          phrases: ["control {key}"],
          captures: { key: letter() },
          run: ({ captures }) => {
            expectTypeOf(captures.key).toEqualTypeOf<
              "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m"
              | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z"
            >()
          },
        },
      },
    })

    expect(config.commands.control!.captures!.key).toEqual({ type: "letter" })
  })

  it("infers choice literals from arrays and canonical object keys", () => {
    const config = defineHexConfig({
      commands: {
        direction: {
          phrases: ["move {direction}"],
          captures: { direction: choice(["left", "right"] as const) },
          run: ({ captures }) => {
            expectTypeOf(captures.direction).toEqualTypeOf<"left" | "right">()
          },
        },
        navigation: {
          phrases: ["go {direction}"],
          captures: {
            direction: choice({
              left: ["left", "back"],
              right: ["right", "forward"],
            } as const),
          },
          run: ({ captures }) => {
            expectTypeOf(captures.direction).toEqualTypeOf<"left" | "right">()
          },
        },
      },
    })

    expect(config.commands.direction!.captures!.direction).toEqual({
      type: "choice",
      choices: { left: ["left"], right: ["right"] },
    })
    expect(config.commands.navigation!.captures!.direction.choices.left).toEqual(["left", "back"])
  })

  it("builds flattened unions and infers every member value", () => {
    const nested = union(digit(), choice(["home", "end"] as const))
    const key = union(letter(), nested, choice(["enter", "escape"] as const))
    const config = defineHexConfig({
      commands: {
        key: {
          phrases: ["key {key}"],
          captures: { key },
          run: ({ captures }) => {
            expectTypeOf(captures.key).toEqualTypeOf<
              | "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m"
              | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z"
              | number | "home" | "end" | "enter" | "escape"
            >()
          },
        },
      },
    })

    expect(key.members).toHaveLength(4)
    expect(key.members.map((member) => member.type)).toEqual(["letter", "digit", "choice", "choice"])
    expect(config.commands.key!.captures!.key).toBe(key)
  })

  it("rejects invalid and internally overlapping unions", () => {
    expect(() => union(letter(), choice(["alpha"]))).toThrow("overlap")
    expect(() => union(digit(), choice(["two"]))).toThrow("overlap")
    expect(() => union(letter(), text() as never)).toThrow("does not accept text")
    expect(() => union(...Array.from({ length: 17 }, (_, index) => choice([`key${index}`])) as [ReturnType<typeof choice>, ReturnType<typeof choice>, ...ReturnType<typeof choice>[]]))
      .toThrow("at most 16")
    if (false) {
      // @ts-expect-error A union requires at least two bounded members.
      union(letter())
      // @ts-expect-error text() is not a bounded one-token capture.
      union(letter(), text())
    }
  })

  it("requires handlers for schema-bearing commands", () => {
    defineHexConfig({
      commands: {
        invalid: {
          phrases: ["control {number}"],
          captures: { number: digit({ min: 1, max: 3 }) },
          // @ts-expect-error Typed captures require a run handler.
          action: press("1"),
        },
        invalidRun: {
          phrases: ["control {number}"],
          captures: { number: digit({ min: 1, max: 3 }) },
          // @ts-expect-error Typed captures require a handler rather than a native run value.
          run: press("1"),
        },
      },
    })
  })
})
