# Personal Commands - Product and Implementation Plan

**Status:** MVP implemented. The registry, supervised host, SDK APIs, workspace
provisioning, persisted runtime status, and Settings integration are complete.

## Goal

Let a user define personal voice commands in a type-safe TypeScript workspace
without moving HEX's protected recognition and dictation state machines out of
native Rust. TypeScript owns ordinary literal commands. The compiled registry is
limited to protected system behavior and typed captures that the TypeScript MVP
cannot express yet.

The first motivating example is a personal phrase such as `open training` that
opens a user-specific URL and does not belong in compiled product configuration.

## Decided Direction

- Keep wake, sleep, cancellation, dictation lifecycle, and meeting lifecycle as
  protected native commands.
- Add custom commands as a second registry layer and validate them against the
  minimal compiled system registry before activation.
- Keep protected system phrases reserved regardless of personal-command context.
  Resolve all ordinary compiled and personal commands by context specificity:
  browser host, then application, then global. A more-specific personal command
  may specialize an ordinary global built-in. Reject overlaps at equal
  specificity rather than giving either source silent priority.
- Use TypeScript as the authoring language and Bun as an acceptable optional
  power-user dependency for the MVP.
- Keep ordinary dictation fully functional when Bun is absent, broken, or the
  personal configuration is invalid.
- Use one exported configuration assembled through ordinary TypeScript imports,
  rather than global registration side effects.
- Require one explicit `~/.config/hex/hex.config.ts` entrypoint. Do not add
  automatic command-file discovery or derive command identity from file paths
  in the MVP.
- Use a typed options object as the primitive authoring format. Let users compose
  command records with ordinary imports, functions, spreads, loops, and
  conditional TypeScript however they prefer.
- Allow both structured native actions, such as opening a URL, and arbitrary
  TypeScript handlers.
- Evaluate configuration outside the native listener and transact one
  serializable registry into Rust. Never put Bun or IPC in the partial-transcript
  recognition path.
- Activate a new registry only after TypeScript evaluation, schema validation,
  unique-ID validation, and overlap validation all succeed. Preserve the last
  valid registry after a failed reload.
- Do not gate automatic reload on a full `tsc --noEmit` pass in the MVP. Bun
  evaluation, boundary schema decoding, and Rust validation determine runtime
  activation. Editors and `hex commands check` provide explicit type checking.
- Give the SDK strong contextual types through `defineHexConfig` and typed
  command constructors. Treat Rust validation as authoritative.
- Provide both vanilla Promise and Effect v4 SDK entrypoints. Implement the
  command host and supervision model in Effect; adapt vanilla handlers and
  capabilities into that Effect runtime rather than maintaining two runtimes.
- Store the user workspace at a stable location, provisionally `~/.config/hex/`.
- Make the workspace directly accessible from Settings through one Edit Config
  action and show reload errors only when action is required.
- Scaffold agent-facing documentation with the workspace, including `AGENTS.md`
  and a versioned personal-command skill.
- Create the workspace only after explicit user opt-in.
- Dogfood the feature by keeping Kit-specific literal commands in the TypeScript
  workspace. Keep only protected lifecycle commands and unsupported typed
  captures in Rust. Do not add a one-off migration for user-specific phrases.

## MVP Surface

The MVP intentionally excludes variable captures. Commands use one or more
complete literal phrases.

```ts
import { defineHexConfig } from "@hex/commands"

import navigation from "./commands/navigation"
import training from "./commands/training"

export default defineHexConfig({
  commands: {
    ...navigation,
    ...training,
    "open-training": {
      phrases: ["open training"],
      run: ({ hex }) => hex.openUrl("https://hub.kitlangton.dev/training"),
    },
  },
})
```

Command object keys are stable IDs. Helper modules return plain immutable typed
records rather than mutating a global registry. Tagged templates or fluent
builders may be added later as optional syntax for typed captures; they are not
the semantic foundation.

Personal commands may declare optional presentation metadata:

```ts
"slack.search": {
  group: "Slack",
  description: "Search Slack messages",
  phrases: ["search messages", "search slack"],
  when: { application: "Slack" },
  run: press({ key: "g", modifiers: ["command"] }),
}
```

`group` is a display label, not part of command identity or resolution. Commands
without a group appear under `Other`. Do not infer groups from ID prefixes.

Context predicates remain a closed native algebra in the MVP: global, exact
foreground application, and browser host. Browser-host commands are candidates
only when a browser host is available. Commands at distinct application names
or browser hosts are disjoint. The resolver and overlap validator must share the
same specificity and coexistence rules.

The initial structured action vocabulary should remain small and capability
based. The MVP actions are opening a URL, application, or path; pressing any
supported key with optional modifiers and bounded repetition; and typing fixed
text. Arbitrary TypeScript handlers execute only in the supervised Bun command
host.

```ts
openUrl("https://example.com")
openApplication("Slack")
openPath("/Users/kit/code/open-source/voice-control")
press("escape")
press({ key: "p", modifiers: ["command", "shift"] })
press({ key: "down", repeat: 5 })
typeText("fixed text")
```

`press` is the native primitive; a `shortcut` helper may exist as ergonomic
sugar. Promise and Effect handlers use the same capability vocabulary as
declarative commands. Defer notifications, clipboard manipulation, mouse
control, window management, arbitrary Accessibility traversal, sound playback,
scheduling, and direct voice-mode mutation until real commands require them.

The default SDK should accept ordinary synchronous or Promise-returning handlers:

```ts
import { defineHexConfig } from "@hex/commands"

export default defineHexConfig({
  commands: {
    "open-training": {
      phrases: ["open training"],
      run: async ({ hex }) => {
        await hex.openUrl("https://hub.kitlangton.dev/training")
      },
    },
  },
})
```

The Effect entrypoint exposes capabilities as services. `run` accepts either an
Effect value or a function that produces an Effect:

```ts
import { Effect } from "effect"
import { defineHexConfig, Hex } from "@hex/commands/effect"

export default defineHexConfig({
  commands: {
    "open-training": {
      phrases: ["open training"],
      run: Effect.gen(function* () {
        const hex = yield* Hex
        yield* hex.openUrl("https://hub.kitlangton.dev/training")
      }),
    },
  },
})
```

Internally, vanilla handlers are wrapped at the host boundary and run as Effect
fibers. Every Promise capability call is attached to its invocation, even when
the handler forgets to await it, and the invocation completes only after all of
its calls complete. The generated workspace pins one exact Effect v4 version;
the local SDK, user config, and host resolve that same installation rather than
passing Effect values between runtimes.

Handlers run concurrently. Do not expose or specify a user-facing active-handler
limit in the MVP. Keep transport frames and native delivery bounded so a broken
host cannot block recognition, but defer handler admission policy until runtime
evidence demonstrates a problem.

## Runtime Shape

```text
~/.config/hex/hex.config.ts
        |
        | evaluated by Bun
        v
serializable personal registry
        |
        | transactional validation and activation
        v
native Rust command resolver
        |
        +---- structured action ----> native executor
        |
        +---- handler invocation ---> supervised Bun host
```

The wire protocol must be runtime-neutral. Bun is the first host, not part of
the command model. This preserves a future path to a bundled runtime without
changing configuration semantics or native recognition.

## Configurable Dictation Phrases

Custom aliases for dictation activation and controls are desirable but are not
ordinary personal commands. Activation is recognized from streaming prefixes;
stop, send, and cancel are recognized from stable streaming suffixes. Their
aliases must be registered as native grammar data and matched in Rust.

A future config surface may resemble:

```ts
export default defineHexConfig({
  dictation: {
    start: ["dictate"],
    stop: ["over"],
    send: ["send it"],
    cancel: ["scratch that"],
  },
})
```

Exact words and whether aliases replace or extend built-in controls remain open.

## Deferred Typed Captures

Variable phrases are a follow-up, not part of the MVP. The intended architecture
is a serializable parser algebra authored in TypeScript, compiled into the Rust
matcher, and returned to a handler as typed capture values.

Candidate primitives include:

- Literal sequences.
- Closed choices and spoken-to-value lists.
- Integers.
- Optional elements.
- Named captures.

```ts
const project = list({
  opencode: "/path/to/opencode",
  hex: "/path/to/hex",
})

command({
  id: "open-project",
  phrase: sequence(
    literal("open project"),
    capture("project", project),
  ),
  run: ({ captures, hex }) => hex.openPath(captures.project),
})
```

TypeScript parser descriptors retain phantom result types for authoring, while
their runtime representation is plain data. Rust independently validates and
compiles that data. TypeScript never runs during partial speech matching.

Do not add free-form spoken captures until real commands establish command
boundary and finalization semantics.

## Agent Experience

The workspace should make a coding agent successful without repository
knowledge. It should include:

- The active SDK and HEX protocol versions.
- Minimal examples for structured actions and TypeScript handlers.
- Available contexts and action capabilities.
- Protected-command and overlap rules.
- Commands for checking, reloading, listing, and diagnosing configuration.
- Structured diagnostics that identify source locations and conflicting command
  IDs or phrases.

Initial CLI:

```sh
hex commands init
```

Provisioning creates `~/.config/hex` with a local SDK, the exact Effect
dependency, `package.json`, `tsconfig.json`, `AGENTS.md`, a personal-command
skill, and a starter `hex.config.ts`. Follow-up diagnostics remain candidates:

```sh
hex commands check
hex commands reload
hex commands list
hex commands logs
```

## Observability and HUD

The MVP must emit structured observations for personal command recognition,
dispatch, completion, duration, cancellation, timeout, host failure, and handler
failure. Include command ID, config generation, invocation ID, execution kind,
and bounded failure details. Settings, Activity, and CLI diagnostics should
project the same events.

The Commands screen groups commands by their optional explicit `group` metadata
and shows canonical phrase, aliases, and context. Runtime implementation details
such as registry source, execution kind, and config generation stay in
diagnostics rather than the ordinary catalog UI. Group metadata has no
behavioral meaning.

A visual HUD indication that shows the recognized personal command and its
running or completed state is a follow-up. Build it from the structured command
observations rather than coupling the TypeScript host directly to HUD state.

## Unresolved Decisions

- Whether runtime evidence warrants a user-visible handler timeout or admission
  limit beyond the bounded transport and generation-retirement behavior.
- Whether the MVP permits unrestricted filesystem, network, subprocess, and npm
  package access.
- The exact Settings creation and editor-opening flow.
- Whether structured actions continue to work from the last valid registry while
  Bun is unavailable.
- Dictation alias replacement, extension, conflict, and recovery semantics.
- The personal-command HUD presentation and timing.

## Validation Direction

- Unit-test schema decoding and every structured action.
- Reject duplicate IDs and overlaps against both personal and compiled commands.
- Preserve the active registry after syntax, evaluation, schema, or overlap
  failures.
- Prove Bun crashes, hangs, and queue pressure cannot block recognition,
  dictation, or native structured actions.
- Verify Settings and CLI report the same active revision and last reload error.
- Verify agent instructions against a clean generated workspace.
