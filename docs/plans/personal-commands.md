# Personal Commands

**Status:** Personal commands, typed capture descriptors, dictation control
phrases, and text transformations are implemented on macOS. This document
records the shipped contract and the remaining workspace lifecycle work.

## What Users Can Configure

Personal commands live in `~/.config/hex/hex.config.ts`. TypeScript defines
ordinary commands, capture descriptors, named text transformations, and optional
dictation control phrases. Rust validates and matches the grammar and retains
wake, sleep, cancellation, and dictation lifecycle behavior. Meeting lifecycle
commands are native and developer-only.

Spoken commands and dictation control phrases require the persisted Commands
opt-in, which defaults off. Creating a config does not enable it. Selected mode
transformations can run with Commands off; hotkey dictation does not require
voice commands.

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

Commands may use a trailing `{name}` placeholder for normalized text, or a
`captures` schema built with `digit`, `letter`, `choice`, `union`, and trailing
`text`. Schema-bearing commands bind every declared capture exactly once in
every phrase alias and require a `run` handler. Rust matches these commands
only on completed command transcripts, not partial updates.

`openUrl` accepts absolute web URLs and app deep links, for example
`hex.openUrl("slack://channel?team=T_EXAMPLE&id=C_EXAMPLE")`. The URL is passed
unchanged to macOS, which requires an installed handler for its scheme. Use
`openApplication("Slack")` to launch an app by name or path, not to navigate a
deep link. The API spelling is `openUrl`, not `openURL`. File URLs and inline
script/data URLs remain unsupported; use `openPath` for filesystem paths.

## Dictation Transformations

The config may register named string transformations. Select them in a mode's
Transformations section to run them in displayed order after that mode's
corrections and optional OpenCode rewrite. This pipeline applies to Paste and
Send, not Voice Action or meetings. If the transformation stage fails, HEX
keeps the text from before that stage, not partially transformed output.

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
  commands: {},
})
```

Do not reuse the built-in transformation IDs `lowercase` or `spongebob-case`;
those IDs execute Rust's built-ins rather than a registered TypeScript function.

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

The optional `dictation` section replaces the native voice-dictation phrases,
not the native capture lifecycle:

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

`start` matches an utterance prefix while HEX listens. `stop`, `send`, and
`cancel` match suffixes during active voice dictation. Each list must contain at
least one phrase. Omitting the section uses the native protocol.

## Workspace

Install Bun separately, then choose Create Config in the Commands pane or run:

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

`.hex-sdk` is a bundled local package managed by HEX, not a published npm
dependency, and must not be edited. When the personal-command host starts, HEX
stages and replaces it from the running app bundle and runs the targeted
`bun update @hex/commands` when its contents or required workspace dependencies
change, or the installed host is missing. A plain install can retain stale
Effect peer metadata for the local SDK in `bun.lock`. User config, unrelated
package metadata, scripts, and third-party dependencies are preserved. Existing
scaffolded instructions and `tsconfig.json` are not overwritten by init or SDK
refresh.

There is one narrow metadata exception: on init and host startup, HEX adds a
missing Effect dependency or upgrades the exact legacy pins `4.0.0-beta.97` and
`4.0.0-beta.107` to the bundled SDK's exact
`peerDependencies.effect` version (currently `4.0.0-rc.112`). Effect is required
by the host even for Promise-only configs. The current exact pin is left
untouched. Effect entries in dependency, dev/optional/peer dependency, override, and
resolution maps are checked together so duplicate pins cannot conflict.
Custom Effect specifications are never silently replaced. Effect-targeting
indirect selectors (such as `**/effect` or `@hex/commands>effect`) and nested
Effect overrides/resolutions must be removed rather than automatically rewritten;
use the SDK-required exact version in `dependencies.effect`. Unrelated scoped
packages such as `@effect/platform` and `@other/effect` are preserved. Bun 1.3.14
honors `resolutions["**/effect"]`; some other selector forms are currently ignored
or warned about, but HEX rejects them rather than relying on that behavior.

Manifest validation failures leave existing files unchanged and report how to
repair the manifest. A symlinked `package.json` is retained when already current;
links needing migration and dangling links are never replaced automatically.
Update the linked manifest manually or use a regular manifest before retrying. Rewrites
preserve regular-file permissions. SDK refresh and manifest replacement are
separate operations, not a multi-file transaction: if writing the manifest
fails, the SDK may already be refreshed while the old manifest remains. Fix
the reported filesystem problem and rerun `hex commands init`; the unchanged
legacy pin still triggers migration and installation on retry.

Run `bun run check` in `~/.config/hex` after changing the config. This checks
TypeScript, not Rust's phrase-overlap validation. While the personal-command
host is running, HEX watches regular `.ts` and `.json` files in the workspace,
activates valid changes, and reports the last reload error in the Commands pane.
It does not watch `node_modules`, `.git`, or symlink targets. If host startup
stopped because the config, Bun, or dependencies were unavailable, restart HEX
after repairing them.

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
- Add CLI checks, listing, reload, and log inspection when the Commands pane is
  insufficient.
- Add user-visible handler admission or timeout policy only if runtime evidence
  requires it.
