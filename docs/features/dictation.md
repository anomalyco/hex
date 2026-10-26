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

Settings shortcut capture persists the candidate before showing Saved. If the
write fails, capture remains active with an error; retry or cancel it explicitly.
The GPUI regression that injected this failure went with GPUI; no Rune test
injects a failed shortcut save yet.

**Shortcut:** Option by default, after macOS permissions and the selected local
model are ready. Commands and OpenCode are not required. General > Dictation >
Shortcut accepts modifier-only, modifier-plus-key, standalone Fn/Globe, and
standalone function-key bindings. macOS groups the primary and any number of
alternate bindings under **Shortcut**. The primary row's **+** records another
shortcut; each alternate has a remove button. Cancelling an addition saves nothing.
All rows share aligned side, key-badge, recorder, and action columns. All bindings
drive one capture/hold/toggle/lock state. Existing single-alternate settings migrate
to the list. Capturing a replacement suspends global matching.
Opposite sides of the same single modifier merge into the existing **Either**
row, including when loading older settings containing both sides. Other shortcut
rows remain in order; multi-modifier chords are not broadened this way.
The shortcut's keys sit in a recessed tray as tall as the recorder, between
the macOS side picker and the recorder; with no shortcut the tray reads Off.
On macOS and Windows the recorder shows the modifiers held so far as pressed
keys (`⌥ ⌘ …`); a key records them at once, and releasing them all records the
most held. Saved shows for a second, then the recorder returns to Change
shortcut. The recorder keeps one width in every state, so it, the tray, and the
side picker never move while it records; with the picker showing, keycaps leave
the side to it, and otherwise they name it (`Left ⌥`).
Checks: `a_recorder_shows_the_modifiers_held_so_far` in `rune_ui/tests.rs`,
 `the_shortcut_row_holds_still_through_every_face_and_side`,
 `a_new_shortcut_s_keys_stay_inside_their_tray`, and
 `esc_stays_on_the_recorder_however_many_keys_are_held` in
 `rune_ui/tests/panes.rs`, the `shortcut-row*` scenes, and the `ChordCapture`
 tests in `rune_ui/macos_host.rs`. `alternate_modifier_shortcut_starts_and_finishes_dictation`
 in `suppression.rs` checks that two sided modifier bindings enter the same dictation machine
 (no native end-to-end run).

**CLI:** `hex listen` uses saved settings; `--device` is authoritative. This is
the CLI listener, not Settings. Do not run it beside an existing listener.

**Automatic microphone:** Hex prefers an available built-in microphone over a
Bluetooth headset, so dictation does not switch headphones into a low-quality
call profile. **Avoid Bluetooth microphone** defaults on, including for older
settings files; turning it off allows the default Bluetooth input. Explicitly
selected microphones bypass this policy. While idle, it reopens its input when the resolved automatic route
changes after a device connection or removal. Each successful open records
`opened dictation microphone` and its device name in `process.log`.

Checks: `alternate_dictation_shortcut_is_optional_and_round_trips` covers migration
and defaults; `alternate_rows_validate_and_edit_their_own_binding` checks indexed
editing and duplicate conflicts. `mac-shortcuts` renders the three-row fixture in
light and dark. These checks do not prove native shortcut events or Bluetooth routing.

**Separate targets:** [Voice Action](voice-action.md) has its own opt-in and
capture target, with no double-tap lock. Host-owned API capture has its own
ownership protocol.
Neither proves the physical dictation shortcut; see the [feature index](README.md).

## Sub-features

### Vocabulary Quality Checks

`hex benchmark-transcription` accepts a scoring vocabulary and expected terms,
and reports recall, excess vocabulary occurrences and WER by corpus group, plus
overall latency. Its companion scorer also reports per-group latency.
The [fixture guide](../../tests/fixtures/vocabulary/README.md) describes
the synthetic set, public guard download, and ignored private manifest path.
This is offline evaluation coverage, not a shipped vocabulary feature or proof
of recognition quality on a microphone. Metric checks live in
`transcription_benchmark::tests`; inference results belong in the research report.

### Hold And Short Taps

```ts
Ordinary hold                            // lock activation is mapped below
  ├── Hold -> Speak -> Release -> Finish          // dictate.hold
  ├── Release before 300 ms -> Discard            // dictate.short-tap
  ├── Early unrelated shortcut/click -> Discard   // not intentional dictation
  │   └── Includes another app's global shortcut  // dictate.hidden-chord
  │       (Spaceman's Option-1) -> Seen at HID -> Discard, no double-tap lock
  └── Modifier-only hold + any key, at any time -> Discard  // dictate.held-chord
      (key reaches the app; locked and toggle captures keep recording)

Ordinary shortcut -> No recording mute/pause or idle-sleep prevention
Intentional recording -> No automatic duration limit
```

Source: `handle_hotkey_action` in [recognition.rs](../../src/recognition.rs).
Checks in [suppression.rs](../../src/suppression.rs):
`callback_timestamps_preserve_nanoseconds_and_short_tap_discard`,
`callback_press_reaches_intentional_hold_on_the_audio_clock`.
These cover constructed callback/reducer timing, not physical event delivery.

A chord discard logs its cause to `process.log` (`dictation press discarded as
a chord`, or `dictation press dropped` when HID evidence overtook the press)
with the key code, the milliseconds after the press, and for HID evidence the
raw timestamp, how it was read, and its arrival delay. The `discarded` event in
`live.ndjson` carries the same cause as `reason`: `key`, `hid-key`, `mouse`, or
`extra-modifier`. Short taps have no `reason`.

**Fixed October 22, 2026:** a global shortcut registered with Carbon (Spaceman's
Option-1 to Option-0 workspace keys, for example) consumes its key before the
session taps, so Hex saw only the Option press and release. Holding Option past
300 ms while switching workspaces recorded and pasted whatever the microphone
heard, and two quick switches could double-tap lock. A listen-only HID tap now
reports key presses as chord evidence only; session events still own key state,
and Escape and Hex's own shortcuts are excluded. Evidence that overtakes its
press still prevents the capture. Command-Tab likewise swallows Tab's release,
which caused the routine `resynchronized stale input tracking` warnings, now
logged with `key_codes`. Checks:
`hid_evidence_discards_a_chord_hidden_by_a_global_shortcut`,
`hidden_chords_in_quick_succession_never_double_tap_lock`,
`hid_evidence_that_overtakes_its_press_prevents_the_capture`. Observed with
listen-only session and HID taps on Kit's Studio: Spaceman's Option-1/2/3
reached HID but never the session. The installed fix has no physical-keyboard
run yet.

**Changed October 22, 2026:** Kit holds Option a little past 300 ms before
pressing the digit, which 2.3.0 and the Swift app both ignored after the
threshold. Any non-modifier key pressed while a modifier-only shortcut is still
held now discards the capture, with no transcription or paste. Keyed shortcuts
keep the 300 ms rule. Check:
`held_modifier_only_shortcut_plus_any_key_discards_after_the_hold_threshold`.

### Shortcut Mode

```ts
General > Shortcut mode = Hold (default)             // dictate.shortcut-mode
  -> Hold and lock behave as described above
General > Shortcut mode = Toggle
  Press -> Start (pre-roll as usual) -> Release keeps recording
  -> Fresh shortcut press -> Finish (with the release tail)
  -> Escape -> Cancel
  // Double-tap to lock and Double-tap only are hidden and ignored
  // Shortcut chords during the first 300 ms still discard
```

Toggle reuses the lock state on every platform: each press behaves as the
second tap of a double-tap. Persisted as `shortcut_mode` (`hold`/`toggle`) in
each platform's settings; files without it load as Hold. Checks:
`toggle_mode_starts_on_a_press_and_finishes_on_the_next` and
`toggle_mode_escape_cancels_a_locked_capture` in
[suppression.rs](../../src/suppression.rs) (macOS reducer), the same names in
[windows_input.rs](../../src/windows_input.rs), and
`toggle_mode_starts_on_a_tap_and_finishes_on_the_next` in
[linux_input.rs](../../src/linux_input.rs), whose one state machine serves X11
grabs, Wayland portal shortcuts, and the `hex dictation` commands alike. Only
the Windows build was compiled and tested locally (2026-10-19); macOS relies on
CI. The Linux state machine and X11 grab tests ran on 2026-10-22 (the grabs on
an isolated Xwayland display). No physical keyboard check yet.

```ts
General > Shortcut mode > How it works                // dictate.shortcut-explainer
  -> Modal: Hold and Toggle cards, each the shortcut's keycaps (one per key,
     pressing together) beside two lanes (key above, recording below) on a
     chart-recorder tape: a still pen at the right, the tape scrolling left
     without end and fading out at its left edge; opening it ends a shortcut
     recording and closes the model picker
  ├── Opens centred at its Hold height in either mode; a window resize
  │   centres it again
  ├── Long chord -> The panel widens (up to the window) so each card's lanes
  │   keep at least 64 pt beside the keycaps
  ├── Pick a card (click, Enter, Space) -> Saves Shortcut mode
  ├── Hold -> Double-tap to lock row: switch + striped hands-free lane
  ├── Toggle -> No lock row; it folds inside its own slot and the panel's
  │   bottom edge rises with it (the top stays put)
  ├── Tab -> Hold, Toggle, lock switch, Close; focus stays inside
  ├── Close, Escape, or a press outside -> Closes; focus returns to the link
  ├── Change pane, menu bar, or `hex window show` -> Closes
  └── Reduce Motion -> One whole take, still, no wakes; turned off while
      open, the tape starts again when the window regains focus
```

The lanes are Rune shapes drawn at their laid-out width and redrawn from the
window's poll clock while the modal is open (`src/rune_ui/explainer.rs`).
Checks: `how_it_works_opens_the_shortcut_explainer_and_keeps_focus_inside`,
`explainer_cards_pick_the_shortcut_mode`,
`the_explainer_loops_on_the_poll_clock`,
`reduce_motion_shows_the_finished_explainer_without_waking`,
`the_explainer_restarts_when_reduce_motion_turns_off`,
`the_explainer_centres_at_its_hold_height_in_either_mode`,
`a_long_chord_widens_the_explainer` (five keys),
the tape's own `nothing_draws_past_the_pen`, `the_tape_scrolls_without_steps`,
`the_still_frame_shows_a_whole_take`, `narrow_lanes_draw_nothing` and
`the_past_leaves_the_tape` in `explainer.rs`,
`the_explainer_draws_a_keycap_per_key_and_lines_up_the_lanes`, and
`the_explainer_picker_and_shortcut_recording_never_share_the_window` in
[tests/panes.rs](../../src/rune_ui/tests/panes.rs), plus the
`shortcut-explainer`, `shortcut-explainer-toggle`, and
`shortcut-explainer-chord` (Ctrl+Alt+Space) goldens (frozen at 1.8 s into the
tape). The animation itself has been seen only in headless
frames, not on a real window; a headless trace of Hold, Toggle and rapid
switching on 2026-10-25 kept every element inside the panel and clear of
Close on every frame (no automated check yet).

On Linux, `hex dictation start` and `stop` bypass Hold, Toggle, and the
double-tap rules: start records unless something already records, stop
finishes, and `toggle` does whichever applies. Bound to a key's press and
release they give hold-to-dictate; a shortcut press still finishes a capture a
command started.

The short-tap threshold belongs to ordinary hold activation. Voice-delimited
and explicit programmatic capture have different intentional-start rules.

### Lock, With Or Without Hold

```ts
General > Double-tap to lock                          // dictate.lock; Hold only
  Hold -> Ordinary hold path remains available
  Tap -> Tap
    ├── Second press AND release <300 ms after first release -> LOCKED
    └── Slow second release -> No lock

General > Advanced > Double-tap only                  // dictate.double-tap-only
  Requires a key binding AND locking enabled
  Single press/hold -> No capture                     // disables hold, not a toggle
  Same two qualifying complete taps -> LOCKED         // same lock, not a third mode

LOCKED -> Speak hands-free -> Fresh shortcut press -> Finish
       -> Escape -> Cancel
       -> Missed key release -> Neutral keyboard repairs tracking -> Still LOCKED
```

Checks in [suppression.rs](../../src/suppression.rs):
`callback_double_tap_locks_for_modifier_and_key_bindings`,
`callback_hold_and_double_tap_boundaries_are_milliseconds`,
`second_tap_finishes_as_press_and_hold_when_double_tap_cannot_lock`,
`double_tap_only_waits_for_two_complete_key_chord_taps`,
`locked_dictation_finishes_after_a_missing_key_release`.
These cover gesture decisions, not hardware keyboard-layout or remapping equivalence.

### Smart Turn Auto-Stop (Experiment `smart-turn`)

macOS only, off unless the `smart-turn` [experiment](experiments.md) is on.
It finishes a hands-free capture when the speaker seems done, so a locked
capture needs no finishing press.

```ts
Shortcut or spoken-start dictation records         // dictate.smart-turn
  // only double-tap LOCKED, Toggle after its release, `hex dictation start`
  // (it locks), or the spoken start phrase may stop; never Hold, Voice
  // Action, edit, API capture, or meetings
  -> First such capture -> Find the ONNX Runtime library, then download and
       verify the pinned model           // models/smart-turn-v3.2-cpu.onnx, size + SHA-256
  ├── Model not ready yet -> Nothing is followed; no check, no fallback
  └── Model ready -> 16 kHz, 20 ms frames, the meeting pause detector's speech rule
        // shortcut: from the press, so speech while the key is down counts
        // spoken start: only audio after the start phrase ends
        ├── Under 200 ms of speech so far -> Pauses don't count
        ├── Not hands-free yet (key still down) -> Speech counts, pauses don't
        │     // the silence clock starts again at the lock
        ├── Silence reaches the check point (300 ms)
        │     -> Worker scores the last 8 s     // off the audio loop; newest pause wins
        │     ├── P >= threshold (0.7) -> Finish as a locked finishing press
        │     │     // hotkey: same release tail, transcription, paste, tones;
        │     │     // shortcut presses during the tail are ignored
        │     │     // spoken start: as the stop phrase; a control phrase at
        │     │     // the end of the transcript still applies, so
        │     │     // "dictate cancel" cancels and "dictate send" sends
        │     ├── P < threshold -> Keep listening
        │     └── P not a finite number -> `failed`; keep listening
        ├── Silence reaches the fallback (2 s) -> Finish the same way
        ├── Speech resumes, or the capture ends or changes -> Late answer ignored
        ├── Escape or shortcut press queued -> Handled first; the stop waits
        └── Download, runtime, or model fails -> Logged; dictation as if it were off
```

`HEX_SMART_TURN_CHECK_MS`, `HEX_SMART_TURN_THRESHOLD`, and
`HEX_SMART_TURN_FALLBACK_MS` override the defaults at launch (invalid values
are logged and ignored; the fallback is at least 500 ms after the check, so
the model is always asked first). A
patient preset is `1500`, `0.8`, `3000`. Every check writes one `smart_turn`
event to `live.ndjson` with the probability, silence, decision (`stop`,
`continue`, `fallback`, `stale`, `failed`), and inference time, never text.
The model runs on the ONNX Runtime library Moonshine already ships, loaded
with `ort`'s `load-dynamic`; its Whisper log-mel features are computed in Rust.

Checks in [smart_turn.rs](../../src/smart_turn.rs): `log_mel_matches_whisper_feature_extractor`
(against `transformers`' `WhisperFeatureExtractor(chunk_length=8)` on a
synthetic signal), `checks_once_per_pause_after_speech`,
`silence_alone_never_checks_or_stops`,
`a_confident_answer_stops_and_a_doubtful_one_waits_for_the_fallback`,
`answers_about_an_earlier_pause_are_stale`,
`the_model_is_always_asked_before_the_fallback`,
`stops_a_hands_free_capture_when_the_model_hears_the_end`,
`an_answer_for_an_ended_or_newer_capture_does_nothing`,
`an_ended_capture_drops_its_waiting_check`,
`nothing_is_followed_or_stopped_before_the_model_is_ready`,
`a_failed_load_leaves_dictation_alone`,
`speech_while_held_counts_but_only_a_hands_free_capture_stops`,
`listening_starts_after_the_start_phrase`,
`a_non_finite_probability_fails_the_check`,
`non_finite_samples_read_as_silence`, and `off_reads_and_loads_nothing`,
all with a scripted predictor; `a_spoken_control_decides_a_smart_turn_voice_target`
in [parakeet.rs](../../src/parakeet.rs). The ignored
`real_model_matches_python_on_the_reference_signal` downloads the model and
matched Python onnxruntime 1.23.2 (P 0.8643) on 2026-10-24. Not covered: the
recognition-loop wiring (queued input, the release tail, spoken controls end
to end), a physical microphone, or whether the defaults suit real speech.

### Warm And Cold Capture

```ts
Keep ready -> Press shortcut                          // dictate.boundaries
  -> Include up to 450 ms of available pre-roll
  -> Delayed handling still uses original event boundaries
  -> Release excludes audio captured after physical release + 200 ms

Release or locked finishing press                     // dictate.release-tail
  -> Hold threshold judged on the physical release alone
  -> Wait for audio through release + 200 ms (POST_RELEASE_TAIL)
     ├── Arrives -> Clip ends exactly at release + 200 ms
     └── Stream stalls/fails -> Finish within tail + 100 ms with what arrived
  -> Same for Voice Action and promotion; not Escape, short taps, voice
     controls, programmatic stop, or meetings
  -> Linux and Windows wait the same tail; Linux ends at the release it observes
     (an X11 key release, a portal Deactivated, or `hex dictation stop`)
  -> A press inside the tail starts after the old clip finishes, with its own pre-roll

Release that will transcribe                          // dictate.stop-tone
  -> Stop tone and mute/pause/sleep restore at the release, while the tail is
     still captured; a muted output (macOS, Linux) unmutes first, then the tone
     ├── Short tap or chord -> No stop tone; the start tone is retracted
     ├── Clip later near-silent -> Tone already played; discard, nothing pasted
     └── Escape after the release -> Not bound; the clip still transcribes
  -> macOS: the audio owner announces it before draining the tail; HUD
     Transcribing and submission still follow the tail
  -> Paused media resumes without delaying the tone
  -> Voice controls (no tail) -> Tone when the clip is submitted, as before
  -> The silence gate ignores the tail, so a tone the microphone hears there
     never makes a silent clip transcribe

Release when idle -> Press shortcut                   // dictate.cold-microphone
  -> Open microphone asynchronously, no pre-roll
     ├── Release before readiness -> Discard
     ├── Stream fails before the hold threshold -> Reopen for the same press
     └── Ready while held or locked -> Capture
  -> Capture idle -> Microphone can close             // accepted jobs do not keep it open
```

Source: `DictationCapture` in [dictation.rs](../../src/dictation.rs); checks:
`delayed_press_reconstructs_audio_from_the_original_boundary`,
`delayed_release_removes_audio_captured_after_the_physical_release`,
`capture_continues_past_sixty_seconds_until_explicitly_finished`,
`release_tail_keeps_audio_through_the_tail_and_no_further`,
`short_tap_discards_on_its_physical_release_despite_the_tail`,
`resampled_clips_keep_the_last_word_through_a_delayed_release` (44.1/48 kHz,
streamed and not, with the tail). These cover sample-buffer/timestamp logic, not
a microphone recording. The physical release stays exact; the 200 ms tail is an
explicit extension of the trim boundary. The opt-in `release_boundary_harness`
([dictation_tail_tests.rs](../../src/dictation_tail_tests.rs); `HEX_TAIL_MODEL`,
`HEX_TAIL_WAVS`) and `evaluate_dictation_corpus`
([dictation_corpus_tests.rs](../../src/dictation_corpus_tests.rs);
[`scripts/dictation-corpus.sh`](../../scripts/dictation-corpus.sh),
`HEX_DICTATION_CORPUS_DIR`) replay recordings through the capture timeline at
release offsets (`-300 ms` to `+200 ms`) and onset offsets (`-300 ms` to
`+100 ms`). On 2026-10-15, before the tail, across 102 clips (70 real voice
recordings, 20 macOS `say` clips, and 12 ElevenLabs `eleven_v4` clips) with
Unified English, clips matched the release to the sample; releasing at
`0 ms`..`+200 ms` kept the final word on 10/10 real clips (`-50 ms`: 9/10;
`-100 ms`: 7/10; `-150 ms`: 4/10; `-200 ms`: 2/10). Warm 450 ms pre-roll kept the
initial word even when speech began `-300 ms` before press, whereas cold 0 ms
pre-roll lost initial syllables when speech preceded readiness by `150–300 ms`.
The since-removed pause chunking on or off, and 1 versus default decoder
threads, gave identical tail results. The harnesses now apply the tail; they have not been re-run with it. The
tail also keeps any key-up or click sound made at release; no filtering is applied.
The owner in [dictation_audio.rs](../../src/dictation_audio.rs) has
`cold_capture_restart_rejects_late_open_results_and_closes_after_finish` and
`pending_open_uses_physical_press_for_intentional_threshold`: controlled opening
and ownership transitions, not physical device startup latency. Tail checks there
drive the owner with a fake stream: `release_waits_for_its_tail_and_keeps_no_later_audio`,
`a_press_inside_the_tail_starts_its_own_capture_after_the_old_one_finishes`,
`a_stalled_or_failed_stream_bounds_the_tail_wait`, and
`escape_and_short_taps_finish_without_waiting_for_a_tail`. The macOS coordinator
blocks on the tail, so HUD Transcribing and submission follow the release by up
to 200 ms (300 ms on a stalled stream). The stop tone no longer does: the audio
owner's `Finish` ends the recording environment through
`end_recording_environment_then` and plays the tone from the environment worker
once a muted output is restored (at once with paused media), then drains the
tail; `finish_dictation` plays the tone itself only for tail-free finishes. This
macOS change compiles and passes Clippy on hailmary (2026-10-22); physical tone
timing and the unmute-then-tone order on a Mac audio device remain unverified.
Linux and Windows play the stop tone at the release too. On Linux this used
to follow the tail too: on 2026-10-22, an isolated service on apollo (ALSA
Loopback microphone and speaker, `hex dictation stop`) measured the stop tone
requested 203 ms after the release was handled before the fix and within the same
loop pass after it (about 40 ms after the CLI mark, mostly process start), with
the tail still collected for 203 ms afterwards; the output was already open, so
no device reopen was involved. Checks: `a_release_finishes_its_stream_with_the_complete_clip`
(a release that transcribes is announced before its tail),
`escape_and_short_taps_cancel_the_stream_and_submit_nothing` (a short tap is
silent), `restoring_without_a_mute_runs_its_callback_first`, and
`the_stop_tone_in_the_tail_does_not_make_silence_transcribe`. The mute ordering
(unmute, then tone) is checked by reading the code, not on a muted device, and a
physical F12 release has not been recorded.

### Streaming Finish

```ts
Model advertises streaming, GPU device             // dictate.streaming
  // Metal on macOS, Vulkan on Linux and x64 Windows
  Load or switch model -> Probe a one-second silent stream (also prewarms the stream graph)
       ├── Succeeds -> Stream shortcut captures
       └── Fails, CPU device, Apple Speech, or HEX_STREAMING=0 -> Whole-clip path only
  Shortcut capture passes the hold threshold (Toggle on Windows and Linux: on press)
    -> Open one live stream; feed the 16 kHz backlog, then every 160 ms
       // same audio as the final clip, pre-roll included; never blocks capture
  Release -> Collect the 200 ms tail -> Finish with the complete clip
    -> Feed any unfed remainder -> Finalize -> Final text
       ├── Stream failed, produced no text, or was fed > 250 ms past the clip
       │   -> Transcribe the whole clip offline (source=fallback)
       ├── No final text within 1.5 s -> Abort the stream -> Whole clip
       └── Model switched during the capture -> Stream closed -> Whole clip
  Short tap, Escape, chord discard, microphone failure -> Drop the stream
  Voice-delimited and programmatic captures (macOS) -> Whole clip after release
  Nothing is shown or pasted until the final text exists
  Picker -> "Streams" tag on catalog models whose artifact streams   // setup.model-card
  Picker -> Per-language Recommended/Fastest/Most accurate cards         // setup.model-routing
  ├── el: Cohere; tr, ms, fil, fa, mk, ar, ja, fi, sv, da: Whisper turbo first
  ├── auto, yue, ko: no SenseVoice; mt: Parakeet v3 only (Whisper fails Maltese)
  └── Saved or installed model no longer recommended -> Still listed, no badge,
      selectable and deletable; never auto-switched or auto-downloaded
```

Source: [transcription_engine.rs](../../src/transcription_engine.rs) (one thread
owns the model on every OS; streams, finishes, offline runs, and macOS model
loads and switches share its FIFO because a live stream holds the model's
compute lease), `probe_stream` and `with_stream` in
[gguf_session.rs](../../src/gguf_session.rs), the stream entry points in
[local_transcriber.rs](../../src/local_transcriber.rs) and
[parakeet.rs](../../src/parakeet.rs), and `begin_stream` in
[dictation.rs](../../src/dictation.rs): the capture owns its stream, feeds it as
audio arrives, cancels it with the recording, and hands it to the finished clip.
macOS streams hotkey captures from the audio owner in
[dictation_audio.rs](../../src/dictation_audio.rs); `DictationWorker::transcribe`
hands the clip to the engine when it is submitted, so its stream finishes before
a later capture opens one, and the job then waits for its text in order. Linux
([linux_dictation.rs](../../src/linux_dictation.rs)) and Windows
([windows_dictation.rs](../../src/windows_dictation.rs)) drive the same engine
from their capture loops. Parakeet buffers with its largest context menu
(Unified English: 5.6 s left, 1.04 s chunk, 1.04 s right), its most accurate and
least compute-intensive setting; `HEX_STREAM_CHUNK_MS` overrides the chunk for
benchmarks and `HEX_STREAMING=1` streams on any device, including a CPU
(diagnostics and the Linux CI fixture). Each dictation logs
`source=stream|offline|fallback` and `finish_ms`. The macOS "Transcribe during
pauses" setting no longer affects dictation.

Checks: `recorded_audio_is_a_prefix_of_the_finished_clip` (16, 44.1, and
48 kHz), `resampled_clips_keep_the_last_word_through_a_delayed_release` (streamed
and not), `a_stream_starts_with_the_backlog_and_feeds_in_blocks`, and
`short_taps_cancellation_and_unstreamable_models_end_the_stream` in dictation.rs;
`hotkey_captures_stream_once_intentional_and_others_do_not` in dictation_audio.rs;
`hold_streams_after_the_threshold_and_toggle_streams_from_the_press`,
`a_release_finishes_its_stream_with_the_complete_clip`,
`escape_and_short_taps_cancel_the_stream_and_submit_nothing`, and
`a_model_that_cannot_stream_transcribes_the_whole_clip_after_release` in
linux_dictation.rs; `source_labels_distinguish_fallbacks`,
`streams_send_their_audio_in_order_and_cancel_when_dropped`, and (macOS, a real
engine thread without a model) `a_finish_without_a_stream_transcribes_the_clip_offline`,
`an_unusable_stream_falls_back_to_the_complete_clip`,
`work_runs_in_order_and_failures_reach_the_caller`, and
`dropping_the_engine_stops_despite_an_open_stream` in transcription_engine.rs.
These use synthetic audio and recorded engine commands. The Linux CI fixture
[`scripts/test-linux-dictation.sh`](../../scripts/test-linux-dictation.sh)
replays the same speech through a virtual microphone twice with the real model:
once on the CPU (whole clip, `source=offline`) and once with `HEX_STREAMING=1`
(`source=stream`, or `fallback` when the CPU stream misses its deadline). The
hidden Windows `model stream-bench <wav> [--speed N]` command and the opt-in
macOS `metal_stream_tracks_offline_transcription` (parakeet.rs;
`HEX_STREAM_MODEL`, `HEX_STREAM_WAVS`, `HEX_STREAM_SPEED`) replay a WAV in real
time and compare both paths.

**Observed October 19, 2026, RTX 5070 Ti, Vulkan, Unified English Q8, Windows
TTS clips replayed at 4× (131 s also at 1×):** release-to-text 55/55/93 ms
(10/27/131 s) streaming versus 82/185/873 ms whole-clip; GPU busy 3.1–3.6% of
the dictation; process peak working set 214 MB and about 3% of one core during
a real-time 131 s stream. Against the script, both paths transcribed the 10 s
and 27 s clips perfectly; on 131 s the stream added one homophone error ("cache"
→ "cash") to formatting differences both paths share. Smaller chunks (560 or
160 ms) added more errors and up to 20% GPU without a faster release. After the
engine became shared (same machine and clips at 4×): 56/91 ms streaming versus
92/865 ms whole-clip for 10/131 s, with 0/28 and 1/408 word differences, matching
the earlier build.

**Observed October 19, 2026, MacBook Air M4, Metal, Unified English Q8, the same
clips at 4× through `metal_stream_tracks_offline_transcription`:** Metal
qualified for streaming and both clips finished from the stream: 140/253 ms
release-to-text (10/131 s) versus 156/2,838 ms whole-clip, with 0/28 and 1/408
word differences. This drives the production engine and Parakeet adapter, not
the shortcut, microphone, or paste. Not measured: physical microphone corpora,
noisy rooms, or Linux on a real GPU.
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
[local_transcriber.rs](../../src/local_transcriber.rs). Cohere's pinned backend
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

The macOS `Transcriber` owns backend input padding for ordinary and
voice-protocol transcription. `short_unified_input_is_minimum_padded_before_trailing_context`
in [transcription.rs](../../src/transcription.rs) checks minimum-duration padding
followed by Unified English's additional trailing silence. This is a pure sample
policy check, not native inference-quality proof. Padding is now included in
`inference_ms`; `prepare_ms` covers clip conversion and diagnostic retention.

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
Accepted job on a GPU device             // dictate.inference-threads
├── Inference thread -> macOS user-interactive QoS
└── GGUF session -> One CPU decoder thread, no ggml worker pool
    // CPU-only devices keep the backend's default thread count
```

Source: `session_options` in [gguf_session.rs](../../src/gguf_session.rs) and
`prioritize_inference_thread` in [parakeet.rs](../../src/parakeet.rs); check
`gpu_sessions_bound_decoder_threads_and_cpu_sessions_keep_library_default`.
Parakeet's per-token TDT decoder runs on CPU even with Metal encoding. ggml's
default pool (up to 8 spin-waiting threads) is created with plain `pthread_create`,
so its workers start at default QoS rather than inheriting the inference thread's.

**Observed October 14, 2026, standalone `transcribe-cpp` 0.2.4 bench on a Mac
Studio (18 cores) already busy with other work, Unified English Q8, Metal:** under
36 extra `yes` processes, user-interactive QoS medians for 3/30/90-second clips were
170/703/1,819 ms with one thread, 136/1,907/4,940 ms with two, and 178/4,304/13,519 ms
with four. Without the extra load, one thread measured 83/364/1,631 ms and two
64/705/2,889 ms. This is backend timing, not physical capture/paste on the
reporter's MacBook Pro, and not Linux runtime proof.

```ts
Finish
  ├── Near-silent capture -> Discard, no inference    // dictate.silence-gate
  │       // loudest 30 ms window (DC removed) below -50 dBFS; no paste,
  │       // History, or last-result update; process.log records the level
  ├── Capacity available -> Accepted job -> Local transcription -> Mode processing
  │    -> Paste successful nonempty output once, at current focus
  │       // the clipboard write carries nspasteboard.org Transient and
  │       // AutoGenerated markers, so conforming clipboard managers skip it
  │       // Chromium-browser focus (by bundle ID) waits 150 ms between the
  │       // clipboard write and ⌘V (#136); every other app posts ⌘V at once
  │       // synthetic ⌘V resolves on any active layout: it uses the layout's
  │       // Command layer ("Dvorak – QWERTY ⌘"), and a non-Latin layout
  │       // falls back to the ASCII-capable layout like AppKit itself
  └── Queue full -> Explicit rejection                // bounded admission

A pending -> Start capture B                          // dictate.overlap
  -> B records independently; A is not cancelled
  -> Accepted outputs retain submission order
  -> Completing/pasting A must not end B
```

Source: `finish_dictation` in [recognition.rs](../../src/recognition.rs).
The silence gate stops models from hallucinating closing phrases such as "thank
you" on an empty hold ([#108](https://github.com/anomalyco/hex/issues/108)). It
covers hotkey, voice-delimited, and Voice Action captures on macOS, and the Linux
hotkey loop. The local API's programmatic dictation and host-audio transcription
are deliberately left ungated: they return the model's raw output to the caller.
The threshold is conservative, so speech is not dropped; a noisy room, the start
tone, or a key click can lift a capture above it, and those captures still reach
the model. Hallucinations after real speech are not addressed.
`silence_gate_discards_only_near_silent_captures` in
[dictation.rs](../../src/dictation.rs) checks the level policy on synthetic
samples, not real microphone noise floors.
Key resolution: [keyboard.rs](../../src/keyboard.rs); non-Latin layout
fallback details live in [shortcut recovery](recovery.md#keyboard-layout-resolution).
Checks in [parakeet.rs](../../src/parakeet.rs):
`parallel_processing_results_are_released_in_submission_order`,
`ordered_waiting_outputs_remain_bounded_after_channel_drain`.
These cover output bookkeeping/admission, not sustained end-to-end inference/paste.
`transcript_write_is_marked_for_clipboard_managers` in [paste.rs](../../src/paste.rs)
checks the markers on a private pasteboard; whether a given clipboard manager
(Maccy, Raycast) honors them is not verified here. The restored previous clipboard
is written back unmarked, exactly as captured.
`only_chromium_browsers_delay_the_paste_shortcut` and
`chromium_delay_runs_between_the_clipboard_write_and_the_paste_shortcut` check the
bundle-ID policy and ordering with fakes, not a real Chrome page.

**Known issue:** Chrome 155 and 156 on macOS drop the insertion into web page
text fields when ⌘V arrives right after a pasteboard write
([#136](https://github.com/anomalyco/hex/issues/136); Chromium
[CL 8503629](https://chromium-review.googlesource.com/c/chromium/src/+/8503629),
expected in Chrome 157). HEX waits `CHROMIUM_PRE_PASTE_DELAY` (150 ms) before ⌘V
when the frontmost app's bundle ID belongs to Chrome (all channels), Brave, Arc,
Dia, Edge, Vivaldi, Opera, or Chromium, including their installed web apps. This
workaround is untested against a live Chrome 155 page; Electron apps and
unlisted Chromium browsers keep the immediate ⌘V. The delay follows commitment,
so Escape cannot cancel it. Remove it once Chrome 157 is the floor.

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
  Shortcut press -> Start tone ~50 ms later                  // dictate.start-tone
     -> Chord within 40 ms (e.g. Option-Command-I) -> Silent
     -> Chord or short tap before 300 ms -> Tone fades out over 20 ms
     -> Hold reaches 300 ms -> Tone already played; mute/pause/sleep begin now
     -> Voice Action takeover -> Retracted; Voice Action rings at its own hold
     -> Release microphone while idle -> Output warms on press; tone once
        the microphone is open and the hold counts
  Tone requested -> Playback worker opens the default output, then plays
     -> Output stays open five minutes past the last tone, then releases
     -> Released output holds no macOS idle-sleep assertion
  Feedback volume = 0 -> No tones
     -> Playback worker releases output on its next 250 ms observation
     -> Volume enabled again -> Reopen for the next tone, never capture/UI
  Cold audio stack exceeds preload admission -> Warn once; recognition continues
     -> Loader keeps running; later tones play once the output opens
```

Feedback admission is advisory on every platform: a cold audio stack can exceed
the two-second preload timeout, so the recognition worker logs a warning and
continues rather than stopping. The loader thread keeps running and registers
the player when the default output finally opens, restoring tones for the rest
of the session. Linux already logged and continued; the macOS worker now
matches that behavior instead of stopping desktop recognition.

The playback worker never opens the output at startup. It opens the default
output for the first tone, keeps it through a five-minute idle grace so bursts
of dictation share a warm device, and releases it after that grace or on the
next observation after sounds turn Off. macOS idle-sleep timers count from the
last user input, and a tone implies recent input, so the grace never delays
sleep by itself. A device open already in progress must return before the
worker can observe that change. Output-open failures retry at most once per
two seconds, and a released device reopens without that backoff. Bundled
samples remain decoded in memory. A tone that arrives after the grace pays a
cold device open first; the tone is delayed, not clipped or dropped.
Overlapping tones extend the playing window rather than shortening it.

**Fixed after 2.3.0:** the start tone used to wait for the 300 ms intentional
hold, then pay rodio's default 50 ms output buffer, so it sounded about 345 ms
after the press (about 375 ms from a cold device) where the Swift app rang on
key-down. The press now requests a provisional tone. The worker opens the
output at once, waits out a 40 ms chord grace (on Kit's Mac about 40% of
discarded presses were modifier chords landing within ~25 ms), then plays
through a 512-frame (~10 ms) buffer: about 50 ms warm or cold on the built-in
speakers. A chord or short tap retracts the tone, dropping it inside the grace
or fading it over 20 ms. Recording mute, media pause, and idle-sleep prevention
still begin only at the 300 ms threshold. Checks:
`provisional_start_sounds_after_the_chord_grace_unless_retracted`,
`retracted_tone_fades_out_without_a_click_or_channel_skew`, and the ignored
`native_start_tone_path_latency` (open-to-presentation timing, with
`HEX_TONE_PATH=legacy` for the old path; route output to a silent device or set
`HEX_TONE_MUTED`). The chord-to-retraction path is checked by reading the code,
not by a physical keyboard run.

**Fixed in 2.1.21:** through 2.1.20 the worker opened the output at startup
and kept it open for as long as sounds were On, so `coreaudiod` held a
`PreventUserIdleSystemSleep` assertion for HEX's PID and an idle Mac never
slept ([#95](https://github.com/anomalyco/hex/issues/95)). 2.1.18 released the
stream only when sounds were Off. Startup admission therefore no longer
reports `could not open the audio output`; a device failure surfaces as a
warning when the first tone plays.
`output_opens_for_a_tone_and_releases_after_the_idle_grace` and
`overlapping_tones_extend_playback_instead_of_truncating_it` check ownership,
grace, and drop with a controlled sink;
`failed_output_initialization_retries_with_a_bounded_backoff` checks
failure-to-success recovery without an audio device. The opt-in
`native_output_release_clears_the_idle_sleep_assertion` opens the real default
output and reads `pmset -g assertions`; on September 22, 2026 it observed the
assertion appear for the test PID and clear within a second of release on the
built-in speakers. That is not an installed-app or USB/Bluetooth measurement.
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

Check: in a debug app, `hex dev hud recording|transcribing|processing|reset`
or the HUD Lab pane drives the real HUD with deterministic state, so
fullscreen and Space transitions can be checked. (The `dictation-hud` preview
that looped the sequence went with GPUI.) This does not prove that actual
capture caused that state.

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
- **Streaming:** `dictate.streaming` has benchmark, synthetic, and Linux CI
  (CPU-forced) evidence only. Real microphones, room noise, Bluetooth inputs,
  and a physical shortcut on a Mac or a Linux GPU have not been observed with a
  live stream.
- **Paste consumption:** current paste policy waits a fixed interval rather than
  acknowledging target consumption. Internal ordering does not establish
  correctness for an arbitrarily slow target application.
- **Native checks require deliberate setup:** permissions, model, keyboard,
  microphone, output device, and a disposable text target must be recorded. This
  map does not authorize operating the installed app or injecting global input.
