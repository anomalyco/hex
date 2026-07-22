import { Effect } from "effect"
import { defineHexConfig, Hex } from "../src/effect.js"

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
  },
})
