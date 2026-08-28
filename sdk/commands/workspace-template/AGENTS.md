# HEX Custom Commands

This workspace configures literal custom voice commands for HEX. Run
`bun run check` after editing `hex.config.ts`. HEX reloads valid changes and
keeps the previous registry if evaluation or validation fails.

Config and dependencies execute as the current user with normal filesystem,
network, environment, and subprocess access. This host is supervised, not
sandboxed. Add only trusted packages and review commands before enabling them.

`.hex-sdk` is managed by HEX and provides the local `@hex/commands` package. Do
not edit it or replace it with an npm dependency. HEX refreshes the managed SDK
from the running app bundle on startup while preserving user-owned workspace
files and third-party dependencies.

- Import `defineHexConfig` from `@hex/commands` and use `run` with the provided
  `hex` capabilities for ordinary commands.
- Import Effect APIs, `Hex`, and `defineHexConfig` from `@hex/commands/effect`.
- Command keys are stable IDs. Phrases must not overlap a command whose context
  can coexist at the same specificity, or any protected native phrase.
- `dictation` declaratively replaces the native streaming protocol. `start`
  phrases match only at the beginning while listening; `stop`, `send`, and
  `cancel` match only at the end while voice dictation is active. These are not
  handlers and are unavailable as ordinary commands.
- Keep at least one phrase in every dictation control. Invalid or overlapping
  protocol changes are rejected together with command changes, preserving the
  last valid native snapshot.
- `when` accepts exactly one of `application` or `browserHost`.
- Without a `captures` schema, a phrase may end in one `{name}` placeholder, e.g.
  `"search amazon for {query}"`. It requires at least one spoken word before
  the placeholder and one or more words after the prefix; the normalized
  remainder arrives as `captures.name` in the handler (and on the Effect `Hex`
  service). Capture phrases require `run`; they cannot use a native `action`.
  A capture phrase conflicts with any coexisting phrase that shares its
  literal prefix.
- Import `digit`, `letter`, `choice`, `union`, or `text` to define typed captures. `digit()`
  accepts zero through nine; `digit({ min, max })` restricts that range. Digit
  captures may appear multiple times anywhere in every alias and arrive as
  numbers. `choice(["left", "right"] as const)` returns those exact strings;
  `choice({ left: ["left", "back"], right: ["right", "forward"] } as const)`
  normalizes spoken aliases to its canonical keys. Choice aliases are one word.
  `letter()` accepts literal, common spoken, and NATO alphabet names and returns
  the canonical lowercase `"a" | ... | "z"` letter.
  `union(letter(), digit(), choice(["home", "end"] as const))` composes bounded
  one-token captures and infers the union of their canonical values. Union
  members must not overlap; `text()` is not allowed in a union.
  `text()` arrives as a string and must be trailing. Every alias must
  bind every declared capture exactly once. Schema-bearing commands require
  `run`.
- Capabilities are `openUrl`, `openApplication`, `openPath`, `press`, and
  `typeText`.
- `openUrl` opens absolute web URLs and app deep links such as
  `slack://channel?team=T_EXAMPLE&id=C_EXAMPLE` using an installed macOS handler.
  `openApplication` takes an app name or path, not a deep link. Use `openPath`
  for files; file, inline-script, and data URLs are unsupported.
- Structured native action descriptors are available for fixed single actions,
  but handlers are the clearest default and compose multiple capabilities.
- Effect `run` accepts an Effect value or a function that returns an Effect.
- Context contains `application`, `browserHost`, `browserUrl`, and `windowTitle`
  when available.
- `transformations` registers named string functions. Selected transformations
  run after a mode's corrections and optional OpenCode rewrite. Return only the
  final transformed string; failures preserve the previous pipeline output.

See `.agents/skills/personal-commands/SKILL.md` for examples.
