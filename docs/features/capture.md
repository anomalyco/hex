# Captures (experimental, a build option)

Scrawl's capture engine hosted inside Hex, steps 3 and the first half of 4
of [the plan](../plans/capture-in-hex.md). It runs the real `scrawl-capture`
crate, imported into `crates/capture`: it records the active display with
ScreenCaptureKit and narration from Hex's microphone, takes Shift ink and
clicks from Hex's event tap, transcribes the narration with word timestamps on
Hex's loaded model, and writes a Scrawl bundle into Scrawl's captures folder.
Settings › Experiments › Scrawl turns it on and holds its settings. No
capsule, ink or laser overlay, or routing yet.

```ts
Turn it on                               // capture.gate
├── macOS build with `--features capture`, debug or release; off in every public build
│   ├── `HEX_CAPTURE=1 scripts/build-app.sh` -> Hex.app with scrawl-helper signed
│   │   beside `hex`; scripts/check and macOS CI test with the feature
│   └── + `hex experiments enable capture` (or HEX_EXPERIMENTS=capture) -> restart
├── Any build without the feature (every public release, Linux, Windows, Nix) ->
│   `capture` is not an experiment: absent from `hex experiments list`, `enable
│   capture` is an unknown name, and a file line naming it is ignored; `hex capture`
│   -> unsupported. The crate is not compiled at all
├── Release path -> release-app.sh refuses HEX_CAPTURE; validate-app.sh refuses a
│   bundle with scrawl-helper or the capture engine in `hex`
├── Experiment on -> Settings › Experiments › Scrawl appears
└── Off -> no capture host, engine, sink, thread, or folder; `hex capture` -> feature-off

Settings › Experiments › Scrawl          // capture.pane
├── Capture with Hex (capture.enabled, default off) -> off: no host, shortcut,
│   input, retention, or folder, as with the experiment off
│   └── On while the shortcut collides -> refused, the shortcut row says why
├── Shortcut (capture.hotkey, default ⌥⇧S) -> same keycap and recorder as dictation;
│   a key is required; refused when it overlaps dictation, Voice Action, or paste
├── Screen Recording -> Allow asks macOS once, then opens its System Settings list;
│   never part of onboarding (macOS also asks at the first capture)
├── Transcriber (capture.transcriber) -> Parakeet (default) | Apple
├── Captures folder (capture.folder, default Scrawl's) -> Reveal | choose another
├── Keep (capture.media_days / bundle_days) -> Scrawl's presets, or imported days
├── Scrawl.app running -> "Scrawl is running" card + Use Hex for captures
│   └── Click -> asks Scrawl to quit (nothing else quits it); Hex takes the
│       shortcut within 2 s of Scrawl leaving
└── No iPhone row: pairing needs the ingest server, which Hex doesn't run yet

First launch with the experiment on      // capture.import
├── ~/.config/scrawl/config.json -> transcriber (apple; else Parakeet), capturesDir,
│   retention, read once (migration `scrawl-capture-settings`); token not used
├── Scrawl's Application Support config.json -> its shortcut, unless it collides
└── Scrawl's files are never written

⌥⇧S (capture.shortcut; captures on, Scrawl not running)
├── Press -> start, as `hex capture start`        // a fresh press only, no repeats
├── Press again -> stop, as `hex capture stop`    // ignored within 400 ms of the start
├── Esc while a capture records -> cancel it, before any dictation job
├── The binding is swallowed, never chord evidence for dictation
└── Hold the dictation shortcut mid-capture -> dictation pastes as usual

While a capture records                  // capture.input
├── The existing session tap adds mouse down/up/drag/scroll and Shift (no second tap)
├── -> Capture::input() through a bounded queue; full -> events dropped and counted
├── Keys pressed in the app -> logged; keys Hex consumes (its shortcuts, Esc) and
│   anything while a shortcut is being recorded -> not logged
├── Scrawl's rules in the engine (hosted_input): Shift drag inks, a click pins,
│   clicks on Hex's own windows ignored
└── Shift still held from ⌥⇧S -> must come up once before it can ink; Shift up
    at the start -> the first Shift drag inks

hex capture start                        // capture.start; needs a listening Hex
├── Creates <captures folder>/<unix ms>.scrawl (owner-only) -> reply: recording + bundle path
├── Two stopped captures still writing -> refused `busy`
├── Copies the dictation timeline from now into a bounded sink (no second stream)
├── In the background: Scrawl's engine starts on the active display
│   ├── Needs Screen Recording for Hex (or the terminal running it)
│   └── Fails to start -> sink closed, bundle removed, last outcome `failed`;
│       Esc and the next ⌥⇧S see nothing recording
├── Release microphone while idle -> the capture keeps the microphone open
└── Already recording -> unchanged, same bundle

While it records                         // capture.audio
├── Dictation, Voice Action, and meetings work as usual on the same audio
├── Sink full (collector stalled) -> capture audio dropped as a gap, dictation untouched
└── Microphone gap or recovery -> the gap is recorded, positions stay on the clock

Never on Hex's path                      // capture.isolation
├── No microphone, event tap, global shortcut, or model of its own
├── No ingest server (47777), delivery retries, or recovery workers
├── No ingest token used; Scrawl's two config files read once by the import, never
│   written; nothing else of Scrawl's touched (its log goes to Hex's tracing)
└── Settings from Hex (capture.*), not Scrawl's config: auto-crop on (display
    captures), the agentContext summary format off

Retention                                // capture.retention
└── Hex's own thread: Scrawl's retention pass with capture.*'s policy, 90 s after
    captures turn on, then hourly; skipped while a capture records or writes,
    or while Scrawl.app runs (it cleans the shared folder then)

hex capture stop                         // capture.stop
├── Closes the sink (≤ 100 ms wait) -> reply: idle + the bundle path, at once
├── The screen recording, window log, and input stop at once (Capture::stop)
└── In the background, on one capture-finish thread, one capture at a time:
    ├── 16 kHz audio with its start offset and gaps -> SealedCapture::finish
    ├── Words: 30 s chunks, each a foreground job on the dictation engine's model,
    │   after queued dictation, before meeting chunks; waits while dictation records
    │   or transcribes; never interrupts a live stream
    ├── Word times moved earlier by the model's measured lag (Parakeet Unified
    │   English: starts −280 ms, −520 ms after a ≥ 250 ms pause; ends −195 ms);
    │   other models and Apple keep their own times
    ├── Past 20 min -> audio kept to 20 min, the rest one gap
    ├── recording.mp4, audio.wav, frames/, scrawl.json, summary.md; then every
    │   file 0600 and folder 0700
    ├── Keyframes decoded from recording.mp4 by scrawl-helper (beside `hex`: in
    │   Hex.app's Contents/MacOS or target/<profile>; or $SCRAWL_HELPER); helper
    │   missing -> the 480 px sampled frames
    └── Transcription fails -> logged; the bundle has no words, and the outcome says why

hex capture cancel -> engine cancelled, sink closed, bundle removed
hex capture status -> recording (bundle, elapsed), bundles still writing, last outcome
                      (words, audio gaps, or why it has no transcript)
Hex quits mid-capture -> cancelled, bundle removed; captures still writing stop
transcribing between chunks and keep their audio (shutdown waits ≤ 2 s)
```

`scrawl.json` is Scrawl's bundle manifest. Its `transcript.words` are
`{t0, t1, text}` in milliseconds from the capture's start (`t0_boot_ns`, the
boot-time clock audio and shortcuts share), and `audio.wav` starts at that
start too, padded with silence when the microphone began late or dropped
audio.

`hex dev capture-replay <wav>…` (debug, with the feature) runs the same
capture host on a fresh transcription engine with the selected model, feeding
WAV files instead of the microphone, and prints the bundle path. It needs the
experiment on too, and records the screen meanwhile.

Sources: [capture_host.rs](../../src/capture_host.rs), the tap in
[suppression.rs](../../src/suppression.rs), the shortcut and Escape in
[recognition.rs](../../src/recognition.rs), `CaptureSettings` and the import in
[app_settings.rs](../../src/app_settings.rs), the pane in
[rune_ui/scrawl.rs](../../src/rune_ui/scrawl.rs), the input rules in
[hosted_input.rs](../../crates/capture/src/hosted_input.rs), the sink in
[dictation_audio.rs](../../src/dictation_audio.rs),
`transcribe_with_words` in [parakeet.rs](../../src/parakeet.rs), and the
`scrawl-capture` control command in [control.rs](../../src/control.rs).

Checks:

- Gate: `capture_exists_only_with_its_build_option` in
  [experiments.rs](../../src/experiments.rs);
  `the_host_is_absent_while_the_experiment_is_off` in capture_host.rs;
  [test-app-identity.sh](../../scripts/test-app-identity.sh) for the
  release-path refusals.
- Sink, in dictation_audio.rs: `capture_sink_shares_the_timeline_with_dictation`,
  `a_full_capture_sink_drops_audio_as_a_gap_and_never_stalls_dictation`,
  `capture_sink_records_microphone_gaps`,
  `capture_sink_holds_a_released_microphone_open_until_it_closes`,
  `capture_sink_keeps_every_frame_through_timestamp_jitter`, and
  `a_dropped_capture_receiver_closes_the_sink`.
- Words, in parakeet.rs: `chunked_word_transcription_keeps_word_times_across_chunks`
  and `word_transcription_is_a_foreground_request_on_the_shared_engine`;
  `foreground_work_waits_for_an_open_stream_instead_of_interrupting_it` in
  [transcription_engine.rs](../../src/transcription_engine.rs).
- Host, in capture_host.rs (a fake engine that writes through Scrawl's real
  `finalize_capture_bundle_with_flags`, without a screen):
  `controls_are_idempotent_and_stop_writes_a_scrawl_bundle`,
  `an_engine_that_cannot_start_leaves_nothing_behind`,
  `scrawls_engine_claims_nothing_machine_wide`,
  `the_timeline_resamples_pads_gaps_and_ends_at_the_stop`,
  `a_pending_gap_and_a_stalled_tail_come_back_from_the_closed_sink`,
  `padding_past_the_length_limit_stays_bounded`,
  `captures_finish_one_at_a_time_and_starts_wait_for_room`,
  `a_capture_queued_behind_a_slow_finish_has_already_stopped`,
  `a_failed_start_raced_by_stop_never_wedges_later_starts`,
  `the_engine_config_comes_from_hex`, and
  `shutdown_cancels_the_recording_capture_and_waits_for_its_cleanup`.
- Shortcut and input, in capture_host.rs: `the_host_is_absent_while_captures_are_off`,
  `the_shortcut_starts_and_a_later_press_stops`,
  `input_reaches_only_the_capture_that_is_recording`, `tap_flags_become_modifiers`,
  `only_a_fresh_press_of_the_shortcut_counts`,
  `a_failed_start_ends_before_escape_or_the_shortcut_sees_it`;
  `the_capture_shortcut_is_suppressed_and_never_chord_evidence` and
  `a_capture_logs_only_keys_that_reach_the_app` in suppression.rs;
  the ink and click rules in hosted_input.rs, including
  `a_capture_started_by_its_shortcut_waits_for_shift_to_come_up`,
  `a_capture_started_without_shift_held_inks_from_the_first_stroke`, and
  `a_click_with_the_chords_shift_still_held_is_no_pin`.
- Pane: `captures_default_off_and_refuse_a_shortcut_in_use` and
  `retention_names_presets_and_imported_days` in rune_ui/scrawl.rs; scenes
  `mac-scrawl-off`, `mac-scrawl`, `mac-scrawl-running`, `mac-scrawl-permission`
  (feature builds only, so no goldens: those run on Linux).
- Settings: every `capture.*` key in `hex settings`
  (`every_persisted_setting_has_a_cli_key`).

Observed 2026-10-25 on pro (M2 Max), before the real engine, with the
stand-in: `HEX_EXPERIMENTS=capture hex dev capture-replay` over three
vocabulary fixtures (6.5 s) loaded Parakeet unified English on Metal and
wrote 18 words with plausible times. The real engine has not run end to end:
the unit tests use a fake engine over Scrawl's real bundle writer, and the
mini (the test Mac) has no Screen Recording grant for a dev build, so neither
`hex capture` against a live app nor `capture-replay` has recorded a screen.
The shortcut, tap input, Escape, Scrawl coexistence, and import are covered by
unit tests only; none has run against a live Hex.
