# HEX Custom Commands

This workspace configures literal custom voice commands for HEX. Run
`bun run check` after editing `hex.config.ts`. HEX reloads valid changes and
keeps the previous registry if evaluation or validation fails.

- Import `defineHexConfig` from `@hex/commands` and use `run` with the provided
  `hex` capabilities for ordinary commands.
- Import Effect APIs, `Hex`, and `defineHexConfig` from `@hex/commands/effect`.
- Command keys are stable IDs. Phrases must not overlap another command at the
  same context specificity or any protected native phrase.
- `when` accepts exactly one of `application` or `browserHost`.
- Capabilities are `openUrl`, `openApplication`, `openPath`, `press`, and
  `typeText`.
- Structured native action descriptors are available for fixed single actions,
  but handlers are the clearest default and compose multiple capabilities.
- Effect `run` accepts an Effect value or a function that returns an Effect.
- Context contains `application`, `browserHost`, `browserUrl`, and `windowTitle`
  when available.

See `.agents/skills/personal-commands/SKILL.md` for examples.
