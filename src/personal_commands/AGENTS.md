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
- Import only `@hex/commands` and other files in this folder by relative path
  (`./lib.ts`, `./data.json`). npm packages are not available.
- No filesystem, network, process, environment, or timer APIs (`fetch`,
  `setTimeout`, `require`, `process` do not exist). `console.log` writes to
  Hex's log.
- Each transformation call must finish within one second; a script that runs
  longer is stopped and fails that call. Scripts share a 64 MiB memory limit.

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
