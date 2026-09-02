# Dictation

Hold the configured shortcut, speak, and release. HEX transcribes locally and
inserts the result at the current focus. New captures do not cancel accepted
work; cancellation prevents insertion only before output commits.

This map covers the macOS release path. Linux shares the basic journey, not
every timing, input, or recovery guarantee; see the
[platform map](README.md#other-platforms-and-consumers). This is a map, not a
verified end-to-end driver. Linked checks are locators; no tests or previews
were run for this rewrite.

## How To Get To It

**Shortcut:** Option by default, after macOS permissions and the selected local
model are ready. Commands and OpenCode are not required. Settings > Dictation
shortcut accepts modifier-only, modifier-plus-key, standalone Fn/Globe, and
standalone function-key bindings. Capturing a replacement suspends global matching.

**CLI:** `hex listen` uses saved settings; `--device` is authoritative. This is
the CLI listener, not Settings. Do not run it beside an existing listener.

**Separate targets:** [Voice Action](voice-action.md) has its own opt-in and
capture target, with no double-tap lock. Host-owned API capture has its own
ownership protocol.
Neither proves the physical dictation shortcut; see the [feature index](README.md).

## Sub-features

### Hold And Short Taps

```ts
Ordinary hold                            // lock activation is mapped below
  ├── Hold -> Speak -> Release -> Finish          // dictate.hold
  ├── Release before 300 ms -> Discard            // dictate.short-tap
  └── Early unrelated shortcut/click -> Discard   // not intentional dictation

Ordinary shortcut -> No recording mute/pause or idle-sleep prevention
Intentional recording -> No automatic duration limit
```

Source: `handle_hotkey_action` in [recognition.rs](../../src/recognition.rs).
Checks in [suppression.rs](../../src/suppression.rs):
`callback_timestamps_preserve_nanoseconds_and_short_tap_discard`,
`callback_press_reaches_intentional_hold_on_the_audio_clock`.
These cover constructed callback/reducer timing, not physical event delivery.

The short-tap threshold belongs to ordinary hold activation. Voice-delimited
and explicit programmatic capture have different intentional-start rules.

### Lock, With Or Without Hold

```ts
Settings > Double-tap to lock                         // dictate.lock
  Hold -> Ordinary hold path remains available
  Tap -> Tap
    ├── Second press AND release <300 ms after first release -> LOCKED
    └── Slow second release -> No lock

Settings > Double-tap only                            // dictate.double-tap-only
  Requires a key binding AND locking enabled
  Single press/hold -> No capture                     // disables hold, not a toggle
  Same two qualifying complete taps -> LOCKED         // same lock, not a third mode

LOCKED -> Speak hands-free -> Fresh shortcut press -> Finish
       -> Escape -> Cancel
```

Checks in [suppression.rs](../../src/suppression.rs):
`callback_double_tap_locks_for_modifier_and_key_bindings`,
`a_slow_second_release_does_not_lock`,
`double_tap_only_waits_for_two_complete_key_chord_taps`.
These cover gesture decisions, not hardware keyboard-layout or remapping equivalence.

### Warm And Cold Capture

```ts
Keep ready -> Press shortcut                          // dictate.boundaries
  -> Include up to 450 ms of available pre-roll
  -> Delayed handling still uses original event boundaries
  -> Release excludes audio captured after physical release

Release when idle -> Press shortcut                   // dictate.cold-microphone
  -> Open microphone asynchronously, no pre-roll
     ├── Release before readiness -> Discard
     └── Ready while held or locked -> Capture
  -> Capture idle -> Microphone can close             // accepted jobs do not keep it open
```

Source: `DictationCapture` in [dictation.rs](../../src/dictation.rs); checks:
`delayed_press_reconstructs_audio_from_the_original_boundary`,
`delayed_release_removes_audio_captured_after_the_physical_release`,
`capture_continues_past_sixty_seconds_until_explicitly_finished`.
These cover sample-buffer/timestamp logic, not a microphone recording.
The owner in [dictation_audio.rs](../../src/dictation_audio.rs) has
`cold_capture_restart_rejects_late_open_results_and_closes_after_finish` and
`pending_open_uses_physical_press_for_intentional_threshold`: controlled opening
and ownership transitions, not physical device startup latency.

### Optional Voice Controls

Optional spoken entry requires Commands enabled, its model ready, and recognition
awake. [Voice Commands](commands.md#spoken-dictation-controls) owns
`dictate.voice-controls`, its start/stop/Send/cancel phrases, config, and checks;
this page owns the resulting capture and output.

### Ongoing Jobs And Output

```ts
Finish
  ├── Capacity available -> Accepted job -> Local transcription -> Mode processing
  │    -> Paste successful nonempty output once, at current focus
  └── Queue full -> Explicit rejection                // bounded admission

A pending -> Start capture B                          // dictate.overlap
  -> B records independently; A is not cancelled
  -> Accepted outputs retain submission order
  -> Completing/pasting A must not end B
```

Source: `finish_dictation` in [recognition.rs](../../src/recognition.rs).
Checks in [parakeet.rs](../../src/parakeet.rs):
`parallel_processing_results_are_released_in_submission_order`,
`ordered_waiting_outputs_remain_bounded_after_channel_drain`.
These cover output bookkeeping/admission, not sustained end-to-end inference/paste.

### Cancellation Before Commit

```ts
Escape                                               // dictate.cancel
  ├── Active capture -> Cancel that capture first
  └── No active capture -> Cancel newest cancellable accepted job
     -> Repeat Escape -> Walk back through unfinished jobs

Cancelled before output commit -> No insertion
Already committed clipboard insertion -> Not undone   // not target-app undo
```

Checks in [parakeet.rs](../../src/parakeet.rs):
`output_stays_cancellable_through_preparation_but_not_after_mutation`,
`repeated_cancellation_walks_back_through_pending_jobs`.
These use a controlled paste boundary and temporary history store;
no foreground application consumes the clipboard.

### Feedback

```ts
Record -> Release -> Transcribing/processing/paste -> Finished // dictate.feedback
  HUD + tones distinguish capture from pending work          // never take focus
  Pending processing/paste -> Still unfinished, not recording
  Feedback volume = 0 -> No tones
```

Check: [capture-preview.sh](../../scripts/capture-preview.sh), `dictation-hud`
preview. This renders a real window with deterministic state; it does not prove
that actual capture caused that state.

## Gaps And Constraints

- **Physical input:** Fn/Globe, assistive input, remapped modifier sides, and
  cross-tap delivery ordering need their own native evidence. A passing Option
  fixture does not cover every binding. See [shortcut recovery](recovery.md).
- **Full user path:** source connections and individual checks do not prove the
  complete path on a user's Mac. Retain action-to-result evidence for hold,
  lock, Escape, and a second capture during older processing in a controlled
  target app. Observe insertion and input responsiveness, not just the HUD.
- **Microphone modes:** verify onset and idle closure separately for Keep ready
  and Release when idle on an identified device. The macOS microphone indicator
  while Keep ready is idle is expected, not proof of recording.
- **Voice Action is distinct:** its
  [cold modifier-order gap](voice-action.md#cold-modifier-order) is not covered by
  ordinary cold-dictation checks.
- **Paste consumption:** current paste policy waits a fixed interval rather than
  acknowledging target consumption. Internal ordering does not establish
  correctness for an arbitrarily slow target application.
- **Native checks require deliberate setup:** permissions, model, keyboard,
  microphone, output device, and a disposable text target must be recorded. This
  map does not authorize operating the installed app or injecting global input.
