# HEX Agent Guide

## Private Kit Tool

Commit and push straight to `dev` (the default branch), no PRs. Merge any open
PR without asking. After pushing, update the installed copy. See
`~/code/control/AGENTS.md`. `production` holds what was released and moves
only by a fast-forward from `dev` (`git push origin dev:production`) when Kit
releases; never commit to it. There is no `main`.

`dev` is canonical and contains the Rune front end on every OS; GPUI is
removed. The `rune-ui` integration branch is merged and kept only for history:
branch from and push to `dev`.

## Purpose

Build HEX as a local, observable macOS voice appliance with explicit Linux
X11, wlroots-compatible Wayland, and Windows (x64 and ARM64) beta contracts. Keep the engine native Rust
and keep consequential behavior explicit. Protected commands and typed captures
remain compiled Rust; ordinary literal commands live in the explicit TypeScript user config. User-facing
runtime settings persist in Application Support.

The distributed release starts in hotkey-dictation-only mode. Voice commands and
their catalog remain available as a persisted opt-in that defaults off.
Voice Action also requires its own persisted opt-in, defaulting off even for
existing settings without an explicit enabled flag. Meetings (beta) is a third
persisted opt-in (`meetings.enabled`, default off) that adds call detection, the
Meetings pane, menu-bar recording controls, and meeting paste in release builds.
`DEVELOPER_FEATURES_ENABLED` keeps meetings always available in debug builds and
keeps the meeting CLI, Activity, and HUD Lab debug-only.

## Feature Map

[`docs/features/README.md`](docs/features/README.md) indexes existing user-facing
capabilities, entry points, checks, and gaps. Before designing, changing,
debugging, or reviewing a user-facing feature, read the index and relevant
feature files, then check their referenced implementation. The map is not a
substitute for source inspection or a second roadmap.

- Update the affected map in the same change when behavior, entry points,
  defaults, prerequisites, platform availability, recovery, or verification
  coverage changes. Do not churn unrelated maps for internal-only refactors.
- Organize by user capability, not Rust module or UI pane. Dictation, Voice
  Commands, and Voice Action are distinct features; link shared flows and
  recovery rather than duplicating their descriptions. Keep the index current
  when adding or splitting a feature file.
- Prefer small action/outcome diagrams in `ts` fences with Unicode tree
  characters, not tables. Preserve stable behavior IDs and keep source/check
  references and proof limits beside the behavior they explain.
- Distinguish implemented behavior, available checks, and observed verification.
  A test's existence is not a passing run, and a preview is not native end-to-end
  proof. Record missing coverage and known defects explicitly; never rewrite an
  expected outcome merely to hide a regression. A map does not authorize live
  device tests. Keep future work in `ROADMAP.md`.

## Architecture

- `audio`: `cpal` device enumeration, input resolution (`--device`, the saved
  microphone, else the system default, never Hex Microphone), the CoreAudio
  default-input and device-list listeners, timestamped mono float PCM
  delivery, live selection, and bounded stream recovery.
- `moonshine`, `moonshine_lab`: the only Moonshine C adapter, the streaming
  recognizer, and the debug interactive/batch command-fixture evaluator.
- `suppression`: the macOS event taps, shortcut suppression, and the
  configurable dictation-hotkey state machine.
- `keyboard`: active-layout key resolution and balanced synthetic shortcuts.
- `dictation`, `dictation_diagnostics`: warm pre-roll, growable capture, 16 kHz
  local-transcription resampling, and owner-only diagnostic WAV retention when
  `HEX_RETAIN_DICTATION_AUDIO` is set.
- `pause_segmenter`: pure 16 kHz pause detection that cuts meeting audio into
  rolling transcription chunks.
- `dictation_audio`: the authoritative microphone timeline, lazy stream
  lifecycle, recording owner, exact shortcut boundaries, recovery handoff,
  disposable bounded command audio projection, and the bounded capture sink
  (`open_capture_sink`/`close_capture_sink`) that copies the same timeline to
  a Scrawl capture.
- `recording_environment`: serialized RAII ownership of idle-sleep prevention,
  output muting, and supported media-player pause/resume behavior, each held
  from the first dictation or capture session that wants it to the last.
- `dictation_processor`: the one OpenCode client and ordered processing policy
  for macOS, Windows, and Linux: context-selected corrections, deadline-bounded
  OpenCode rewrite profiles with corrected-transcript fallback, the ranked
  model catalog, Voice Action generation, and the transformation stage. Every
  OS discovers the `opencode2` beta executable and links missing installs to
  `https://v2.opencode.ai/`. On macOS and Linux, `opencode2 api get` discovers
  or starts the managed service, and generation uses the matching owner-only
  service registration and authenticated loopback HTTP through the system curl;
  request bodies and credentials use stdin, never argv or new temporary files.
  Windows sends the same requests through the `opencode2 api` CLI inside a job
  object.
- `parakeet`: the strict-Metal `transcribe.cpp` adapter plus the dictation
  worker that hands clips to the transcription engine and runs bounded,
  ordered processing, output, paste, last-result, and meeting-delta work, plus
  `transcribe_with_words`, the capture path's word-timestamped foreground
  requests on the same engine.
- `apple_speech`: the Swift `SpeechAnalyzer` bridge with per-locale support
  checks, asset reservation, and batch transcription.
- `transcription`, `transcription_benchmark`: runtime selection, transactional
  warm-model activation, and the fixed-corpus benchmark runner behind
  `hex benchmark-transcription`.
- `gguf_session`: the `transcribe.cpp` session owner shared by `parakeet` and
  `local_transcriber`; it recreates the native session around each run (and
  each live stream) so inference scratch stays bounded, and owns chunked
  long-audio transcription.
- `local_transcriber`: the Linux and Windows GGUF loader, prewarm, whole-clip
  transcription, and stream entry point.
- `transcription_engine`: the thread that owns the loaded model on every OS and
  runs every inference in order: live streams fed during capture, their finish,
  whole-clip runs, and (macOS) model loads and switches. The capture owns its
  `LiveStream` and hands it to the finished clip; finishing returns a
  `PendingTranscript` whose wait enforces the stream deadline and falls back to
  the whole clip. On macOS it also runs background work (meeting chunks) that
  yields to any dictation request sent before it starts.
- `transcription_preparation`: application-owned model preparation shared by
  Settings and the menu bar on macOS and by the Windows runtime, with one
  worker, latest-choice pending admission, cancellation fences, and an
  installed-only path that never downloads a model.
- `transcription_models`: the compiled model catalog, language recommendations,
  pinned artifact verification, and lazy installation.
- `transcription_service`: bounded host-audio admission, hostile WAV validation,
  normalization, cancellation, and inference. In the app, clips up to 30 s run
  on the dictation engine's model when it is the one requested; otherwise the
  service loads its own, released after a long clip when the engine holds it.
- `transcription_stream`: live `/streams` transcription of another app's
  pushed audio on the engine's loaded model: pause-cut windows fed through
  yielding engine streams that any dictation request closes, a bounded
  backlog held while dictation is busy, offline fallback windows, the lease,
  and one live stream at a time.
- `local_api`: authenticated loopback discovery or direct-child endpoint
  handoff, bounded HTTP parsing, model preparation progress, and raw
  transcription routes, the `/streams` routes (contract:
  [`docs/streams.md`](docs/streams.md)), plus the always-on authenticated
  `POST /control`.
- `control`: the release `hex` control surface: the versioned
  `ControlRequest`/`ControlReply` protocol, the settings registry behind
  `hex settings` (one dotted key per persisted setting, with the window's
  rules), and the CLI client (`control::cli`). `macos_rune::control` is the
  one host that carries the commands out on macOS; Linux and Windows report
  "not supported on this OS" for now. Contract: `docs/cli.md`.
- `sdk/typescript`: Promise and Effect v4 host wrappers for direct-child
  lifecycle, authenticated model preparation, and host-audio transcription.
- `developer_control`: the typed debug-only command/reply protocol the local
  API uses to drive the running app from `hex dev`.
- `paste`: clipboard insertion, continuation joins, and generation-safe
  clipboard restoration.
- `accessibility`: bounded focused-window and selected-text capture through
  Accessibility without touching the clipboard.
- `recognition`: the semantic coordinator for Moonshine, command mode, hotkey
  and voice-delimited controls, workers, and observations. It does not own the
  authoritative microphone timeline.
- `command_grammar`, `spoken_text`: typed command patterns, captures,
  spoken-number and ASCII-punctuation normalization, overlap detection, and
  command catalog metadata.
- `keys`: the platform-neutral key and modifier vocabulary of command actions.
- `commands`: pure contextual resolution plus bounded asynchronous action
  execution (`open` and CoreGraphics keystrokes on macOS; shell-opened URLs,
  paths, and applications on Windows).
- `personal_commands`: the Bun-hosted `@hex/commands` TypeScript workspace:
  managed SDK provisioning (built `dist` on macOS, the embedded TypeScript
  `src` on Windows and Linux), the watch-reloaded user command and
  transformation host, bounded invocation dispatch, and status snapshots.
  macOS, Windows, and Linux share it; Windows and Linux host transformations
  only.
- `text_replacements`: phrase-boundary-aware compilation of configured text
  replacements applied longest-match-first to transcripts.
- `config`: compiled commands and dictation profiles.
- `context`: the foreground context snapshot and selectors shared by commands
  and modes, plus native macOS capture. Browser host is the domain concept;
  Brave AppleScript is the first macOS adapter and `windows_context` reads any
  browser through UI Automation.
- `application_catalog`: cached installed-application discovery for the Modes
  picker, one module per OS: macOS bundles, Windows executables from Start menu
  shortcuts, App Paths, and open windows, Linux `.desktop` entries.
- `app_settings`: the one persisted `settings.json` on every OS and its live
  runtime projection for commands, hotkeys, microphone and transcription
  selection, recording behavior, processing, sound volume, indicator position,
  interface language, and Dock policy. It records one-time changes in
  `migrations` and migrates the Windows and Linux beta files once.
- `login_item`: the native `SMAppService.mainAppService` adapter and its worker
  thread. macOS owns registration state; it is deliberately not duplicated in
  `settings.json`.
- `app_paths`: the Application Support owner for runtime logs and shared state,
  including the one-time macOS `voice-control` to `hex` folder rename, which runs
  in `main` before logging, the instance lock, or any file in the folder opens.
- `onboarding`, `permission_guide`: required permission health, the native
  floating System Settings guide, selected dictation-model installation, the
  release startup gate, and opt-in command-model setup.
- `hex_microphone`, `voice_filter`: the experimental macOS CoreAudio virtual
  microphone (`Hex Microphone`), lock-free capture-to-sink ring buffer,
  real-time voice-filter DSP presets, and the offline WAV renderer behind
  `hex dev voice-filter render`.
- `linux`, `linux_app`, `linux_dictation`, `linux_paste`, `linux_migration`:
  the Linux beta CLI (including the `hex dictation`, `hex voice-action`, and
  `hex paste-last` keybinding commands and launch forwarding), the one Linux
  process (host thread, window on demand through one seam, Quit), the hotkey
  capture-transcribe-paste loop with Paste Last and Voice Action,
  paste and primary-selection reads, and the one-time `linux-settings.json`
  migration.
- `linux_input`: the one shortcut state machine (hold, double-tap lock, toggle,
  double-tap only, Escape, Paste Last, Voice Action, and `hex` commands) and the
  monitor that feeds it from X11 key grabs, the Wayland GlobalShortcuts portal,
  or, without the portal, the `hex` commands alone.
- `linux_portal`: the GlobalShortcuts portal client on a private bus
  connection: `Register("hex")`, one session for Hex's shortcuts, a second
  session binding Escape only while recording, granted-trigger descriptions,
  and session-closed reporting.
- `linux_file_chooser`: one blocking `OpenFile` call on the FileChooser
  portal over a private bus connection: `file_dialog`'s Linux adapter.
- `linux_context`: the frontmost X11 application for modes; Wayland has none.
- `linux_audio_control`: While-dictating mute through `wpctl`/`pactl` and
  MPRIS pause over D-Bus, restoring only what it changed.
- `linux_session`: display-backend selection matching winit's (Rune's)
  nonempty `WAYLAND_DISPLAY` rule, independent of persisted preferences.
- `linux_service`: the single-instance lock and owner-only, same-user Unix
  socket that forwards `Activate` (with the activation token), shortcut
  commands, `Status`, and `Quit` to the one Hex process, plus the per-user
  systemd lifecycle (`Type=notify` readiness, `session.env`, start/restart).
- `linux_rune_hud`: the focus-free, click-through recording/processing HUD as
  a Rune overlay in the Hex process: layer-shell on Wayland, override-redirect
  on X11, at the top or bottom. The listener reports through `LinuxIndicator`
  and never waits on it; it shows once the window's app runs, whether or not
  the window is open. Linux has no tray icon.
- `updater`: the in-app updater on every OS, a client of the Anomaly release
  service (`docs/plans/release-service.md`). `updater/protocol` is the
  protocol's strict SemVer, lenient manifest parsing, and client rules (the
  service's shared test vectors in `tests/fixtures/release-protocol`);
  `updater/transport` is curl with `If-None-Match` and resumable,
  size- and SHA-256-checked downloads; `updater/{linux,macos,windows}` detect
  the install method, verify (OS code signature on macOS and Windows), stage,
  and restart into an update.
- `windows`, `windows_app`, `windows_rune`, `windows_dictation`,
  `windows_input`, `windows_paste`, `windows_migration`, `windows_rune_hud`,
  `desktop_indicator_model`, `desktop_indicator_shader`,
  `windows_context`, `windows_audio_control`, `windows_login_item`: the Windows
  beta CLI, the runtime host (`WindowsDesktopHost`: listener, model
  preparation, OpenCode catalog, transformations, updater) and single-instance
  claim, the tray app on `rune::App` (tray, window icon, and runtime thread;
  `rune_ui/windows_host.rs` is the window's host), the WASAPI dictation loop,
  low-level keyboard hook, clipboard paste, the one-time
  `windows-settings.json` migration, the HUD (`desktop_indicator_model` springs
  and `desktop_indicator_shader` CPU fragment shader drawn into a Rune
  overlay), UI Automation context, audio ducking, and the
  login item.
- `dictation_modes`: the mode runtime the Windows and Linux listeners share:
  mode selection by application and website, then global replacements, mode
  corrections, OpenCode rewriting, and transformations, and the History entry.
- `desktop_i18n`: interface translations keyed by the English source string,
  `tr`, `tr_fill`, and `tr_named`. A test fails if any string the window
  translates lacks a language.
- `history`: the owner-only bounded retained-dictation store: retention
  windows with hard entry and byte caps, atomic crash-safe persistence, and
  search. Text and bounded metadata only, never audio.
- `transcript_cli`: read-only macOS release commands `hex history` and
  `hex meetings list|show|search`, versioned JSON projections and filters.
  Reads history snapshots without opening the writable store, and shares
  meeting selection, search, and Markdown with `meeting::query` / MCP.
- `events`, `dashboard`: bounded asynchronous append-only NDJSON observations
  and bounded incremental reading; the debug `dashboard` TUI and the Activity
  pane are read-only projections.
- `desktop_activity`: the shared listener, device, session, and latest-failure
  projection over `EventReader`. Transcript rows read retained events directly.
- `desktop_host`: the in-process listener, transcription, shortcut, and update
  snapshot-and-action types shared by the Linux and Windows hosts, the window,
  and `hex status`.
- `rune_ui`: the one window on macOS, Windows, and Linux, on Rune with
  `rune-kit` and the OpenCode theme: General, Modes with mode-owned
  corrections and processing, Voice Action, opt-in Commands, Meetings (each
  feature owns its settings), History, and under Experiments the Voice Filter
  and developer-only HUD Lab and Activity, the transcription
  picker, the onboarding tour, the macOS meeting capsules, and the `--snap`
  fixture scenes. `platform::Capabilities` decides which panes and rows exist;
  `rune_ui/linux_host.rs`, `windows_host.rs`, and `macos_host.rs` connect it to
  each OS's host. Keep shared UI in the pane modules and only OS behavior in
  the hosts.
- `opencode_catalog`: the OpenCode model catalog as the window shows it:
  loading off the UI thread, the missing/failing notice, model names, search,
  and thinking variants.
- `status_item`: the persistent macOS menu-bar owner for Settings, Paste Last
  Dictation, meeting recording state and Stop & Transcribe, update checks, and
  orderly application shutdown.
- `macos_rune`: the macOS app as one menu-bar process on
  `rune::App`: the status item, the updater, main menu, Dock reopen Apple event,
  and HUD on the main AppKit loop, and a runtime thread for model
  preparation and the window's state. `rune_ui/macos_host.rs` is the
  window's host and the panes' `NativeHost`.
- `dictation_indicator`, `spring`: the click-through native Metal/AppKit
  capture and processing HUD (an `NSPanel` driven from `macos_rune`'s main-loop
  timer), with `spring` for its frame-rate-independent springs and Reduce
  Motion.
- `meeting`: explicit capture through `meeting_capture`, owner-only WAV and transcript
  artifacts, Screen Recording permission checks, final local-model publication,
  the user-facing Markdown copy, and recovery.
- `meeting_capture`: independent self-healing microphone (cpal) and system
  audio (Core Audio process tap, ScreenCaptureKit fallback) sources; one
  failing never stops the other, and resumed audio is padded onto the timeline.
- `meeting_transcript`: streaming 16 kHz resampling, short pause-aware
  overlapping chunks transcribed as background work on the dictation engine's
  loaded model (a process without the engine, such as the CLI, loads its own),
  seam deduplication, echo suppression, the rolling in-call transcript and its
  `TranscriptReader`, and Markdown rendering.
- `meeting_echo`: 20 ms energy envelopes of both tracks and the signal-level
  bleed/local classification of microphone segments.
- `meeting_notes`: a bounded provisional title while recording, then one
  post-publication call over a bounded transcript excerpt for the final title
  and a two-sentence summary, with deadline, manifest storage, and
  marker-delimited in-place Markdown updates.
- `meeting_chapters`: live chapters (experiment `live-chapters`): one
  on-device pass per ~3 minutes of rolling transcript while recording, with
  Rust deciding topic changes, the per-boundary fix-up after publication,
  chapter-extended titles that never touch a user-typed one, and section
  Markdown for Copy section.
- `meeting_query`: read-only meeting queries for agents (`meeting::query`):
  lock-aware listing, in-memory BM25 full-text search with stemming and fuzzy
  matching, transcript paging, stored notes, and Markdown rendering. It never
  writes to the meetings folder.
- `meeting_mcp`: the thin `rmcp` stdio adapter behind `hex mcp`, exposing
  `meeting_query` as read-only tools and `hex-meeting://` resources.
- `apple_intelligence`: the weak-linked FoundationModels Swift bridge:
  availability, context size, and guided-generation notes at utility priority.
- `microphone_activity`: permission-light CoreAudio process input observation.
- `meeting_detection`: pure provider classification, debounce, per-call snooze,
  and call-ended presence.
- `calendar`: calendar meetings on every OS as a pure model (which events
  count as meetings, call-link parsing, the wall-clock reminder scheduler),
  plus the macOS EventKit bridge (`calendar.swift`): access, the change
  counter, bounded event reads, and the Settings snapshot.
- `meeting_watcher`: the meeting controller loop: call detection, offers,
  calendar reminders, recording workers, call-ended presence, notes, and the
  menu-bar meeting state. The capsules that show its events are
  `rune_ui::meeting_capsules`.
- `file_dialog`: the one native open dialog for a file or folder (Modes'
  "Choose a file…", the macOS meetings folder): `NSOpenPanel` modeless on
  AppKit's main thread, `IFileOpenDialog` on its own STA thread, the portal on
  its own thread. Answers on a channel; cancel and a dialog that never opened
  read as nothing chosen.
- `experiments`: the compiled list of unsupported experiments and the
  launch snapshot of `HEX_EXPERIMENTS` and `~/.config/hex/experiments`
  behind `experiments::enabled` and `hex experiments`. `capture` exists
  only in builds with its build option.
- `capture_host`: Scrawl's capture engine (the `scrawl-capture` crate in
  `crates/capture`, copied from Scrawl's `engine/split` at `e351ddb` and
  kept out of the workspace so only `--features capture` on macOS builds it;
  `scripts/check` names it there) hosted in Hex for the `capture` experiment
  (`docs/plans/capture-in-hex.md`), compiled only as `cfg(hex_capture)`: a
  macOS build with the `capture` Cargo feature, a build option off in every
  public build (`HEX_CAPTURE=1 scripts/build-app.sh` bundles and signs
  `scrawl-helper` beside `hex`). Hex's `Host`,
  `ConfigSource`, and Parakeet or Apple `Transcriber` for the engine, the
  listener-owned `CaptureHost` behind `hex capture` and the capture shortcut,
  its 16 kHz gap-padded timeline handed to the engine as `AudioIn::External`,
  the tap's pointer input forwarded to `Capture::input()`, Scrawl.app
  coexistence, the retention thread, and the `CaptureSnapshot` every view
  reads (`capture_host::snapshot`/`watch`, published on each change, so a
  capsule never reaches into the host), with `last.route` from
  `capture_host::routing` (Scrawl's `crates/router` in-process: look, decide,
  undo window, send, Undo and Redirect) and `phone` from a `capture-phone`
  look for a cabled iPhone. `capture_host::library` reads the captures
  folder for `hex capture list|show|send|route`. `rune_ui/scrawl.rs` is its pane
  (Experiments › Scrawl) and `CaptureSettings` its `capture.*` settings.
- `instance`: the exclusive Application Support lock that prevents concurrent
  command listeners.
- `process_exit`: how the macOS app ends. Every quit path (menu, `hex quit`
  and `hex restart`, Ctrl-C, and AppKit's terminate) stops and joins Hex's
  workers, bounded by a 5 s deadline, kills any personal-command host left,
  then calls `_exit`, so C++ static destructors (ggml's Metal device,
  Moonshine's transcriber map) never run under live threads. A restart
  schedules its relaunch first; the new copy waits for the old pid to exit.
- `feedback`: bounded volume-controlled mode, capture, cancellation, and
  failure tones.

Keep modules deep. Callers should not coordinate Moonshine stream handles,
CoreAudio formats, AppleScript details, or event serialization.

## Invariants

- Every build starts dictation-ready without loading Moonshine or a command
  executor unless the persisted command opt-in is enabled. Enabling commands
  loads Moonshine off the audio-consumption loop; disabling them unloads the
  recognizer and executor.
- On a new Mac, release dictation starts only after Microphone, Input Monitoring,
  Accessibility, and the selected dictation model are ready. Opt-in command
  recognition additionally requires Moonshine, but its background preparation or
  failure must never block hotkey dictation. Failed preparation exposes explicit
  retry and disable actions without silently changing the command opt-in.
- Among recognized voice commands, sleeping mode accepts only standalone wake
  phrases. Dictation and explicit paste shortcuts remain available.
- Unmatched completed speech is ignored and logged.
- The dictation shortcut defaults to Option but supports modifier-only,
  modifier-plus-key, standalone Globe/Fn, and standalone function-key bindings.
  Capturing a new binding suspends global matching.
- GUI startup must successfully call `keyboard::initialize_layout` on the OS
  main thread before settings, keyboard-using workers, or Rune start. That
  snapshot is fixed until restart; cached misses must not re-enter TIS/TSM.
  Uncached headless lookups retain live-layout resolution, with all native
  lookup and snapshot construction serialized by the same lock.
- macOS launch order: `main` calls `rune::prewarm_gpu()` right after parsing
  the command line, only when it will run the app (`hex app` without Rune's
  headless flags, or the bundled executable with no command), never for a CLI
  command. A hidden or login launch prewarms too and Rune drops it: whether
  the window opens depends on settings, which load after the keyboard layout.
  `macos_rune` observes AppKit's will-terminate before the loop runs, so a
  terminate at any point drains workers. It sets up the status item, Dock
  policy, HUD, main menu, reopen handler, and pump timer once, after the
  window's first frame (`App::on_ready` queues it as a frameless `update_if`),
  or at most 1 s after the loop starts if no frame comes; a launch without a
  window sets them up in the loop's first turn. Nothing the window's first
  frame needs may wait on them.
- Another app's global shortcut (Carbon hotkeys, such as Spaceman's Option-1)
  consumes its key before the session taps. A listen-only HID tap reports key
  presses as chord evidence only: it discards an early hold or pending gesture
  like any unrelated key, never tracks key state, and skips Escape and Hex's own
  shortcuts. Its timestamps may be Mach ticks or nanoseconds.
- Stale shortcut recovery requires at least 100 ms of sampled keyboard neutrality,
  no pending input, and no hold or pending gesture. A lock is repaired only when
  it tracks a held key, and it stays locked so a fresh press can still finish it.
  It emits no capture actions.
  One exception skips the 100 ms wait: when tracked keys alone block a press of
  the binding itself (Idle, or Dirty for any binding; never Locked or a pending
  gesture), the press resynchronizes and counts as a fresh press only if a
  modifier-change event reported no modifiers after each tracked key's own
  press (typing in between is fine), only the binding's modifiers went down
  since, and every tracked key and every other modifier reads up in
  combined-session state (Fn metadata as below). It fences input stamped
  before the press, not at it, so a same-tick chord still discards, and keeps
  the binding's key safe from an older delayed release. Fenced edges, edges
  from before a disabled tap, and a suspension leave no modifier evidence.
  Any other press stays ignored and logs `shortcut press ignored` with its
  reason and key codes.
  Native `SecondaryFn` navigation metadata may be ignored only with timestamped
  modifier-change evidence that Fn is up; the full key scan and delayed-event
  fences remain required. Never strip Fn globally from shortcut matching.
- Hold the shortcut to dictate and release to transcribe. Captures shorter than
  300 ms discard. A 450 ms hotkey pre-roll and one-second voice-trigger pre-roll
  protect speech onset. Capture has no automatic duration limit; release,
  explicit stop, or Escape ends it.
- When enabled, a second shortcut tap within 300 ms locks dictation. Press the
  shortcut again to finish or Escape to cancel.
- When commands are enabled, every dictation or paste hotkey action resets
  Moonshine so shortcut audio cannot leak into a later command.
- Recording audio behavior and idle-sleep prevention begin only after the
  intentional-hold threshold. Ordinary shortcut chords must not mute output,
  pause media, or prevent sleep.
- A modifier-only shortcut (not Globe/Fn) passes through, so once Hex owns
  the press (hold past the threshold, Toggle start after the 40 ms chord
  grace, lock-finishing press) it posts one marked press of unassigned key
  code 0x44 at HID while exactly its modifiers are still down, so other
  apps' modifier-tap hotkeys read a chord. Hex's annotated-session key tap
  drops it, so the focused app never receives it (and never beeps). Short
  taps, chords, and presses during voice-delimited dictation never get one,
  and it never reaches Hex's state machines.
- The start tone confirms the shortcut press itself, not the hold threshold: it
  is provisional, sounds after a 40 ms chord grace, and a chord, short tap,
  cancel, or Voice Action takeover retracts it (silently inside the grace, else
  a 20 ms fade). With the microphone released while idle, the tone waits for
  the open microphone and the hold.
- `Release microphone while idle` and Commands are mutually exclusive through
  explicit confirmed transitions and a shared runtime policy. When release is
  enabled, pressing the shortcut opens the selected device asynchronously with
  no pre-roll, preserves the physical press for the hold threshold, discards a
  release before readiness, and closes only after the authoritative capture is
  idle. Accepted jobs never keep the stream open. A stream failure before the
  hold threshold returns the press to pending and reopens; later failures fail
  the capture. Cold opens retry the selected device briefly before falling back.
- Dictation remains available while command recognition sleeps.
- Model inference, optional post-processing, paste, and application actions must
  never block authoritative audio capture. Their queues remain bounded.
- CoreAudio capture timestamps and annotated-session CGEvent shortcut timestamps
  share the macOS boot-time nanosecond clock. Do not apply the Mach timebase to
  annotated-session timestamps. Physical HID-tap timestamps arrived as raw Mach
  ticks on the tested Apple Silicon system, while some HID events already used
  nanoseconds; changing tap location requires revalidating timestamp units.
  Delayed press handling reconstructs the original onset from the timeline;
  delayed release handling excludes audio captured after the physical release
  plus `POST_RELEASE_TAIL` (200 ms).
- A shortcut release or locked press that finishes a transcribed capture keeps
  audio through that exact event plus the 200 ms tail. The event timestamp is
  never moved; the hold threshold uses it alone. The audio owner waits at most
  the tail plus 100 ms for that audio, and a stream stall, failure, or gap in
  the tail finishes with the audio already delivered instead of failing the
  capture. Escape, short taps, voice controls, programmatic stops, and meetings
  get no tail. A press during the tail is handled after the old clip finishes.
- Active dictation capture is lossless with respect to Moonshine, event, UI,
  context, and worker stalls. Command recognition is explicitly best-effort:
  its backlog is bounded by duration, and pressure invalidates the generation,
  discards stale audio and updates, and resets Moonshine without touching the
  active recording.
- The dictation shortcut has a persisted `shortcut_mode`: `hold` (default)
  or `toggle`. Toggle reuses the lock state on every platform: each press acts
  as the second tap of a double-tap, so release locks and the next shortcut
  press finishes. Only Escape cancels, and a modifier pressed alone never ends a
  key-chord capture. Double-tap settings apply to Hold only.
- Streaming dictation is speculative on every OS: a hotkey capture's stream
  receives exactly the capture's 16 kHz audio, the finish always carries the
  complete clip, and a failed, empty, overfed, late, or interrupted stream
  falls back to the whole clip. Nothing pastes, and no partial text shows,
  before the final text. One engine thread runs every inference because a live
  stream holds the model's compute lease. Streams run only on GPU devices
  (Metal, Vulkan); models that cannot stream transcribe the whole clip after
  release. Dictation is never cut at pauses; meetings are.
- Starting a new capture never cancels accepted dictation work. Pending jobs
  preserve submission-order output while capture remains immediately available.
- Completing or pasting an older job must not reconcile or finish a newer
  capture. Shortcut boundaries come from delivered CGEvent timestamps; do not
  fabricate a release timestamp from later physical state.
- Escape cancels the active capture first, then the newest unfinished dictation.
  Cancelled jobs never paste, update the last result, or block later output.
- Escape never cancels a programmatic dictation; its owner cancels through the
  API, and the lease ends an abandoned one. While one records, Hex claims no
  Escape at all, even with other jobs queued.
- Model switches activate only after the pinned artifact is checksum-verified,
  loaded on strict Metal, and prewarmed. A failed switch preserves the active
  model and persisted selection.
- The menu-bar model picker lists only downloaded, available models and remembers
  each model's last selected language and recognition hints. It works without a
  Settings window. Settings and the menu bar share one preparation owner; a newer
  choice supersedes uncommitted work, and preparation/save failures leave the old
  selection intact. Menu-bar selections never silently download or repair a model.
- Cohere's advertised encoder input capacity is not its recommended inference
  window. Both GGUF adapters cap Cohere chunks at the reference processor's
  35 seconds (or a stricter advertised bound); capture duration remains unlimited.
- The microphone is either System default (`microphone` unset) or one saved
  device. System default follows the OS default input live: macOS through
  CoreAudio listeners on the default input and the device list (their callbacks
  only bump a counter, never on the audio callback), Windows through the
  default-input stream's change report, and Linux at the next stream open
  (ALSA's default resolves when it opens). Hex never captures from Hex
  Microphone: a Hex Microphone default stands in with the built-in microphone,
  else the first real input, logged and shown in Settings. A saved microphone
  that is missing or will not open falls back to the system default without
  changing the setting, and Hex switches back when it returns. Any switch
  (selection, default, or device list) happens only while capture is idle,
  after the current capture, and opening the replacement must succeed before
  the old stream is dropped; a device that resolves to the one already open is
  kept. An explicit CLI `--device` remains authoritative for that listener
  process and never falls back. A runtime stream failure cancels an incomplete
  capture (a programmatic one keeps the audio delivered before the failure for
  its owner's `/finish`, which answers `microphone-lost` when there is none),
  discards stale chunks, and retries with bounded backoff; it must not
  require a settings change or process restart. An initial open failure uses
  the same bounded recovery without stopping the listener. Cancellation does
  not stop warm-microphone retries; enabling release-while-idle stops them once
  capture is idle.
- Feedback volume is persisted from zero through one; zero disables tones.
  Volume changes apply immediately and preview one recording-start tone.
- Launch at Login uses `SMAppService` for the signed main app. Treat both
  `NotRegistered` and `NotFound` as disabled before registration, represent
  `RequiresApproval` with a link to Login Items settings, and poll macOS as the
  source of truth instead of persisting a parallel Boolean.
- Voice Action is an optional second capture target. Its saved shortcut defaults
  to Option-Command, but no shortcut is active or reserved until the user enables
  Voice Action. Disabling it cancels its active capture, not accepted jobs, and
  preserves the saved shortcut and model. The toggle remains reachable when
  OpenCode is unavailable. Promoting an active Option capture preserves all recorded
  audio. Selected text is optional prompt context; inaccessible or empty
  selections act as no selection. Use the dedicated OpenCode model and deadline,
  return only paste-ready text, and paste at the current focus. Failed, empty,
  cancelled, or timed-out actions paste nothing. Voice Action jobs share normal
  queueing and cancellation but never update the last dictation.
- Mode processing is best-effort and ordered: corrections, optional OpenCode
  rewriting, then selected transformations. A failed stage preserves its input;
  the transformation chain is one stage, so failure discards its partial results.
  It applies to Paste and Send, but not Voice Action or meetings.
- OpenCode availability checks stay off the UI thread. When the app finds the
  `opencode2` executable, `opencode2 api` discovers or starts the managed service;
  the catalog uses authenticated loopback HTTP to avoid large CLI pipe truncation.
  HEX never invokes
  `opencode2 serve --service` or owns that service's lifecycle directly. A
  missing beta install is retried at a coarse interval so installing
  `opencode2` while Settings is open refreshes the model catalog without
  restarting HEX. Catalog failures require an explicit retry rather than
  spawning clients in a tight loop.
- Ordinary resolver commands execute only from completed Moonshine lines.
  Voice-delimited activation and stable control phrases are deliberate exceptions.
- Contextual commands enter the candidate set only when their predicate matches.
- Literal and typed command patterns compile into one registry. Reject overlaps
  at configuration time and generate the catalog from that registry.
- Successful commands rely on their action for feedback. Wake/sleep use quiet
  tones; execution failures use an error tone.
- Clipboard insertion is the fixed text-insertion path. Restore the previous
  clipboard only if no newer paste or external clipboard change superseded it.
- Option-Shift-V pastes the last completed dictation. Option-Control-V pastes
  completed meeting turns added since the previous successful invocation.
- Do not persist captured audio by default. Explicit foreground meeting
  recording must remain visibly active. Diagnostic dictation retention is an
  explicit, bounded, owner-only opt-in through `HEX_RETAIN_DICTATION_AUDIO`.
- Retained dictation history records only successful pasted output: raw and
  final text plus bounded metadata, never audio, full browser URLs, or window
  titles. History defaults to seven days, and every retention window remains
  subject to hard entry and byte caps. No new entries are accepted while Off.
  Turning retention Off preserves existing entries; clearing history is explicit.
  Time-based retention expires during idle uptime through the shared history
  owner; searches never return expired entries while waiting for disk cleanup.
  The hard caps are 10,000 entries and 16 MiB of raw + final text (oldest evicted
  first), with existing per-entry metadata and text limits. There is no archive.
- `hex history` and `hex meetings` are read-only, release-available on macOS,
  independent of feature opt-ins. Dispatch before folder migration, process
  logging, the instance lock, local API, GUI, permissions, or Hex Microphone.
  Resolve the existing data folder without renaming it. History reads one open
  file through concurrent atomic replacements, applies the saved retention at
  query time, and reports malformed files/settings without repairing them.
  JSON contracts are documented in `docs/transcript-cli.md`; incompatible
  changes require a version increment. Debug `meeting list/show` use the same
  read-only path; recording and other debug commands keep their existing roles.
- Meeting detection may inspect process audio metadata but must not capture
  samples. Detection offers recording, and starts it automatically only for a
  provider (website or app) the user explicitly set to Always record in
  Meetings › Settings › Detected calls or an offer's "…"; every provider
  defaults to Ask. An automatic start passes the same stability time and
  snooze as an offer, so a stopped or dismissed call never restarts until its
  app leaves the microphone, and it shows as visibly as a manual recording. An
  offer that times out only closes. Release builds keep the meeting controller
  idle, without inspecting microphone activity, until the user opts in. When the
  detected call app leaves the microphone, offer `Keep Recording` or
  `Stop & Transcribe`; never stop automatically. An active recording stays
  visible and stoppable from the menu bar without the Settings window.
- Calendar access is asked for only by Meetings › Settings' Connect calendar, never at
  launch, and only with Meetings on; Meetings off means no EventKit reads and
  no reminders. `meetings.calendar.connected` defaults off. A meeting has other
  attendees or a call link and is not all-day, declined, free, or cancelled.
  Reminders are Hex capsules, not notifications, shown once per occurrence at
  the lead time (or on a wake within 5 min of the start) and withdrawn when the
  meeting is cancelled or moved. One reminder shows at a time, never over a
  recording. EventKit is read on the controller thread on store changes or once
  a minute and on Settings' reader thread, bounded, with no network, never on
  the audio or UI thread; nothing touches the store before Connect. A reminder
  never records without a click; Always record still needs the detected call.
  Recording or dismissing a reminder snoozes the same call's offer and
  automatic start; without a call link, only the next call near the start.
- Meeting transcription defers to dictation and never loads a second copy of
  the model: each chunk runs on the dictation engine's loaded model at utility
  QoS, starts only while no dictation capture (from the shortcut press) or
  dictation inference is active, never interrupts a live stream, and is retried
  when a dictation request was sent first. A chunk already running cannot be
  aborted on Metal; it finishes during the hold threshold and release tail.
  Chunks are short (first pause after 6 s, hard cut at 10 s of new audio plus
  2 s overlap), so a running one
  finishes well within the hold threshold. It uses whatever model
  dictation has loaded, so a model switch applies from the next chunk; Apple
  Speech and a process without the engine use their own transcriber. Its queue
  (one-second audio blocks) and chunk backlog stay bounded; a source that
  falls behind skips its oldest audio, keeps going, and is rebuilt from its
  WAV after Stop. A capture gap restarts that source's chunks after the gap.
- Quitting while a meeting records stops capture and saves its WAVs and
  manifest (`Interrupted`, `final.pending`) without transcribing; once
  quitting begins, no meeting work starts a chunk or loads a model. The next
  launch with Meetings on runs the final pass on the dictation engine's model
  (or its own after 30 s without an engine).
- Meetings run no Moonshine. Every in-call view and copy (the Meetings pane,
  Option-Control-V, provisional titles, `hex meetings`, and MCP) reads the
  rolling Parakeet transcript, which publishes only entries no later chunk
  can replace; after Stop they read the final transcript.
- Meeting system audio uses a Core Audio process tap (System Audio Recording
  consent), never requiring a display; ScreenCaptureKit is only the fallback.
  Losing either source never stops the other or the meeting: it retries until
  Stop, and resumed audio is padded with silence so WAV positions stay on the
  shared capture clock.
- Meeting recording prevents idle sleep but does not apply dictation mute or
  pause-media behavior.
- Apple Intelligence meeting notes run only after final publication, on a
  utility-QoS thread that yields to dictation, within a bounded chunk count and
  total deadline. Failures record `notes_error` and leave the Markdown
  untouched. Notes never rename the file and rewrite only the heading HEX wrote
  and the `<!-- hex:notes -->` block.
- Captures (the `capture` experiment) are a build option, off in every
  public build: only a macOS build with `--features capture` (debug, or
  release through `HEX_CAPTURE=1 scripts/build-app.sh`) has them. Any other
  build does not know the experiment, never compiles `scrawl-capture`, and
  refuses `scrawl-capture` with `unsupported`; Linux, Windows, and Nix never
  compile it. The Release and test-build workflows never pass the feature;
  `release-app.sh` refuses `HEX_CAPTURE` and `validate-app.sh` refuses a
  bundle with `scrawl-helper` or the capture engine. With the experiment
  off, the listener builds no capture host: no engine, sink, thread, bundle
  folder, or work, and the same holds with the experiment on while
  `capture.enabled` (the Scrawl pane's opt-in, default off) is off. Scrawl's engine
  records the screen, runs with `SharedResources::none()` (Hex never calls
  `install_shared_resources`: no ingest server, ingest token, or global
  shortcuts), logs to tracing, and writes only to the captures folder
  (`capture.folder`, Scrawl's own by default) and Scrawl's router folders. A
  written capture is copied to the clipboard (never pasted) and, with
  `capture.routing`'s autoSend on, routed by Scrawl's router in Hex's process
  (`capture_host::routing`): one `capture-look` thread per stop, one
  `capture-route` thread with a bounded queue, a rule's send held 3 s for
  Undo, guesses sent only when picked. Queued sends retry only while neither
  Scrawl.app runs nor anything serves the ingest port, and never in tests.
  `capture.routing` never holds a Jev key. A cabled iPhone records through
  `scrawl-helper mirror --no-microphone`, narrated by the dictation timeline;
  only `HEX_CAPTURE=1` bundles carry `NSCameraUsageDescription`, and
  `validate-app.sh` refuses a release bundle with it. Hex's capture
  shortcut (default ⌥⇧S, a key required, never overlapping another binding)
  is a fresh press: idle, it starts once one of its modifiers comes up (Esc
  before then records nothing); recording, it stops; with the capsule's
  picker open, the picker records. A switch chip discards the recording
  (no outcome) rather than retargeting it; while Scrawl.app
  (`dev.kitlangton.scrawl`) runs, Hex registers no capture shortcut and runs
  no retention, and only the pane's "Use Hex for captures" quits Scrawl.
  Escape never ends a recording capture, and Hex's tap lets it through to
  the app being recorded; discarding is the capsule's two-step ✕ (✕, then
  Discard). Pointer
  events and Shift reach the engine from the one session tap, only while a
  capture records, through a bounded queue that drops rather than block; they
  never reach the listener. Scrawl's settings are imported once, read-only;
  Screen Recording is asked at the first capture or from the pane, never in
  onboarding. The capsule, picker, and ink are left out of every screen
  recording, and a capture leaves out all of Hex's chrome, unless
  `capture.show_overlays_in_recordings` (default off; the pane's Show in
  screen recordings or the menu bar's checkmark) is on; it applies at once
  to open overlays and a recording capture, never recreating a window. A
  capture never opens a
  microphone, installs a tap of its own, or loads a model: it copies the dictation
  timeline through a bounded sink that records a gap rather than block, so a
  stalled capture never stalls dictation, and while a sink is open the
  release-while-idle close waits for it. While a capture records it holds a
  recording-environment session with the same "While dictating" policy
  (`recording_audio_behavior`, read at each start; no capture setting),
  released at stop, cancel, or quit; it composes with dictation's,
  so neither restores what the other still holds. One capture engine per
  process, one capture recording at a time. Its words are foreground requests on the
  dictation engine's loaded model in 30 s chunks: after dictation already
  sent, before meeting chunks, never a second model, and dictation output is
  unchanged. A chunk waits while dictation captures or transcribes and never
  interrupts a live stream; the engine holds it until the stream ends. `stop`
  replies at once and the screen recording stops then (`Capture::stop`), not
  when the capture is written; one thread writes stopped captures one at a
  time, at most two wait, and a start beyond that is refused `busy`. The
  engine's log goes to tracing from before it is built, and its settings come
  from Hex (`ConfigSource`), never `~/.config/scrawl` after the import. Bundles are
  owner-only. Quitting
  cancels a recording capture, stops transcription between chunks, and waits
  at most 2 s for capture threads.
- `hex mcp` is read-only and available in release builds regardless of the
  Meetings opt-in. It dispatches before process logging, the instance lock,
  GUI, or permission setup; never writes, recovers, or indexes on disk inside
  the meetings folder; runs no model; and makes no network request. Stdout
  carries only MCP messages.
- `hex` control commands (`status`, `quit`, `dictation`, `meetings record`,
  `settings set`, `model use`, `window show`, …) are release-available and
  drive only the one running Hex through the owner-only discovery file and
  bearer-authenticated `POST /control` (64 KiB request, 1 MiB reply, 10 s
  in-app deadline). They dispatch before folder migration, process logging,
  or the instance lock, and never launch Hex; only `hex start` and
  `hex restart` open it. Every command is carried out by the app's own paths
  (the menu bar's meeting request, the window's settings save followed by
  `ReloadSettings`, the shortcut state machine), so CLI-started recordings
  look and stop exactly like the UI's and opt-ins stay explicit settings.
  Commands are idempotent (`changed: false`). `settings::every_persisted_setting_has_a_cli_key`
  fails when a persisted setting has no `hex settings` key; a protocol change
  that breaks either shape increments `CONTROL_VERSION` and `docs/cli.md`.
- Meeting echo is judged on one timeline built from WAV sample positions, never
  from packet arrival. A `You` line or rolling entry is judged only once the
  system audio that could explain it has arrived (bounded when the system track
  stalls). Measure echo changes with `meeting_echo_replay` on a recorded
  speaker-mode call, counting genuine `You` lines lost as well as echo kept.
- Moonshine sessions run single-threaded (`MOONSHINE_ORT_SINGLE_THREAD`, set
  first thing in `main`); ONNX Runtime's default spinning pools cost several
  times the CPU of the decoding itself.
- The rolling transcript remains available if final transcription fails. Final
  publication is atomic; adjacent same-source entries within three seconds form
  one displayed turn labeled `You` or `Them`. Speaker echo never shows as `You`:
  the rolling snapshot, live paste, and final transcript drop a microphone
  utterance when delayed system audio explains its energy, or when its words
  repeat overlapping remote speech unless the signal shows local speech
  (double-talk). Each finished
  meeting is also written as Markdown to the configured folder (default
  `~/Documents/Hex Meetings`) without overwriting existing files; an export
  failure does not fail the meeting.
- Meeting paste cursors advance only after successful insertion and reset when
  HEX restarts. They track, per source, the end of the text already handed
  out, so rewrites near the live edge and the switch to the final transcript
  neither repeat nor drop turns.
- Deleting a meeting moves its exported Markdown and then its recording folder
  to the Trash (never `rm`), after a two-step confirmation, and is refused while
  it records, transcribes, or writes notes. Renaming rewrites the manifest and
  only a Markdown heading that still reads the previous title; files keep
  their names.
- Visible state remains `Transcribing` until every accepted transcription or
  paste output has completed, including ordered post-processing and insertion.
- The HUD is observational and click-through. It must not alter capture
  boundaries, steal focus, or block controls in the foreground application.
- Desktop UI is governed by [`docs/design-system.md`](docs/design-system.md);
  read it before changing any pane, capsule, or picker. Rune is the only UI
  toolkit: compose `rune-kit` components and Hex's pieces in
  `rune_ui/controls.rs` and `rune_ui/lists.rs`, and name only the OpenCode
  theme's tokens, never colour literals. Use sentence case and the product name
  "Hex" in all copy. Reserve the accent (Primary) fill for at most one main
  action per screen; states such as recording or an available update use a
  Secondary button with a status dot. Extend a piece instead of adding a
  parallel helper, and verify visual changes with `--snap` scenes in light and
  dark.
- Neither Hex nor Rune (`anomalyco/rune`, public MIT) is published to
  crates.io; never run `cargo publish`. Hex depends on `rune`, `rune-kit`,
  `rune-theme-opencode`, and (with `capture`) `rune-motion` at one pinned Git
  rev in `Cargo.toml` and `Cargo.lock`; every pin bump
  must also update `cargoLock.outputHashes."rune-0.0.1"` in `nix/package.nix`
  and keep `windows` 0.62.2 for `gpu-allocator` in `Cargo.lock`.
- At the start of each work round, bump Hex's Rune pin to Rune's `dev` head:
  read the `## dev` section of Rune's `CHANGELOG.md` since the current pin,
  adapt to its breaking changes, update `nix/package.nix`'s outputHash, and
  run `scripts/check rune_ui`, goldens included. If Rune itself breaks, report
  it to Rune rather than working around it.
- Hex runs as one long-lived process per user and data root on every OS: a
  menu-bar process on macOS (`macos_rune`, native `NSStatusItem`, native Metal
  `NSPanel` HUD, Rune meeting-capsule overlays), a tray process on Windows
  (`windows_rune`, named-event single instance, Rune overlay HUD), and a
  single-instance process on Linux (`linux_app`, socket + lock in the data
  root, Rune overlay HUD). Closing the window never stops the runtime, listener,
  HUD, or accepted work.
- Updates follow the release protocol's client rules exactly (the shared
  vectors must pass). Versions are display text and never compared: each
  channel has one current release, and Hex takes it whenever it is not the
  running build (Linux and Windows: the executable's SHA-256 against the
  target's; macOS: the exact version). A rollback is the service making an
  earlier release current. `required` releases install without waiting for
  the rollout; others wait for a rollout bucket drawn once from the OS's
  secure random source, kept in `<support>/update-bucket`, and never sent.
  Each build knows the channel it was published on (`HEX_BUILD_CHANNEL`,
  stamped by Release); when the Update channel setting names another, Hex
  offers that channel's current release as "Switch to Stable 2.3.1" and
  installs it only once the user confirms, never by itself. Dev builds are
  `0.0.0-dev-<run number>`, as OpenCode numbers them. Requests carry nothing about the install. Checks run 10 seconds
  after launch, hourly (every 10 minutes and on window focus, at most once a minute, on the Dev channel), and on the sidebar footer's Check for updates, backing off on errors; manifests are
  not signed, so nothing runs before the download's size and SHA-256 match
  the manifest and, on macOS and Windows, its code signature passes: macOS
  `app/update-requirement.txt` (Developer ID team `5NZ4Q7NXJ4`, `anomaly.hex`,
  notarized), Windows a valid Authenticode signature whose signer is
  `AUTHENTICODE_SIGNER`. Linux relies on HTTPS and the SHA-256 alone.
- Only direct installs replace themselves: Linux and Windows with our
  installer's receipt (`install.json` in the user-owned data root, which also
  names the channel) and the version layout, macOS a `Hex.app` this user can
  rename in a writable, non-translocated folder. A read-only macOS location
  shows Download update (the DMG); package-managed installs (pacman, Nix) show
  their manager's command once it serves another version; source and test
  builds never check; `DisableUpdates` (macOS `anomaly.hex` defaults domain,
  Windows `SOFTWARE\Policies\Anomaly\Hex` in HKLM or HKCU) turns checks off.
  Never escalate privileges, and never overwrite development, root, or
  package-manager-owned files.
- A staged update waits for Restart to update. Linux and Windows activate a
  new version directory and switch `current`/`current-version` atomically at
  staging; macOS unpacks beside the bundle and swaps it in with
  `renamex_np(RENAME_SWAP)` at restart or quit, and the new version removes
  the old bundle at launch.
- The Wayland beta requires compatible clipboard, virtual-keyboard, and
  layer-shell protocols. Shortcuts come from the GlobalShortcuts portal when the
  desktop offers it; otherwise the user binds the `hex dictation`,
  `hex voice-action`, and `hex paste-last` commands in the desktop's own
  keybindings. One general path: no compositor-specific code, no raw input
  devices, no input-group requirement, and no silent fallback to XWayland or
  privileged input injection. The desktop may grant a different trigger or
  refuse one; Settings shows what it granted. Escape is bound only while
  something records. Modifier-only shortcuts stay unsupported on Linux.
- Every Linux shortcut source (X11 grabs, portal presses, `hex` commands) feeds
  the same `linux_input` state machine and emits the same events. Commands are
  idempotent: starting while recording or stopping while idle does nothing.
  They talk only to a running, listening Hex and fail with a clear error
  otherwise; they never start one. The socket hands them straight to the
  listener's queue, never through the window's thread.
- One Linux Hex process per user and data root owns the microphone, model
  workers, saved settings, History, the HUD, and the window. The window is
  optional and opened on demand; closing it never stops capture, accepted
  work, or the HUD, and cancels only an uncommitted shortcut capture. A
  relaunch forwards `Activate` (with any activation token) and exits 0;
  hidden and autostart launches never show the window. With the managed unit
  loaded and the default data root, launches start Hex through systemd
  (`Type=notify`), which owns its lifetime and restart; a crash restart opens
  no window. Otherwise the launching process becomes Hex. The socket stays
  owner-only and peer-UID checked, with bounded framing, queues, and I/O, and
  carries only `Activate`, shortcut commands, `Status`, and `Quit`. Window
  failures (no display, no GPU) are logged and never stop Hex. The unit's
  `ExecStart … service`, `hex app`, `hex app --hidden`, and `hex start` stay
  valid entry points forever: self-updates replace only the executable.
- On Linux and Windows, saved settings apply to the running listener in place
  while no dictation records; a settings change never stops listening or
  changes the shown status. Shortcut changes register the shortcuts again,
  a microphone change reopens the stream, and a model or recognition-hint
  change loads the new model beside the old one and swaps it in once no
  transcription is pending (`local_transcriber::ModelSwap`). Recording a
  shortcut in the window suspends only global matching.
- Linux shortcut capture uses focused input in the window. Recording a
  shortcut suspends the running listener's global matching and registers the
  shortcuts again when it ends; cancellation restores the old binding unless
  its save already committed. The window and the CLI must expose Hex's state, recovery, and
  errors. `hex quit`, the window's Quit Hex, and SIGTERM drain workers before
  exit. HUD teardown only hides that listener's HUD.
- Linux paste keeps transcripts off helper argv and bounds helper I/O. X11 waits
  for physical modifiers without discarding accepted output, and shutdown
  cancels that wait; on Wayland, `wtype`'s virtual keyboard carries its own
  modifiers, so held keys never change its Ctrl+V. New installs use Ctrl-V;
  the terminal-paste preference selects Ctrl-Shift-V and defaults on for
  persisted legacy X11 settings. The beta retains
  the transcript clipboard; arbitrary MIME restoration and consumption
  acknowledgments remain unimplemented.

## Development

### TypeScript SDK Releases

The public TypeScript SDK is `@kitlangton/hex` in `sdk/typescript`. The first
release, `0.1.0`, was bootstrapped manually; subsequent user-facing SDK changes
must include a Changeset under `sdk/.changeset/` unless preparing an initial
unpublished package. Run package commands from the `sdk` workspace root.

Before release, run:

```sh
cd sdk
bun install --frozen-lockfile
cd typescript
bun run check
bun run test
bun run build
npm pack --dry-run
```

When exports or packaging change, install the packed tarball in a clean consumer
and import both `@kitlangton/hex` and `@kitlangton/hex/effect`. The release
workflow is `.github/workflows/release-typescript.yml`; npm trusted publishing
authorizes `anomalyco/hex` and that exact workflow filename for `npm publish`.
The workflow uses npm trusted publishing without requesting a Sigstore
provenance bundle.
Use the configured Changesets release command (`cd sdk && bun run release`), not
direct `npm publish`, after the bootstrap release. Pending changesets on `dev`
keep a version pull request open into `dev`; the workflow publishes only when
`production` is fast-forwarded to an unpublished SDK version.

```sh
./scripts/setup.sh
./scripts/setup-parakeet.sh
bun run --cwd sdk/commands build
cargo run -- listen
cargo run -- status --json   # asks the running Hex (docs/cli.md)
cargo run -- dev dashboard   # debug recognition TUI
cargo run -- meeting record --title "Design sync"
cargo run -- meeting probe
cargo run -- mcp --meetings-dir tests/fixtures/meetings
cargo run -- app -- --list-scenes
cargo run -- app -- --snap /tmp/hex-settings.png --scene mac-settings
cargo run -- app -- --snap /tmp/hex-modes.png --scene modes --theme light
cargo run -- app -- --snap /tmp/hex-onboarding.png --scene mac-onboarding
./scripts/install-app.sh
scripts/check            # fmt, clippy, every test
scripts/check meeting::query  # fmt, then only matching tests
git diff --check
./scripts/test-app-identity.sh # macOS only
./scripts/test-install-linux-release.sh # x86_64 Linux only
cd sdk/typescript && bun run check && bun run test && bun run build
```

Unfinished features land on `dev` dormant behind an experiment
([docs/features/experiments.md](docs/features/experiments.md)): add a variant to
`Experiment` in `src/experiments.rs` and gate where the feature's work starts
(worker, thread, capture), not just its UI. Turn one on for yourself with
`hex experiments enable <name>` or `HEX_EXPERIMENTS=<name>`; it applies at the
next launch. Experiments are unsupported and never on by default, in debug
builds too; remove the variant when the feature ships or is dropped.

Use `scripts/check [filter]` for the local loop, not raw `cargo test`. It runs
tests through cargo-nextest (`brew install cargo-nextest`) with
`.config/nextest.toml`, which kills a hung test and reports it by name; CI uses
the same config's `ci` profile. Read its output whole rather than piping it
through `grep | head`; failures are summarized at the end.

Use Rune's headless renders as the desktop-UI iteration loop: `hex app --
--snap <png> --scene <name>` draws one named fixture scene from `SCENES` in
`src/rune_ui.rs` without starting the runtime, on every OS (`--list-scenes`
lists them; `--theme light|dark`, `--size WxH`, and `--scale` adjust the
render). Scenes cover every pane and state the goldens check: the Linux and
Windows panes (`modes`, `voice-action`, `history`, `picker`, `onboarding`,
`windows-settings`, …) and the macOS ones (`mac-settings`, `mac-commands`,
`meetings*`, `mac-voice-filter*`, `mac-hud-lab`, `mac-activity`, `mac-onboarding*`).
`live` renders the running Linux Hex's real state. On a host without a GPU,
render on lavapipe with `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`.
`cargo test rune_ui` compares the light and dark goldens in `tests/golden/`.
A scene is a fixture, not native end-to-end proof; the meeting capsules and
HUDs have no scene.

The supported Linux beta and release host use x86_64 Arch Linux. Install its
native build dependencies with:

```sh
sudo pacman -S --needed base-devel git rustup python alsa-lib curl jq openssl xxd \
  util-linux glib2 libxkbcommon libxkbcommon-x11 libx11 libxcb openblas \
  vulkan-headers vulkan-icd-loader \
  shaderc spirv-headers clang cmake pkgconf wl-clipboard wtype
rustup default stable
```

For a source install, run `scripts/install-linux.sh`, then `hex model install`.
The installer owns the user-local version layout, desktop entry, and autostart
entry; only that managed layout participates in automatic updates.

`nix develop` supplies the same native build environment as the Nix package;
`nix flake check` checks modules, shell configuration, session readiness, and
the display-free Rust suite. See `docs/nix.md`. Nix owns package updates and
optional Home Manager autostart. Native Linux PR checks do not replace real
X11/Wayland, portal or keybinding shortcuts, microphone, and target-application
smoke tests.

Microphone selection is System default (the OS default input, live) unless a
saved microphone is connected; override everything with `--device`. The app
bundle build requires Xcode 26 for Icon Composer compilation and a Developer ID
signing identity. `scripts/release-app.sh prepare` builds the notarized and stapled
DMG and ZIP the Release workflow publishes. Hex publishes every platform to
the Anomaly release service (`anomalyco/release`, `https://anoma.ly/release`;
its dev stage, `dev.anoma.ly/release`, only tests the service itself, and Hex
never publishes to or reads from it) from the Release workflow with curl and its
GitHub OIDC token, under app `hex`, artifact `desktop`;
[`docs/plans/release-service.md`](docs/plans/release-service.md) pins the
target and file names and the direct-install receipt, and
[`docs/releasing.md`](docs/releasing.md) owns the workflow, channels, secrets,
and old clients. The 2.3.1 feeds and appcast at `https://anoma.ly/hex/files`
(and Kit's `downloads.hex.kitlangton.dev`/`.com`, which must keep redirecting
there) stay frozen. GitHub app releases use `v<VERSION>` tags (`app-v…`
through 2.3.1) with `--latest=false`, keeping them distinct from SDK
releases. Never overwrite a published artifact with different bytes.
`https://anoma.ly/hex` (in-repo `site/`) is the only Hex website. One
`site/cloudflare.config.ts` defines the production Worker `hex-production-site` (anoma.ly/hex,
bucket `hex-production-files`) and the dev Worker `hex-dev-site` (dev.anoma.ly/hex,
bucket `hex-dev-files`); `cf deploy --prebuilt --mode <mode>` picks one, and
only CI deploys: a push to `dev` touching `site/` deploys dev, a stable
Release deploys production. No subdomains. The site's download links come
from the service's stable manifest when it builds. After publishing a version,
bump the explicit GitHub mirror/version link in `README.md`; the Release
workflow updates the Homebrew cask. `bun run --cwd site check:downloads` fails until every download link
serves a file; every deploy runs it. Verify the live links afterwards.
The Rust app is named `Hex`, packaged as `Hex.app`,
with bundle identifier `anomaly.hex` (deliberately not reverse-DNS) and executable
`hex`; its SDK helper is `anomaly.hex.service` and the virtual microphone driver
`anomaly.hex.microphone.driver`. Releases through 2.3.1 were `com.kitlangton.hex2`,
signed by Kit's team `QC99C9JE59`; macOS treats `anomaly.hex` as a new app (fresh
permissions, login item, and preferences domain), while the `hex` Application
Support folder, and with it settings, history, and models, is shared. Signing requires
an explicit `VOICE_CONTROL_TEAM_ID` and matching `HEX_NOTARY_PROFILE`; the build
and release scripts verify the selected team rather than assuming an Anomaly team.
`scripts/validate-app.sh` checks this identity before preparation and publication.
The move to Anomaly's team (`5NZ4Q7NXJ4`) is planned in
`docs/plans/macos-signing.md`; the manual `macOS signed build` workflow signs
and notarizes an unpublished candidate once the Apple secrets are in the
`release` environment.
`scripts/release-app.sh prepare` leaves `Hex-<version>-macos-arm64.dmg` and
`Hex-<version>-macos-arm64.zip` (root `Hex.app`, the updater's archive); both
contain the same signed, stapled app. Do not reuse a released version for
changed code. Macs on 2.3.1 or earlier (Sparkle, `com.kitlangton.hex2`) get
one informational appcast item, `packaging/macos/legacy-appcast.xml`, that
links to the new app; they never auto-update into it.
There is no Swift migration: never import Swift preferences or data, adopt its
bundle identifier, publish Rust artifacts to its S3 feed, or automatically quit,
delete, or replace the Swift app. Prefer a website-only informational item in
the legacy Sparkle feed, with no enclosure, pointing to the new app for manual
installation and fresh setup. Publish it only after the new artifact is live. See
`docs/plans/swift-app-handoff.md`. The Rust data root remains unchanged, but the
new app identity requires fresh macOS permission grants. Validate signed
distribution with the selected signing team before publishing an app update.

`SMAppService` is meaningful only from a signed app installed in
`/Applications`. When replacing a local bundle outside Finder during a login-item
smoke test, register the new bundle with Launch Services before launch:

```sh
'/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister' \
  -f /Applications/Hex.app
```

Pass the actual installed Rust host path to `lsregister` if it was renamed.

Verify both registration and unregistration with `sfltool dumpbtm`, and restore
the user's original login-item state after the test. Do not add release test
hooks or persist a second launch-at-login flag.

Every platform is built, signed, and published by one run of
`.github/workflows/release.yml` ("Release"; see `docs/releasing.md`).
Run it with `workflow_dispatch` (`ref`, `channel`: `check` builds and
verifies without publishing, `dev` publishes the dev channel, `stable` the
public release; dispatched from `production` only). Tags never start a
release: like OpenCode, the stable run tags `v<VERSION>` (the crate version)
on the released commit only after every platform published, then creates the
GitHub release with the built-in `GITHUB_TOKEN`, so a failed run leaves no
tag; `v*` tags are unrestricted because nothing triggers on them. Like OpenCode's release, build jobs (Linux x86_64 in an Arch
container, Windows x64 and ARM64 through `.github/workflows/build-desktop.yml`,
macOS as an ad-hoc signed `HEX_RELEASE_UNSIGNED=1` app, installers through
`windows-installers.yml`, AUR and Homebrew staging) run repository code with
no environment, secret, or id-token; sign and publish jobs run in `release`,
check out only `.github/release/`, and run only those scripts and SHA-pinned
actions on artifacts treated as data (macOS Developer ID signing,
notarization, and stapling; Azure Authenticode; the release service through
OIDC; the AUR and Homebrew pushes). Every artifact carries a `SHA256SUMS`
whose digest its job reports and every consumer checks. Keep that boundary:
never add a step that builds, installs dependencies, or runs repository code
to a job that holds a secret or an OIDC token, and pin every action by SHA.
`.github/` changes need Dax (CODEOWNERS and the "release boundary" ruleset).
`.github/workflows/test-builds.yml` calls the same desktop
workflow on every push to `dev` with `test-build`, plus an ad-hoc signed
`Hex Test.app` (`HEX_TEST_BUILD=1 scripts/build-app.sh`: bundle ID
`anomaly.hex.test`, its own `hex-test` data folder and `~/.config/hex-test`;
it must never read the real `hex` data), as unsigned 14-day artifacts for
teammates (`docs/test-builds.md`). `HEX_TEST_BUILD=1` builds never update
themselves. Every push to `dev` (except docs/site-only changes) publishes a dev-channel
build (the service lets a run from `dev` publish only the `dev` channel, and
only signed platforms are published). Stable runs only from `production`.
Like OpenCode, every run uses the one `release` environment, with no
required reviewer; its deployment policy admits `dev` and `production`. Rerunning
after an interruption is safe: the service stores files by hash and accepts
the same version again.

`Hex-Setup-<version>-windows-<x64|arm64>.exe` is a per-user Inno Setup
installer (`packaging/windows/hex.iss`, built by
`scripts/build-windows-installer.ps1`) that installs exactly the managed
layout `install-windows.ps1` creates, so self-updates keep working:
`%APPDATA%\voice-control\versions\<version>\hex.exe`, `current-version`, the
`install.json` direct-install receipt, and a Start Menu shortcut carrying the
managed mark. Its uninstaller lives in `voice-control\uninstall` because
updates prune old version directories; it removes every version, the
receipt, the shortcut, and Launch at login, and keeps settings. It installs
its version over whatever is installed (versions are never compared). Keep
its `AppId` forever: the updater rewrites that Apps & Features entry's
version. Release and the Windows installer workflow install, reinstall,
install over another version with, and uninstall
it silently on x64 and ARM64 runners (`scripts/test-windows-installer.ps1`).
Release's `sign-windows` job Authenticode-signs both `hex.exe` through Azure
Artifact Signing (GitHub OIDC, SHA-256, RFC 3161 timestamp), the installers
are built around the signed bytes, and `sign-windows-installers` signs them
(`.github/release/verify-authenticode.ps1` checks both). Compile jobs and test
builds never hold signing credentials; until the Azure values exist in
`release`, dev and check runs have no Windows targets. See `docs/windows-signing.md`.

Run `scripts/build-linux.sh` on x86_64 Linux and `scripts/build-windows.sh`
from Git Bash once per architecture with `HEX_WINDOWS_ARCH=x86_64` (the
default; Vulkan SDK required) or `aarch64` (clang-cl and Ninja; best on an
ARM64 host). Builds use `-DGGML_NATIVE=OFF` so the CPU kernels stay portable,
and the static runtime in `.cargo/config.toml` keeps the Visual C++
Redistributable unnecessary; the release workflow asserts neither Windows
executable imports it. `scripts/install-linux-release.sh` and
`scripts/install-windows.ps1` (served at `https://anoma.ly/hex/`) install from
the service's stable manifest and check each file's size and SHA-256;
`scripts/test-install-linux-release.sh` exercises the Linux one against a
fixture service.

Use `termctrl` to verify TUI changes at both wide and narrow dimensions. The
`hex dev dashboard` keys are `1` for commands, `2` for the activity log, `3` for meetings,
Tab to cycle, and `q`/Escape to quit.

## Diagnostics

- `~/Library/Application Support/hex/logs/live.ndjson`: state,
  transcript, command decision, outcome, processing, and context.
- `~/Library/Application Support/hex/logs/process.log`: Rust,
  CoreAudio, Moonshine, and context-adapter diagnostics.
- `~/Library/Application Support/hex/meetings/`: manifests, separate
  tracks, the rolling in-call transcript, and atomically published final
  transcripts.

Older macOS builds used `voice-control`; the first launch of a newer build
renames it to `hex` and leaves a `voice-control -> hex` symlink. If the rename
fails, Hex keeps using `voice-control` and logs why. Windows and Linux keep
`voice-control`.

When diagnosing a missed command, inspect both logs and distinguish microphone
capture, transcription, command mode/dictation, context matching, command resolution,
and action execution before changing aliases or thresholds.

## Future Direction

See `ROADMAP.md`. Do not add hypothetical seams for roadmap items. Introduce a
seam once there are two real adapters or a current test requires substitution.

## Current Work

`ROADMAP.md` is the authoritative work list. Keep these constraints visible:

- Release freeze (Kit, 2026-10-22): publish no Hex release on any OS, stable or
  staging-to-stable, and don't move `production` (it stays at `app-v2.3.1`),
  until Kit lifts it after the Rune front end has been tested thoroughly on
  real Macs and Windows PCs. Test builds are fine.
- Validate public onboarding from a clean macOS account and signed Linux updates
  on the supported Arch/i3 host.
- Validate native Wayland on a real compatible compositor, including physical
  portal shortcuts, the `hex` commands from a desktop keybinding,
  focus/click-through, target paste, and tray-less shutdown.
- Add a second real browser adapter before generalizing browser context.
- Do not turn concrete macOS, Linux X11, or command modules into hypothetical
  platform or plugin frameworks.
