# Custom Commands

Replace the native voice-dictation protocol declaratively when desired:

```ts
import { defineHexConfig } from "@hex/commands"

export default defineHexConfig({
  dictation: {
    start: ["begin note"],
    stop: ["finish note"],
    send: ["send note"],
    cancel: ["discard note"],
  },
  commands: {},
})
```

`start` is a streaming utterance prefix while HEX is listening. The other
controls are streaming suffixes only during an active voice capture. This block
replaces the native phrases exactly and does not define command handlers. If it
is omitted, HEX uses its built-in protocol.

Use a handler with the provided `hex` capabilities for ordinary commands:

```ts
import { defineHexConfig } from "@hex/commands"

export default defineHexConfig({
  commands: {
    slack: {
      phrases: ["open slack"],
      run: ({ hex }) => hex.openApplication("Slack"),
    },
  },
})
```

Handlers may issue several calls. HEX waits for all calls started by the handler,
including calls that were not explicitly awaited:

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
