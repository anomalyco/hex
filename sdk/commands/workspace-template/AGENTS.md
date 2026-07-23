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
- Capabilities are `openUrl`, `openApplication`, `openPath`, `press`, and
  `typeText`.
- Structured native action descriptors are available for fixed single actions,
  but handlers are the clearest default and compose multiple capabilities.
- Effect `run` accepts an Effect value or a function that returns an Effect.
- Context contains `application`, `browserHost`, `browserUrl`, and `windowTitle`
  when available.
- `transformations` registers named string functions. Selected transformations
  run after a mode's corrections and optional OpenCode rewrite. Return only the
  final transformed string; failures preserve the previous pipeline output.

See `.agents/skills/personal-commands/SKILL.md` for examples.
