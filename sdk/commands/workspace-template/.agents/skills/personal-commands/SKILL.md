# Custom Commands

Edit `hex.config.ts`, not `.hex-sdk`. Config and installed dependencies execute
with the user's normal permissions; use only trusted packages and avoid network,
filesystem, environment, or subprocess access unless the requested command
requires it. Run `bun run check` after editing.

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

Register named finishing transformations when a dictation mode needs a
deterministic text rule:

```ts
export default defineHexConfig({
  transformations: {
    lowercase: {
      name: "Lowercase",
      description: "Convert the final text to lowercase",
      transform: (text) => text.toLowerCase(),
    },
  },
  commands: {},
})
```

The transformation appears in each mode's Custom transformations section. It
runs after that mode's corrections and optional AI rewrite. Transformations may
be async, receive the foreground context, and must return a string.

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

End a phrase with one `{name}` placeholder to capture the rest of the spoken
words. The normalized remainder (lowercase, punctuation stripped) arrives as
`captures.name`. Capture phrases require `run` and at least one spoken word
before the placeholder:

```ts
export default defineHexConfig({
  commands: {
    "search-amazon": {
      phrases: ["search amazon for {query}"],
      run: ({ hex, captures }) =>
        hex.openUrl(`https://www.amazon.com/s?k=${encodeURIComponent(captures.query ?? "")}`),
    },
  },
})
```

Saying "search amazon for wool socks" opens the Amazon search for
"wool socks". A capture phrase conflicts with any coexisting phrase that
shares its literal prefix, and the capture is bounded (24 words / 512 bytes).

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
