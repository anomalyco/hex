# App-Shell Synthesis

**Status:** Archived design synthesis. Source-line references and production
gaps reflect the prototype period and may no longer match the current code.

## Recommended Direction

Use Variant A's calm workspace as the shell foundation.

- Keep one persistent left navigation and compact global listening status.
- Open on Settings.
- Preserve large, quiet content regions rather than showing operational detail
  everywhere.
- Let the meeting controller transform in place through recording, end
  confirmation, transcription, and readiness.

Borrow Variant B's chronological explanation model for Activity/Debug. The
default row should answer what happened in plain language. Selecting it reveals
the raw event facts.

Borrow Variant C's list/detail inspector depth inside Commands and the raw Debug
view. Commands remain grouped by task; context predicates and implementation
facts appear only after selection.

## Production Data Gaps

The intended Activity experience is not fully derivable from today's event log.
See `ACTIVITY_DATA.md`. The smallest required additions are typed command
resolution reasons, correlation IDs for asynchronous command/dictation work,
failure stages, meeting lifecycle facts, and narrow audio/model health events.

## Family Values

- Simplicity: fundamentals remain visible; inspectors and advanced controls are
  revealed contextually.
- Fluidity: persistent elements transform or move rather than being replaced by
  duplicates.
- Delight: stronger motion is reserved for dictation and meaningful meeting
  transitions, not every row or pane.
