# Activity/Debug Data Audit

**Status:** Archived audit. It records the evidence available during app-shell
prototyping; source-line references and listed gaps are not maintained as the
production event model changes.

## Recommendation

Build Activity as a projection over a fixed, bounded window of facts, not as a
second log format. Human rows should summarize command tasks, dictation
captures, meeting lifecycles, and actionable health failures. Expanding a row
should show the contributing `VoiceEvent` records and, for meetings, the
`MeetingManifest` facts.

The current data supports useful summaries, but it does not support the
prototype's context-mismatch, audio-drop, model-state, meeting-detection, or
"pasted into Slack" explanations. Those claims should remain absent until the
small additions below exist.

## Truthful Today

| Human explanation | Current evidence and limit |
| --- | --- |
| "Voice Control session started" | `VoiceEvent::SessionStarted` is an exact boundary (`src/events.rs:10`, emitted at `src/recognition.rs:73`). The following `State` normally supplies `VoiceState` and device. Sound settings are not present. |
| "Voice Control is listening/sleeping/dictating/transcribing/stopping on DEVICE" | Exact `VoiceEvent::State` facts (`src/events.rs:13-17`). The event does not record what caused the transition, so only adjacent command/dictation facts may explain it. |
| "Heard TEXT" | A completed `Transcript` records text and inference `latency_ms` (`src/events.rs:18-23`, `src/recognition.rs:280`). `Started` and `Updated` are provisional facts, not separate user actions. |
| "TEXT woke Voice Control" / "put Voice Control to sleep" | `CommandOutcome::Woke` and `Slept` are exact resolver outcomes and are paired with `mode.wake` / `mode.sleep` (`src/recognition.rs:401-412`). |
| "TEXT matched COMMAND and was submitted/executed" | `Command` has `heard`, command id, outcome, and the decision-time context label (`src/events.rs:24-33`). `Submitted` means queued, not successful. A later `Executed` means the action returned success, not that an external application visibly changed. |
| "COMMAND failed: ERROR" | `CommandOutcome::Failed(String)` retains the returned error (`src/events.rs:75-83`, `src/recognition.rs:432-460`). It does not distinguish queue submission failure from action execution failure without interpreting the error text. |
| "TEXT was ignored" | `CommandOutcome::Ignored` is exact. No current fact says why it was ignored. |
| "Dictation started/transcribed/pasted/logged/repasted/was discarded/cancelled/failed" | `DictationPhase` records those lifecycle facts and final text where available (`src/events.rs:61-72`, `src/recognition.rs:611-652`). Destination application, capture duration, trigger, transcription target, and failure stage are absent. |
| "Foreground context changed to APPLICATION / browser URL" | `VoiceEvent::Context` records application and URL (`src/events.rs:39-44`, `src/recognition.rs:98-105`). `window_title` is captured in `ContextSnapshot` but omitted from the event (`src/context.rs:11-15`, `src/context.rs:68-76`). |
| "Meeting recording began at TIME and capture ended at TIME" | `MeetingManifest.created_at_ms` and `ended_at_ms` support those statements (`src/meeting.rs:21-34`). Current/final `MeetingStatus`, duration, error, per-track files, sample rates, timing, sample counts, and `dropped_packets` are also available from the manifest (`src/meeting.rs:37-55`). Intermediate status transition times are not. |

## Not Derivable

- **Ignored-command reason:** `Decision::Ignore` conflates non-wake speech while
  `Mode::Sleeping`, no phrase match while listening, and a phrase whose
  `ContextPredicate` did not match (`src/commands.rs:329-370`). The emitted
  command has `command: None`; it cannot support "requires x.com, current
  context Slack" or name a rejected candidate.
- **Historical command context:** the command stores only `ContextSnapshot::label()`.
  The nearest `Context` event is useful, but capture failures are only warnings,
  the previous snapshot remains active, and updates may be dropped when the
  channel is full (`src/context.rs:23-46`). The event does not expose context
  freshness or `window_title`.
- **One command task:** asynchronous actions emit `Submitted` and then another
  `Command` with `Executed` or `Failed` (`src/recognition.rs:432-460`). There is
  no task id, so those records cannot be joined reliably when ids, text, and
  context repeat.
- **One dictation capture:** phase records have no capture id. They cannot be
  joined safely across queued worker results, voice controls, Option capture,
  Captain's Log, paste-last, or repaste. `Failed(String)` also collapses worker
  rejection, model failure, transcription failure, and paste failure.
- **Audio health:** microphone queue drops are counted in `AudioInput` but only
  reported through `tracing` once per second (`src/audio.rs:14-15`,
  `src/audio.rs:57-59`, `src/recognition.rs:87-96`). CoreAudio stream errors are
  also tracing-only (`src/audio.rs:98-100`). No `VoiceEvent` supports an audio
  drop row, affected interval, or current stream-health claim.
- **Model state:** successful loading, model identity/version, loading time, and
  Moonshine failure are absent from `VoiceEvent`. `WorkerEvent::ModelFailed` is
  flattened into `DictationPhase::Failed` (`src/recognition.rs:636-642`), while
  startup failures can occur before `SessionStarted` is emitted.
- **Meeting chronology:** `ControllerEvent::{Offer, RecordingStarted,
  Transcribing, Finished, Failed}` is transient UI state and is never appended
  to `logs/live.ndjson` (`src/meeting_watcher.rs:38-44`,
  `src/meeting_watcher.rs:351-374`). Offer acceptance/dismissal, provider source,
  and status transition timestamps therefore disappear after the process exits.
- **Meeting detection ended:** while recording, `controller_loop` does not poll
  microphone applications (`src/meeting_watcher.rs:449-466`). The prototype's
  "microphone-process activity disappeared; recording is still active" fact is
  not currently observed.
- **Why a meeting is interrupted:** `meeting::list` projects any persisted
  `Recording` manifest as `Interrupted` (`src/meeting.rs:554-556`). Capture
  setup/start/stop/writer errors after the initial manifest can leave that same
  state without persisting `error`, so the cause cannot be recovered.

## Smallest Additions

### First Activity Slice

1. Add a `task_id` to `VoiceEvent::Command`, generated before resolution and
   preserved from `Submitted` through `Executed` / `Failed`. Add a `capture_id`
   to `VoiceEvent::Dictation` and to command records that control that capture.
   These two ids are enough to form stable command and dictation rows; transcript
   updates do not need ids for the first slice.
2. Add a typed ignored reason produced by `CommandConfig::resolve`, rather than
   reconstructed by the UI: `Sleeping`, `NoPhraseMatch`, or
   `ContextMismatch { command, required: CommandScope }`. For a mismatch, record
   every same-phrase rejected contextual command if more than one exists. Also
   record the evaluated application/browser host and context observation time;
   do not parse the display `context` label.
3. Add a failure stage to failed command and dictation facts. The minimum useful
   values are command `Submission` / `Execution` and dictation `Capture` /
   `Model` / `Transcription` / `Paste` / `Log`. Keep the existing error string.
4. Append meeting lifecycle facts to the same bounded chronology: offered,
   dismissed, recording, transcribing, complete, and failed, each with timestamp,
   one meeting task id, title, `MeetingCandidate.source`, and eventual meeting
   id/error. Keep track counters and file names in `MeetingManifest`; do not copy
   them into every activity event.
5. Persist `MeetingStatus::Failed` plus `error` for every failure after manifest
   creation. Reserve the read-time `Interrupted` projection for an abandoned
   `Recording` manifest; never present it as a known crash explanation.

### Debug/Health Follow-up

Add only three domain-specific issue facts, emitted on change or failure rather
than periodically: microphone dropped chunks (count and interval), microphone
stream failure (device and error), and foreground-context capture failure (error
and last successful observation time). Add explicit Moonshine and Parakeet
`loading` / `ready` / `failed` facts only if the pane promises model health.
This is sufficient for the named questions and avoids a generic metrics or
observability schema.

Do not emit normal audio levels, per-chunk events, spans, counters with arbitrary
names, or copies of `process.log` into `VoiceEvent`.

## Projection Rules

### User-Facing Activity Rows

- One command row per `task_id`: completed `heard` text, matched command id,
  final outcome, and a typed explanation. Show `Submitted` only while pending.
- One dictation row per `capture_id`: start through pasted/logged/discarded/
  cancelled/failed, with final text where appropriate. Do not name a destination
  application until it is recorded at the paste action.
- One meeting row per meeting task id: offer/user decision, recording,
  transcribing, and final status. Surface nonzero `dropped_packets` as a warning
  after the manifest is finalized.
- One row for actionable audio, context-capture, or model failures. Normal
  context changes and normal model/audio state stay secondary.
- `SessionStarted` is a divider, not an action row. `Stopping` can close a
  session; without it, label the prior session "end not recorded" rather than
  inferring a crash.

### Raw Inspector Facts

- All contributing NDJSON records, including `TranscriptPhase::Started` /
  `Updated`, latency, `State` transitions, exact timestamps, device, context URL,
  intermediate `Submitted`, and exact error strings.
- The selected meeting's manifest fields, especially per-track timing, sample
  counts, and `dropped_packets`.
- Context changes may be browsable as raw facts, but "commands are now
  available" is only historical truth if the resolver decision/candidate set was
  recorded at that time.

Apply the fixed limit to raw events, then group only records inside that window.
Mark a task as beginning before the window when its opening fact is absent. Keep
every `SessionStarted` inside the window as a visible divider. The current
dashboard instead drains everything before the latest session
(`src/dashboard.rs:89-100`), so its `read_events` behavior should not define the
app-shell projection.

## Commands Pane

`CommandConfig::catalog` already provides the read-only source of phrase,
aliases, description, id, and `CommandScope` (`src/commands.rs:375-430`). It has
no task/group field. Add one task label to resolver definitions and carry it
through `CommandInfo`; do not derive grouping from id prefixes or prose
descriptions. Use `catalog()` for the full grouped list and current resolver
state to annotate availability. `available_catalog()` alone hides contextual
commands whose predicate does not currently match (`src/commands.rs:432-447`).
