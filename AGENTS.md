# HEX Agent Guide

## Purpose

Build HEX as a local, observable macOS voice appliance with explicit Linux
X11 and wlroots-compatible Wayland beta contracts. Keep the engine native Rust
and keep consequential behavior explicit. Protected commands and typed captures
remain compiled Rust; ordinary literal commands live in the explicit TypeScript user config. User-facing
runtime settings persist in Application Support.

The distributed release starts in hotkey-dictation-only mode. Voice commands and
their catalog remain available as a persisted opt-in that defaults off.
`DEVELOPER_FEATURES_ENABLED` keeps meetings and their UI/CLI surfaces available
only in debug builds.

## Architecture

- `audio`: `cpal` device enumeration and timestamped mono float PCM delivery,
  live selection, and bounded stream recovery.
- `moonshine`: the only Moonshine C adapter and the streaming recognizer.
- `suppression`: the macOS event tap, shortcut suppression, and the
  configurable dictation-hotkey state machine.
- `keyboard`: active-layout key resolution and balanced synthetic shortcuts.
- `dictation`: warm pre-roll, growable capture, and 16 kHz
  local-transcription resampling.
- `dictation_audio`: the authoritative microphone timeline, lazy stream
  lifecycle, recording owner, exact shortcut boundaries, recovery handoff, and
  disposable bounded command audio projection.
- `recording_environment`: serialized RAII ownership of idle-sleep prevention,
  output muting, and supported media-player pause/resume behavior.
- `dictation_processor`: context-selected, deadline-bounded OpenCode rewrite
  profiles with raw-transcript fallback. The macOS app discovers the `opencode2`
  beta executable, links missing installs to `https://v2.opencode.ai/`, and uses
  `opencode2 api get` to discover or start its managed service. Generation uses
  CLI-managed discovery with the matching owner-only service registration and
  authenticated loopback HTTP through the system curl; request bodies and
  credentials use stdin, never argv or new temporary files.
- `parakeet`: the strict-Metal `transcribe.cpp` adapter plus bounded inference,
  processing, ordered output, paste, last-result, and meeting-delta workers.
- `apple_speech`: the Swift `SpeechAnalyzer` bridge with per-locale support
  checks, asset reservation, and batch transcription.
- `transcription`: runtime selection and transactional warm-model activation.
- `transcription_models`: the compiled model catalog, language recommendations,
  pinned artifact verification, and lazy installation.
- `transcription_service`: bounded host-audio admission, hostile WAV validation,
  normalization, cancellation, and warm inference ownership.
- `local_api`: authenticated loopback discovery or direct-child endpoint
  handoff, bounded HTTP parsing, model preparation progress, and raw
  transcription routes.
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
- `command_grammar`: typed command patterns, captures, overlap detection, and
  command catalog metadata.
- `commands`: pure contextual resolution plus bounded asynchronous macOS action
  execution.
- `personal_commands`: the Bun-hosted `@hex/commands` TypeScript workspace:
  managed SDK provisioning, the watch-reloaded user command and transformation
  host, bounded invocation dispatch, and status snapshots.
- `text_replacements`: phrase-boundary-aware compilation of configured text
  replacements applied longest-match-first to transcripts.
- `config`: compiled commands, preferred input devices, and dictation profiles.
- `context`: native foreground application and window context capture plus
  browser context. Browser host is the domain concept; Brave AppleScript is only
  the first adapter.
- `application_catalog`: cached installed-application discovery with normalized
  bundle identity and rendered icons for the UI.
- `app_settings`: persisted settings and live runtime projection for commands,
  hotkeys, microphone and transcription selection, recording behavior,
  processing, sound volume, and Dock policy.
- `login_item`: the native `SMAppService.mainAppService` adapter. macOS owns
  registration state; it is deliberately not duplicated in `settings.json`.
- `app_paths`: the Application Support owner for runtime logs and shared state.
- `onboarding`: required permission health, selected dictation-model
  installation, the release startup gate, and opt-in command-model setup.
- `sparkle`: packaged-app-only Sparkle lifecycle and manual update checks.
- `linux`, `linux_app`, `linux_dictation`, `linux_input`, `linux_paste`,
  `linux_settings`, `linux_transcriber`: the Linux beta CLI, GPUI shell,
  hotkey capture-transcribe-paste loop, persisted settings, and `transcribe.cpp`.
- `linux_session`: display-backend selection matching GPUI's nonempty
  `WAYLAND_DISPLAY` rule, independent of persisted preferences.
- `linux_wayland_input`: read-only evdev input, explicit physical key mappings,
  cancellable shortcut capture, exact chord state, and bounded device rediscovery.
- `linux_desktop`: the single process-owned GTK thread for the X11 tray and
  focus-free, click-through Wayland recording/processing HUD. A listener never
  initializes or shuts down a separate GTK runtime.
- `linux_updater`: signed direct-install updates, bounded downloads, atomic
  version activation, and restart handoff for user-local Linux installs.
- `history`: the owner-only bounded retained-dictation store: retention
  windows with hard entry and byte caps, atomic crash-safe persistence, and
  search. Text and bounded metadata only, never audio.
- `events`: bounded asynchronous append-only NDJSON observations and bounded
  incremental reading; `dashboard` and the GPUI Activity pane are read-only
  projections.
- `desktop_activity`: the shared listener, device, transcript, and session
  projection over `EventReader`.
- `desktop_host`: semantic desktop capabilities, portable UI snapshots, and
  typed actions implemented by the macOS root and contained Linux adapter.
- `desktop_ui`: platform-neutral GPUI visual tokens and controls shared by both
  desktop roots, including the mandatory pane scaffold: `pane_header` /
  `pane_header_with_action`, `pane_body`, `pane_content`, the shared
  `header_button` action chip, and the single `PANE_CONTENT_WIDTH` and
  `PANE_LIST_WIDTH` layout constants.
- `text_input`: the shared GPUI single- and multi-line text input with editing,
  selection, clipboard, and input-method support.
- `desktop_transcription_picker`: the single GPUI language/model picker used by
  both desktop roots over portable model presentation and platform preparation
  callbacks.
- `app_window`: the production Settings, Modes with mode-owned processing,
  Voice Action, History, Replacements, and opt-in Commands shell plus
  developer-only Meetings, Activity, and HUD Lab panes.
- `status_item`: the persistent macOS menu-bar owner for Settings, Paste Last
  Dictation, update checks, and orderly application shutdown.
- `dictation_indicator`: the click-through Metal/GPUI capture and processing HUD.
- `meeting`: explicit ScreenCaptureKit capture, owner-only WAV and transcript
  artifacts, final local-model publication, and recovery.
- `meeting_live`: bounded dual-stream Moonshine draft transcription and
  byte-offset transcript tailing.
- `microphone_activity`: permission-light CoreAudio process input observation.
- `meeting_detection`: pure provider classification, debounce, and suppression.
- `meeting_watcher`: GPUI application lifecycle, recognition ownership, meeting
  offers, and explicit handoff into capture.
- `instance`: the exclusive Application Support lock that prevents concurrent
  command listeners.
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
  recognition additionally requires Moonshine.
- Among recognized voice commands, sleeping mode accepts only standalone wake
  phrases. Dictation and explicit paste shortcuts remain available.
- Unmatched completed speech is ignored and logged.
- The dictation shortcut defaults to Option but supports modifier-only,
  modifier-plus-key, standalone Globe/Fn, and standalone function-key bindings.
  Capturing a new binding suspends global matching.
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
- `Release microphone while idle` and Commands are mutually exclusive through
  explicit confirmed transitions and a shared runtime policy. When release is
  enabled, pressing the shortcut opens the selected device asynchronously with
  no pre-roll, preserves the physical press for the hold threshold, discards a
  release before readiness, and closes only after the authoritative capture is
  idle. Accepted jobs never keep the stream open.
- Dictation remains available while command recognition sleeps.
- Model inference, optional post-processing, paste, and application actions must
  never block authoritative audio capture. Their queues remain bounded.
- CoreAudio capture timestamps and CGEvent shortcut timestamps share the macOS
  boot-time clock, but raw CGEvent Mach ticks must be converted through the
  current timebase before comparison. Delayed press handling reconstructs the
  original onset from the timeline; delayed release handling excludes audio
  captured after the physical release.
- Active dictation capture is lossless with respect to Moonshine, event, UI,
  context, and worker stalls. Command recognition is explicitly best-effort:
  its backlog is bounded by duration, and pressure invalidates the generation,
  discards stale audio and updates, and resets Moonshine without touching the
  active recording.
- Starting a new capture never cancels accepted dictation work. Pending jobs
  preserve submission-order output while capture remains immediately available.
- Completing or pasting an older job must not reconcile or finish a newer
  capture. Shortcut boundaries come from delivered CGEvent timestamps; do not
  fabricate a release timestamp from later physical state.
- Escape cancels the active capture first, then the newest unfinished dictation.
  Cancelled jobs never paste, update the last result, or block later output.
- Model switches activate only after the pinned artifact is checksum-verified,
  loaded on strict Metal, and prewarmed. A failed switch preserves the active
  model and persisted selection.
- A selected microphone switches live only while capture is idle. Opening the
  replacement must succeed before the old stream is dropped. If the persisted
  device is unavailable at startup, log the failure and fall back through the
  compiled preferred-device order to the macOS default. An explicit CLI
  `--device` remains authoritative for that listener process. A runtime stream
  failure cancels an incomplete capture, discards stale chunks, and retries the
  same selection with bounded backoff; it must not require a settings change or
  process restart.
- Feedback volume is persisted from zero through one; zero disables tones.
  Volume changes apply immediately and preview one recording-start tone.
- Launch at Login uses `SMAppService` for the signed main app. Treat both
  `NotRegistered` and `NotFound` as disabled before registration, represent
  `RequiresApproval` with a link to Login Items settings, and poll macOS as the
  source of truth instead of persisting a parallel Boolean.
- Voice Action is an explicit second capture target, defaulting to
  Option-Command. Promoting an active Option capture preserves all recorded
  audio. Selected text is optional prompt context; inaccessible or empty
  selections act as no selection. Use the dedicated OpenCode model and deadline,
  return only paste-ready text, and paste at the current focus. Failed, empty,
  cancelled, or timed-out actions paste nothing. Voice Action jobs share normal
  queueing and cancellation but never update the last dictation.
- Mode processing is best-effort and ordered: corrections, optional OpenCode
  rewriting, then selected TypeScript transformations. A failed step preserves
  the previous pipeline output. It applies to Paste and Send, but not meetings.
- OpenCode availability checks stay off the UI thread. When the app finds the
  `opencode2` executable, it loads the catalog through `opencode2 api`, which
  automatically starts or reuses the managed service. HEX never invokes
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
  titles. Every retention window remains subject to hard entry and byte caps,
  and recording stops while retention is Off.
- Developer-only meeting detection may inspect process audio metadata but must
  not capture samples. Detection can offer recording; it must never start
  automatically. Release builds must not start the meeting controller.
- Meeting recording prevents idle sleep but does not apply dictation mute or
  pause-media behavior.
- Live meeting drafts remain available if final transcription fails. Final
  publication is atomic; adjacent same-source entries within three seconds form
  one displayed turn labeled `You` or `Computer`.
- Meeting paste cursors advance only after successful insertion and reset when
  HEX restarts.
- Visible state remains `Transcribing` until every accepted transcription or
  paste output has completed, including ordered post-processing and insertion.
- The HUD is observational and click-through. It must not alter capture
  boundaries, steal focus, or block controls in the foreground application.
- Every desktop pane renders the shared scaffold from `desktop_ui`: the
  bounded pane header plus one centered content column at `PANE_CONTENT_WIDTH`,
  with list+detail panes using the fixed `PANE_LIST_WIDTH` list column. Panes
  must not introduce their own header treatments or content widths, and text
  columns beside a fixed column carry `flex_1` with a zero min-width so long
  lines wrap instead of widening the pane.
- Public app updates are Developer ID signed, notarized, stapled, EdDSA signed,
  published artifact-first/feed-last, and installed through Sparkle.
- Linux direct-install updates accept only a strictly newer signed stable
  x86_64 manifest, verify exact size and SHA-256 from a content-addressed
  artifact, and atomically switch the user-local `current` version. Never
  overwrite development, root, or package-manager-owned binaries.
- The Wayland beta requires compatible clipboard, virtual-keyboard, and
  layer-shell protocols plus explicit read access to all `/dev/input/event*`
  nodes. Do not silently fall back to XWayland or privileged input injection.
  Raw input observes, but does not suppress, physical US-labeled keys. Explain
  the broad keystroke access; never grant device permissions automatically.
- X11 shortcut capture uses focused GPUI input, not evdev permissions. Editing
  shortcut/double-tap settings stops and restores only a previously running
  listener; cancellation restores the old binding unless its save already
  committed. Settings must expose listener state, recovery, and errors. Without
  a usable tray, closing Settings quits after draining workers, never detaches
  an unmanageable microphone. HUD teardown only hides that listener's HUD.
- Linux paste keeps transcripts off helper argv, bounds helper I/O, and waits
  for physical modifiers without discarding accepted output. Shutdown cancels
  that wait. New installs use Ctrl-V; the terminal-paste preference selects
  Ctrl-Shift-V and defaults on for persisted legacy X11 settings. The beta retains
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
direct `npm publish`, after the bootstrap release.

```sh
./scripts/setup.sh
./scripts/setup-parakeet.sh
cargo run -- listen
cargo run -- status
cargo run -- meeting record --title "Design sync"
cargo run -- meeting probe
cargo run -- meeting watch --preview
cargo run -- app --preview-dictation
cargo run -- preview onboarding
cargo run -- preview transcription-picker --language zh --model-state installed
./scripts/capture-preview.sh /tmp/hex-preview.png settings
./scripts/capture-preview.sh /tmp/hex-modes.png modes
./scripts/capture-preview.sh /tmp/hex-modes-collapsed.png modes --collapse-mode-processing
./scripts/capture-preview.sh /tmp/hex-modes-picker.png modes --collapse-mode-processing --open-transformation-picker
./scripts/capture-preview.sh /tmp/hex-modes-global.png modes --collapse-mode-processing --select-global-mode
./scripts/capture-preview.sh /tmp/hex-voice-action-unavailable.png voice-action --opencode-unavailable
HEX_PREVIEW_SKIP_BUILD=1 ./scripts/capture-preview.sh /tmp/hex-modes.png modes
./scripts/install-app.sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
./scripts/test-app-identity.sh # macOS only
./scripts/test-install-linux-release.sh # x86_64 Linux only
cd sdk/typescript && bun run check && bun run test && bun run build
```

Use `scripts/capture-preview.sh` as the default desktop-UI iteration loop. It
builds the release binary, launches one isolated deterministic preview, waits
for that process's `HEX` window, captures only that window, and terminates the
preview. Targets include `settings`, `modes`, `voice-action`, `commands`,
`meetings`, `activity`, `onboarding`, `transcription-picker`, `hud-lab`, and
`dictation-hud`. The Modes preview includes representative activation,
correction, model-variant, and transformation data so it exercises the complete
editor. Pass `--collapse-mode-processing` to expose the lower transformation
and deletion states without scrolling. Use `--open-transformation-picker`,
`--select-global-mode`, and `--opencode-unavailable` to capture those states
directly instead of scripting pointer coordinates. After a successful build, set
`HEX_PREVIEW_SKIP_BUILD=1` for repeated
captures that do not require recompilation; `HEX_PREVIEW_BINARY` can override
the release binary path. Keep release previews authoritative for production
navigation because debug builds expose developer-only panes.

The supported Linux beta and release host use x86_64 Arch Linux. Install its
native build dependencies with:

```sh
sudo pacman -S --needed base-devel git rustup python alsa-lib curl jq openssl xxd \
  util-linux gtk3 gtk-layer-shell libappindicator-gtk3 libxkbcommon \
  libxkbcommon-x11 libx11 libxcb openblas vulkan-headers vulkan-icd-loader \
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
X11/Wayland, keyboard hotplug, microphone, and target-application smoke tests.

Automatic microphone selection follows the compiled preference order in
`src/config.rs`, then falls back to the macOS default. A saved microphone takes
precedence while available; override everything with `--device`. The app bundle
build requires Xcode 26 for Icon Composer compilation and a Developer ID signing
identity. `scripts/release-app.sh` prepares a notarized and stapled DMG plus its
signed Sparkle appcast; run `scripts/release-app.sh publish` only after validating
the prepared artifact. The Anomaly app is named `Hex`, packaged as `Hex.app`,
with bundle identifier `ly.anoma.Hex` and executable `hex`. Signing requires an
explicit Anomaly `VOICE_CONTROL_TEAM_ID` and `HEX_NOTARY_PROFILE`; the main app
build and release scripts must not default to the personal signing team.
`scripts/validate-app.sh` checks this identity before preparation and publication.
There is no Swift migration: never import Swift preferences or data, adopt its
bundle identifier, publish Rust artifacts to its S3 feed, or automatically quit,
delete, or replace the Swift app. Prefer a website-only informational item in
the legacy Sparkle feed, with no enclosure, pointing to the new app for manual
installation and fresh setup. Publish it only after the new artifact is live. See
`docs/plans/swift-app-handoff.md`. The Rust data root remains unchanged, but the
new app identity requires fresh macOS permission grants. Validate signed
distribution with the Anomaly team before publishing an app update.

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

Linux releases are prepared and published separately on the supported x86_64
Linux host. Inject the Ed25519 private PEM as `HEX_LINUX_SIGNING_KEY`, run
`scripts/release-linux.sh prepare`, validate the artifact, then run
`scripts/release-linux.sh publish`. The script refuses a key that does not match
the public key compiled into HEX, refuses a non-monotonic stable release, and
publishes the signed feed last.

Use `termctrl` to verify TUI changes at both wide and narrow dimensions. The
dashboard keys are `1` for commands, `2` for the activity log, `3` for meetings,
Tab to cycle, and `q`/Escape to quit.

## Diagnostics

- `~/Library/Application Support/voice-control/logs/live.ndjson`: state,
  transcript, command decision, outcome, processing, and context.
- `~/Library/Application Support/voice-control/logs/process.log`: Rust,
  CoreAudio, Moonshine, and context-adapter diagnostics.
- `~/Library/Application Support/voice-control/meetings/`: manifests, separate
  tracks, recoverable live drafts, and atomically published final transcripts.

When diagnosing a missed command, inspect both logs and distinguish microphone
capture, transcription, command mode/dictation, context matching, command resolution,
and action execution before changing aliases or thresholds.

## Future Direction

See `ROADMAP.md`. Do not add hypothetical seams for roadmap items. Introduce a
seam once there are two real adapters or a current test requires substitution.

## Current Work

`ROADMAP.md` is the authoritative work list. Keep these constraints visible:

- Validate public onboarding from a clean macOS account and signed Linux updates
  on the supported Arch/i3 host.
- Validate native Wayland on a real compatible compositor, including physical
  device reconnect, focus/click-through, target paste, and tray-less shutdown.
- Add a second real browser adapter before generalizing browser context.
- Do not turn concrete macOS, Linux X11, or command modules into hypothetical
  platform or plugin frameworks.
