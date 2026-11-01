# Hex Custom Transformations

<!-- Hex writes this file and replaces it when it changes. -->

This folder configures custom text transformations for Hex. Edit
`hex.config.ts` (or `hex.config.js`; keep only one). `hex.d.ts` types the
`@hex/commands` module; Hex owns it, so do not edit it.

Hex runs the config itself in an embedded JavaScript engine (QuickJS) and
reloads it about a third of a second after any `.ts`, `.js`, or `.json` file
here changes. A config that fails to load keeps the previous one active; the
error, with its file, line, and column, shows in the Modes pane's
Transformations section. No install step and no Node, Bun, or npm are involved.

## What a script can do

- Plain modern JavaScript (ES2023 modules). TypeScript works too: Hex erases
  type annotations, interfaces, type aliases, generics, `as`, `satisfies`, and
  `!` before running, keeping lines and columns. Syntax that would need
  generated code is rejected: enums, namespaces, constructor parameter
  properties, and `import x = require()`.
- Import `@hex/commands`, `node:child_process` (`exec`, `execFile`),
  `node:util` (`promisify`), and other files in this folder by relative path
  (`./lib.ts`, `./data.json`). npm packages are not available.
- `fetch` (WHATWG, string bodies, no streaming), `setTimeout`,
  `AbortController`, and `AbortSignal` exist. No filesystem, environment,
  `process`, or `require`. `console.log` writes to Hex's log.
- `fetch` and `node:child_process` work only after the person allows this
  config in Hex's Output pane; any edit asks again. Use `execFile` with an
  argument array (and the `input` option for stdin) for anything involving
  dictated text, never a shell command built from it.
- Each transformation has `timeout` milliseconds: 1000 by default, 30000 when
  the config uses `fetch` or programs, 60000 at most. Past it, Hex aborts its
  requests, ends its programs, and pastes the text unchanged. Scripts share a
  64 MiB memory limit.

The full API, with examples: https://anoma.ly/hex/docs/transforms

To check types in an editor or agent, run `npx -y -p typescript tsc` here.

## Transformations

`transformations` registers named text functions that appear as optional
final steps in Hex's dictation modes. Selected transformations run in order
after the mode's corrections and optional AI rewrite. A transformation receives
the text and the foreground context and returns the new text. If any
transformation fails, Hex keeps the text from before the transformations.

```ts
import { defineHexConfig } from "@hex/commands"

export default defineHexConfig({
  transformations: {
    "trim-whitespace": {
      name: "Trim whitespace",
      description: "Remove leading and trailing whitespace",
      transform: (text) => text.trim(),
    },
  },
})
```

Do not reuse `lowercase`, `spongebob-case`, or `no-trailing-punctuation`; those
IDs are Hex's native built-in transformations.
