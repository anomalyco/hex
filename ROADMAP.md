# Roadmap

## Browser Context

Generalize the current Brave AppleScript implementation into browser adapters
without leaking browser identity into command definitions. Candidate adapters
include Chromium-family AppleScript, Safari AppleScript, browser extensions,
and accessibility APIs. Contextual commands should continue to depend on facts
such as foreground application, active URL, host, and page identity.

## Assisted Diagnostics

Add an explicit log-audit workflow that can summarize a selected time range and
recommend command-taxonomy improvements. It should separate:

- Audio capture or device failures
- Recognition errors and recurring observed forms
- Sleeping command utterances or discarded Option taps
- Context predicate mismatches
- Ambiguous, unmatched, or incorrectly resolved commands
- Action execution failures

Recommendations should be inspectable proposals, not automatic mutations of
the command configuration.

## Dictation Pipelines

Option push-to-talk uses Parakeet v2 through `transcribe-rs` ONNX inference and
remains available while command recognition sleeps. Voice-delimited sessions
now support paste, send, cancel, and Captain's Log journal capture. Add
composable effectful `String -> Result<String>` processors, including optional
OpenCode-backed processors.

The HEX desktop app now owns command recognition and presents a click-through,
top-center Option HUD. Its red recording capsule is driven by live RMS/peak
levels and contracts into a fixed blue processing orb without changing capture
semantics.

Add end-of-speech detection without weakening the explicit consequential
controls. Keep Option dictation available while command recognition sleeps;
voice-delimited sessions remain gated by listening mode.

## Meetings

The first macOS 15+ vertical slice records separate ScreenCaptureKit microphone
and system-audio tracks, writes owner-only artifacts, transcribes 30-second
chunks locally, and exposes merged source-labeled transcripts in the dashboard.

The signed HEX app observes CoreAudio process input state for supported
meeting applications, debounces activations, and offers explicit Record/Not Now
actions in a non-activating GPUI panel. Recording remains opt-in; the same panel
becomes a visible recording timer, Stop control, transcription state, and
terminal outcome.

Next priorities are local EventKit correlation for calendar titles and scheduled
reminders, asking whether to stop when a meeting application's microphone becomes
inactive, menu-bar controls, crash recovery for active WAV files, resumable
transcription, explicit input-device selection, capture-gap events, and transcript
scrolling/search. Evaluate synchronized audio playback, echo annotation, and
person-level diarization only against a real recording corpus. Add a Linux capture
seam only when implementing the PipeWire adapter.
