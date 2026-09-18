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

Ordinary shortcut -> No recording mute/reduce/pause or idle-sleep prevention
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
Completed long recording                  // dictate.model-windows
├── Cohere -> Runs of at most 35 seconds, including the final remainder
└── Other GGUF models -> Existing backend-advertised input limit
    -> Transcribe each chunk -> Join text in original order

Chunking bounds inference, not capture duration. No silence is removed.
```

The shared `max_audio_chunk_samples` policy in
[transcription_models.rs](../../src/transcription_models.rs) is used by
[parakeet.rs](../../src/parakeet.rs) and
[linux_transcriber.rs](../../src/linux_transcriber.rs). Cohere's pinned backend
reports a 400-second encoder capacity, while its reference processor uses a
35-second transcription window. Treating the encoder capacity as a useful
inference window caused [#73](https://github.com/anomalyco/hex/issues/73)'s lost
passages. A stricter backend bound still wins; other model policies are unchanged.

`cohere_chunks_at_reference_window_not_encoder_capacity` checks the policy.
The opt-in `cohere_long_form_keeps_all_sections_with_and_without_pauses` runs the
real pinned model on synthetic speech. Reproduce it on macOS with
[`scripts/test-cohere-long-form.sh`](../../scripts/test-cohere-long-form.sh) and an
explicit model path. The script uses Samantha speech synthesis, never microphone
capture, and retains fixtures on failure (`HEX_KEEP_COHERE_FIXTURES=1` keeps successes).

**Observed September 4, 2026:** the same 57-second fixture went from 44 word errors
to zero after chunking; the 73-second paused fixture went from 71 errors to one.
Short references transcribed correctly before and after. The retained native
regression also passed a roughly 235-second repeated fixture, retaining all three
sections in each repetition. This is Metal inference evidence on an M2 Max, not
physical capture/paste or Linux runtime proof. Chunk boundaries can still affect
individual words; this is not a claim of perfect recognition or silence handling.

```ts
Listening idle                            // dictate.model-memory
├── Keep the selected model weights warm // avoids loading on each dictation
└── Completed offline inference
    -> Release input-sized GGML/Metal compute scratch
    -> Keep only reusable model/session state
```

The selected GGUF remains resident by design; Parakeet v3's pinned Q8 artifact is
739,508,576 bytes before runtime metadata and decoder state. Input-sized scheduler
scratch must not remain at its high-water mark after a dictation. The pinned
`transcribe-cpp` 0.1.3 runtime owns that scratch for the session lifetime, so HEX
rotates the lightweight session after every offline run while retaining the loaded
model. A later dictation recreates its session without reloading the weights.

**Observed September 18, 2026, isolated Metal probe on M2 Max:** with the similarly
sized Parakeet Unified English model, `transcribe-cpp` 0.1.3 grew from a 792 MB loaded
footprint to 1,195 MB after a 45-second input and retained 1,195 MB after another
short run. The same 0.1.3 runtime with HEX's session rotation measured 792 MB loaded,
801 MB after the long input, and 802 MB after the following short run. The probe used
generated silence and no live microphone, UI, Commands model, or paste. This
establishes the backend scratch cause and reclamation on that model and machine, not
Parakeet v3's exact whole-app footprint on the reporter's Mac. Activity Monitor can
include additional app and optional Commands-model memory.

The ignored `dictation_protocol_audio` native fixture was also run with the selected
Unified English model. Before and after session rotation it reached the same existing
second-pass mismatch (`say stop` decoded as `say stay`) on the question fixture, so
that run is not claimed as a passing transcription regression check.

```ts
Finish
  ├── Capacity available -> Accepted job -> Local transcription -> Mode processing
  │    -> Paste successful nonempty output once, at current focus
  │       // synthetic ⌘V resolves on any active layout: a non-Latin layout
  │       // falls back to the ASCII-capable layout like AppKit itself
  └── Queue full -> Explicit rejection                // bounded admission

A pending -> Start capture B                          // dictate.overlap
  -> B records independently; A is not cancelled
  -> Accepted outputs retain submission order
  -> Completing/pasting A must not end B
```

Source: `finish_dictation` in [recognition.rs](../../src/recognition.rs).
Key resolution: [keyboard.rs](../../src/keyboard.rs); non-Latin layout
fallback details live in [shortcut recovery](recovery.md#keyboard-layout-resolution).
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
     -> Playback worker releases output on its next 250 ms observation
     -> Volume enabled again -> Reopen on playback worker, never capture/UI
  Cold audio stack exceeds preload admission -> Warn once; recognition continues
     -> Loader keeps running; later tones play once the output opens
```

Feedback admission is advisory on every platform: a cold audio stack can exceed
the two-second preload timeout, so the recognition worker logs a warning and
continues rather than stopping. The loader thread keeps running and registers
the player when the default output finally opens, restoring tones for the rest
of the session. Linux already logged and continued; the macOS worker now
matches that behavior instead of stopping desktop recognition.

The playback worker skips output initialization when sounds are Off. A live
volume change to Off drops the output stream; a device open already in progress
must return before the worker can observe that change. Output-open failures
retry at most once per two seconds. Bundled samples remain decoded in memory.
`sound_off_releases_output_and_reenable_reopens_it` checks actual ownership/drop
with a controlled sink; `failed_output_initialization_retries_with_a_bounded_backoff`
checks failure-to-success recovery without an audio device.
`timed_out_admission_still_publishes_a_usable_player` checks the production
publication path after the admission receiver has gone away. These checks do
not measure the reporter's CoreAudio CPU usage or establish audible playback.

**Contributor-reported September 10, 2026 (PR #84):** a locally built `/Applications/Hex.app`
(approximating 2.1.16 plus this change) was started right after a restart where
the stock build had stopped its desktop recognition worker with
`timed out preloading feedback audio`. The patched build started on the first
attempt: the worker reached `dictation listener started` and
`resolved paste key … paste_key_code=9`, and held-Option dictations pasted into
the focused app. The late-registration path (loader still running when the
admission wait times out) is implemented, but its end-to-end tone recovery has
not yet been observed on hardware;
it is expected behavior, not executed evidence.

Linux now uses the same recording sounds with its own persisted Sound volume
control; see [Linux feedback](README.md#other-platforms-and-consumers) for its
entry points, timing, defaults, and verification limits.

Check: [capture-preview.sh](../../scripts/capture-preview.sh), `dictation-hud`
preview. It repeats the recording/processing sequence until the preview exits,
so fullscreen and Space transitions can be checked without a ten-second deadline.
This renders a real window with deterministic state; it does not prove that
actual capture caused that state.

The macOS HUD is a click-through, nonactivating `NSPanel` with
`CanJoinAllSpaces`, `CanJoinAllApplications`, and `FullScreenAuxiliary` collection
behaviors. The cross-application flag explicitly allows the overlay to join
another application's fullscreen Space (#75); panel style flags on an ordinary
`NSWindow` were not sufficient.

**Observed September 14, 2026:** the unmodified HUD was not found on screen over
a native fullscreen TextEdit fixture. Changing only the window class to
`NSPanel` did not produce a visible HUD in the initial check. The candidate with
`CanJoinAllApplications` and the repeating fixture visibly showed the red HUD
over fullscreen TextEdit, captured through Cua Driver. This was a debug preview
with no microphone capture, not a signed installed-app or multi-display test.
Kit independently confirmed the repeating candidate was visible in fullscreen.

**Combined verification September 14, 2026:** 468 Rust tests passed with ten
native/opt-in checks ignored, plus all twelve keyboard-layout child scenarios
in debug and release. Strict all-target/all-feature Clippy, 46 command-SDK tests,
and the command-SDK build passed. The native preview also built successfully.
These checks preceded the app release.

**2.1.18 candidate verification September 14, 2026:** the optimized suite passed
all 468 Rust tests and twelve keyboard-layout child scenarios; ten opt-in checks
were skipped. Strict release all-target/all-feature Clippy and app-identity
fixtures passed. The signed, notarized bundle's isolated HUD preview was visibly
present over native fullscreen TextEdit. This exercises the packaged renderer,
not microphone capture or an installed-app update. The preview and disposable
TextEdit process were stopped afterward.

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
