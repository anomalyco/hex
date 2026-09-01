---
name: hex-personal-commands
description: Configure HEX personal voice commands, voice-dictation control phrases, and deterministic transcript transformations. Use when the user wants to add or change commands or transformations for the HEX macOS voice app.
license: MIT
---

# HEX Personal Commands

Use HEX's managed command workspace rather than creating an independent package.

1. Check for `~/.config/hex/hex.config.ts`. If it is missing, ask the user to
   open HEX's Commands pane and choose Create Config. If the `hex` CLI is
   available, `hex commands init` performs the same provisioning. Bun must be
   installed separately.
2. Read `~/.config/hex/AGENTS.md` and
   `~/.config/hex/.agents/skills/personal-commands/SKILL.md`. Check
   `.hex-sdk/package.json` and its exported declarations for the installed API
   and exact Effect requirement; scaffolded instructions are not overwritten
   on upgrades and may be older than the SDK.
3. Edit only user-owned workspace files, normally `hex.config.ts`. Never edit
   `.hex-sdk`; HEX refreshes that managed SDK from the app bundle.
4. Use `defineHexConfig` from `@hex/commands` for Promise handlers, dictation
   controls, and transformations. For Effect handlers, import `defineHexConfig`
   and `Hex` from `@hex/commands/effect`, and Effect APIs from `effect`.
5. Keep command phrases unambiguous and use only the capabilities required by
   the request. Treat config and dependencies as trusted executable code with
   the user's filesystem, network, environment, and subprocess authority.
   Use `digit()` for zero through nine, `digit({ min, max })` for a restricted
   range, `choice(["left", "right"] as const)` for exact choices, an object-form
   `choice()` to normalize one-word aliases to canonical keys,
   `union(letter(), digit(), choice(["home", "end"] as const))` for disjoint
   bounded one-token alternatives, and trailing `text()` for explicit text
   captures. `letter()` accepts literal, common spoken, and one-word NATO letter
   names and returns lowercase `"a" | ... | "z"`. With a `captures` schema,
   every phrase alias must bind every declared name exactly once.
6. Spoken commands and dictation controls need the Commands opt-in, which
   defaults off; creating a config does not enable it. Transformations must be
   selected in the mode's Transformations section and can run with Commands off.
   They run after mode corrections and optional AI rewriting, not for Voice
   Action or meetings. Do not enable voice commands merely to run a transformation.
7. Run `bun run check` in `~/.config/hex`. Do not finish until it passes. This
   checks TypeScript; runtime validation can still reject phrase overlaps.
   A running host watches workspace `.ts` and `.json` files and reports reload
   failures in the Commands pane. If startup stopped because the config, Bun,
   or dependencies were unavailable, restart HEX after repairing them. Confirm
   runtime activation when the host is enabled, or report that it was not tested.

Completion criterion: the requested behavior is represented in
`hex.config.ts`, the managed SDK was not modified, `bun run check` passes, and
runtime activation is confirmed or its verification gap is reported.
