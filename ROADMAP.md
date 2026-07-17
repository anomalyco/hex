# Roadmap

HEX ships the coworker product loop: configurable hold-to-talk local dictation,
explicit selected-text voice editing, optional context-aware OpenCode
post-processing, a Metal HUD, bounded paste workers, settings, and a signed GPUI
app bundle. Streaming commands and meetings remain implemented developer
prototypes, but are intentionally absent from distributed builds until their
product surfaces are redesigned.

## Validate The Coworker Release

Runtime logs now live in Application Support, first-run setup installs the
selected dictation model, and the release flow signs, notarizes,
staples, Gatekeeper-assesses, and publishes Sparkle updates through R2.

The quarantined public DMG and a genuine automatic 2.0.0-to-2.0.1 Sparkle update
have been validated on the development Mac. Before broadly sharing it, validate
the DMG from a clean macOS account, including every TCC permission, model
installation, and source-free launch. Preserve the stable bundle identity that
owns Microphone, Accessibility, Input Monitoring, and Automation permissions.

## Finish Settings And First-Run Health

Microphone selection, idle-safe live switching, unavailable-device fallback,
sound-effect volume, application/browser-host processing modes, and the native
`SMAppService` login-item toggle now ship. Runtime settings changes project live
and persist atomically; launch-at-login remains owned by macOS rather than the
settings file.

Add a persistent permission-health surface so revoked Microphone, Accessibility,
or Input Monitoring access remains diagnosable after onboarding. Keep true
processor chains as future work only if real profiles need composition.

Import useful original-Hex preferences once from the preferred Application
Support file. Validate each field, ignore malformed and unknown values, and
record completion. The legacy files disagree about launch-at-login, so migration
must ask or defer rather than guess.

## Harden The Real-Time Boundary

Move append-only observation serialization and flushing off the microphone loop
without making event delivery unbounded. Coalesce replaceable partial transcript
updates if pressure requires dropping work; preserve completed transcripts,
state transitions, command outcomes, and failures.

Diagnose recurring microphone chunk drops with runtime evidence before changing
queue sizes or recognition cadence. Physically smoke-test custom modifier and
key chords, lock behavior, cancellation, foreground insertion, and both reserved
paste shortcuts.

Add end-of-speech detection only if it preserves explicit Send, Cancel, and
locked-capture controls.

## Make Diagnostics Incremental

Replace repeated full-file parsing in Ratatui and GPUI with a shared bounded tail
projection. Normal shutdown already emits `Stopping`; add heartbeat or process
liveness evidence so a crash cannot leave an old `Listening` event looking live.

Add an inspectable audit that separates capture failures, recognition errors,
sleeping or discarded utterances, context mismatches, resolution misses, queue
pressure, and action failures. Recommendations may propose command-taxonomy
changes but must not mutate compiled configuration automatically.

## Generalize Context From Evidence

Replace System Events foreground polling with
`NSWorkspace.frontmostApplication` when context capture is next revised. Keep
command and dictation-profile semantics expressed in application and browser-host
terms rather than Brave-specific types.

Add one real second browser adapter before extracting a generalized adapter
framework. Candidate implementations include Chromium-family AppleScript,
Safari AppleScript, browser extensions, or Accessibility APIs. Diagnose Slack
Huddle detection separately from generic browser context. Surface context age
and failures instead of silently retaining an indefinitely stale snapshot.

## Complete Meeting Lifecycle Recovery

When meeting microphone activity disappears, ask `Keep Recording` or
`Stop & Transcribe`; never stop automatically. Add EventKit correlation for
titles and reminders, explicit input-device selection, structured capture-gap
observations, active-WAV repair, and resumable final transcription. Status and
final-publication recovery already exist; do not replace them.

Keep active recording discoverable after the main window closes through a
persistent menu-bar/status affordance. Add transcript search; scrolling and
follow-live behavior are already implemented.

Give durable transcript entries stable identities so live-to-final reconciliation,
incremental UI refresh, and meeting-delta paste can advance without rereading
broad history. Keep `You` and `Computer` source labels and the recoverable live
draft when final transcription fails.

Add end-to-end coverage for voice-triggered start/stop, shutdown during capture,
forced final-transcription failure, and live provider detection. Evaluate
synchronized playback, echo annotation, summarization, and person-level
diarization only against real meeting recordings.

## Deliberate Deferrals

Keep commands and profiles in compiled Rust until a second real consumer needs a
configuration format. Do not add a plugin or agent framework around the current
deep modules.

Do not add abstract platform seams before a concrete Linux implementation. If
Linux work begins, introduce adapters only where PipeWire capture, input
monitoring, keyboard output, foreground context, or action execution requires a
real alternative. Follow the phased scope, capability contracts, and exit
criteria in [`LINUX_PORT_PLAN.md`](LINUX_PORT_PLAN.md).
