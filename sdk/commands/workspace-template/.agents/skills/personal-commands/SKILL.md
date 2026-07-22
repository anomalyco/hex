# Personal Commands

Use structured actions when a command needs one fixed native operation:

```ts
import { defineHexConfig, openApplication } from "@hex/commands"

export default defineHexConfig({
  commands: {
    slack: { phrases: ["open slack"], action: openApplication("Slack") },
  },
})
```

Vanilla handlers may issue several calls. HEX waits for all calls started by the
handler, including calls that were not explicitly awaited:

```ts
run: ({ hex }) => {
  hex.openApplication("Slack")
  hex.press({ key: "k", modifiers: ["command"] })
}
```

Effect handlers use the Effect entrypoint and may be values or functions:

```ts
import { Effect } from "effect"
import { defineHexConfig, Hex } from "@hex/commands/effect"

export default defineHexConfig({
  commands: {
    training: {
      phrases: ["open training"],
      run: Effect.gen(function* () {
        const hex = yield* Hex
        yield* hex.openUrl("https://example.com/training")
      }),
    },
    contextual: {
      phrases: ["show context"],
      run: ({ context }) => Effect.gen(function* () {
        const hex = yield* Hex
        yield* hex.typeText(context.browserHost ?? context.application ?? "unknown")
      }),
    },
  },
})
```
