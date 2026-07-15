# Voice Control Agent Guide

## Purpose

Build a local, observable, distributable voice-control system for macOS. Keep
the engine native Rust. Configuration is compiled Rust code until real usage
justifies a plugin or data configuration seam.

## Architecture

- `audio`: CoreAudio capture through `cpal`; emits mono float PCM.
- `moonshine`: the only Moonshine C interface adapter and primary unsafe seam.
- `suppression`: the macOS modifier-state and event-tap adapter.
- `keyboard`: the macOS keyboard-layout and synthetic-event adapter.
- `dictation`: warm pre-roll, bounded capture, and 16 kHz resampling.
- `dictation_indicator`: click-through GPUI Option-dictation lifecycle and
  live microphone meter projection.
- `parakeet`: bounded background ONNX model worker and transcription lifecycle.
- `recognition`: owns microphone, recognizer, command mode, and dictation capture.
- `commands`: typed command definitions, pure resolution, generated catalog,
  and macOS action execution.
- `config`: the compiled personal command and device configuration.
- `context`: foreground application and browser context capture. Browser host is
  the domain concept; Brave AppleScript is only the first adapter.
- `events`: append-only NDJSON observation format.
- `dashboard`: read-only Ratatui projection of events and command catalog.
- `feedback`: nonverbal mode and failure tones.
- `meeting`: explicit macOS ScreenCaptureKit microphone and system-audio
  capture, owner-only artifacts, chunked transcription, and transcript storage.
- `microphone_activity`: permission-light CoreAudio process input observation.
- `meeting_detection`: pure supported-provider classification and suppression.
- `meeting_watcher`: GPUI desktop lifecycle, recognition worker ownership,
  floating meeting offers, and explicit handoff into meeting capture.

Keep modules deep. Callers should not coordinate Moonshine stream handles,
CoreAudio formats, AppleScript details, or event serialization.

## Invariants

- Start in listening mode. Sleeping is an explicit user action.
- While sleeping, only standalone wake phrases are actionable.
- Unmatched completed speech is ignored and logged.
- Hold Option to dictate; release to transcribe and paste. Brief taps discard.
- Pressing or releasing Option resets Moonshine so dictation audio cannot leak
  into a later command.
- Dictation remains available while command recognition sleeps.
- Model inference and paste must never block the audio-consumption loop.
- Dictation capture and worker queues must remain bounded.
- Execute commands only from completed Moonshine lines.
- Contextual commands enter the candidate set only when their predicate matches.
- The command catalog must be generated from resolver definitions, never
  maintained separately.
- Successful commands rely on their action for feedback. Wake/sleep use quiet
  tones; execution failures use an error tone.
- Do not persist captured audio by default. Explicit foreground meeting
  recording is the sole current exception and must remain visibly active.
- Meeting detection may inspect process audio metadata but must not capture
  samples. Detection can offer recording; it must never start automatically.
- The Option HUD is observational and click-through. It must not alter capture
  boundaries, steal focus, or block controls in the foreground application.

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
./scripts/install-app.sh
cargo fmt --check
cargo test
cargo check
```

The listener prefers Universal Audio Thunderbolt, then Studio Display
Microphone, then the macOS default. Override with `--device`.

Use `termctrl` to verify TUI changes at both wide and narrow dimensions. The
dashboard keys are `1` for commands, `2` for the activity log, `3` for meetings,
Tab to cycle, and `q`/Escape to quit.

## Diagnostics

- `logs/live.ndjson`: state, transcript, command decision, outcome, and context.
- `logs/process.log`: Rust, CoreAudio, Moonshine, and context-adapter diagnostics.

When diagnosing a missed command, inspect both logs and distinguish microphone
capture, transcription, command mode/dictation, context matching, command resolution,
and action execution before changing aliases or thresholds.

## Future Direction

See `ROADMAP.md`. Do not add hypothetical seams for roadmap items. Introduce a
seam once there are two real adapters or a current test requires substitution.

## Follow-up

- Add Linux support while preserving the native Rust core and existing macOS
  behavior. Introduce platform seams only where the current macOS adapters for
  input monitoring, keyboard output, foreground context, or action execution
  require a real Linux counterpart.
- Move potentially slow application actions off the audio-consumption loop.
- Make normal Ctrl-C shutdown emit a final stopping event, and let the dashboard
  distinguish a stale listener from a live one.
- Track dictation-worker occupancy explicitly so queued jobs cannot make the
  visible state return to listening too early.
- Tail a bounded event projection instead of reparsing the full append-only log
  on every dashboard refresh.
- Package the executable in a signed app bundle with stable Screen Recording
  and Microphone permission identity before distribution.
- Replace foreground-application polling through System Events with native
  `NSWorkspace.frontmostApplication` when context capture is next revised.
