# Prototype Decisions

- Meeting recording always starts explicitly.
- If meeting-app microphone activity disappears during recording, ask before
  stopping: `Keep Recording` or `Stop & Transcribe`.
- Activity/Debug defaults to explaining what happened; raw event facts belong
  in a secondary inspector.
- Activity shows a fixed recent event window with session boundaries.
- Commands are grouped by task and are read-only in the first product slice.
- Meeting detail is transcript-first with `Reveal Files`; synchronized audio
  playback is a later slice.
- The app opens on Settings by default.
- Meetings, Commands, Activity/Debug, and Settings share one application shell.

## Interaction Values

- Simplicity through gradual revelation: panes show fundamentals first and
  reveal inspectors or secondary controls only in context.
- Fluidity through continuity: persistent navigation and shared objects move or
  transform; do not replace an element with a duplicate during transitions.
- Transient meeting offers, end confirmation, and recording status behave like
  focused trays that preserve the current app context.
- Delight is selective. Reserve stronger motion for the dictation HUD, meeting
  state transitions, and occasional meaningful completion moments.

These are prototype inputs, not a production specification. Delete or promote
this file when a shell direction is selected.
