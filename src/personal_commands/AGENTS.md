# Hex Custom Commands

<!-- Hex writes this file and replaces it when it changes. -->

This folder configures custom voice commands, typed captures, dictation control
phrases, and text transformations for Hex. Edit `hex.config.ts` (or
`hex.config.js`; keep only one). `hex.d.ts` types the `@hex/commands` module;
Hex owns it, so do not edit it.

Hex runs the config itself in an embedded JavaScript engine (QuickJS) and
reloads it about a third of a second after any `.ts`, `.js`, or `.json` file
here changes. A config that fails to load keeps the previous one active; the
error, with its file, line, and column, shows in the Modes pane's
Transformations section. On macOS, `hex commands check` loads the config as Hex
does and prints what it registers or why it fails, phrase overlaps included.
No install step and no Node, Bun, or npm are involved.

## What a script can do

- Plain modern JavaScript (ES2023 modules). TypeScript works too: Hex erases
  type annotations, interfaces, type aliases, generics, `as`, `satisfies`, and
  `!` before running, keeping lines and columns. Syntax that would need
  generated code is rejected: enums, namespaces, constructor parameter
  properties, and `import x = require()`.
- Import only `@hex/commands` and other files in this folder by relative path
  (`./lib.ts`, `./data.json`). npm packages are not available.
- No filesystem, network, process, environment, or timer APIs (`fetch`,
  `setTimeout`, `require`, `process` do not exist). `console.log` writes to
  Hex's log.
- Each handler or transformation call must finish within one second; a script
  that runs longer is stopped and fails that call. Scripts share a 64 MiB
  memory limit.

To check types in an editor or agent, run `npx -y -p typescript tsc` here.
That checks TypeScript, not phrase overlaps; Hex reports those on load.

## Commands

- Import `defineHexConfig` from `@hex/commands`. Command keys are stable IDs.
- A command has `phrases` and exactly one of `action` (a fixed action) or `run`
  (a handler).
- A handler receives `{ hex, context, captures }`. It returns an action, a list
  of actions, or nothing, and may also queue actions with `hex.openUrl(...)`,
  `hex.press(...)`, and the rest. Hex runs the queued actions, then the
  returned ones, in order after the handler returns; the first failing action
  fails the command.
- Actions: `openUrl(url)` (absolute web URLs and app deep links such as
  `slack://channel?team=T_EXAMPLE&id=C_EXAMPLE`; not `file:`, `javascript:`,
  or `data:`), `openApplication(nameOrPath)`, `openPath(path)`,
  `press("k")` or `press({ key, modifiers, repeat })` (a character, or `home`,
  `end`, `up`, `down`, `left`, `right`, `enter`, `escape`; modifiers
  `command`, `control`, `option`, `shift`; repeat 1 through 100), and
  `typeText(text)`.
- `context` has `application`, `browserHost`, `browserUrl`, and `windowTitle`
  when available.
- `when` accepts exactly one of `application` or `browserHost`. Browser-host
  commands take precedence over application commands, then global commands.
- Phrases must not overlap a command whose context can coexist at the same
  specificity, or any protected native phrase. Hex rejects the whole config
  when they do.
- Spoken commands and dictation controls require Hex's Commands opt-in, which
  defaults off. A config does not enable it.

```ts
import { defineHexConfig, openApplication, press } from "@hex/commands"

export default defineHexConfig({
  commands: {
    slack: { phrases: ["open slack"], action: openApplication("Slack") },
    "slack-search": {
      phrases: ["search slack"],
      run: () => [openApplication("Slack"), press({ key: "k", modifiers: ["command"] })],
    },
    "type-app": {
      phrases: ["type app name"],
      run: ({ hex, context }) => hex.typeText(context.application ?? "unknown"),
    },
  },
})
```

## Captures

End a phrase with one `{name}` placeholder to capture the rest of the spoken
words as `captures.name`: ASCII letters lowercased, punctuation trimmed from
word boundaries, whitespace collapsed, and spoken digits zero through nine as
`0` through `9`. Captures need at least one spoken word before the
placeholder and one or more words after it (at most 24 words, 512 bytes).
Capture phrases require `run`.

```ts
"search-amazon": {
  phrases: ["search amazon for {query}"],
  run: ({ captures }) =>
    openUrl(`https://www.amazon.com/s?k=${encodeURIComponent(captures.query ?? "")}`),
},
```

For typed captures, declare a `captures` schema; every phrase must bind each
name exactly once:

- `digit()` (or `digit({ min, max })` within 0 through 9) arrives as a number
  and may appear anywhere.
- `letter()` accepts letters, spoken names ("bee"), and NATO names ("bravo"),
  and arrives as the lowercase letter.
- `choice(["left", "right"] as const)` returns the spoken value;
  `choice({ left: ["left", "back"], right: ["right", "forward"] } as const)`
  returns the key for any of its one-word aliases.
- `union(letter(), digit(), choice(["home", "end"] as const))` composes bounded
  captures whose spoken words do not overlap.
- `text()` captures the trailing words as a string and must be last.

```ts
"control-key": {
  phrases: ["control {key}"],
  captures: { key: union(letter(), digit(), choice(["home", "end"] as const)) },
  run: ({ captures }) => press({ key: String(captures.key), modifiers: ["control"] }),
},
```

## Dictation controls

`dictation` replaces Hex's native voice-dictation phrases (not the capture
lifecycle). `start` phrases match at the beginning of an utterance while
listening; `stop`, `send`, and `cancel` match at the end during voice
dictation. Every list needs at least one phrase, and they must not collide.

```ts
dictation: {
  start: ["begin note"],
  stop: ["finish note"],
  send: ["send note"],
  cancel: ["discard note"],
},
```

## Transformations

`transformations` registers named text functions that appear as optional
final steps in Hex's dictation modes. Selected transformations run in order
after the mode's corrections and optional AI rewrite, for Paste and Send, not
meetings. A transformation receives the text and the
foreground context and returns the new text. If any transformation fails,
Hex keeps the text from before the transformations.

```ts
transformations: {
  "trim-whitespace": {
    name: "Trim whitespace",
    description: "Remove leading and trailing whitespace",
    transform: (text) => text.trim(),
  },
},
```

Do not reuse `lowercase`, `spongebob-case`, or `no-trailing-punctuation`; those
IDs are Hex's native built-in transformations.
