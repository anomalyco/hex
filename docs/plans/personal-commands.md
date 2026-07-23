# Personal Commands

**Status:** The literal-command MVP is implemented on macOS. This document
records the shipped contract and the remaining workspace lifecycle work.

## What Users Can Configure

Personal commands live in `~/.config/hex/hex.config.ts`. TypeScript owns ordinary
literal commands, named text transformations, and optional dictation control phrases. Rust retains protected
wake, sleep, cancellation, dictation lifecycle, meeting lifecycle, and typed
capture behavior.

```ts
import { defineHexConfig } from "@hex/commands"

export default defineHexConfig({
  commands: {
    "open-training": {
      phrases: ["open training"],
      group: "Websites",
      description: "Open the training site",
      run: ({ hex }) => hex.openUrl("https://example.com/training"),
    },
  },
})
```

Command keys are stable IDs. A command may be global or match one exact
application or browser host. Browser-host context currently comes from Brave
Browser. More-specific commands may specialize ordinary global commands;
commands that can coexist at equal specificity may not share a phrase.

Available capabilities are `openUrl`, `openApplication`, `openPath`, `press`,
and `typeText`. Handlers may compose several capabilities and may use ordinary
TypeScript, local modules, Effect, and installed npm packages.

## Dictation Transformations

The config may register named string transformations. They appear as optional
final steps in every dictation mode and run in their displayed order after the
mode's corrections and optional OpenCode rewrite.

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

Each dictation selects one complete mode. The required Global mode applies
unless a more specific application or browser-host mode matches. A mode owns
its corrections, optional AI rewrite, and selected custom transformations;
contextual modes do not inherit hidden processing from Global.

## Trust Boundary

Config handlers and dependencies execute under Bun with the user's normal
filesystem, network, environment, and subprocess authority. The host is
supervised for lifecycle and bounded IPC; it is not a security sandbox. Review
agent changes and install only trusted dependencies.

Invalid config never replaces the last valid registry. Rust validates schema,
stable IDs, protected phrases, context predicates, and overlaps before
activation. Dictation remains available when Bun is absent or the config fails.

## Dictation Control Phrases

The optional `dictation` section replaces the native voice-dictation protocol:

```ts
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

`start` matches an utterance prefix while HEX listens. `stop`, `send`, and
`cancel` match suffixes during active voice dictation. Each list must contain at
least one phrase. Omitting the section uses the native protocol.

## Workspace

Create the workspace from the Commands pane or run:

```sh
hex commands init
```

Provisioning installs:

```text
~/.config/hex/
├── hex.config.ts
├── package.json
├── tsconfig.json
├── AGENTS.md
├── .agents/skills/personal-commands/SKILL.md
└── .hex-sdk/
```

`.hex-sdk` is bundled and managed by HEX; it is not an npm package and must not
be edited. On startup, HEX atomically refreshes it from the running app bundle
and reinstalls only the managed `@hex/commands` dependency when its contents
change. User config, package metadata, and third-party dependencies are
preserved. Run `bun run check` after changing the config. HEX watches the
workspace, activates valid changes, and reports the last reload error in the
Commands pane.

## Runtime Boundary

```text
hex.config.ts -> supervised Bun host -> validated serializable registry
                                      -> native Rust resolver and executor
```

Bun and IPC never run in the partial-transcript recognition path. Transport,
pending invocations, tool calls, and status files are bounded. A failed host
preserves native commands and the last valid runtime snapshot where possible.

## Remaining Work

- Keep the managed SDK, skill, TypeScript, and Effect versions current across
  app updates.
- Recover automatically when Bun or workspace dependencies become available
  without requiring an app restart.
- Add typed capture descriptors only after real commands establish their
  boundary and finalization needs.
- Add CLI checks, listing, reload, and log inspection when the Commands pane is
  insufficient.
- Add user-visible handler admission or timeout policy only if runtime evidence
  requires it.
