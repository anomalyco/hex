# Captures (experimental, a build option)

Scrawl's capture engine hosted inside Hex, step 3 of
[the plan](../plans/capture-in-hex.md). It runs the real `scrawl-capture`
crate, imported into `crates/capture`: it records the active display with
ScreenCaptureKit and narration from Hex's microphone, transcribes the
narration with word timestamps on Hex's loaded model, and writes a Scrawl
bundle. No shortcut, input, overlay, or
routing yet.

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
└── Off -> no capture host, engine, sink, thread, or folder; `hex capture` -> feature-off

hex capture start                        // capture.start; needs a listening Hex
├── Creates <support>/captures/<unix ms>.scrawl (owner-only) -> reply: recording + bundle path
├── Two stopped captures still writing -> refused `busy`
├── Copies the dictation timeline from now into a bounded sink (no second stream)
├── In the background: Scrawl's engine starts on the active display
│   ├── Needs Screen Recording for Hex (or the terminal running it)
│   └── Fails to start -> sink closed, bundle removed, last outcome `failed`
├── Release microphone while idle -> the capture keeps the microphone open
└── Already recording -> unchanged, same bundle

While it records                         // capture.audio
├── Dictation, Voice Action, and meetings work as usual on the same audio
├── Sink full (collector stalled) -> capture audio dropped as a gap, dictation untouched
└── Microphone gap or recovery -> the gap is recorded, positions stay on the clock

Never on Hex's path                      // capture.isolation
├── No microphone, event tap, global shortcut, or model of its own
├── No ingest server (47777), delivery retries, retention or recovery workers
├── No ingest token read or written; nothing read or written under ~/.config/scrawl
│   or Scrawl's Application Support or logs (its log goes to Hex's tracing)
└── Settings from Hex, not ~/.config/scrawl/config.json: auto-crop on (display
    captures), the agentContext summary format off; no Hex settings for them yet

hex capture stop                         // capture.stop
├── Closes the sink (≤ 100 ms wait) -> reply: idle + the bundle path, at once
├── The screen recording, window log, and input stop at once (Capture::stop)
└── In the background, on one capture-finish thread, one capture at a time:
    ├── 16 kHz audio with its start offset and gaps -> SealedCapture::finish
    ├── Words: 30 s chunks, each a foreground job on the dictation engine's model,
    │   after queued dictation, before meeting chunks; waits while dictation records
    │   or transcribes; never interrupts a live stream
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

Sources: [capture_host.rs](../../src/capture_host.rs), the sink in
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

Observed 2026-10-25 on pro (M2 Max), before the real engine, with the
stand-in: `HEX_EXPERIMENTS=capture hex dev capture-replay` over three
vocabulary fixtures (6.5 s) loaded Parakeet unified English on Metal and
wrote 18 words with plausible times. The real engine has not run end to end:
the unit tests use a fake engine over Scrawl's real bundle writer, and the
mini (the test Mac) has no Screen Recording grant for a dev build, so neither
`hex capture` against a live app nor `capture-replay` has recorded a screen.
