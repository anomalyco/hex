# Scrawl

Tap a shortcut (`⌥⇧S` by default) to record the display under the pointer alongside
your voice, then tap it again to finish. Hex transcribes your speech with word
timings, aligns it with mouse clicks and window activity to extract keyframes and
a numbered contact sheet, writes a `.scrawl` bundle, and copies the Markdown
summary + artifact paths to the clipboard.

This map covers M1 on macOS (`experiment/scrawl`). Windows and Linux capture crates
compile with explicit `"unsupported"` trait responses so cross-platform CI passes.

## How To Get To It

```ts
Hex > Settings > Scrawl                              // scrawl; off by default
├── Enable Scrawl (beta) -> Activate saved shortcut  // checks Screen Recording only when enabled
├── Screen & System Audio Recording -> Open settings // reuses meeting::screen_recording_permitted
├── Scrawl shortcut -> ⌥⇧S by default                // independent key chord
└── Auto-crop full-screen captures -> On by default

Tap ⌥⇧S -> Record display under pointer + microphone -> Tap ⌥⇧S again -> Bundle + clipboard
```

Requires ordinary setup permissions, a local transcription model, and Screen &
System Audio Recording permission once Scrawl is enabled.

## Sub-features

### Independent Opt-In

```ts
New settings OR existing settings without scrawl.enabled=true // scrawl.opt-in
  -> Off -> Saved ⌥⇧S binding neither active nor reserved
  -> Does not check or request Screen Recording permission

Enable -> Check shortcut conflicts -> Check Screen Recording -> Persist opt-in
Disable -> Saved shortcut and auto-crop preference remain intact
```

Sources: `ScrawlSettings` in [app_settings.rs](../../src/app_settings.rs) and
`render_scrawl_settings` in [app_window.rs](../../src/app_window.rs).
Checks: `missing_fields_receive_defaults` in [app_settings.rs](../../src/app_settings.rs)
and `disabled_scrawl_does_not_reserve_its_shortcut` in
[app_window.rs](../../src/app_window.rs).

### Chord Matching Without Option-Dictation Leak

```ts
⌥⇧S while Scrawl is disabled                         // scrawl.shortcut
  -> Ignored by paste_action and not suppressed

⌥⇧S while Scrawl is enabled
  ├── Fast chord (⌥ -> ⇧ <300ms -> S) -> Discard Option hold -> Emit HotkeyAction::Scrawl
  ├── Slow chord (⌥ >300ms -> ⇧ -> S) -> Dirty state -> Cancel dictation -> Emit HotkeyAction::Scrawl
  └── Shift-first (⇧ -> ⌥ -> S)       -> Never starts dictation -> Emit HotkeyAction::Scrawl

Escape during active Scrawl recording
  -> Cancels dictation only; never cancels Scrawl screen recording
```

Sources: `paste_action` and `ShortcutSuppression::process_all` in
[suppression.rs](../../src/suppression.rs), and `HotkeyAction::Scrawl` dispatch in
[recognition.rs](../../src/recognition.rs).
Check: `scrawl_chord_is_inert_when_disabled_and_never_leaks_into_option_dictation`
in [suppression.rs](../../src/suppression.rs).

### Composable Pipeline And Bundle Assembly

```ts
Start (first ⌥⇧S)                                    // scrawl.capture
  -> ScreenCaptureSession::start (display under pointer)
  -> MacosInputSource::start (mouse clicks + WindowLogger)
  -> DictationAudio::set_scrawl_sink (reuses single cpal stream)

Finish (second ⌥⇧S)                                  // scrawl.pipeline
  -> Stop screen + input + audio sink
  -> HexWordTranscriber (TimestampKind::Word via WarmTranscriber)
  -> plan_keyframes -> solve_crop -> render_frames -> assemble_bundle
  -> Write ~/Library/Application Support/voice-control/scrawl/<id>.scrawl/
  -> Copy Markdown summary + contact-sheet.png + frame-*.png paths to clipboard
```

Sources: [scrawl.rs](../../src/scrawl.rs), [pipeline.rs](../../crates/scrawl-core/src/pipeline.rs),
and [crates/scrawl-capture-macos/src/lib.rs](../../crates/scrawl-capture-macos/src/lib.rs).
Checks: `stages_can_run_independently_and_accept_modified_intermediates` in
[pipeline.rs](../../crates/scrawl-core/src/pipeline.rs) and
`clipboard_summary_includes_transcript_contact_sheet_and_keyframe_paths` in
[scrawl.rs](../../src/scrawl.rs).
