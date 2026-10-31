# Control the running app from a terminal

```ts
Agent or terminal                                  // control.cli; macOS release
├── hex status [--json] -> Listener, capture, meeting, model, features,
│   permissions, window, update, version, pid, memory
│   └── Hex not running -> exit 3, "Hex is not running; run `hex start`"
├── hex start -> Already running: nothing | else open Hex.app, wait ≤ 20 s
│   └── The caller's HEX_* variables -> open --env (values never logged)
├── hex quit | restart [--force]
│   ├── Meeting or dictation in progress, no --force -> refused, exit 1
│   ├── Listener didn't answer, no --force -> busy (it might be recording)
│   └── restart -> reopens with the running Hex's HEX_* variables
├── hex dictation start|stop|cancel|toggle
│   ├── Same shortcut state machine as the hotkey; start locks the capture
│   ├── start -> intentional at once: tone once the mic is open, recording
│   │   behavior, live stream
│   ├── stop -> done only when the clip went to transcription;
│   │   no audio yet or silent -> discarded, nothing transcribing
│   ├── Idle cancel while another capture records -> busy (Escape's order:
│   │   the recording before any job)
│   ├── Repeat a command -> unchanged, exit 0
│   └── dictation stop --print -> stops at once, then waits ≤ 60 s and prints
│       the transcript (processed as a paste) instead of pasting it
│       ├── No speech -> no-speech, exit 4, prints nothing
│       ├── Not recording -> not-recording, exit 1
│       ├── Failure, cancel, or timeout -> exit 1, prints nothing, never pastes
│       └── History keeps it as Printed; Paste Last skips it (also on Linux)
├── hex paste-last -> pastes the last dictation
│   ├── Setup gates the listener -> not-ready
│   └── Recording or shortcut gesture in progress -> busy (menu bar: nothing)
├── hex meetings record [--title] | stop | copy-last
│   ├── Meetings off -> feature-off; recording shows in the menu bar and HUD
│   │   exactly like a menu-bar recording
│   └── No meetings cancel: Hex cannot discard a recording
├── hex settings list | get <key|prefix> | set <key> <value>
│   ├── The window's save, then ReloadSettings -> live, no restart
│   ├── Unknown key, bad value, read-only key -> refused, nothing written
│   └── Shortcut conflict -> refused, as the window's recorder would
├── hex model list | use <id> | install <id> | cancel
│   └── use -> installed models only; never downloads or repairs; keeps the
│       model's remembered language and hints (--language overrides)
├── hex window show [pane] | hide
│   ├── settings -> the General pane (status window.pane stays "settings")
│   ├── meetings pane with Meetings off -> the pane's opt-in card
│   └── story <story> [--variant V] [--play] -> a playground story, as a
│       hex://story link opens (labs.md); Labs off -> feature-off
├── hex history clear --yes -> without --yes refused before Hex is asked
├── hex check-for-updates -> The updater checks now
├── hex launch-at-login [on|off] -> SMAppService; changes need Hex.app
└── hex logs [--process] [-n N] [-f] -> local tail; works with Hex stopped
```

Every command except `start`, `restart`, and `logs` talks only to the running
Hex through the owner-only discovery file and the bearer-authenticated
`POST /control` route; none of them launch Hex. Commands dispatch in `main`
before logging, and the instance lock, so they never compete
with the running app for its files. Opt-ins stay explicit settings: no command
turns Meetings on as a side effect.

Linux and Windows add only `hex settings`, which reports `unsupported`; the
Linux service socket keeps its own `dictation`, `paste-last`,
`status`, `quit`, and `restart`.

Sources: [control.rs](../../src/control.rs) (protocol, status, quit rules),
[control/settings.rs](../../src/control/settings.rs) (settings registry),
[control/cli.rs](../../src/control/cli.rs) (clap, output, exit codes, start,
logs), [macos_rune/control.rs](../../src/macos_rune/control.rs) (the macOS host),
the `/control` route in [local_api.rs](../../src/local_api.rs), and early
dispatch in [main.rs](../../src/main.rs).
[Command and JSON contracts](../cli.md) document flags, exit codes, and schema.

Checks: `control::tests` round-trip every command and reply (including
`a_printing_stop_answers_through_its_reply_channel`), refuse other
protocol versions, and cover the quit rules (an unanswered listener refuses)
and `model_selection` (remembered hints, wildcard-language validation); `control::settings::tests` include
`every_persisted_setting_has_a_cli_key`, value parsing, shortcut conflicts,
and zero volume; `control::cli::tests` pin
exit codes, output, and `hex start`'s `open`
arguments; `onboarding::tests::relaunch_forwards_only_the_hex_environment`
pins restart's; `macos_rune::control::tests` check that `model use` prepares
its exact selection installed-only;
`dictation_audio::tests::marked_captures_are_intentional_without_a_hold`
covers the immediate stream and `ReadyIntentional`; `recognition::tests`
cover the stop replies and the idle-cancel and paste-last refusals; local API's
`control_requests_are_authenticated_versioned_and_typed` covers authentication,
bounds, and error codes; `tests::control_commands_parse_beside_the_read_commands`
covers parsing next to `history` and `meetings`.

Gaps: no command has run against a live app; the unit checks substitute the
host. The control loop's start/stop wiring (tone timing, recording behavior)
has no end-to-end test. A CLI volume change does not preview a tone as the window does. Onboarding
permission prompts, calendar connection, and meeting deletion or renaming have
no command. Linux socket and Windows hosts do not carry control commands yet.
