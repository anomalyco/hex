# HEX Agent Guide

## Purpose

Build HEX as a local, observable macOS voice appliance with an explicit Linux
X11 beta contract. Keep the engine native Rust and keep consequential behavior
explicit. Command definitions and preferred devices remain compiled Rust until
a second real consumer justifies a data or plugin seam; user-facing runtime
settings persist in Application Support.

The distributed release starts in hotkey-dictation-only mode. Voice commands and
their catalog remain available as a persisted opt-in that defaults off.
`DEVELOPER_FEATURES_ENABLED` keeps meetings and their UI/CLI surfaces available
only in debug builds.

## Architecture

- `audio`: `cpal` device enumeration and capture, mono float PCM, and
  dropped-chunk accounting.
- `moonshine`: the only Moonshine C adapter and the streaming recognizer.
- `suppression`: the bounded macOS event tap, shortcut suppression, and the
  configurable dictation-hotkey state machine.
- `keyboard`: active-layout key resolution and balanced synthetic shortcuts.
- `dictation`: warm pre-roll, bounded capture, duration limits, and 16 kHz
  local-transcription resampling.
- `recording_environment`: serialized RAII ownership of idle-sleep prevention,
  output muting, and supported media-player pause/resume behavior.
- `dictation_processor`: context-selected, deadline-bounded OpenCode rewrite
  profiles with raw-transcript fallback.
- `parakeet`: the strict-Metal `transcribe.cpp` adapter plus bounded inference,
  processing, ordered output, paste, last-result, and meeting-delta workers.
- `transcription_models`: the compiled model catalog, language recommendations,
  pinned artifact verification, and lazy installation.
- `paste`: clipboard insertion, continuation joins, and generation-safe
  clipboard restoration.
- `selected_text`: bounded selected-text capture through Accessibility without
  touching the clipboard.
- `recognition`: the microphone-loop coordinator for Moonshine, command mode,
  hotkey and voice-delimited capture, workers, and observations.
- `command_grammar`: typed command patterns, captures, overlap detection, and
  command catalog metadata.
- `commands`: pure contextual resolution plus bounded asynchronous macOS action
  execution.
- `config`: compiled commands, preferred input devices, and dictation profiles.
- `context`: foreground application and browser context capture. Browser host is
  the domain concept; Brave AppleScript is only the first adapter.
- `app_settings`: persisted settings and live runtime projection for commands,
  hotkeys, microphone and transcription selection, recording behavior,
  processing, sound volume, sleep prevention, and Dock policy.
- `login_item`: the native `SMAppService.mainAppService` adapter. macOS owns
  registration state; it is deliberately not duplicated in `settings.json`.
- `app_paths`: the Application Support owner for runtime logs and shared state.
- `onboarding`: required permission health, selected dictation-model
  installation, the release startup gate, and opt-in command-model setup.
- `sparkle`: packaged-app-only Sparkle lifecycle and manual update checks.
- `linux_updater`: signed direct-install updates, bounded downloads, atomic
  version activation, and restart handoff for user-local Linux installs.
- `events`: append-only NDJSON observations; `dashboard` and the GPUI Activity
  pane are read-only projections.
- `app_window`: the production Settings, Modes, Replacements, and opt-in
  Commands shell plus developer-only Meetings and Activity panes.
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
  modifier-plus-key, and standalone function-key bindings. Capturing a new
  binding suspends global matching.
- Hold the shortcut to dictate and release to transcribe. Captures shorter than
  300 ms discard, 450 ms of warm pre-roll protects speech onset, and the
  60-second limit finalizes automatically.
- When enabled, a second shortcut tap within 300 ms locks dictation. Press the
  shortcut again to finish or Escape to cancel.
- When commands are enabled, every dictation or paste hotkey action resets
  Moonshine so shortcut audio cannot leak into a later command.
- Recording audio behavior begins only after the intentional-hold threshold.
  Ordinary shortcut chords must not mute output or pause media.
- Dictation remains available while command recognition sleeps.
- Model inference, optional post-processing, paste, and application actions must
  never block the audio-consumption loop. Their queues remain bounded.
- Starting a new capture never cancels accepted dictation work. Pending jobs
  preserve submission-order output while capture remains immediately available.
- Completing or pasting an older job must not reconcile or finish a newer
  capture. Physical-state recovery runs only after the event tap drops input.
- Escape cancels the active capture first, then the newest unfinished dictation.
  Cancelled jobs never paste, update the last result, or block later output.
- Model switches activate only after the pinned artifact is checksum-verified,
  loaded on strict Metal, and prewarmed. A failed switch preserves the active
  model and persisted selection.
- A selected microphone switches live only while capture is idle. Opening the
  replacement must succeed before the old stream is dropped. If the persisted
  device is unavailable at startup, log the failure and fall back through the
  compiled preferred-device order to the macOS default. An explicit CLI
  `--device` remains authoritative for that listener process.
- Feedback volume is persisted from zero through one; zero disables tones.
  Volume changes apply immediately and preview one recording-start tone.
- Launch at Login uses `SMAppService` for the signed main app. Treat both
  `NotRegistered` and `NotFound` as disabled before registration, represent
  `RequiresApproval` with a link to Login Items settings, and poll macOS as the
  source of truth instead of persisting a parallel Boolean.
- Voice edit is an explicit second capture path, defaulting to Option-Command.
  Snapshot a non-empty selection before recording, use the spoken transcript as
  an edit instruction, and replace only after the foreground application and
  exact selection are revalidated. Missing, oversized, changed, or unsupported
  selections and failed processing must leave the document unchanged. Edit jobs
  share normal queueing and cancellation but never update the last dictation.
- Post-processing is optional and best-effort. Empty, failed, or timed-out
  processing falls back to the raw local transcript. It applies to Paste and
  Send, not Captain's Log or meetings.
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
  recording is the sole current exception and must remain visibly active.
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
- Public app updates are Developer ID signed, notarized, stapled, EdDSA signed,
  published artifact-first/feed-last, and installed through Sparkle.
- Linux direct-install updates accept only a strictly newer signed stable
  x86_64 manifest, verify exact size and SHA-256 from a content-addressed
  artifact, and atomically switch the user-local `current` version. Never
  overwrite development, root, or package-manager-owned binaries.

## Development

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
./scripts/install-app.sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
```

The automatic microphone setting prefers Universal Audio Thunderbolt, then
Studio Display Microphone, then the macOS default. A saved microphone takes
precedence while available; override everything with `--device`. The app bundle
build requires Xcode 26 for Icon Composer compilation and a Developer ID signing
identity. `scripts/release-app.sh` prepares a notarized and stapled DMG plus its
signed Sparkle appcast; run `scripts/release-app.sh publish` only after validating
the prepared artifact.

`SMAppService` is meaningful only from a signed app installed in
`/Applications`. When replacing a local bundle outside Finder during a login-item
smoke test, register the new bundle with Launch Services before launch:

```sh
'/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister' \
  -f /Applications/HEX.app
```

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

- `~/Library/Application Support/voice-control/logs/live.ndjson`: state, transcript, command decision, outcome, processing,
  and context.
- `~/Library/Application Support/voice-control/logs/process.log`: Rust, CoreAudio, Moonshine, and context-adapter diagnostics.
- `~/Library/Application Support/voice-control/meetings/`: manifests, separate
  tracks, recoverable live drafts, and atomically published final transcripts.

When diagnosing a missed command, inspect both logs and distinguish microphone
capture, transcription, command mode/dictation, context matching, command resolution,
and action execution before changing aliases or thresholds.

## Future Direction

See `ROADMAP.md`. Do not add hypothetical seams for roadmap items. Introduce a
seam once there are two real adapters or a current test requires substitution.

## Current Gaps

- Validate a genuine signed Linux update between two released versions on the
  target Arch/i3 machine, including restart and retained-version rollback.
- Verify onboarding and model installation from a clean macOS account before
  broadly sharing the coworker download; the signed Sparkle update path is live.
- Finish validated legacy import and a persistent permission-health surface.
- Physically smoke-test custom shortcut injection and last-dictation paste in
  release; test the meeting paste shortcut separately in developer builds.
- Move observation writes off the microphone loop, tail bounded event
  projections, and distinguish a stale or crashed listener from a normal
  persisted `Stopping` state.
- Replace System Events foreground polling with
  `NSWorkspace.frontmostApplication`; add a second browser adapter before
  generalizing the context interface further.
- Add a persistent active-recording status, meeting idle-stop prompting, calendar
  correlation, active-WAV repair, resumable finalization, transcript search, and
  durable incremental transcript cursors.
- Diagnose command-audio drops and Slack Huddle detection with runtime evidence.
- Add edge tests for shutdown during capture, final-transcription failure, and
  voice-triggered meeting start/stop.
- Do not generalize the concrete macOS and Linux X11 adapters into hypothetical
  platform or plugin seams. Follow `LINUX_PORT_PLAN.md` for the selected target.
