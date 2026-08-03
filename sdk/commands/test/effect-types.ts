import { Effect } from "effect"
import { choice, defineHexConfig, digit, Hex, letter, press, text, union } from "../src/effect.js"

defineHexConfig({
  commands: {
    value: {
      phrases: ["effect value"],
      run: Effect.gen(function* () {
        const hex = yield* Hex
        yield* hex.typeText("value")
      }),
    },
    factory: {
      phrases: ["effect factory"],
      run: ({ context }) => Effect.gen(function* () {
        const hex = yield* Hex
        yield* hex.typeText(context.browserHost ?? "factory")
      }),
    },
    captures: {
      phrases: ["move {column} named {label}"],
      captures: {
        column: digit({ min: 1, max: 3 }),
        label: text(),
      },
      run: ({ captures }) => Effect.gen(function* () {
        const column: number = captures.column
        const label: string = captures.label
        const hex = yield* Hex
        yield* hex.typeText(`${column}:${label}`)
      }),
    },
    choice: {
      phrases: ["move {direction}"],
      captures: { direction: choice(["left", "right"] as const) },
      run: ({ captures }) => Effect.gen(function* () {
        const direction: "left" | "right" = captures.direction
        const hex = yield* Hex
        yield* hex.typeText(direction)
      }),
    },
    letter: {
      phrases: ["control {key}"],
      captures: { key: letter() },
      run: ({ captures }) => Effect.gen(function* () {
        const key: "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m"
          | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z" = captures.key
        const hex = yield* Hex
        yield* hex.press({ key, modifiers: ["control"] })
      }),
    },
    union: {
      phrases: ["key {key}"],
      captures: { key: union(letter(), digit(), choice(["home", "escape"] as const)) },
      run: ({ captures }) => Effect.gen(function* () {
        const key: string | number = captures.key
        const hex = yield* Hex
        yield* hex.press(String(key))
      }),
    },
  },
})

defineHexConfig({
  commands: {
    invalidPromise: {
      phrases: ["invalid promise"],
      // @ts-expect-error Effect configs do not silently accept Promise handlers.
      run: async () => undefined,
    },
    invalidValue: {
      phrases: ["invalid value"],
      // @ts-expect-error Effect handlers must return an Effect.
      run: () => undefined,
    },
    invalidCapturedNativeAction: {
      phrases: ["control {number}"],
      captures: { number: digit() },
      // @ts-expect-error Typed captures require an Effect handler rather than a native run value.
      run: press("1"),
    },
  },
})
