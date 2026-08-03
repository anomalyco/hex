---
name: hex-personal-commands
description: Configure HEX personal voice commands, voice-dictation control phrases, and deterministic transcript transformations. Use when the user wants to add or change commands or transformations for the HEX macOS voice app.
license: MIT
---

# HEX Personal Commands

Use HEX's managed command workspace rather than creating an independent package.

1. Check for `~/.config/hex/hex.config.ts`. If it is missing, ask the user to
   open HEX's Commands pane and choose Set Up. If the `hex` CLI is available,
   `hex commands init` performs the same provisioning.
2. Read `~/.config/hex/AGENTS.md` and
   `~/.config/hex/.agents/skills/personal-commands/SKILL.md`. They describe the
   exact API bundled with the installed HEX version and are authoritative over
   examples in this bootstrap skill.
3. Edit only user-owned workspace files, normally `hex.config.ts`. Never edit
   `.hex-sdk`; HEX refreshes that managed SDK from the app bundle.
4. Use `defineHexConfig` from `@hex/commands` for commands, dictation controls,
   and transformations. Use `@hex/commands/effect` only when the requested
   handler benefits from Effect composition.
5. Keep command phrases unambiguous and use only the capabilities required by
   the request. Treat config and dependencies as trusted executable code with
   the user's filesystem, network, environment, and subprocess authority.
    Use `digit()` for zero through nine, `digit({ min, max })` for a restricted
    range, `choice(["left", "right"] as const)` for exact choices, an object-form
     `choice()` to normalize one-word aliases to canonical keys,
     `union(letter(), digit(), choice([...]))` for disjoint bounded one-token
     alternatives, and trailing `text()` for explicit text captures. `letter()` accepts literal, common
    spoken, and NATO letter names and returns lowercase `"a" | ... | "z"`;
    every phrase alias must bind the
    declared names once.
6. Run `bun run check` in `~/.config/hex`. Do not finish until it passes. HEX
   watches valid changes automatically and reports runtime reload failures in
   the Commands pane.

Completion criterion: the requested behavior is represented in
`hex.config.ts`, the managed SDK was not modified, and `bun run check` passes.
