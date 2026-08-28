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

To navigate inside an app, use `openUrl` (lowercase `rl`) with its deep link.
`openApplication` takes an app name or path, not a URL. macOS must have an
installed handler for the URL scheme:

```ts
run: ({ hex }) => hex.openUrl("slack://channel?team=T_EXAMPLE&id=C_EXAMPLE")
```

URLs are passed unchanged, including their query and fragment. File URLs and
inline script/data URLs are unsupported; use `openPath` for filesystem paths.

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

Use an explicit schema for typed, composable captures. Every alias must bind
the same names exactly once. `digit()` captures one spoken digit as a number
and may appear anywhere; `text()` captures normalized trailing words as a
string and must be last:

```ts
import { defineHexConfig, digit } from "@hex/commands"

export default defineHexConfig({
  commands: {
    "control-number": {
      phrases: ["control {number}"],
      captures: { number: digit({ min: 1, max: 3 }) },
      run: ({ hex, captures }) =>
        hex.press({ key: String(captures.number), modifiers: ["control"] }),
    },
  },
})
```

Import `text` and declare `query: text()` when explicit trailing text should
compose with digit captures. Schema-bearing commands always require `run`.

Use `choice()` for bounded one-word alternatives. An array returns the spoken
value itself; object keys are canonical values and each array contains spoken
aliases:

```ts
import { choice, defineHexConfig } from "@hex/commands"

export default defineHexConfig({
  commands: {
    move: {
      phrases: ["move {direction}"],
      captures: {
        direction: choice({
          left: ["left", "back"],
          right: ["right", "forward"],
        } as const),
      },
      run: ({ hex, captures }) => hex.press(captures.direction),
    },
  },
})
```

The handler sees `"left" | "right"`; saying "back" produces `"left"`.

Use `union()` to compose disjoint bounded one-token captures. Members may be
`letter()`, `digit()`, `choice()`, or nested unions; nested unions are flattened.
`text()` is rejected, as are members whose spoken alternatives overlap:

```ts
import { choice, digit, letter, union } from "@hex/commands"

const key = union(letter(), digit(), choice(["home", "end", "escape"] as const))
// captures.key is Letter | number | "home" | "end" | "escape"
```

Use `letter()` for a single keyboard letter. Literal letters, common spoken
names such as "bee", and one-word NATO names such as "bravo" all arrive as the
canonical lowercase `"a" | ... | "z"` union:

```ts
import { defineHexConfig, letter } from "@hex/commands"

export default defineHexConfig({
  commands: {
    "control-letter": {
      phrases: ["control {key}"],
      captures: { key: letter() },
      run: ({ hex, captures }) =>
        hex.press({ key: captures.key, modifiers: ["control"] }),
    },
  },
})
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
