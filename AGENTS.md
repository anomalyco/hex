# Hex Agent Guide

## Private Kit Tool

`dev` and `production` take changes only through pull requests (Dax's
ruleset; `.github/` changes also need Dax). Branch from `dev`, open a PR to
`dev`, and merge it yourself once its checks pass; merge any open PR without
asking. After merging, update the installed copy. See
`~/code/control/AGENTS.md`. `production` holds what was released and moves
only by a fast-forward from `dev` when Kit releases; never commit to it.
There is no `main`.

Any change to what the window draws regenerates the goldens in the same PR:
run `HEX_UPDATE_GOLDENS=1 cargo test --locked --bin hex rune_ui`
on Linux (the golden tests only run there; from a Mac, dispatch
`check-linux.yml` on the branch and take its `golden-actual` artifact),
look at every changed image, and commit them with the change.

`dev` is canonical and contains the Rune front end on every OS; GPUI is
removed. The `rune-ui` integration branch is merged and kept only for history:
branch from and push to `dev`.

## Purpose

Build Hex as a local, observable macOS voice appliance with explicit Linux
X11, wlroots-compatible Wayland, and Windows (x64 and ARM64) beta contracts. Keep the engine native Rust
and keep consequential behavior explicit. Deterministic personal transcript
transformations live in the explicit TypeScript user config (`~/.config/hex/hex.config.ts`). User-facing
runtime settings persist in Application Support.

Every build shows General, Output, History, and Labs. Labs holds features
we're unsure about, especially ones people asked for that we built: **new or
user-requested features land in Labs, off by default, not on by default.**
Labs has no master switch; Settings › Labs is one flat list of rows (name,
description, toggle), each off by default, plus Send feedback and Run setup
again. A feature with its own pane joins the sidebar as a normal item while
on and is hidden while off; a behaviour-only feature is just its toggle. Today:
Meetings (beta), Voice Filter (locked on while its macOS driver is
installed), Playground, Live text, Live chapters, Vocabulary biasing, Switch
model with keyboard language (#216), Mouse shortcuts (#209, macOS), Also
pause the browser (#301, macOS), Share dictation shortcut (#176, Windows),
Unload model when idle (#442), and Terminal paste (Windows). Externally contributed features land there too.
Annotate is a normal feature in every build, stable included, on macOS,
Linux, and Windows; only developer tools (`dev_build::is_dev_build()` with
`DEVELOPER_FEATURES_ENABLED`) stay gated to dev builds.

## Feature Map

[`docs/features/README.md`](docs/features/README.md) indexes existing user-facing
capabilities, entry points, checks, and gaps. Before designing, changing,
debugging, or reviewing a user-facing feature, read the index and relevant
feature files, then check their referenced implementation. The map is not a
substitute for source inspection or a second roadmap.

- Update the affected map in the same change when behavior, entry points,
  defaults, prerequisites, platform availability, recovery, or verification
  coverage changes. Do not churn unrelated maps for internal-only refactors.
- Organize by user capability, not Rust module or UI pane; link shared flows and
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
- `suppression`: the macOS event taps, shortcut suppression, and the
  configurable dictation-hotkey state machine.
- `keyboard`: active-layout key resolution and balanced synthetic shortcuts.
- `keyboard_input_sources`, `keyboard_model_switching`: keyboard input-source
  observation (macOS TIS, Windows foreground-window layout, X11 XKB groups;
  Wayland has no general protocol) and the shared debounced policy that turns a
  mapped source into an installed-only model-picker choice.
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
- `input_level`: the latest microphone peak as one atomic, stored per chunk by
  each OS's audio owner and read by the window's level bars (General and the
  practice dictation); a stale peak reads as no audio.
- `recording_environment`: serialized RAII ownership of idle-sleep prevention,
  output muting and lowering, and supported media-player pause/resume
  behavior on macOS, each held from the first dictation or capture session
  that wants it to the last.
- `output_ducking`: the shared Lower audio ramp (relative, channel-preserving,
  respecting manual changes, rolled back on failure) over each OS's volume
  adapter: CoreAudio output channels, the Linux default sink's channels
  (`pactl`, else `wpctl`), and other apps' WASAPI session volumes.
- `idle_sleep`: idle-sleep prevention while a dictation records: an IOKit
  assertion on macOS, a logind `idle:sleep` inhibitor (only `idle` where
  polkit refuses a process outside the seat session) on Linux, and a
  system-required power request on Windows.
- `dictation_processor`: the one OpenCode client and ordered processing policy
  for macOS, Windows, and Linux: context-selected corrections, deadline-bounded
  OpenCode rewrite profiles with corrected-transcript fallback, the ranked
  model catalog, and the transformation stage, over `opencode_service`.
- `opencode_service`: the only way Hex reaches OpenCode, on every OS. It reads
  the running service's owner-only registration
  (`${XDG_STATE_HOME:-~/.local/state}/opencode/service.json`, then other
  channels' `service-*.json`), confirms the registered PID through
  `/api/info`, and talks authenticated loopback HTTP over `TcpStream` with
  cancellation and deadlines. It never runs an OpenCode executable or curl;
  without a service, features say "OpenCode isn't running".
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
- `speech_detector`: the optional Silero voice activity detector, loaded once on
  the CPU and downloaded in the background, that keeps speechless dictation
  captures and meeting chunks away from the transcription models.
- `local_transcriber`: the Linux and Windows GGUF loader, prewarm, whole-clip
  transcription, and stream entry point.
- `transcription_engine`: the thread that owns the loaded model on every OS and
  runs every inference in order: live streams fed during capture, their finish,
  whole-clip runs, and (macOS) model loads and switches. The capture owns its
  `LiveStream` and hands it to the finished clip; finishing returns a
  `PendingTranscript` whose wait enforces the stream deadline and falls back to
  the whole clip. It also runs background work (meeting chunks, local API
  host audio) that yields to any dictation request sent before it starts. The
  listener shares its engine (`TranscriptionEngine::share`) on every OS;
  `transcription_engine::dictation_busy` is the press-to-paste capture state
  on macOS, and on Linux and Windows only the engine's dictation streams and
  finishes, so a host clip can start during a CPU-only capture's hold. With
  Labs › Unload model when idle on, its worker unloads the GGUF model after
  the chosen idle time when nothing records (`idle_sleep::held`) and reloads
  it before the next command that needs it; the parked model still counts as
  held, so callers never load a second copy.
- `transcription_preparation`: application-owned model preparation shared by
  Settings and the menu bar on macOS and by the Windows runtime, with one
  worker, latest-choice pending admission, cancellation fences, and an
  installed-only path that never downloads a model.
- `transcription_models`: the compiled model catalog, language recommendations,
  pinned artifact verification, and lazy installation.
- `transcription_service`: bounded host-audio admission, hostile WAV validation,
  normalization, cancellation, and inference. In the app, clips up to 30 s run
  on the dictation engine's model when it is the one requested; otherwise the
  service loads its own (`WarmTranscriber` on macOS, `local_transcriber::WarmLocal`
  on Linux and Windows), released after a long clip when the engine holds it.
- `transcription_stream`: live `/streams` transcription of another app's
  pushed audio on the engine's loaded model: pause-cut windows fed through
  yielding engine streams that any dictation request closes, a bounded
  backlog held while dictation is busy, offline fallback windows, the lease,
  and one live stream at a time.
- `local_api`: authenticated loopback discovery or direct-child endpoint
  handoff, bounded HTTP parsing, model preparation progress, and raw
  transcription routes, the `/streams` routes (contract:
  [`docs/streams.md`](docs/streams.md)), plus the always-on authenticated
  `POST /control`, and `GET /profile/vitals` (process footprint and CPU from
  Rune's `procwatch`, sampled by `macos_rune`; debug builds also count the heap
  and report the `speech.model_weights` gauge from `parakeet`). The running
  app serves it on every OS, as does `hex service [--embedded]`; the discovery
  file is `local-api.json` in the data folder (mode 0600 on Unix, the per-user
  `%APPDATA%` ACL on Windows). `/dictations` (`local_api::dictation`) runs on
  the macOS recognition loop only; elsewhere it answers
  `service-capture-unavailable` at runtime, and `/profile/vitals` answers 503.
  `/control` carries the `hex` control commands on every OS (`control`).
- `control`: the release `hex` control surface on every OS: the versioned
  `ControlRequest`/`ControlReply` protocol, the settings registry behind
  `hex settings` (one dotted key per persisted setting, with the window's
  rules and the capability each needs), and the one CLI tree and client
  (`control::cli`), which each OS's `main` flattens beside its own commands.
  The transport on every OS is the local API's `/control`.
  `control::host` carries commands out on Linux and Windows through a thin
  adapter (`linux_app::LinuxControl`, `windows_rune::WindowsControl`);
  `macos_rune::control` is macOS's host. What an OS can't do answers
  `unsupported` at runtime. Contract: `docs/cli.md`.
- `sdk/typescript`: Promise and Effect v4 host wrappers for direct-child
  lifecycle, authenticated model preparation, and host-audio transcription.
- `developer_control`: the typed debug-only command/reply protocol the local
  API uses to drive the running app from `hex dev`.
- `paste`: clipboard insertion, continuation joins, and generation-safe
  clipboard restoration.
- `accessibility`: bounded focused-window and selected-text capture through
  Accessibility without touching the clipboard.
- `recognition`: the macOS listener coordinator for hotkey and programmatic
  dictation controls, workers, and observations. It does not own the
  authoritative microphone timeline.
- `personal_commands`: the user's `hex.config.ts` (or `.js`) in
  `~/.config/hex`, run in-process by embedded QuickJS (`rquickjs`) on one
  worker thread: TypeScript erased to same-position JavaScript by
  `personal_commands/typescript` (oxc's parser), the `@hex/commands` module
  and managed `hex.d.ts` typings, watch-reloaded deterministic transformation
  calls, per-call time and runtime memory limits, status snapshots, and
  `hex commands init|check`. macOS, Windows, and Linux share it.
- `background_process`: the one constructor for child processes Hex starts for
  its own work (OpenCode CLI, curl, PowerShell); on Windows they
  never open a console window.
- `text_replacements`: phrase-boundary-aware compilation of configured text
  replacements applied longest-match-first to transcripts.
- `config`: dictation profiles.
- `context`: the foreground context snapshot and selectors for modes,
  plus native macOS capture. Browser host is the domain concept. macOS
  reads the front tab of a table of scriptable browsers (Safari and Chromium
  dictionaries) through AppleScript and any other app's focused AXWebArea
  `AXURL`; `windows_context` reads any browser through UI Automation and
  `linux_atspi` through AT-SPI.
- `application_catalog`: cached installed-application discovery for the Modes
  picker, one module per OS: macOS bundles, Windows executables from Start menu
  shortcuts, App Paths, and open windows, Linux `.desktop` entries.
- `app_settings`: the one persisted `settings.json` on every OS and its live
  runtime projection for hotkeys, microphone and transcription
  selection, recording behavior, processing, sound volume, indicator position,
  interface language, and Dock policy. It records one-time changes in
  `migrations`.
- `login_item`: the native `SMAppService.mainAppService` adapter and its worker
  thread. macOS owns registration state; it is deliberately not duplicated in
  `settings.json`.
- `app_paths`: the Application Support owner (`hex` on every OS) for runtime
  logs and shared state.
- `onboarding`, `permission_guide`: required permission health, the native
  floating System Settings guide, selected dictation-model installation, the
  release startup gate, and opt-in command-model setup.
- `hex_microphone`, `voice_filter`: the experimental virtual microphone
  (`Hex Microphone`: a CoreAudio driver on macOS, a PipeWire virtual source
  fed through `pw-cat` on Linux, none on Windows, which would need a signed
  kernel driver), the lock-free capture-to-sink ring buffer, real-time
  voice-filter DSP presets, and the offline WAV renderer behind
  `hex dev voice-filter render`.
- `linux`, `linux_app`, `linux_dictation`, `linux_paste`:
  the Linux beta CLI (including the `hex dictation` and
  `hex paste-last` keybinding commands and launch forwarding), the one Linux
  process (host thread, window on demand through one seam, Quit), the hotkey
  capture-transcribe-paste loop with Paste Last,
  and paste.
- `linux_clipboard`: the clipboard a Linux paste restores: every format of the
  CLIPBOARD selection read within a size cap and deadline, then served again
  from Hex (wlr-data-control on Wayland, its own selection owner with INCR on
  X11) until another client takes it.
- `linux_input`: the one shortcut state machine (hold, double-tap lock, toggle,
  double-tap only, Escape, Paste Last, and `hex` commands) and the
  monitor that feeds it from X11 key grabs, the Wayland GlobalShortcuts portal,
  or, without the portal, the `hex` commands alone.
- `linux_portal`: Hex's Linux app id `anomaly.hex` (reverse-DNS, #579; the
  windows' Wayland app_id and X11 WM_CLASS, and the `anomaly.hex.desktop`
  entry installs ship), the move of a managed pre-#579 `hex.desktop` to that
  name, the portal bus (a private zbus connection, `Register("anomaly.hex")`
  after writing a managed fallback entry for a loose binary, requests and
  sessions) and the GlobalShortcuts client on it: one session binding all of
  Hex's shortcuts once, Cancel dictation included, granted-trigger descriptions, ShortcutsChanged, ConfigureShortcuts, and
  session-closed reporting.
- `linux_remote_desktop`: the RemoteDesktop portal client Wayland paste uses
  without the virtual keyboard protocol: a keyboard-only session with a
  persisted owner-only restore token, and Ctrl+V as keysyms.
- `linux_context`: the focused application for modes: the X11 active
  window's process, or on Wayland the activated `zwlr_foreign_toplevel`
  window's app ID named by its `.desktop` entry (none on GNOME and KDE).
- `linux_atspi`: the focused window's page address through AT-SPI on X11 and
  Wayland, matched by title and process and bounded by a deadline. Chromium
  exposes pages only with renderer accessibility on.
- `linux_audio_control`: While-dictating mute through `wpctl`/`pactl`, Lower
  audio through `output_ducking`, and MPRIS pause over D-Bus (zbus),
  restoring only what it changed, plus idle-sleep prevention for dictations.
- `linux_input_route`: System default's Bluetooth avoidance on Linux. ALSA's
  default cannot name another PipeWire source, so Hex marks its ALSA streams
  (`PIPEWIRE_ALSA`: `state.restore-target = false` and its PID, set on the
  main thread at startup) and moves them off a Bluetooth default source to
  the built-in (PCI) input, else the first other hardware input.
- `linux_session`: display-backend selection matching winit's (Rune's)
  nonempty `WAYLAND_DISPLAY` rule, independent of persisted preferences.
- `linux_service`: the single-instance lock and owner-only, same-user Unix
  socket that forwards `Activate` (with the activation token), shortcut
  commands, `Status`, and `Quit` to the one Hex process, plus the per-user
  systemd lifecycle (`Type=notify` readiness, `session.env`, start/restart).
- `linux_rune_hud`: the Linux side of `rune_hud`: `LinuxIndicator` turns the
  listener's capture, meter, and pending-output reports into HUD events and
  never waits on the HUD; it shows once the window's app runs, whether or not
  the window is open (layer-shell on Wayland, override-redirect on X11, and
  override-redirect on XWayland when Wayland has no layer-shell).
- `rune_hud`: the dictation HUD on every OS: a click-through Rune overlay
  whose compositor steps `dictation_indicator`'s model on Rune's frame clock
  (the display's refresh) and draws Dot scope on the GPU with
  `rune_hud.wgsl`. `rune_hud/macos` places it on the pointer's screen
  through `hud_placement` (below the menu bar, in it, or out of the notch);
  Linux and Windows place it at the top or bottom edge.
  `rune_hud_frames` renders a fixed dictation through the shaders offscreen.
- `linux_tray`: the optional tray icon, a `rune::Tray` (StatusNotifierItem
  with a dbusmenu built by `tray_menu`). It registers whenever a tray host
  (`org.kde.StatusNotifierWatcher`) appears; without one Hex runs unchanged.
- `tray_menu`: the tray menu Linux and Windows share (recording state, Show
  Hex, Paste last dictation, the downloaded-model picker, the update item,
  Enable / Disable Hex, Quit Hex) and the model entries the macOS menu bar
  lists too. Each tray maps a `TrayCommand` onto its own runtime.
- `linux_login_item`: Launch at login on Linux, the managed XDG autostart
  entry the installer also writes; the file is the only state.
- `updater`: the in-app updater on every OS, a client of the Anomaly release
  service (`docs/plans/release-service.md`). `updater/protocol` is the
  protocol's strict SemVer, lenient manifest parsing, and client rules (the
  service's shared test vectors in `tests/fixtures/release-protocol`);
  `updater/transport` is curl with `If-None-Match` and resumable,
  size- and SHA-256-checked downloads; `updater/{linux,macos,windows}` detect
  the install method, verify (OS code signature on macOS and Windows), stage,
  and restart into an update.
- `windows`, `windows_app`, `windows_rune`, `windows_dictation`,
  `windows_input`, `windows_paste`, `windows_context`, `windows_audio_control`, `windows_login_item`: the Windows
  beta CLI, the runtime host (`WindowsDesktopHost`: listener, model
  preparation, OpenCode catalog, transformations, updater) and single-instance
  claim, the tray app on `rune::App` (tray, window icon, and runtime thread;
  `rune_ui/windows_host.rs` is the window's host), the WASAPI dictation loop,
  low-level keyboard hook, clipboard paste, UI Automation context, While
  dictating (mute, lower, or pause other apps' audio) with idle-sleep
  prevention, and the login item.
- `live_text`: Labs' live text of the recording in
  progress: the recording's `Publisher` shows it and its drop clears it, and
  each OS's host installs `rune_ui::live_text`, one click-through line just
  past the HUD, as a watcher, placed where `rune_hud` last showed the HUD
  (`rune_hud::live_text_place`).
- `dictation_modes`: the mode runtime the Windows and Linux listeners share:
  mode selection by application and website, then global replacements, mode
  corrections, OpenCode rewriting, and transformations, and the History entry.
- `desktop_i18n`: interface translations keyed by the English source string,
  `tr`, `tr_fill`, and `tr_named`. A test fails if any string the window
  translates lacks a language.
- `history`: the owner-only bounded retained-dictation store: retention
  windows with hard entry and byte caps, atomic crash-safe persistence, and
  search. Text and bounded metadata only, never audio.
- `transcript_cli`: read-only release commands `hex history` and
  `hex meetings list|show|search` on every OS, versioned JSON projections and
  filters.
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
  `rune-kit` and the OpenCode theme: General, Output, and History, then in dev
  builds the opt-in Meetings once on (each feature owns its
  settings, rarer ones in the pane's Advanced fold), and under Labs (`labs`:
  the Labs switch and the opt-ins) Annotate and the Voice Filter, plus in
  Playground (`⇧⌘D`) HUD Lab's live Dot scope sliders (`hud_stories`), the transcription
  picker, the Sounds sheet (its stage replays `rune_hud`'s HUD, drawn by the
  window's compositor with `rune_hud.wgsl`), the onboarding tour, the macOS
  meeting capsules, the `--snap`
  fixture scenes, and the macOS `hex://story/…` playground links
  (`story_link`, `hex-test://` in test builds). `platform::Capabilities` decides which panes and rows exist (and, through `rune_ui::capabilities`, which `hex settings` keys act and what `hex status` reports); every difference between the OSes names a reason in `platform::DIFFERENCES` (an OS pattern, impossible, or pending), which `parity_tests` enforce;
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
- `hud_placement`: pure geometry for where a HUD sits on its screen (below
  the menu bar, in it, or as an island grown out of the notch), shared by
  the dictation HUD and, later, the capture capsule.
- `dictation_indicator`, `dot_scope`, `spring`: the HUD's events, phases,
  jobs, and Dot scope (the one HUD look: a plate of dots lit by a braided,
  auto-levelled oscilloscope trace), the same on every OS and drawn by
  `rune_hud`; `spring` holds the frame-rate-independent springs.
- `meeting`: explicit capture through `meeting_capture`, owner-only WAV and transcript
  artifacts, final local-model publication, the user-facing Markdown copy, and
  recovery, on every OS. Its `os` adapter (`meeting_macos`, `meeting_linux`,
  `meeting_windows`) owns the system audio source and its consent, idle-sleep
  prevention while recording, and reveal and Trash.
- `meeting_capture`: independent self-healing microphone (cpal) and system
  audio sources (macOS: Core Audio process tap, ScreenCaptureKit fallback;
  Linux: `pw-record` of the default output's monitor; Windows: WASAPI
  loopback); one failing never stops the other, and resumed audio is padded
  onto the timeline.
- `meeting_runtime`: the Linux and Windows apps' meeting controller thread,
  capsules, recording state, and tray items (macOS runs the same controller
  from `macos_rune`).
- `durable`: crash-safe writes on every OS (`write`, `sync_file`,
  `sync_directory`). Windows flushes only through a writable handle and
  cannot open a directory, so file syncs and directory syncs go through it.
- `meeting_generator`: who writes notes and live chapters: Apple
  Intelligence on a Mac that has it, else OpenCode when
  `meetings.summarize_with_opencode` is on (off by default; it sends the
  transcript excerpt).
- `dictation_activity`: whether dictation is capturing or transcribing, so
  meeting chunks and the local API yield, and the background priority they
  run at, on every OS.
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
- `meeting_chapters`: live chapters (Labs): one
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
- `microphone_activity`: permission-light observation of which apps use the
  microphone: CoreAudio processes (macOS), a `pw-dump --monitor` of PipeWire
  capture streams (Linux), audio sessions on capture devices (Windows). No
  samples are read.
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
  "Choose a file…", the macOS meetings folder) through `rune::FileDialogs` on
  its own thread. Answers on a channel; cancel and a dialog that never opened
  read as nothing chosen.
- `labs`: the Labs feature registry (`labs::Feature`: name, settings key,
  description), the runtime gates (`labs::live_text`, `live_chapters`,
  `playground`, `vocabulary_biasing`, `mouse_shortcuts`), and
  `hex labs [on|off <feature>]`. `dev_build`: whether this is a dev, local, test,
  or dev-channel build.
- `capture_host`: Scrawl's capture engine (the `scrawl-capture` crate in
  `crates/capture`, copied from Scrawl's `engine/split` at `e351ddb` and
  kept out of the workspace; `scripts/check` names it on macOS) hosted in
  Hex as Annotate (its user-facing name; code keeps `capture`;
  `docs/plans/capture-in-hex.md`),
  compiled as `cfg(hex_capture)` into every macOS, Linux, and Windows build
  (`scripts/build-app.sh` bundles and signs `scrawl-helper` beside `hex`).
  Everything here is shared by every OS; the engine's own platform
  adapter (`crates/capture/src/capture/{macos,linux,windows}.rs`) is the seam for
  the screen, window and area source, cursor, permission flow, and encoder.
  The Linux adapter (`crates/capture/src/linux/`) asks the ScreenCast
  portal over `zbus` (a screen keeps the portal's restore token at
  `<support>/screencast-token`; a window is picked each time), reads the
  PipeWire stream with `libpipewire-0.3` loaded at run time, imports each
  frame as a linear DMA-BUF into VA-API (`libva`, also loaded at run time),
  scales and encodes H.264 on the GPU with Hex-written SPS/PPS/slice
  headers, and muxes and reads movies with the `ffmpeg`/`ffprobe`
  executables; without VA-API it copies frames and encodes with x264.
  Host-side, Linux differs only in where the audio comes from
  (`capture_host::ListenerTimeline`, fed by `linux_dictation`'s
  microphone), "While dictating" (`Environment`: an `AudioSuppressor` per
  capture), words (`LocalTranscriber::transcribe_words` as one foreground
  job on the listener's engine), the clipboard and paste (`linux_paste`),
  and the shortcut (`linux_input`'s `Shortcut::Capture`). Windows records
  with Windows.Graphics.Capture (`crates/capture/src/win/media.rs`) and
  shares Linux's host side (`ListenerTimeline` fed by `windows_dictation`,
  the same words path) except "While dictating" (`Environment`: a
  `windows_audio_control::AudioSuppressor` per capture), paste and copy
  (the engine's own Ctrl+V and clipboard), and input: `windows_input`'s
  hooks give the Annotate key (`HotkeyAction::Capture`/`CaptureUp`), Escape
  while drawing, and, while a capture records, every pointer move, click,
  wheel turn, and modifier change in physical pixels
  (`capture_host::report_input`, as the macOS tap does). The overlays lay
  out in the primary display's points, so the picker and ink convert with
  `capture_host::pixels_per_point`. Hex's `Host`,
  `ConfigSource`, and Parakeet or Apple `Transcriber` for the engine, the
  listener-owned `CaptureHost` behind `hex capture` and the capture shortcut,
  its 16 kHz gap-padded timeline handed to the engine as `AudioIn::External`,
  the tap's pointer input forwarded to `Capture::input()`, Scrawl.app
  coexistence, the retention thread, and the `CaptureSnapshot` every view
  reads (`capture_host::snapshot`/`watch`, published on each change, so a
  capsule never reaches into the host), with `last.route` from
  `capture_host::routing` (Scrawl's `crates/router` in-process: look, decide,
  undo window, send, Undo and Redirect) and `phone` from a `capture-phone`
  look for a cabled iPhone. The engine records the screen on macOS through
  `crates/capture-core` (`Recorder`, one source per target, also outside the
  workspace), which writes `recording.mp4` and its `capture.json` manifest
  and changes what a recording leaves out with `Recording::set_exclusions`.
  `capture_host::library` reads the captures
  folder for `hex capture list|show|send|route`. `rune_ui/scrawl.rs` is its pane
  (Labs › Scrawl) and `CaptureSettings` its `capture.*` settings;
  `hex capture` is `control::cli::capture` (macOS through the running app;
  Linux reads the folder itself and hands start/stop/cancel to the
  listener over `linux_service`; Windows sends every `hex annotate` command
  to the running Hex).
- `instance`: the exclusive Application Support lock that prevents concurrent
  command listeners.
- `process_exit`: how the macOS app ends. Every quit path (menu, `hex quit`
  and `hex restart`, Ctrl-C, and AppKit's terminate) stops and joins Hex's
  workers, bounded by a 5 s deadline, then calls `_exit`, so C++ static
  destructors (such as ggml's Metal device) never run
  under live threads. A restart
  schedules its relaunch first; the new copy waits for the old pid to exit.
- `feedback`: bounded volume-controlled capture, cancellation, and
  failure tones.

Keep modules deep. Callers should not coordinate
CoreAudio formats, AppleScript details, or event serialization.

## Invariants

- On a new Mac, release dictation starts only after Microphone, Accessibility,
  and the selected dictation model are ready. Hex never asks for Input
  Monitoring: Accessibility also grants the listen-event access its shortcut
  taps need (measured on macOS 26 and 27).
- The dictation shortcut defaults to Option but supports modifier-only,
  modifier-plus-key, standalone Globe/Fn, and standalone function-key bindings.
  Capturing a new binding suspends global matching.
- Every shortcut is a chord: the set of keys held together, any order and
  nothing else, each modifier key with its side (left, right, or any) plus at
  most one other key or mouse button. It is down on the edge that completes
  the set and up when any of its keys lifts; hold, double-tap lock, Toggle,
  the hold threshold, the provisional start tone, extra-key discards, stale
  recovery, and the release tail all run on those edges, on the events' own
  timestamps. A one-key chord is down at once, with no added wait. Both sides
  of one modifier (`[⌥ left, ⌥ right]`) are down only when the second side
  goes down within `BOTH_SIDES_WINDOW` (80 ms) of the first; one held and the
  other added later presses nothing. A sided key never matches with its twin
  also down; any side matches one or both, as before chords, so it conflicts
  with a both-sides chord. Settings store `{"keys": [...]}`; the old
  `modifiers`/`key` shape loads as the same chord and is written back once.
  macOS and Windows match sides (device flags, the low-level hook); Linux
  binds a key with side-less modifiers only, and its recorder says so when
  modifiers are pressed alone.
- Windows accepts one modifier alone (Right Alt, Right Ctrl) as a dictation
  shortcut. Any key or mouse button joining a held modifier-only shortcut
  ends it as a chord (Cancel only while it records). Releasing Alt or Windows
  after a modifier-only shortcut held them is swallowed and replayed after a
  tap of unassigned VK 0xE8, so the menu bar and Start stay closed (not with
  Share shortcut). Hex marks every event it injects with `HEX_INJECTED` and its
  hook skips them, so a paste's Ctrl never presses a Ctrl-alone shortcut.
- Windows onboarding adds a Microphone row only while Settings' microphone
  switch (device, user, or "Let desktop apps access your microphone") is off;
  Allow opens `ms-settings:privacy-microphone`, and setup waits for it.
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
  the binding itself (Idle or Locked for a modifier-only binding, or Dirty for
  any binding; never a pending gesture), the press resynchronizes and counts
  as a fresh press (a lock stays locked, so the press finishes it) only if a
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
  300 ms discard. A 450 ms hotkey pre-roll
  protects speech onset. Capture has no automatic duration limit; release,
  explicit stop, or Escape ends it.
- When enabled, a second shortcut tap within 300 ms locks dictation. Press the
  shortcut again to finish or Escape to cancel.
- Recording audio behavior and idle-sleep prevention begin only after the
  intentional-hold threshold. Ordinary shortcut chords must not mute output,
  pause media, or prevent sleep.
- A modifier-only shortcut (not Globe/Fn) passes through, so once Hex owns
  the press (hold past the threshold, Toggle start after the 40 ms chord
  grace, lock-finishing press) it posts one marked press of unassigned key
  code 0x44 at HID while exactly its modifiers are still down, so other
  apps' modifier-tap hotkeys read a chord. Hex's annotated-session key tap
  drops it, so the focused app never receives it (and never beeps). Short
  taps and chords never get one,
  and it never reaches Hex's state machines.
- The start tone confirms the shortcut press itself, not the hold threshold: it
  is provisional, sounds after a 40 ms chord grace, and a chord, short tap, or
  cancel retracts it (silently inside the grace, else
  a 20 ms fade). With the microphone released while idle, the tone waits for
  the open microphone and the hold.
- When `Release microphone while idle` is
  enabled, pressing the shortcut opens the selected device asynchronously with
  no pre-roll, preserves the physical press for the hold threshold, discards a
  release before readiness, and closes only after the authoritative capture is
  idle. Accepted jobs never keep the stream open. A stream failure before the
  hold threshold returns the press to pending and reopens; later failures fail
  the capture. Cold opens retry the selected device briefly before falling back.
- Model inference, optional post-processing, and paste must
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
  capture. Escape, short taps, programmatic stops, and meetings
  get no tail. A press during the tail is handled after the old clip finishes.
- Active dictation capture is lossless with respect to event, UI,
  context, and worker stalls.
- The dictation shortcut has a persisted `shortcut_mode`: `hold` (default)
  or `toggle`. Toggle reuses the lock state on every platform: each press acts
  as the second tap of a double-tap, so release locks and the next shortcut
  press finishes. Only Escape cancels, and a modifier pressed alone never ends a
  key-chord capture. Double-tap settings apply to Hold only.
- Streaming dictation is speculative on every OS: a hotkey capture's stream
  receives the capture's 16 kHz audio with each pause cut to its first 2.5 s
  and last 0.3 s (Parakeet drops words after pauses past its left context),
  the finish always carries the complete clip, and a failed, empty, overfed, late, or interrupted stream
  falls back to the whole clip. Nothing pastes or reaches the focused app
  before the final text. With Labs › Live text on, a hotkey capture's
  stream publishes its provisional text for Hex's own HUD only; it is never
  pasted, copied, or kept in History or Paste Last. One engine thread runs every inference because a live
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
  else the first real input, logged and shown in Settings. With a MacBook's
  lid closed (`AppleClamshellState`), the built-in microphone is off in
  hardware but still listed, so it is passed over the same way, and with no
  other input Hex reports why instead of recording silence. With
  `avoid_bluetooth_microphone` on (the default; General › Advanced), a
  Bluetooth default is passed over the same way (macOS: CoreAudio transport
  Bluetooth or Bluetooth LE; Windows: WASAPI enumerator `BTHENUM`, or a
  headset/handset of unknown bus such as a `BTHHFENUM` hands-free endpoint;
  Linux: a `bluez5` default source, whose stream Hex moves after opening), so the headset never drops into call audio, but kept when
  no other usable input exists; a saved Bluetooth microphone and `--device`
  are always honoured, and a change to the setting re-resolves like a
  device-list change. Settings names the stand-in. A saved microphone
  that is missing or will not open falls back to the system default without
  changing the setting, and Hex switches back when it returns. Any switch
  (selection, default, device list, or lid) happens only while capture is idle,
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
- Mode processing is best-effort and ordered: corrections, optional OpenCode
  rewriting, then selected transformations. A failed stage preserves its input;
  the transformation chain is one stage, so failure discards its partial results.
  It applies to Paste, not meetings.
- Hex never starts, probes, or searches for an OpenCode executable, and never
  starts the service: it only talks to an already running OpenCode service
  found through its registration file (`opencode_service`). Nothing touches
  OpenCode unless a feature that uses it runs: mode rewriting, meeting notes
  with Summarize with OpenCode, sending an Annotate capture, or the model
  catalog while the Modes pane shows. Checks stay off the UI thread. A stopped
  service is looked for again at a coarse interval only while that pane shows;
  catalog failures require an explicit retry. Hex's OpenCode workspace folder is
  created by the first request that uses it.
- Personal transformations run in-process: no child process,
  no Node or Bun, and no filesystem, network, process, or timer API in the
  script engine; scripts import only `@hex/commands` and files inside the
  config folder. One worker thread owns the QuickJS runtime; each
  transformation call is interrupted after 1 s and a load after 2 s, and the
  runtime is capped at 64 MiB. A config that fails to load or compile keeps
  the previous registry and reports its error with file, line, and column;
  a removed config unregisters everything. The built-in transformations stay
  native Rust and never touch the script engine.
- Clipboard insertion is the fixed text-insertion path. Restore the previous
  clipboard only if no newer paste or external clipboard change superseded it.
  The one exception: on macOS, a frontmost app embedding Chromium 155 or 156
  (read from its bundle's framework version) gets the text typed, because
  those versions can drop a ⌘V that races their pasteboard notification.
- Dictate without processing (`raw_dictation_hotkey`, Output pane) is
  unbound by default and registers, grabs, or reserves nothing until bound
  (on Linux a portal entry and X11 grab exist only while bound). It follows
  hold, toggle, double-tap lock, Escape, and queueing as dictation does; the
  shortcut that started a capture owns it. It pastes the trimmed transcript
  with no Output step, records History kind Raw, and updates Paste Last.
  `hex dictation start|toggle --raw` does the same; a printing stop of a raw
  dictation prints the transcript as heard.
- `hex dictation stop --print` (macOS and Linux) stops through the listener's
  own queue with a one-shot reply: recording stops at once and only that
  command waits, at most 60 s, for the text, processed as a paste. It never
  pastes, even on failure or timeout; no speech exits 4. History records it as
  Printed and Paste Last skips it.
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
- `hex history` and `hex meetings` are read-only, release-available on every OS,
  independent of feature opt-ins. Dispatch before process
  logging, the instance lock, local API, GUI, permissions, or Hex Microphone.
  History reads one open
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
- Every in-call meeting view and copy (the Meetings pane,
  Option-Control-V, provisional titles, `hex meetings`, and MCP) reads the
  rolling Parakeet transcript, which publishes only entries no later chunk
  can replace; after Stop they read the final transcript.
- Meeting system audio uses a Core Audio process tap on macOS (System Audio
  Recording consent), never requiring a display; ScreenCaptureKit is only the
  fallback. Linux records the default output's PipeWire monitor through
  `pw-record` and Windows its WASAPI loopback; neither asks for consent.
  Losing either source never stops the other or the meeting: it retries until
  Stop, and resumed audio is padded with silence so WAV positions stay on the
  shared capture clock.
- Meeting recording prevents idle sleep but does not apply dictation mute or
  pause-media behavior.
- Meeting notes (Apple Intelligence, or OpenCode only once the user turns on
  Summarize with OpenCode) run only after final publication, on a
  utility-QoS thread that yields to dictation, within a bounded chunk count and
  total deadline. Failures record `notes_error` and leave the Markdown
  untouched. Notes never rename the file and rewrite only the heading Hex wrote
  and the `<!-- hex:notes -->` block.
- Annotate (captures; `hex annotate`, alias `hex capture`) is compiled into
  every macOS, Linux, and Windows build and on by default in every build,
  stable included (`capture.enabled`, the pane's switch; the `annotate-on-by-default`
  migration turns it on once for older settings). It is not an experiment.
  Nothing asks for a screen permission until the first
  capture: macOS's Screen Recording at its first capture, Linux's portal
  dialog at its first Screen capture; Windows asks nothing. Default shortcuts
  share no key with the other defaults: ⌥⇧S on macOS, F9 on Linux and
  Windows. One key does everything:
  a tap (under `DRAW_HOLD`) starts and stops; held while recording, the
  pointer draws with no click until it comes up (Linux: portal
  Activated/Deactivated). The recording capsule is `● 0:07 │ ✎ │ ■ ✕`,
  icons only, with no target chips; its pen stays on until clicked
  again and draws only while the mouse button is down (drag to draw,
  move freely otherwise), and Esc stops drawing, never the recording. Shift still
  draws on the Mac. While drawing, the ink overlay takes the pointer with
  a pen cursor (Linux leaves a hole over the capsule's pill), so clicks
  reach neither the app nor anything but the capsule. Ink has no pins and
  no undo. There is no separate draw shortcut and no send-to list
  shortcut; Send to… is a capsule click. `hex annotate draw` toggles the
  pen for desktops without the portal. While a capture records, the
  dictation shortcut starts nothing on either OS. Sends
  carry the contact sheet and keyframes as inline `data:` image parts, never
  a local path. The Linux capsule doesn't open suggestions by itself.
  `validate-app.sh` requires
  `scrawl-helper` and the camera description in a macOS bundle. Linux
  never links PipeWire, VA-API, or FFmpeg: the first two load when a
  capture starts and `ffmpeg` runs as a child, so a missing one fails that
  capture with a reason and nothing else. With `capture.enabled` off, the
  listener builds no capture host: no engine, sink, thread, bundle folder,
  or work. Scrawl's engine
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
  `scrawl-helper mirror --no-microphone`, narrated by the dictation timeline
  (macOS only: Linux lists no phone, so it never shows). Hex's capture
  shortcut (default ⌥⇧S, F9 on Linux, a key required, never overlapping
  another binding) is, on Linux, the portal's or an X11 grab's press and
  release: idle a press starts, recording a tap stops (debounced) and a
  hold draws. On macOS it is a fresh press: idle, it starts once one of its
  modifiers comes up (Esc before then records nothing); recording, a tap
  stops and a hold draws; with the capsule's picker open, the picker
  records. A switch (CLI) discards the recording (no outcome) rather than
  retargeting it; while Scrawl.app
  (`dev.kitlangton.scrawl`) runs, Hex registers no capture shortcut and runs
  no retention, and only the pane's "Use Hex for captures" quits Scrawl.
  Escape never ends a recording capture (while drawing it only stops
  drawing), and otherwise Hex's tap lets it through to the app being
  recorded; discarding is the capsule's two-step ✕ (✕, then
  Discard). Pointer
  events and Shift reach the engine from the one session tap, only while a
  capture records, through a bounded queue that drops rather than block; they
  never reach the listener. Scrawl's settings are imported once, read-only;
  Screen Recording is asked at the first capture or from the pane, never in
  onboarding. The capsule, picker, and ink are left out of every screen
  recording while shown; hidden (prewarmed at alpha zero on every Space)
  they are shareable, since one non-shareable window blanks every Space's
  Mission Control thumbnail. A capture leaves out all of Hex's chrome, unless
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
  when the capture is written. The movie finishes after the stop on the
  core's own bounded thread (detached before its stream stops, its final
  frame held to the stop); only the steps that read `recording.mp4` wait
  for it, and a discarded capture's bundle is removed only once it is done.
  One thread writes stopped captures one at a
  time, at most two wait, and a start beyond that is refused `busy`. The
  engine's log goes to tracing from before it is built, and its settings come
  from Hex (`ConfigSource`), never `~/.config/scrawl` after the import. Bundles are
  owner-only. Quitting
  cancels a recording capture, stops transcription between chunks, and waits
  at most 2 s for capture threads, leaving a movie still finishing behind.
- `hex mcp` is read-only and available in release builds regardless of the
  Meetings opt-in. It dispatches before process logging, the instance lock,
  GUI, or permission setup; never writes, recovers, or indexes on disk inside
  the meetings folder; runs no model; and makes no network request. Stdout
  carries only MCP messages.
- `hex` control commands (`status`, `quit`, `dictation`, `meetings record`,
  `settings set`, `model use`, `window show`, …) are release-available on
  every OS and drive only the one running Hex through the owner-only
  discovery file and bearer-authenticated `POST /control` (64 KiB request,
  1 MiB reply, 10 s in-app deadline). A command an OS can't do answers `unsupported` at runtime. They dispatch before process logging,
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
  Hex restarts. They track, per source, the end of the text already handed
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
  `AUTHENTICODE_SIGNER`. Linux has no OS signature, so Release signs each
  Linux executable with Anomaly's ed25519 key (`HEX_RELEASE_SIGNING_KEY`,
  `.github/release/sign-linux.sh`, which refuses a key that does not match
  `.github/release/linux-signing.pub`) and publishes the 64-byte signature
  as the `<target>-sig` file; the updater (`updater/linux.rs`
  `SIGNING_KEY`) and `install-linux.sh` refuse an executable without a valid
  signature over its SHA-256. Download URLs must be on the manifest's host
  (`protocol::resolve_url`).
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
- A staged update waits for Restart to update, and checks continue while it
  waits: another release this install would take by itself (update or
  required) replaces it, and Restart to update runs one more bounded check
  first, so the restart lands on the newest release. Staging never removes
  the running version. Linux and Windows activate a
  new version directory and switch `current`/`current-version` atomically at
  staging; macOS unpacks beside the bundle and swaps it in with
  `renamex_np(RENAME_SWAP)` at restart or quit, and the new version removes
  the old bundle at launch.
- The Wayland beta requires compatible clipboard protocols, and layer-shell or
  XWayland for the HUD.
  Paste sends its shortcut through `wtype` when the compositor advertises
  `zwp_virtual_keyboard_manager_v1`, and otherwise through the
  RemoteDesktop portal, the general injection path: one keyboard-only session
  approved once (restore token, persist mode 2) and kept with the paster.
  With neither, the transcript stays copied and the failure names the
  missing portal. Shortcuts come from the GlobalShortcuts portal when the
  desktop offers it; otherwise the user binds the `hex dictation` and
  `hex paste-last` commands in the desktop's own
  keybindings. One general path: no compositor-specific code, no raw input
  devices, no input-group requirement, and no silent fallback to XWayland or
  privileged input injection (uinput). One allowed exception: on a compositor
  without layer-shell (GNOME), Rune opens only the HUD, an input-free overlay,
  as an override-redirect X11 window on XWayland when `DISPLAY` is set (empty
  input shape, never focused); everything else stays on Wayland, and without
  XWayland Settings reports the HUD unavailable. The desktop may grant a different
  trigger, keep an older one, or refuse one; Settings shows what it granted,
  follows ShortcutsChanged, and opens ConfigureShortcuts (portal version 2).
  Hex's app id is `anomaly.hex`: every install (direct, AUR, deb/rpm, Nix)
  ships `anomaly.hex.desktop` with `StartupWMClass=anomaly.hex`, and the
  windows report that id, so there is one launcher entry. Launch at login is
  `~/.config/autostart/anomaly.hex.desktop`. At startup Hex renames its own
  managed `hex.desktop` entries (the names before #579) and never touches
  unmanaged ones. Only a loose binary without any entry writes its own
  managed one so the portal can identify it, NoDisplay beside an older
  package's `hex.desktop`, and removed once a package ships the entry. Every portal shortcut, Cancel dictation included, is bound
  in one session with one BindShortcuts call, and that set never changes
  while Hex runs: desktops keep one set per app, and KDE replaces it with
  each call (a second session once wiped Hex's shortcuts,
  [#482](https://github.com/anomalyco/hex/issues/482)). Enable and Disable
  (window, tray) also save `enable_on_launch`, so a disabled Hex stays disabled
  after a restart (#575). On GNOME only, Disable then binds an empty set in a
  fresh session: GNOME Settings stores exactly the set an app binds, so Hex's
  entries leave its list without a dialog, and Enable asks for approval again.
  Never on KDE, where an empty set would drop the user's chosen keys (#482).
  At startup on GNOME, Hex also removes its pre-#579 `hex` set from GNOME's
  `org.gnome.settings-daemon.global-shortcuts` settings through `gsettings`,
  since the portal can no longer reach that id; it touches no other app. A portal shortcut
  is held for as long as it is bound and consumed before the focused app
  sees it, so the portal never binds Escape: Cancel dictation has no
  suggested trigger, the user assigns one in the desktop, and it acts only
  while something records or Annotate draws. Modifier-only shortcuts stay
  unsupported on Linux.
- Every Linux shortcut source (X11 grabs, portal presses, `hex` commands) feeds
  the same `linux_input` state machine and emits the same events. Commands are
  idempotent: starting while recording or stopping while idle does nothing.
  They talk only to a running, listening Hex and fail with a clear error
  otherwise; they never start one. The control host hands them straight to
  the listener's queue, never through the window's thread.
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
  cancels that wait. Wayland hides held keys, and some compositors (Hyprland)
  merge held physical modifiers into `wtype`'s virtual keyboard, as the
  RemoteDesktop portal shares the desktop's keyboard. So Wayland paste waits on
  `ShortcutKeys`: a modifier shortcut's portal release plus 250 ms (at most
  2 s after a press without release), or 250 ms after a `hex` command. Docs
  recommend modifier-free binds there. A sent paste is not a verified one.
  New installs use Ctrl-V;
  the terminal-paste preference selects Ctrl-Shift-V and defaults on for
  persisted legacy X11 settings.
- Linux paste restores the previous clipboard by default, as macOS does: it
  snapshots every format of the selection (32 MiB cap, 1 s deadline) before
  writing the transcript, and 500 ms after the paste shortcut restores it only
  if the paste's generation is still the latest and the clipboard still holds
  the paste (the same bytes on Wayland, the paste's owner on X11). An empty
  clipboard is cleared again; an oversized or unreadable one is not restored,
  and the log says why. A paste that only reached the clipboard keeps it for
  Ctrl+V, and a capture copy is never restored over. Wayland restoration needs
  wlr-data-control; there is no consumption acknowledgment.

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
./scripts/test-install-linux-release.sh # x86_64 or aarch64 Linux
cd sdk/typescript && bun run check && bun run test && bun run build
```

New, unproven, user-requested, and externally contributed features
(including ones the issue pipeline builds) land in Labs, off by default
([docs/features/labs.md](docs/features/labs.md)): add a `labs::Feature` with
its settings key and description, a row on the Labs page, and a
`hex settings` key; gate where the feature's work starts (worker, thread,
capture), not just its UI. Never add a master switch or ship such a feature
on by default. Remove it from Labs when it graduates or is dropped.

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
Windows panes (`modes`, `history`, `picker`, `onboarding`,
`windows-settings`, …) and the macOS ones (`mac-settings`,
`meetings*`, `mac-voice-filter*`, `mac-hud-lab`, `mac-activity`, `mac-onboarding*`).
`live` renders the running Linux Hex's real state. On a host without a GPU,
render on lavapipe with `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`.
`cargo test rune_ui` compares the light and dark goldens in `tests/golden/`.
A scene is a fixture, not native end-to-end proof; the meeting capsules and
HUDs have no scene.

The supported Linux beta host is x86_64 Arch Linux; releases build on Ubuntu
24.04 (x64 and ARM64) for a glibc 2.39 floor. Install its
native build dependencies with:

```sh
sudo pacman -S --needed base-devel git rustup python alsa-lib curl jq openssl xxd \
  util-linux libxkbcommon libxkbcommon-x11 libx11 libxcb openblas \
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
an explicit `HEX_TEAM_ID` and matching `HEX_NOTARY_PROFILE`; the build
and release scripts verify the selected team rather than assuming an Anomaly team.
`scripts/validate-app.sh` checks this identity before preparation and publication.
The move to Anomaly's team (`5NZ4Q7NXJ4`) is planned in
`docs/plans/macos-signing.md`; the manual `macOS signed build` workflow signs
and notarizes an unpublished candidate once the Apple secrets are in the
`release` environment.
`scripts/release-app.sh prepare` leaves `hex-<version>-macos-arm64.dmg` and
`hex-<version>-macos-arm64.zip` (root `Hex.app`, the updater's archive); both
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
tag; `v*` tags are unrestricted because nothing triggers on them. Like OpenCode's release, build jobs (Linux x64 and ARM64 on
Ubuntu 24.04 runners with their `.deb` and `.rpm`, Windows x64 and ARM64 through `.github/workflows/build-desktop.yml`,
macOS as an ad-hoc signed `HEX_RELEASE_UNSIGNED=1` app, installers through
`windows-installers.yml`, AUR and Homebrew staging) run repository code with
no environment, secret, or id-token; sign and publish jobs run in `release`,
check out only `.github/release/`, and run only those scripts and SHA-pinned
actions on artifacts treated as data (macOS Developer ID signing,
notarization, and stapling; Azure Authenticode; the release service through
OIDC; the AUR and Homebrew pushes). Every artifact carries a `SHA256SUMS`
whose digest its job reports and every consumer checks. Site deploy follows
the same split: its build job has no secret, and its `release` job runs only
`.github/release/deploy-site.sh`, which deploys the verified Build Output only
when it names exactly that mode's site Worker, routes, and bindings, with the
cf CLI locked in `.github/release/cf/`; Legacy appcast checks out only
`.github/release/` and the appcast. Keep that boundary:
never add a step that builds, installs dependencies, or runs repository code
to a job that holds a secret or an OIDC token, and pin every action by SHA.
`.github/` changes need Dax (CODEOWNERS and the "release boundary" ruleset).
`.github/workflows/test-builds.yml` calls the same desktop
workflow on every push to `dev` with `test-build`, plus an ad-hoc signed
`Hex Test.app` (`HEX_TEST_BUILD=1 scripts/build-app.sh`: bundle ID
`anomaly.hex.test`, its own `hex-test` data folder and `~/.config/hex-test`;
it must never read the real `hex` data), as unsigned 14-day artifacts for
teammates (`docs/test-builds.md`). `HEX_TEST_BUILD=1` builds never update
themselves, and neither do local builds (`HEX_LOCAL_BUILD=1`, the owner's own
build from a checkout: footer "Local build · updates off", version label with its
commit, no channel row). Every push to `dev` (except docs/site-only changes) publishes a dev-channel
build (the service lets a run from `dev` publish only the `dev` channel, and
only signed platforms are published). Stable runs only from `production`.
Like OpenCode, every run uses the one `release` environment, with no
required reviewer; its deployment policy admits `dev` and `production`. Rerunning
after an interruption is safe: the service stores files by hash and accepts
the same version again.

`hex-setup-<version>-windows-<x64|arm64>.exe` is a per-user Inno Setup
installer (`packaging/windows/hex.iss`, built by
`scripts/build-windows-installer.ps1`) that installs exactly the managed
layout `install-windows.ps1` creates, so self-updates keep working:
`%APPDATA%\hex\versions\<version>\hex.exe`, `current-version`, the
`install.json` direct-install receipt, and a Start Menu shortcut carrying the
managed mark. Its uninstaller lives in `hex\uninstall` because
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

Run `scripts/build-linux.sh` on x86_64 or aarch64 Linux (then
`scripts/package-linux.sh` for the `.deb` and `.rpm`) and `scripts/build-windows.sh`
from Git Bash once per architecture with `HEX_WINDOWS_ARCH=x86_64` (the
default; Vulkan SDK required) or `aarch64` (clang-cl and Ninja; best on an
ARM64 host). Builds use `-DGGML_NATIVE=OFF` so the CPU kernels stay portable,
and the static runtime in `.cargo/config.toml` keeps the Visual C++
Redistributable unnecessary; the release workflow asserts neither Windows
executable imports it. Every build embeds Vulkan shaders only for f32, f16,
and q8_0 tensors and no MUL_MAT_ID (`CMAKE_ARGS` in `.cargo/config.toml`,
through the patched `transcribe-cpp-sys` in `Cargo.toml`): about 7 MB of
SPIR-V instead of 44 MB. ggml aborts a Vulkan run whose weights use another
type, so widen `GGML_VULKAN_SHADER_TYPES` before the catalog gains a non-Q8_0
model. Release executables are stripped (`[profile.release]`). The
installers live in the locked `.github/release/` (`install-linux.sh`,
`install-windows.ps1`; the Installers workflow serves them at
`https://anoma.ly/hex/files/`) and install from the service's manifest,
checking each file's size and SHA-256 plus the Linux ed25519 signature or the
Windows Authenticode signature; `scripts/test-install-linux-release.sh`
exercises the Linux one against a fixture service with a fixture key.

Use `termctrl` to verify TUI changes at both wide and narrow dimensions. The
`hex dev dashboard` keys are `1` for the activity log, `2` for meetings,
Tab to cycle, and `q`/Escape to quit.

## Diagnostics

- `~/Library/Application Support/hex/logs/live.ndjson`: state,
  transcript, outcome, processing, and context.
- `~/Library/Application Support/hex/logs/process.log`: Rust,
  CoreAudio, and context-adapter diagnostics.
- `~/Library/Application Support/hex/meetings/`: manifests, separate
  tracks, the rolling in-call transcript, and atomically published final
  transcripts.

Windows uses `%APPDATA%\hex` and Linux `~/.local/share/hex` (or under
`XDG_DATA_HOME`). Hex keeps no compatibility with earlier `voice-control`
folders or per-OS settings files.

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
