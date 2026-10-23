# Meetings (beta)

Record a call locally: HEX captures the other participants (system audio) and
you (microphone) as separate tracks, transcribes them on this Mac, and saves a
Markdown transcript with `You` and `Them` turns. Recording starts automatically
only for calls from a website or app set to Always record, and never stops
automatically. This map covers macOS; Linux has no meetings.

Linked checks are locators unless an executed result is recorded below.

## How To Get To It

```ts
Settings > Meetings > Meetings (beta)               // meetings.opt-in; off by default
├── On -> Meetings pane, menu-bar items, call detection, Option-Control-V
│   ├── System Audio Recording                  // meetings.permission; asked only now
│   │   ├── Allow -> macOS prompt the first time (tour's permission_control)
│   │   ├── Decided before -> Privacy pane + drag guide ("System Audio Recording Only")
│   │   ├── Asked -> pulse; TCC preflight re-read on each poll, no reopen needed
│   │   └── Granted -> Allowed
│   ├── Show recording indicator -> on by default; corner-snapping floating capsule
│   ├── Transcript folder -> ~/Documents/Hex Meetings by default; Reveal / choose
│   ├── Summarize with Apple Intelligence -> on by default; one-line availability
│   └── Detected calls -> Ask / Always record / Never offer per provider // meetings.policy
│       ├── Slack huddles, Zoom, Google Meet, Microsoft Teams, FaceTime (always listed)
│       └── then each website or app chosen for from an offer's "…"
└── Off -> Controller idles; no microphone-activity inspection, pane, or shortcut

Debug builds -> Always available (DEVELOPER_FEATURES_ENABLED)
Agents -> hex mcp (read-only MCP server; works whether or not Meetings is on)
```

Requires the selected local dictation model, Microphone, and System Audio
Recording (the "System Audio Recording Only" list under Privacy & Security ›
Screen & System Audio Recording; `NSAudioCaptureUsageDescription`). Screen
Recording alone still works through the ScreenCaptureKit fallback. The opt-in is `meetings.enabled` in `settings.json`, alongside
`meetings.show_recording_indicator` (default `true`) and
`meetings.recording_indicator_corner` (default `top_right`)
([app_settings.rs](../../src/app_settings.rs) `MeetingSettings`). Recording
refuses to start without either grant and asks macOS for System Audio Recording.

## Sub-features

### Detect And Offer

```ts
Zoom / Teams / Slack huddle / FaceTime / browser call uses the microphone
  -> CoreAudio input process -> canonical host app   // parent PID + bundle ID
  -> 4 stable observations (about 2 s)              // meetings.detect
     └── Browser on a page that is not a known call site -> 40 (about 20 s),
         so voice notes and dictation fields are not offered
  -> Provider = the website when known, else the app; the website's choice
     when it has one, else the app's (a browser's choice covers its other
     sites), else Ask
     ├── Never offer -> nothing shown; an earlier offer stays snoozed
     ├── Always record -> starts recording, no offer   // meetings.auto-record
     │   └── recording capsule says "Started automatically" for 10 s; with the
     │       indicator off, a "Recording automatically" capsule with Stop
     └── Ask -> Capsule below the menu bar, focus-free  // meetings.offer
         ├── Title: "Google Meet call detected", "Call on meet.jit.si",
         │   "Call in Brave Browser" (site unknown), or the meeting's name
         ├── Record -> Start recording
         ├── Not now -> Close
         ├── … -> "<provider>" · "Change anytime in Settings" (capsule widens)
         │   ├── Never offer -> saved, close
         │   ├── Always record -> saved, records this call too
         │   │   └── not offered for a browser on an unknown page
         │   └── Cancel
         └── 20 s timeout -> Close                     // never records on timeout
  -> Same call is snoozed by key and app until off the microphone for about 60 s,
     offered or recorded; stopping an automatic recording does not restart it

Choices persist as `meetings.providers` (`key`, `label`, `policy`:
`ask` / `always_record` / `never_offer`) in `settings.json`. Missing means Ask.
The older `meetings.never_offer` list reads as Never offer entries and is
rewritten as `providers` on the next load.
```

Chromium and Electron apps (Brave, Chrome, Edge, Slack, Teams) capture microphone
audio in sandboxed `audio.mojom.AudioService` utility processes (`*.helper`) that
do not register with `NSRunningApplication`; Zoom uses `us.zoom.CptHost`, Firefox
uses `org.mozilla.plugincontainer`, Safari uses `com.apple.WebKit.GPU` (`Safari
Graphics and Media`), and FaceTime uses `com.apple.avconferenced`.
[microphone_activity.rs](../../src/microphone_activity.rs) attributes each input
process to its canonical host app via parent PID lineage, CoreAudio's
`kAudioProcessPropertyBundleID`, and `NSRunningApplication`. When Brave uses the
microphone and is not snoozed, `ContextSnapshot::capture_for_meeting` in
[context.rs](../../src/context.rs) inspects Brave's active and open tabs for a
`meet.google.com` call even if Brave is behind another window or tab.

On 2026-10-21 Brave opened the microphone for 3-6 s at a time on
problems.kitlangton.com (board voice notes) and each burst got a "Browser call
call detected" offer; that is what the 20 s browser threshold prevents.
Checks: `a_browser_voice_note_is_not_offered_but_a_sustained_browser_call_is`,
`offer_titles_name_the_service_site_or_browser_without_repeating_call`,
`choices_apply_to_the_site_when_known_and_the_app_otherwise`,
`always_record_starts_after_the_same_stability_count_as_an_offer`, and
`never_offer_and_snoozed_calls_never_start` in
[meeting_detection.rs](../../src/meeting_detection.rs);
`meeting_providers_ask_by_default_and_legacy_never_offer_entries_stay_never` in
[app_settings.rs](../../src/app_settings.rs); the capsule checks in
[meeting_capsules.rs](../../src/rune_ui/meeting_capsules.rs)
(`always_record_saves_the_choice_and_records_this_call`,
`never_offer_saves_the_choice_and_dismisses`,
`a_browser_on_an_unknown_page_cannot_be_set_to_always_record`,
`an_offer_names_the_call_without_asking_about_consent`,
`an_automatic_recording_says_so_at_first`); and
`meetings_settings_save_and_ask_for_system_audio` (Linux pane tests). The
controller loop that turns `Detection::Record` into a recording has no
automated test; it shares the manual Record path.

Sources: [meeting_detection.rs](../../src/meeting_detection.rs),
[microphone_activity.rs](../../src/microphone_activity.rs),
[context.rs](../../src/context.rs), the controller in
[meeting_watcher.rs](../../src/meeting_watcher.rs) (which logs `INFO` when an
offer is triggered), and the offer capsule in
[meeting_capsules.rs](../../src/rune_ui/meeting_capsules.rs).
Checks: `resolves_unregistered_chromium_and_electron_audio_helpers_to_their_host_apps`,
`resolves_helper_bundle_ids_when_parent_lookup_is_unavailable`,
`attributes_safari_webkit_gpu_and_facetime_avconferenced_only_when_active`,
`canonical_bundle_matching_covers_supported_apps_and_variants`,
`classifies_helper_and_host_microphone_applications`,
`cleans_google_meet_titles_and_ignores_raw_meet_codes`,
`switching_tabs_during_a_snoozed_browser_call_does_not_re_offer`,
`parses_brave_tab_url_and_title_output`,
`requires_stability_and_offers_once_per_session`,
`a_brief_microphone_release_does_not_re_offer_the_same_call`, and
`named_meetings_keep_their_title_and_app_calls_use_the_app`.
Observed (2026-10-19): live CoreAudio process inspection on Studio and a headless
Brave WebRTC `getUserMedia({audio: true})` reproduction on Pro confirmed
`com.brave.Browser.helper` and `com.tinyspeck.slackmacgap.helper` had
`NSRunningApplication == nil` before parent/bundle resolution and resolved to
`com.brave.Browser` and `com.tinyspeck.slackmacgap` afterward.

The capsules have no `--snap` scene. `HEX_CAPSULE_SNAPS=<folder> cargo test
--bin voice-control capsule_snapshots` renders the offer, its choices, and the
automatic-recording states in light and dark on a Mac with a GPU.

### Record

```ts
Record (offer, menu bar "Record Meeting", Meetings pane, or voice command)
  -> Title: named meeting OR "<App> call — 2:30 PM" OR "Meeting — 2:30 PM"
  -> system.wav + microphone.wav, owner-only, Application Support/meetings/<id>
  -> No idle sleep; no dictation mute or media pause // meetings.record
  -> Menu bar: record symbol + elapsed time
     ├── "Recording · <title> · 12:34" -> Meetings pane
     │   └── title follows the provisional title (see Titles And Summary)
     └── Stop & Transcribe
     (Idle menu bar: "Record Meeting" + "Copy Last Meeting Transcript")
  -> Floating recording indicator (when Show recording indicator is on) // meetings.indicator
     ├── Rune interactive overlay: a non-activating panel that never becomes key
     ├── In the saved recording_indicator_corner
     ├── Red dot · title · mm:ss
     ├── Copy new (N) -> copies the in-call turns not yet copied or pasted
     │   ├── Shares Option-Control-V's cursor: either one advances both
     │   ├── N = speaker turns waiting; disabled at 0; "Copied" for 1.5 s
     │   └── Copies exactly the turns counted (the ticker's last read)
     ├── Copy all -> the in-call transcript so far as speaker-turn Markdown;
     │   disabled until there is text
     │   └── Both read the Parakeet in-call transcript (TranscriptReader) once a
     │       second on the ticker thread, never on the main thread
     ├── Open -> activates Hex, then opens the active meeting in the Meetings pane
     ├── Stop -> Stop & Transcribe
     └── Not built on Rune: dragging to a corner, hover expansion
  -> Settings window may stay closed

Detected app leaves the microphone for about 3 s     // meetings.call-ended
  -> "Zoom left the call" capsule: Keep recording / Stop & transcribe
  -> No timeout; nothing stops automatically

Capture sources (meeting_capture.rs)                // meetings.capture
├── Microphone -> selected input via cpal, resampled to 16 kHz
└── System audio -> Core Audio process tap (global, excluding Hex) on a private
    aggregate device; no display dependency
    └── Tap not authorized + Screen Recording granted -> ScreenCaptureKit fallback

A source fails (display sleep ends SCStream, device unplugged, tap stalls 3 s)
  -> Only that source stops; the other keeps recording
  -> Retried every 0.5-5 s until it reopens or the meeting stops
  -> Resumes into the same track; silence fills the gap (>= 1 s) on the
     shared capture clock, recorded in the manifest as track `gaps`
  -> Rolling transcription restarts that source after the gap; the padded WAV
     keeps the final pass on the same timeline
``````

Starting a recording no longer opens the Settings window; failures still do.
Checks: `call_end_is_reported_once_per_sustained_absence`,
`paste_cursor_counts_coalesced_turns_and_advances_only_on_success` in
[meeting_tests.rs](../../src/meeting_tests.rs),
`meeting_delta_coalesces_turns_in_time_order` in
[parakeet.rs](../../src/parakeet.rs),
`paste_cursor_neither_repeats_nor_drops_text_when_the_transcript_is_rewritten`, and
`meeting_menu_offers_one_action_per_state` in
[status_item.rs](../../src/status_item.rs), and
`mid_meeting_capture_loss_proceeds_to_transcription_when_audio_was_recorded`,
`writer_fills_a_capture_gap_with_silence_and_records_it`, and
`a_source_that_resumes_after_an_outage_is_padded_back_onto_the_timeline` in
[meeting_tests.rs](../../src/meeting_tests.rs). Menu-bar Record Meeting and
Stop & Transcribe, and the recording and ready capsule buttons, were clicked
on a real Mac on 2026-10-22; no automated test covers them.

The capsules are Rune overlays in [meeting_capsules.rs](../../src/rune_ui/meeting_capsules.rs):
interactive, non-activating `NSPanel`s at status level on every Space,
fed by the controller events the menu-bar pump receives
([macos_rune.rs](../../src/macos_rune.rs)). The offer (20 s), "left the
call", ready (15 s, Copy / Reveal, notes refresh) and a recording capsule
(title, elapsed time, Copy new, Copy all, Open, Stop, in the configured
corner) are ported. Not ported yet: hover expansion and dragging the
recording capsule. Copy new and Copy all are checked by
`the_recording_capsule_copies_the_in_call_transcript` and rendered by
`capsule_snapshots` (`recording-empty`, `-live`, `-copied`); they have not
been clicked on a real Mac. A "Never offer" or "Always record" choice is saved and
also written into the window's live settings. On 2026-10-22 a signed build on a MacBook Pro
showed the recording capsule (Open opened the meeting while it kept
recording; Stop transcribed) and the ready capsule (Copy copied the
Markdown). Before that day's fix the recording capsule never appeared: it
closed and reopened in one overlay batch, and the hide ran last. The offer,
"left the call", "…" choices, and automatic-recording capsules remain unseen
on screen.

### Transcribe

```ts
While recording                                      // meetings.rolling
  -> Each track -> one-second blocks -> bounded queue (about 10 minutes)
  -> 16 kHz streaming resampler -> chunker + echo envelope
     ├── First pause after 6 s (pause segmenter; never under 3 s) -> boundary
     ├── No pause by 10 s -> hard cut; next chunk repeats its last 2 s
     │   (so a chunk is at most 12 s of audio)
     └── Silent chunk (no 20 ms frame ≥ -60 dB) -> skipped, no inference
  -> Each chunk runs on dictation's loaded model, only while dictation is idle
     └── A dictation request or live stream came first -> retried once idle
  -> Timed words joined word by word at a shared word near each overlap's middle
     -> sentences (punctuation or a 600 ms pause)
  -> transcript.rolling.ndjson: per-track coverage line, then entries
     ├── Only settled sentences: wholly before the next seam, so a later chunk
     │   never rewrites or retracts a published line
     ├── You entries publish only once the system track is transcribed past them,
     │   or once system capture is over 5 s behind the microphone (stalled)
     ├── Capture gap -> the track restarts after the gap; nothing is dropped
     ├── Backlog over 120 s of audio -> oldest chunks skipped, track marked
     │   incomplete; rebuilt from its WAV after Stop
     └── Model failure -> transcript.rolling.error.txt -> "Live transcript unavailable."
  -> Every in-call view reads it: Meetings pane, `hex meetings show`, MCP,
     provisional titles, Option-Control-V (TranscriptReader)

Stop & Transcribe
  -> Flush tails -> finish pending chunks           // usually seconds
  -> Any track that fell behind or failed -> stream its WAV in 1 s blocks
  -> Join on one timeline -> Echo suppression -> Final publication
```

Design choice: chunks are background jobs on the dictation transcription engine
(`TranscriptionEngine::background`), using whatever model dictation has loaded,
rather than a second copy of the model (about 0.7 GB). Before each chunk the
meeting waits (up to 20 s per chunk) while `parakeet::dictation_busy()` reports
a capture, counted from the shortcut press until its clip is submitted, or
dictation inference; the engine thread checks again before running the job. A
job never interrupts a live stream: one that arrives during a stream, or after
a newer dictation request was sent, is turned away and retried 250 ms later,
once dictation is idle. A chunk already running cannot be aborted
(transcribe.cpp ignores its cancel callback on Metal), but it can only have
started before the press, so it finishes during the 300 ms hold threshold and
200 ms release tail (a chunk of at most 12 s takes well under 0.2 s on the
Pro). Gating at
the intentional hold instead let a chunk start just before the threshold; with
the shortest hold that added about 85 ms at p95 on the Pro. The job runs at utility QoS on
the engine thread, which returns to user-interactive afterwards. Apple Speech runs outside the process, so with
Apple Speech selected, and in a process without the engine (the CLI), the
meeting loads its own transcriber as before.

Seams: the two chunks are spliced at a word both heard within 300 ms of the
middle of the overlap (the same word's timing can shift between chunks);
without one, words starting before the middle belong to the earlier chunk.
Repeated words (two to twelve) at the join are removed, and Stop publishes the
words still waiting on a seam. Joining
words rather than sentences keeps a sentence that runs across a hard cut whole;
joining sentences lost its words on one side (10% word error on `You` lines).

In-call text was a Moonshine draft until 2026-10-22: the pane showed Parakeet
only for time rolling had covered, and Option-Control-V and the GPUI capsule's
Copy new pasted the Moonshine draft alone, which garbled names and jargon. The
meeting now loads no Moonshine; short Parakeet chunks replace it. Always-on
streaming Parakeet was rejected because one loaded model cannot serve it:
transcribe-cpp 0.2.4 allows one active stream per model across all sessions,
and refuses any other run or stream with `Busy` while it is open
(`Session::stream`). Two tracks would need a mixed track (losing You/Them and
echo suppression) or a model copy per track (about 0.7 GB each), and every
dictation would have to close the meeting stream and lose its 5.6 s left
context. The buffered stream also re-encodes a 7.7 s window (5.6 s left,
1.04 s chunk, 1.04 s right) per 1.04 s step, where chunks encode each second
about once: Studio's 1,619 dictation streams of 3 s or longer (3.2 h of audio,
2026-10-23 log) spent 44 ms of feed time per second of audio, which for two
tracks would hold the engine about 9% of the time.

Cost: observed 2026-10-22 on the Pro with `meeting_live_replay`: 10 minutes of
a recorded speaker-mode call fed at real time to the production recorder beside
the warm dictation engine (Parakeet unified English), with a 4 s dictation every
30 s. It feeds packets directly, not through capture. Word error is against the
final transcript the earlier 30 s chunks produced.

```ts
                            Moonshine draft +      Parakeet only,
                            30 s Parakeet chunks   3-12 s chunks
Meeting CPU                 105% of a core         4.4%
Footprint, idle ~830 MB     mean 1345, peak 1438   mean 854, peak 869 MB
Dictation release -> text   p50 80, p95 93 ms      p50 84, p95 89 ms
Speech end -> copyable      p50 16.5, p95 28.7 s   p50 6.3, p95 11.4 s
  (Parakeet; the draft showed in about 1 s)
Option-Control-V text       Moonshine, 17% WER     Parakeet, 5.6% mid-call
Pasted vs final, Them/You   16.7% / 14.9%          0.0% / 0.0%, no repeats
Final transcript            after flush            181 ms after Stop, 3.7% WER
```

Dictation latency stays within run-to-run spread (earlier Parakeet-only runs
measured p50 87-92 ms, p95 103-104 ms).
Commands still run their own Moonshine when that opt-in is on; the meeting
never loads it.

### Echo Suppression

```ts
Call on speakers -> the microphone hears the remote side tens of ms later
Each microphone segment                              // meetings.echo
├── Signal (meeting_echo): 20 ms energy of both tracks on one timeline
│   ├── Lag (-300..+600 ms) and coupling from envelope correlation over ±60 s,
│   │   cached per 10 s block once its audio is complete
│   │   └── Correlation < 0.2 (headphones) -> every sound is local
│   ├── Bleed gain = 35th percentile of mic − delayed system, before or after
│   └── Frame is local when mic > predicted bleed (decaying reverb) + 9 dB
│       ├── ≥ 50% local -> Local -> kept, even with matching words (double-talk)
│       ├── ≤ 20% local and no 0.5 s of local sound within 1 s -> Bleed -> dropped
│       └── otherwise   -> decided by words
└── Words: ≥ 60% match Them speech within ±1.5 s (60% edit similarity) -> echo

Rolling and final: segments are sentences, grouped into utterances
└── An echo utterance keeps its sentences of 3+ words that the signal shows local
Applied in: rolling snapshot (so every in-call view) and the final transcript
```

Dropped counts are saved as `echo_suppressed_utterances` in the manifest.

Root causes of `You` twins in Kit's 2026-10-16 call (laptop speakers, built-in
microphone, Pro under load):

- The live worker's bounded queue dropped 10 ms microphone packets under CPU
  load, and gaps under 20 ms of arrival time were not counted. The draft's
  microphone timeline drifted up to 6 s ahead of the system track over 18
  minutes, so the echo envelope compared unrelated audio and bleed read as
  `Local` (84 lines) or undecided (70). The echoed `You` line appeared before
  its `Them` twin. Packets now carry their WAV sample position.
- The rolling snapshot had no coverage, so the draft took over after the last
  kept `You` entry (616 s) even though rolling had judged that audio as echo.
- A rolling `You` entry could be published before the system chunk that
  explained it.

Observed with an opt-in replay of that recording through every layer
(`meeting_echo_replay`, Mac Studio), before the Moonshine draft was removed
(the draft rows no longer apply). Echo-like = at least 60% of the words occur
in remote speech within 6 s. Six utterances where Kit really spoke were
labeled by hand.

```ts
Pane as recorded (old code)          77 You entries, 49 echo-like
Draft only (rolling unavailable)     11 You, 2 echo-like, 2 of 6 local lost*
Pane, rolling caught up              10 You, 1 echo-like, 0 of 6 lost
Pane while recording, rolling 25-300 s behind: 25 ever shown, 3 echo-like
Final transcript                     10 You, 1 echo-like, 0 of 6 lost
* Moonshine lines that mix bleed with Kit's sentence stay hidden until
  rolling, which splits them into sentences, covers that time.
```

Acoustic echo cancellation (Voice Processing IO) was evaluated, not adopted:
it replaces ScreenCaptureKit's raw microphone with an AVAudioEngine/VPIO input,
ducks other audio, moves Bluetooth headsets to the call profile, adds gain and
noise processing to the transcribed track, runs beside the call app's own voice
processor, and loses ScreenCaptureKit's shared clock. See ROADMAP.md.

Sources: [meeting_transcript.rs](../../src/meeting_transcript.rs),
[meeting_echo.rs](../../src/meeting_echo.rs),
`StreamingResampler` in [dictation.rs](../../src/dictation.rs). Echo checks:
`speaker_bleed_is_classified_as_bleed`, `talking_over_the_remote_side_is_local`,
`speech_during_remote_silence_is_local`, `headphones_make_every_microphone_sound_local`,
`late_muffled_bleed_is_bleed_and_speech_over_it_is_local`,
`a_short_phrase_inside_a_long_bleed_segment_is_not_bleed`,
`the_signal_overrides_word_matching_both_ways`,
`synthetic_speaker_bleed_is_dropped_and_double_talk_kept`,
`a_local_sentence_inside_an_echo_utterance_is_kept`,
`microphone_entries_wait_until_the_system_track_covers_them`, and
`timed_words_become_sentences_at_punctuation_and_pauses` (synthetic signals).
Other checks:
`continuous_speech_is_hard_cut_with_overlap_and_covers_every_sample`,
`chunks_end_at_the_first_pause_after_the_target_length`,
`short_recordings_become_one_chunk_and_silence_tails_are_dropped`,
`overlapping_chunks_join_at_the_middle_of_the_overlap`, `seam_drops_repeated_words`,
`a_sentence_across_a_hard_cut_keeps_every_word`,
`rolling_publishes_only_entries_a_later_seam_cannot_replace`,
`a_capture_gap_restarts_the_track_after_it_instead_of_dropping_it`,
`a_backlog_skips_the_oldest_audio_and_marks_the_track_for_rebuilding`,
`silent_chunks_are_not_transcribed`,
`speaker_echo_of_remote_speech_is_dropped`, `word_level_echo_is_dropped_as_one_utterance`,
`distinct_or_distant_local_speech_is_kept`,
`discontinuous_packets_are_placed_on_the_timeline`, and
`streaming_resampler_matches_whole_buffer_conversion`. They use synthetic audio
and transcripts, not model inference or real speaker echo.

Opt-in measurements (ignored tests; they need a private recording and are never
part of CI): `meeting_echo_replay` (every layer, cached transcripts, hand
labels), `moonshine_command_update_latency`, and `dictation_latency` in
[meeting_echo_replay_tests.rs](../../src/meeting_echo_replay_tests.rs), and
`meeting_live_replay` in
[meeting_live_replay_tests.rs](../../src/meeting_live_replay_tests.rs): the
production recorder fed a recording at real time beside the warm dictation
engine, with a dictation every 30 s, reporting CPU, footprint, dictation
latency, speech-to-copyable lag, mid-call Copy all and Option-Control-V text
against the final transcript.

### Save And Reuse

```ts
Final publication                                   // meetings.output
├── Internal: transcript.ndjson + transcript.md (atomic, owner-only)
└── Markdown copy -> <folder>/<YYYY-MM-DD HHMM> <title>.md
    ├── Title, date, duration, app, You / Them turns
    ├── Never overwrites; adds " (2)"
    └── Failure -> Meeting still Complete; "Markdown not saved: …"

Transcript ready capsule (15 s) -> Copy (Markdown or speaker turns fallback) / Reveal
Meetings pane -> Row Copy (completed rows), detail Copy transcript, Reveal Markdown (or Reveal Files when none)
Menu bar (idle) -> Copy Last Meeting Transcript
Option-Control-V -> `MeetingPasteCursor` in `meeting.rs` over the rolling transcript
├── Pastes settled turns added since the last successful paste
├── Remembers, per source, the capture time spans it pasted (spans within 1.5 s
│   merge); an entry is new when its midpoint is outside them, so nothing
│   repeats or drops when the final transcript replaces rolling after Stop, and
│   text the final pass recovers for audio rolling skipped still pastes
└── Resets when Hex restarts
```

### Meetings Pane

```ts
Header: Search meetings (title, app, notes, final transcript) + Start recording
        (the pane's one Primary button) / Stop & transcribe (Secondary, red dot,
        elapsed time)
List (grouped Today / Yesterday / date)
└── Row: title (generated when present) · time · length · app · status chip · Copy (completed rows, fixed-width inline "Copied")
    └── Recording (red dot) / Transcribing / Interrupted / Failed; Ready in detail
Detail
├── Title, status chip, date · time · duration · app
├── Actions: Copy transcript, Rename (finished only), "…" overflow menu
│   └── Copy summary (stays open to show "Copied"), Reveal Markdown, Summarize /
│       Regenerate summary ("Summarizing…" disabled while running), separator,
│       Delete… (finished meetings only for the last three)
├── Errors at the top of the scroll area
├── Summary: one paragraph, at most two sentences
└── Transcript: Them left, You right in accent; merged turns; timestamps;
    30 s+ pause dividers; live view follows the tail; "Listening..." while recording

Rename -> inline field; Enter or Save          // meetings.rename
  -> manifest title; Markdown heading only while it reads the old title
  -> file name unchanged; empty title refused
Delete… (overflow menu) -> "Trash recording & Markdown" (or "Trash recording")
  / Cancel, which replace the action row until answered
  -> exported Markdown, then Application Support/meetings/<id>, to the Trash
  -> refused while recording, transcribing, or writing notes   // meetings.delete
```

Choice: Delete trashes the Markdown copy too, since it holds the transcript;
both stay recoverable from the Trash. Sources:
[rune_ui/meetings.rs](../../src/rune_ui/meetings.rs), the requests in
[rune_ui/macos_host.rs](../../src/rune_ui/macos_host.rs), and `trash` /
`rename` in [meeting.rs](../../src/meeting.rs). Checks:
`deleting_trashes_the_markdown_then_the_recording_and_refuses_while_recording`,
`renaming_updates_the_manifest_and_an_unedited_markdown_heading`,
`retitling_replaces_only_a_matching_first_heading`, and the Rune pane tests
`meetings_select_search_and_copy_a_transcript`,
`meetings_rename_from_the_title_field`,
`deleting_a_meeting_takes_the_menu_then_a_confirmation`, and
`the_overflow_menu_copies_the_summary_and_asks_for_new_notes`.

Scenes: `meetings`, `meetings-recording`, `meetings-actions`,
`meetings-delete`, `meetings-rename`, `meetings-failed`, `meetings-empty`.

Checks: `markdown_lists_metadata_and_labeled_turns` and
`export_never_overwrites_and_uses_safe_names`.

### Titles And Summary (Apple Intelligence)

```ts
While recording, setting on, model available           // meetings.title.provisional
  -> meeting-title thread, utility QoS; checks the transcript every 10 s
     (rolling chunks, echo-filtered)
  -> ≥ 90 s of speech (or ≥ 600 words; ≥ 60 words either way)
     -> waits (≤30 s) while dictation is busy
     -> ONE title-only call over the transcript so far (≤5,000 chars, 30 s timeout)
  -> Recorder stores provisional_title in the manifest
     -> menu bar "Recording · <title>", Meetings pane list and detail
  -> Refreshed once when speech reaches 10 minutes; a failure retries a minute
     later; at most 3 calls per meeting; Stop discards a call in flight
  -> The exported Markdown file name and heading use it when it arrived first

Final publication complete                               // meetings.notes
  -> meeting-notes thread, utility QoS; waits (≤30 s) while dictation is busy
  -> ONE call over a bounded excerpt, no map-reduce
     ├── Lines "[mm:ss] You|Them: text" (long turns split at 600 chars)
     ├── Whole transcript when it fits; otherwise opening 30 %, up to six
     │   evenly spaced middle passages, ending 20 %, "[…]" between them
     ├── Budget = min(12,000 chars, (context − 1,000 tokens) × 3 chars)
     └── Context exceeded -> retried once with half the excerpt
  -> 90 s total deadline; the call ≤60 s
  -> Manifest: notes { title ≤8 words, summary ≤2 sentences / 400 chars,
     excerpt_chars, transcript_chars }
  -> Markdown copy, same file name, atomic replace
     ├── Heading -> final title, only while it still reads HEX's last title
     ├── <!-- hex:notes --> ## Summary (one paragraph) above ## Transcript
     ├── Text outside the block is preserved; changed mid-write -> not written
     └── Missing file -> manifest only
  -> Failure / timeout -> notes_error; Markdown untouched; recording stays Complete

Unavailable (not eligible / Apple Intelligence off / model not ready / macOS < 26)
  -> Settings row says why; nothing runs
Meetings pane -> title in list and detail; Summary paragraph above the transcript;
  "Summarize" / "Regenerate summary" / "Summarizing…" in the overflow menu
Transcript ready capsule -> "Ready · <title>" once a provisional or final title
  exists; subtitle "Summarizing with Apple Intelligence…", then the summary
```

Title rule: the shown title is the user's title unless it is still the default
`<App> call — 2:30 PM` / `Meeting — 2:30 PM`; then the final notes title, else
the provisional title, else the default. Named or renamed meetings keep their
name. The file is never renamed, so links, open editors, and Reveal keep working.

Action items and decisions are gone: other agents read the transcript for
details (MCP). Older manifests still decode: a list summary is joined into one
paragraph, and action items, decisions, and chunk counts are ignored (and
dropped when that manifest is next written). Their Markdown copies keep their
old notes block until regenerated.

Sources: [meeting_notes.rs](../../src/meeting_notes.rs),
[apple_intelligence.rs](../../src/apple_intelligence.rs), and the
FoundationModels bridge [apple_intelligence.swift](../../src/apple_intelligence.swift)
(`GeneratedNotes` title + summary, `GeneratedTitle` title only), weak-linked from
[build.rs](../../build.rs); the recorder loop in `record` in
[meeting.rs](../../src/meeting.rs). Checks in `meeting::notes::tests` (fake
model): `a_short_transcript_is_sent_whole_in_one_call`,
`long_meetings_are_excerpted_from_the_opening_middle_and_ending`,
`context_overflow_halves_the_excerpt_once_and_failures_stop`,
`titles_are_requested_after_the_opening_then_once_at_ten_minutes`,
`provisional_titles_use_the_title_prompt_over_the_transcript_so_far`,
`generated_and_provisional_titles_replace_only_default_titles`,
`older_notes_with_points_action_items_and_decisions_still_read`,
`notes_are_inserted_above_the_transcript_and_regenerate_idempotently`, and
`user_edits_outside_the_notes_block_are_preserved`; menu text in
`meeting_menu_offers_one_action_per_state`. The recorder thread, manifest write,
and menu/pane refresh during a real recording are unexercised.

Real model (debug CLI): `hex meeting summarize <file.md> [--excerpt-chars N]`
times the provisional title at the point a recording would ask for it, then the
final title and summary, without saving; `hex meeting summarize <id>` writes
them like Regenerate.

Observed (2026-10-15, Mac Studio M2 Max, macOS 27.0.1, context 8,192 tokens,
debug build) on a synthetic 50-minute, 7,700-word transcript (206 turns,
46,000 chars):

```ts
Provisional title at 01:41 (261 words, 101 s of speech)  0.7 s (1.0 s first call)
Final title + summary, one call, 11,151-char excerpt     3.4–3.5 s (3 runs)
  └── budget 9,000 -> 2.6–3.4 s; 18,000 -> 5.7 s
Before: map-reduce over 8 chunks, 9 calls, 51-minute call  96 s
```

### Live Notes (Apple Intelligence, prototype)

```ts
While recording, setting on, model available            // meetings.notes.live
  -> meeting-live-notes thread, utility QoS; checks the transcript every 5 s
  -> First pass at ≥ 45 s of meeting time, then every ≥ 75 s, each needing
     ≥ 40 new words; waits (≤30 s) while dictation is busy
  -> ONE call over only the transcript after the last pass
     ├── "[mm:ss] You|Them: text", newest ≤ 3,600 chars (~1,200 tokens);
     │   older backlog skipped as "[…]"
     ├── No current topic in the prompt (the model just repeats it)
     └── 20 s timeout, ≤ 240 response tokens, temperature 0.2
  -> Model returns a topic for this stretch and ≤ 3 notes
     (decision · action with owner · question · point, each with its [mm:ss])
  -> Topic rule (Rust, not the model)
     ├── Same topic: ≥ half its words were heard in the current section
     ├── Different on one pass: remembered as pending
     └── Different on the next pass too, and the current topic had 90 s
         -> new section named and dated by the first pass away; notes made
            since move with it (≤ 40 sections)
  -> Notes: ≤ 3 per section, in meeting order; a decision, action, or question
     replaces the oldest plain point; repeats of the last two sections dropped
  -> Each note stores its transcript lines (the line before when ≤ 10 s)
  -> live-notes.json in the meeting folder, owner-only, atomic replace
  -> Refused / guardrail-blocked passage -> counted as read, nothing noted
  -> Other failure -> last good notes kept; retried after 1, 2 … 5 min
Unavailable or setting off -> no file, no new UI, nothing logged
Recording capsule (Rune overlay)
  ├── Title = current topic, crossfading when it changes; subtitle
  │   "mm:ss · <meeting title>"
  └── Chevron (only when notes can run or exist) -> overlay grows into a
      scrolling notes panel under the pill (above it in a bottom corner);
      current topic marked "Now"; a note opens its transcript lines
Meetings pane -> "Notes" above the transcript, live or finished, same rows
After Stop -> the final notes call reads "Topics noted during the meeting:"
  (≤ 600 chars) before its excerpt; the Markdown notes block adds a
  "### Notes" outline below the summary
```

Why feed and not replace: the final call reads an excerpt of a long meeting
(opening, six middle passages, ending), so the live outline is the only part
that saw every stretch. The final pass stays one call and keeps its budget.

Sources: [meeting_live_notes.rs](../../src/meeting_live_notes.rs), the
`GeneratedLiveNotes` bridge in
[apple_intelligence.swift](../../src/apple_intelligence.swift),
[rune_ui/meeting_outline.rs](../../src/rune_ui/meeting_outline.rs),
[rune_ui/meeting_capsules.rs](../../src/rune_ui/meeting_capsules.rs), and the
Meetings pane. Checks in `meeting::live_notes::tests` (fake model):
`passes_wait_for_the_opening_then_new_time_and_words_and_back_off`,
`a_pass_reads_only_new_transcript_and_keeps_the_newest_within_budget`,
`topics_change_after_two_passes_away_and_their_dwell`,
`full_sections_keep_decisions_actions_and_questions_over_points`,
`notes_anchor_to_their_line_carry_an_excerpt_and_skip_repeats`,
`outlines_are_bounded`, `notes_files_are_owner_only_and_survive_a_bad_write`,
and `the_outline_feeds_the_final_prompt_and_markdown`; pane in
`live_notes_mark_the_current_topic_and_open_their_transcript_lines` and the
`meetings-notes-open` golden; capsule in
`the_recording_pill_names_the_topic_and_opens_its_notes`, with
`capsule_snapshots` (`HEX_CAPSULE_SNAPS=<dir>`) rendering the pill and notes
panel headless.

Debug CLI: `hex meeting live-notes <transcript> [--minutes N] [--canned]
[--record raw.ndjson | --recorded raw.ndjson] [--snapshots out.ndjson]` runs
the passes a recording would and reports latency and prompt size; prompts don't
depend on earlier passes, so `--recorded` replays a real run to tune the rules.
`hex meeting capsule-lab <snapshots> -- --scene replay|pill|notes|notes-excerpt
--snap …|--frames …` renders the capsule headless over those snapshots.
`hex meeting replay <transcript> --speed 30 [--canned]` (with an empty
`HEX_APPLICATION_SUPPORT_DIR`) writes a fake recording and opens the real
capsule and window over it. Synthetic fixture:
[live-notes-replay.ndjson](../../tests/fixtures/live-notes-replay.ndjson) from
`scripts/fixtures/make-live-notes-replay.py`.

Observed (2026-10-22, the Pro, macOS 27.0.1, debug build, utility QoS):

```ts
Private 114-minute design call    75 passes · p50 4.0 s (2.9–6.5 s)
                                   prompt mean 1,370 chars (~340 tokens)
                                   10 refused/guardrail, 1 timeout
                                   34 sections · 43 KB notes file
Synthetic 9-minute, 4-topic call  7 passes · p50 3.2 s (3.0–4.3 s) · 3 sections
Before the topic rule (model's own "shift" flag): one topic for 114 minutes
```

### Agent Access (MCP)

The release CLI also supports `hex meetings list`, `show <id|latest>`, and
`search <query>` through this query layer. See the [transcript CLI map](transcript-cli.md)
and [versioned JSON contract](../transcript-cli.md).

```ts
hex mcp [--meetings-dir <path>]                     // meetings.mcp; release builds
├── MCP over stdio; independent of the Meetings opt-in (read-only)
├── Dispatches before process logging, the instance lock, GUI, and permissions
│   └── Safe beside the running app; logs only to stderr (HEX_MCP_LOG)
├── Default folder: Application Support/hex/meetings (voice-control on older builds)
│   └── --meetings-dir -> any mirrored copy (rsync of that folder)
├── A copy named hex-meetings-mcp -> always `hex mcp` with the same flags
│   (no subcommand never falls back to listening)
└── Tools (all readOnlyHint)
    ├── list_meetings(since?, until?, query?, limit?, include_in_progress?)
    │   └── id · title (generated when present) · ISO start · mm:ss · status ·
    │       transcript kind · participants "You, Them (<app>)" · notes yes/no
    ├── search_transcripts(query, since?, until?, meeting_id?, limit?, fuzzy?)
    │   └── per-meeting counts + ranked passages: id, title, speaker,
    │       mm:ss @ ISO time, snippet with **matches**, before/after lines
    ├── get_transcript(meeting_id, start?, end?, speaker?, max_chars?, cursor?)
    │   └── merged You/Them turns "[mm:ss · HH:MM:SS] Them: …" + next_cursor
    └── get_meeting_notes(meeting_id)
        └── stored title + two-sentence summary + "read the transcript for
            details, decisions, and action items", or an explicit
            "No Apple Intelligence notes are stored" + get_transcript hint
Resources: hex-meeting://<id> -> Markdown (metadata, notes, transcript)
```

Read-only and private: the server opens files for reading only, never writes,
repairs, or recovers anything in the meetings folder (unlike `meeting::list`,
which persists recovered statuses), creates no index or cache on disk, runs no
model, and makes no network request. It never reads the exported Markdown in
`~/Documents`, so SSH sessions need no Documents access. Transcripts leave the
Mac only through whatever transport the MCP client uses (stdio, or SSH).

Concurrency: a recorder holds `active.lock`; the server probes it with a
read-only, non-blocking shared `flock` and releases it immediately. Held ->
`recording (in progress)` with the rolling transcript (echo-filtered), labeled
provisional. Not held ->
the app's recovery rules apply without writing: published `transcript.ndjson`
is final, an unfinished draft is `interrupted`. Stale lock files from finished
recordings read as not held. A mirror cannot see the Pro's lock, so a meeting
still recording there appears `interrupted` with its rolling transcript so far. Unreadable
manifests are listed as skipped folders; unreadable NDJSON lines are skipped and
counted; an unterminated last line is treated as a write in progress.

Search: in-memory, rebuilt per meeting when its files' sizes or times change.
Passages are same-speaker runs of up to 60 words (3 s turn gap) plus one per
stored notes block. Text is NFKD-folded (case and diacritics), tokenized, and
lightly stemmed (`scheduled`/`scheduling` -> `schedul`). BM25 (k1 1.2, b 0.75)
over expanded terms: exact 1.0, prefix 0.8, edit distance 1 (2 from 8 letters)
0.7, Soundex 0.45 for 3-5 letter words (`Dacks` -> `Dax`); partial-term
coverage and adjacent-term bonuses. `"Quoted words"` are required exact
(stemmed) phrases; `fuzzy: false` keeps only exact stems.

Semantic search is not shipped. On-device candidates are Apple NaturalLanguage
`NLContextualEmbedding` (asset download, macOS 14+) or the older static
`NLEmbedding` sentence model, through a Swift bridge like `apple_speech`, with a
cache under `~/Library/Caches/voice-control/meeting-index` keyed by meeting
fingerprints. Full-text with stemming and fuzzy matching answered the real
queries below; embeddings wait until a query fails for lack of paraphrase.

Sources: [meeting_query.rs](../../src/meeting_query.rs) (`meeting::query`,
queries and rendering) and [meeting_mcp.rs](../../src/meeting_mcp.rs) (`rmcp`
3.5 adapter); `run_mcp` in [main.rs](../../src/main.rs). Checks (synthetic
fixtures in `tests/fixtures/meetings`): `meeting::query::tests::*` (listing and
skipped folders, filters, misspelling tolerance and ranking, stemming, accents,
required phrases, context, drafts, cursor paging, notes, held lock,
`queries_never_write_to_the_meetings_folder`, time parsing) and
`tests/meetings_mcp.rs` (initialize with protocol 2025-06-18, tools/list with
schemas and read-only annotations, tools/call, resources/read, stdout carries
only JSON-RPC, no Application Support state created).

Observed (2026-10-15, Mac Studio, release build, read-only copy of the Pro's 8
meetings without WAVs): first launch of a freshly signed binary 440 ms to
`initialize` (Gatekeeper), then 8 ms; `list_meetings` 4-7 ms cold; searches
0.5-1.2 ms; a 51-minute `get_transcript` 0.5 ms; `resources/read` 0.6 ms.

#### Another Mac's Meetings (Mirror)

```ts
Don't: MCP client config runs `ssh pro hex mcp`         // one SSH per client
  -> 41 OpenCode instances = 41 SSH sessions -> the Pro's sshd reset connections

Do: one periodic pull, then local servers               // meetings.mcp.mirror
scripts/sync-meetings-mirror.sh (LaunchAgent template, every 120 s,
  ProcessType Background, Nice 10, LowPriorityIO)
├── ONE rsync over ssh (BatchMode, ConnectTimeout 5, --timeout 60)
│   pro:Library/Application Support/voice-control/meetings/ (symlink to hex/ once migrated)
│   -> ~/Library/Caches/hex-meetings-mirror/ (0700)
├── Skips *.wav, *.lock, *.tmp; --delay-updates (files land together at the
│   end); --delete-delay (meetings trashed on the Pro disappear)
└── Pro asleep / unreachable -> exit 0 silently; next run retries
Each MCP client -> local `hex mcp --meetings-dir <mirror>` (no SSH, no lock)
└── No lock in the mirror -> a meeting still recording on the Pro reads as
    `interrupted` with its rolling transcript so far; final once published
```

Template: [hex-meetings-mirror.plist](../../scripts/launchd/hex-meetings-mirror.plist)
(placeholders for the checkout and home; not installed automatically).

Observed (2026-10-15, Studio pulling from the Pro): one sync 0.44 s, 8 meetings,
1.0 MB, no WAVs or locks; unreachable host exits 0 at once, unanswered address
exits 0 after the 5 s connect timeout. `hex-meetings-mcp --meetings-dir` (release
build in `~/.local/bin`) over stdio: first launch of the new binary 378 ms to
`initialize` (Gatekeeper), then 7 ms; `list_meetings` 3.5–11 ms cold, 0.5 ms
warm; searches 0.6–1.5 ms; a 48,000-char `get_transcript` 0.6 ms;
`get_meeting_notes` ≤0.3 ms, including an older manifest with action items;
`resources/read` 0.5 ms. 40 parallel instances: all initialized (median 13 ms,
max 22 ms), listed, and exited 0 in 76 ms wall. The LaunchAgent is not loaded,
so ssh-agent access from launchd is unverified.

Granola import: see [granola-import.md](../plans/granola-import.md).

## Gaps

- No executed end-to-end recording with a real call, Screen Recording grant, or
  speakers echo in this change; see the test script in the handoff.
- The process tap, its System Audio Recording prompt, and display-sleep
  recovery have not run in a recorded meeting. TCC status comes from the
  private `TCCAccessPreflight`; a denied tap would otherwise record silence.
- Microphone and system audio now run on separate device clocks aligned by
  arrival time; drift below the 1 s gap threshold is uncorrected and echo
  thresholds were tuned on ScreenCaptureKit's shared clock.
- Calendar titles (EventKit) are not implemented.
- In-call text trails speech by one chunk plus its settling (seconds, see the
  replay numbers); nothing shows a partial sentence before then. A track that
  falls more than 120 s behind skips audio until Stop rebuilds it.
- The recording capsule's Copy new / Copy all are unverified on a real Mac
  screen; its count can trail the transcript by up to a second.
- The Meetings pane is verified by Rune goldens rendered offscreen (on Linux
  and on hailmary's Metal), not on a Mac screen; the capsules have no golden.
- Echo thresholds are tuned on two speaker-mode calls (2026-10-15 and
  2026-10-16), with six hand-labeled local utterances; other rooms, external
  speakers, and Bluetooth are unmeasured. The 2026-10-16 replay audio was lost
  with Studio's temporary directory, so those numbers cannot be re-run. Real
  NSFileManager trashing and the native rename field are unexercised in the app.
- CPU, memory, and latency numbers come from the replay harness on the Pro,
  not a recording in the app with real capture and hotkey dictations.
- The short-chunk rolling transcript has not run in a real call yet.
- Why the rolling worker stalled for over a minute on the Pro is unknown; slow
  chunks (> 5 s) are now logged.
- Titles and summaries have not run after a real recording in the app; the CLI
  path exercised the real model, and the provisional-title thread, its manifest
  write, and the menu/pane refresh are unexercised in a live recording.
- Live notes ran against the real model only through the CLI over saved
  transcripts. The worker thread in a real recording, the native capsule
  panel's resize, scrolling, and click handling as a non-activating panel,
  and `hex meeting replay` on a screen are unexercised; capsule stills and the
  clip come from the headless lab. The model labels many casual remarks
  "Decision", and a two-minute topic can fold into its neighbour.
