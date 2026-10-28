---
name: hex-personal-commands
description: Configure Hex personal voice commands, voice-dictation control phrases, and deterministic transcript transformations. Use when the user wants to add or change commands or transformations for the Hex voice app.
license: MIT
---

# Hex Personal Commands

Hex runs `~/.config/hex/hex.config.ts` itself, in an embedded JavaScript
engine. There is no package to install and no build step.

1. Check for `~/.config/hex/hex.config.ts` (or `hex.config.js`). If it is
   missing, ask the user to open Hex's Commands pane and choose Create Config,
   or run `hex commands init` if the `hex` CLI is available.
2. Read `~/.config/hex/AGENTS.md` for the current API and limits, and
   `hex.d.ts` for exact types. Hex owns both and rewrites them; do not edit
   them.
3. Edit only the config and files it imports. Scripts are plain TypeScript or
   JavaScript: they can import `@hex/commands` and files in the folder, not
   npm packages, Effect, or Node APIs, and they have no filesystem, network,
   or timer access. Enums, namespaces, and constructor parameter properties are
   rejected.
4. Use `defineHexConfig` from `@hex/commands`. A command has `phrases` and
   either a fixed `action` (such as `openUrl(...)`) or a `run` handler that
   returns an action, a list of actions, or nothing (it may also queue actions
   through its `hex` argument). Hex runs the actions after the handler returns.
5. Keep command phrases unambiguous. Use `digit()` for zero through nine,
   `digit({ min, max })` for a restricted range, `choice(["left", "right"] as const)`
   for exact choices, an object-form `choice()` to normalize one-word aliases to
   canonical keys, `union(letter(), digit(), choice(["home", "end"] as const))`
   for disjoint bounded one-token alternatives, and trailing `text()` for
   explicit text captures. `letter()` accepts literal, common spoken, and
   one-word NATO letter names and returns lowercase `"a" | ... | "z"`. With a
   `captures` schema, every phrase alias must bind every declared name exactly
   once.
6. Spoken commands and dictation controls need the Commands opt-in, which
   defaults off; creating a config does not enable it. Transformations must be
   selected in the mode's Transformations section and can run with Commands off.
   They run after mode corrections and optional AI rewriting, not for Voice
   Action or meetings. Do not enable voice commands merely to run a transformation.
7. Run `hex commands check` (macOS) after editing. It loads the config as Hex
   does and prints what it registers or the error with file, line, and column,
   including phrase overlaps. Optionally type-check with
   `npx -y -p typescript tsc` in `~/.config/hex`. A running Hex reloads the
   config on save and shows load errors in Modes' Transformations section.

Completion criterion: the requested behavior is represented in the config,
`hex commands check` succeeds (or, without the CLI, the reload succeeded in
Hex), and any verification gap is reported.
