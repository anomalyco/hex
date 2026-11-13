# Control the running app from a terminal

```ts
Agent or terminal                                  // control.cli; every OS
├── One tree and protocol over the local API's POST /control on every OS;
│   OS can't do it -> unsupported at runtime, exit 1
├── hex status [--json] -> Listener, capture, meeting, model, features,
│   permissions, window, update, version, pid, memory
│   ├── Wayland listener -> its warning names a missing wl-copy before paste fails
│   ├── Capture -> listener query: shortcut/CLI or SDK recording, including a
│   │   pending microphone start and release tail, plus accepted output jobs
│   │   └── Listener does not answer -> capture unknown, never inferred idle from logs
│   └── Hex not running -> exit 3, "Hex is not running; run `hex start`"
├── hex start -> Already running: nothing | else open Hex.app (Linux and
│   Windows: a detached `hex app --hidden`), wait ≤ 20 s
│   └── The caller's HEX_* variables -> open --env (values never logged)
├── hex quit | restart [--force]
│   ├── Meeting or dictation in progress, no --force -> refused, exit 1
│   ├── Listener didn't answer, no --force -> busy (it might be recording)
│   └── restart -> reopens with the running Hex's HEX_* variables (macOS);
│       Linux unit -> systemctl restart; else waits for the drain, then start
├── hex dictation start|stop|toggle|cancel
│   ├── Same shortcut state machine as the hotkey; start locks the capture
│   │   ├── Windows CLI/tray start -> hook adopts the accepted capture; first
│   │   │   press of its shortcut finishes it, in Hold and Toggle alike
│   │   └── SDK /dictations capture -> only its owner or lease ends it;
│   │       ordinary commands and physical keys leave it alone
│   ├── start -> intentional at once: tone once the mic is open, recording
│   │   behavior, live stream
│   ├── stop -> done only when the clip went to transcription;
│   │   no audio yet or silent -> discarded, nothing transcribing
│   ├── cancel -> discards the recording in any mode, nothing pasted or saved;
│   │   jobs already transcribing finish; idle -> unchanged
│   ├── Repeat a command -> unchanged, exit 0
│   └── dictation stop --print -> stops at once, then waits ≤ 60 s and prints
│       the transcript (processed as a paste) instead of pasting it
│       ├── No speech -> no-speech, exit 4, prints nothing
│       ├── Not recording -> not-recording, exit 1
│       ├── Failure or timeout -> exit 1, prints nothing, never pastes
│       └── History keeps it as Printed; Paste Last skips it (also on Linux)
│   ├── dictation start --print -> Labs › Print-only CLI dictation (macOS, off
│   │   by default): starts the same locked recording, but its text comes back
│   │   here however it ends: the shortcut that finishes it, stop, or a kept
│   │   microphone loss. Never pasted or copied. No speech -> no-speech, exit 4;
│   │   cancel or any other end -> exit 1; already recording -> busy; lab off
│   │   -> disabled; Linux and Windows -> unsupported
│   └── dictation start|toggle --raw -> the dictation it starts pastes the
│       transcript as heard (Dictate without processing); History keeps it as Raw;
│       --raw with stop -> usage error, exit 2
├── hex paste-last -> pastes the last dictation
│   ├── Setup gates the listener -> not-ready
│   └── Recording or shortcut gesture in progress -> busy (menu bar: nothing)
├── hex typed-text (Linux) -> the Transform typed text shortcut's one job
│   ├── Labs › Transform typed text off -> disabled
│   ├── Terminal paste on -> disabled (it would send Ctrl+Shift+V)
│   ├── Setup gates the listener -> not-ready
│   └── Recording, a pending start, or a capture still finishing -> done,
│       but the listener drops the job: no keys are sent and no state changes
├── hex transcribe <file> [--model <id>] [--language <code>]
│   ├── WAV, AIFF, CAF, FLAC, MP3, Ogg Vorbis/Opus, WebM, MP4/M4A/MOV
│   │   (AAC-LC, HE-AAC, ALAC; a video's soundtrack) -> decoded here
│   ├── Not media, no audio track, or an unknown codec -> unsupported, exit 1,
│   │   before Hex is asked
│   ├── Any length -> decoded in ≤30 s pieces, their text joined with one space
│   ├── Hex running -> one piece at a time to its POST /transcriptions with
│   │   the model named; each piece on the dictation engine's loaded copy when
│   │   it holds that model; no child starts
│   ├── Hex stopped -> starts and owns `hex service --embedded`, the same
│   │   route, ended on every exit
│   ├── Model not downloaded
│   │   ├── stdin and stderr terminals, no --json -> Download <model>? [Y/n];
│   │   │   yes downloads, loads, and retries the piece
│   │   └── otherwise -> model-unavailable naming `hex model install`, exit 1
│   ├── stdout -> only the transcript or --json reply; progress on a TTY stderr
│   ├── Ctrl-C -> the child ends, nothing printed, exit 130
│   └── No speech in every piece -> no-speech, exit 4; nothing pasted, kept,
│       or processed
├── hex completions <bash|zsh|fish|elvish|powershell>
│   ├── Labs › Shell completions off -> disabled, exit 1; nothing printed
│   ├── Runs here: no running Hex, no /control request
│   └── On -> the script on stdout and nothing else, generated from the same
│       clap tree every command parses (hidden from hex --help)
├── hex meetings record [--title] | stop | copy-last
│   ├── Meetings off -> feature-off; recording shows in the menu bar and HUD
│   │   exactly like a menu-bar recording
│   └── No meetings cancel: Hex cannot discard a recording
├── hex settings list | get <key|prefix> | set <key> <value>
│   ├── The window's save, then ReloadSettings -> live, no restart
│   ├── Unknown key, bad value, read-only key -> refused, nothing written
│   ├── Shortcut conflict -> refused, as the window's recorder would
│   └── apply <file|-> -> a JSON object of key: value, all or none
│       ├── Undeclared settings keep their values; dependent keys in any order
│       ├── transcription.model / .language -> selected, never downloaded
│       ├── Hex running -> SettingsApply: saved and adopted live, like set
│       └── Hex stopped -> written to settings.json under the service lock
│           (Linux); a Hex holding it is waited for (≤ 35 s) instead
├── hex model list | use <id> | install [id] | cancel
│   ├── use -> installed models only; needs the running Hex; never downloads
│   │   or repairs; keeps the model's remembered language and hints
│   │   (--language overrides)
│   └── Linux/Windows: install with Hex stopped downloads and selects here
│       (a Hex that starts meanwhile makes the switch); status, devices,
│       check, transcribe run here (macOS: unsupported)
├── hex window show [pane] | hide
│   ├── settings -> the General pane (status window.pane stays "settings")
│   ├── meetings pane with Meetings off -> the pane's opt-in card
│   └── story <story> [--variant V] [--play] -> a playground story, as a
│       hex://story link opens (labs.md); playground off -> feature-off
├── hex history clear --yes -> without --yes refused before Hex is asked
├── hex check-for-updates -> The updater checks now
├── hex launch-at-login [on|off] -> SMAppService (changes need Hex.app);
│   Windows Run key; Linux XDG autostart entry
└── hex logs [--process] [-n N] [-f] -> local tail; works with Hex stopped
```

Every command except `start`, `restart`, `logs`, `settings apply`,
`completions`, and the read commands talks only to the running Hex; none of
them launch Hex. Every OS reaches it through
the local API's owner-only discovery file and the bearer-authenticated
`POST /control` route. Commands dispatch before logging and the instance lock on every OS,
so they never compete with the running app for its files. Opt-ins stay
explicit settings: no command turns Meetings on as a side effect.

`hex transcribe` is one command that doesn't use `/control`. It opens the
file here and decodes its first audio, so a file Hex can't read is refused
before anything transcribes: WAV through hound (every sample read once up
front, so a truncated WAV is refused), everything else through Symphonia, and
Opus through opuscule (`control/cli/transcribe/pieces/media.rs`). It then
sends the recording one piece at a time to `POST /transcriptions`
(`Content-Type: audio/wav`), choosing the endpoint once
(`control/cli/embedded.rs`): the running app's discovery file, so the app
transcribes it with the model the request names, every piece on its dictation
engine's loaded copy when it holds that model; or, with no Hex running, an
embedded `hex service --embedded` child it starts in its own process group,
reads one ready line from, and ends on success, failure, and Ctrl-C. A Hex
that runs but answers the wrong API version or token is an error, never a
reason to start a second service. Pieces are at most 30 s
(`control/cli/transcribe/pieces.rs`), decoded and resampled incrementally and
cut through the quietest point near their end by `gguf_session::quiet_cut`,
the same cut a long GGUF clip gets; the service skips a piece of only silence
once the speech detector is installed, and the rest of the text is joined
with one space, so a file can be any length and only about one piece is in
memory. A `model-not-ready` piece offers the download on a terminal
(`control/cli/prepare.rs`), prepares the model through the service's
`POST /models/{id}/prepare` events, and retries that piece once.

`hex completions` also never reaches `/control`: it reads its Labs switch from
`settings.json` (`AppSettings::read`), then prints a script generated at
runtime from the top-level `Cli` of the OS running it
(`src/main.rs` on macOS, `src/linux.rs`, `src/windows.rs`, each flattening
`ControlCli`), so every command and flag in this tree is in the script. The
subcommand is `hide = true`: it never shows in `hex --help`, and `hex labs`
lists the toggle that turns it on. Off, it answers `disabled` naming `hex labs
on shell-completions`, prints nothing, and exits 1. On, only the script goes
to stdout.

On Linux and Windows `control::host::answer` carries every command out through
the OS's adapter (`linux::app::LinuxControl`, `windows::rune::WindowsControl`):
dictation, paste-last, and Linux's typed-text go to the listener's own queue
(Windows: the same hotkey loop, as a toggle press would), settings save and
reload as the window's do, and `settings set` refuses a key whose capability
the OS lacks
(`settings list` marks it "not on this OS"). `meetings record|stop|copy-last`
go to the meeting recorder (`meeting/runtime`) as the tray's items do;
`window story` answers `unsupported`; Windows sends every `annotate`
command to the running Hex. `hex history`, `hex meetings list|show|search`, and
`hex mcp` read files on every OS.

`dictation start --print` is macOS's, behind a Labs toggle: the listener
stores the command's reply with the recording (`recognition::Hotkeys::print_capture`)
and reads it at every end. The shortcut's finish passes it to
`DictationWorker::transcribe_printed`; `dictation stop` and `toggle` join it
with their own reply so one transcript answers both (a plain stop still
answers as a plain stop does); `dictation cancel`, a
discard, and a kept microphone loss answer it themselves. Only the dictation
machine's actions may take it, and a reply still held when a new recording
starts, or when the machines are suspended, is failed rather than left to
answer a later dictation. Nothing pastes or
copies it, History records it as Printed, and the reply is answered rather
than dropped on every path, including a discarded or silent clip and a
microphone loss. While it waits, it holds one of the local API's four
connection workers (`local_api::HTTP_WORKERS`) until it answers or reaches
its cap, even if the asking client is killed. Linux and Windows answer
`unsupported` before their host runs anything.

Sources: [control.rs](../../src/control.rs) (protocol, status, quit rules),
[control/host.rs](../../src/control/host.rs) (the Linux and Windows host),
[control/settings.rs](../../src/control/settings.rs) (settings registry),
[control/cli.rs](../../src/control/cli.rs) and
[control/cli/human.rs](../../src/control/cli/human.rs) (clap, output, exit codes, start,
logs), [macos_rune/control.rs](../../src/macos_rune/control.rs) (the macOS host),
the `/control` route in [local_api.rs](../../src/local_api.rs), and early
dispatch in [main.rs](../../src/main.rs).
[Command and JSON contracts](../cli.md) document flags, exit codes, and schema.

Checks: `control::tests` round-trip every command and reply (including
`a_printing_stop_answers_through_its_reply_channel`, which also joins two
printing replies onto one transcript and pins `start --print`'s flags and
deadline), refuse other
protocol versions, and cover the quit rules (an unanswered listener refuses)
and `model_selection` (remembered hints, wildcard-language validation); `control::settings::tests` include
`every_persisted_setting_has_a_cli_key`, value parsing, shortcut conflicts,
and zero volume (`enabling_tones_restores_a_nonzero_level` covers enabling a
saved zero); `control::cli::tests` pin
exit codes, output, and `hex start`'s `open`
arguments; `control::cli::completions::tests` pin the Shell completions gate
and that every shell gets a script naming the command tree, and the
`the_cli_prints_completion_scripts` tests in `linux.rs`, macOS's `main.rs`, and
`windows.rs` each generate bash, zsh, and fish from that OS's own top-level
`Cli`; `desktop::onboarding::tests::relaunch_forwards_only_the_hex_environment`
pins restart's; `macos_rune::control::tests` check that `model use` prepares
its exact selection installed-only;
`dictation::audio::tests::marked_captures_are_intentional_without_a_hold`
covers the immediate stream and `ReadyIntentional`; `recognition::tests`
cover the stop replies and the paste-last refusal;
`recognition::hotkeys::tests::a_printing_start_follows_the_shortcut_that_finishes_it`
covers a printing start's reply surviving to the shortcut's finish and a
cancel, and
`recognition::hotkeys::tests::a_held_print_reply_never_answers_a_later_dictation`
covers another shortcut leaving it alone, a later start failing it, a discard,
and a suspension; `control::tests::cancel_parses_and_takes_no_flags`
covers `cancel` and its flags; local API's
`control_requests_are_authenticated_versioned_and_typed` covers authentication,
bounds, and error codes; `tests::control_commands_parse_beside_the_read_commands`
covers parsing next to `history` and `meetings`; `control::host::tests` cover
the quit refusal, printing stops (and Linux's and Windows' `unsupported` for a
printing start), and runtime `unsupported` replies;
`quit_and_status_use_the_owner_even_without_dictating_log_events` covers SDK
capture, pending jobs, and an unanswered listener. `dictation::service::tests`
cover the recording query through SDK opening, capture, and completion, and
timeout recovery. Windows input's `command_capture_*` and `command_stop_*`
tests cover command-to-key handoff, raw ownership, Toggle cancel, and a CLI
stop during a physical hold. These are owner/transition tests, not a physical
Windows keyboard or live SDK recording smoke test.
`linux::tests` parse the shared tree beside Linux's own commands;
`rune_ui::platform::parity_tests` fail when a capability differs across OSes
without a recorded reason; `settings_name_real_capabilities_and_keys` checks
the settings-to-capability table; `control::cli::transcribe::tests` cover a
file that isn't media and a truncated WAV refused before any transcription,
the selected model and language, a file with no audio as `no-speech`, pieces
joined in order, and a file whose pieces all hear nothing as `no-speech`;
`control::cli::transcribe::pieces::tests` cover a 95 s clip split into pieces
that cover every sample once, a 48 kHz file read into ≤30 s pieces whose
samples match `AudioClip::decode_wav`, every WAV sample format, a file that
changes mid-read naming where reading stopped, every container fixture in
`tests/fixtures/media` (M4A and CAF AAC, ALAC, AIFF, FLAC, MP3, MP4 with
video, Ogg and WebM Opus, Ogg Vorbis) decoding to its 16 kHz length, Opus
pre-skip and end trim, a video with no audio track, and a cut-short file;
`control::cli::prepare::tests` cover the prompt's answers and that `--json`
never asks; `local_api::discovery::tests` cover choosing an endpoint (absent,
stale, wrong version or token) and validating the embedded ready line.

Observed (Oct 30, apollo, an isolated data root and `hex service` without a
display): status, settings get/set (and refusing `show_dock_icon`), vocabulary
add/list/remove, model list/use/status, history clear, window hide,
check-for-updates, commands check, quit, start, and restart all answered as
documented. Windows compiled with `cargo xwin clippy` (x64 only; the ARM64
cross-build fails in rquickjs-sys's build script, unrelated); it has not run
on a Windows PC.

Gaps: no command has run against a live app; the unit checks substitute the
host. The control loop's start/stop wiring (tone timing, recording behavior)
has no end-to-end test. `dictation start --print` in particular has never run
on a Mac with a microphone: its reply routing through the shortcut finish, a
kept microphone loss, and a paused listener is covered only by
`recognition::hotkeys`' unit test and by reading every path that ends a
recording, so expect to verify it by hand (a locked recording finished by the
shortcut, by `stop`, and by `stop --print`) before trusting it. A CLI volume
change does not preview a tone as the window does. Onboarding
permission prompts, calendar connection, and meeting deletion or renaming have
no command. Linux and Windows dictation through the CLI has not run with a
microphone, and meeting recording through the CLI has not run on Linux or
Windows. `hex transcribe` was observed on macOS (Nov 13, Apple Silicon): with
the app running (no child) and stopped (an embedded child, on an empty data
root), WAV, AIFF, M4A AAC-LC, HE-AAC, ALAC, CAF AAC, FLAC, MP3, Ogg Opus, MP4,
MOV, and fragmented MP4 each printed the spoken transcript; a 31-minute M4A
ran in ≤30 s pieces in 21 s at 235 MB peak resident with the model loaded; the
terminal prompt downloaded SenseVoice Small and retried; non-TTY and `--json`
answered `model-unavailable`; a silent file answered `no-speech`; Ctrl-C exited
130 with the child gone. It has not run on Linux or Windows since it moved to
the embedded service.
