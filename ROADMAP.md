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
remains available while command recognition sleeps. Add voice-delimited sessions
and composable effectful `String -> Result<String>` processors, including
optional OpenCode-backed processors.

Add voice-delimited dictation with explicit start/cancel/finish phrases and
end-of-speech detection. It must remain independent of command sleep mode and
share the same bounded capture and processing pipeline as Option push-to-talk.
