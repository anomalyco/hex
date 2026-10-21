# Meetings (beta)

Record a call locally: HEX captures the other participants (system audio) and
you (microphone) as separate tracks, transcribes them on this Mac, and saves a
Markdown transcript with `You` and `Them` turns. Recording never starts or stops
automatically. This map covers macOS; Linux has no meetings.

Linked checks are locators unless an executed result is recorded below.

## How To Get To It

```ts
Settings > Meetings > Meetings (beta)               // meetings.opt-in; off by default
├── On -> Meetings pane, menu-bar items, call detection, Option-Control-V
│   ├── Screen & System Audio Recording missing -> "Open settings" row
│   ├── Show recording indicator -> on by default; corner-snapping floating capsule
│   ├── Transcript folder -> ~/Documents/Hex Meetings by default; Reveal / choose
│   └── Summarize with Apple Intelligence -> on by default; one-line availability
└── Off -> Controller idles; no microphone-activity inspection, pane, or shortcut

Debug builds -> Always available (DEVELOPER_FEATURES_ENABLED)
Agents -> hex mcp (read-only MCP server; works whether or not Meetings is on)
```

Requires the selected local dictation model, Microphone, and Screen & System
Audio Recording. The opt-in is `meetings.enabled` in `settings.json`, alongside
`meetings.show_recording_indicator` (default `true`) and
`meetings.recording_indicator_corner` (default `top_right`)
([app_settings.rs](../../src/app_settings.rs) `MeetingSettings`). Recording
refuses to start without Screen Recording and asks macOS to list HEX.

## Sub-features

### Detect And Offer

```ts
Zoom / Teams / Slack huddle / FaceTime / browser call uses the microphone
  -> CoreAudio input process -> canonical host app   // parent PID + bundle ID
  -> 4 stable observations (about 2 s)              // meetings.detect
  -> Capsule below the menu bar, focus-free          // meetings.offer
     ├── Record -> Start recording
     ├── Not now -> Close
     └── 20 s timeout -> Close                       // never records on timeout
  -> Same call is snoozed by key and app until off the microphone for about 60 s
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

Sources: [meeting_detection.rs](../../src/meeting_detection.rs),
[microphone_activity.rs](../../src/microphone_activity.rs),
[context.rs](../../src/context.rs), and the `MeetingPanel` capsule in
[meeting_watcher.rs](../../src/meeting_watcher.rs) (which logs `INFO` when an
offer is triggered, shown, dismissed, or times out).
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

Preview: `scripts/capture-preview.sh OUT meeting-offer [--meeting-panel recording|recording-expanded|recording-copied|call-ended|ready]` (alias `meeting-capsule`).

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
     ├── Focus-free GPUI capsule; click-through outside the visible pill
     ├── Drag anywhere -> snaps to nearest corner and saves recording_indicator_corner
     ├── Compact pill: pulsing red dot (static under Reduce Motion) · tabular mm:ss · title
     └── Hover expands actions (fixed-width inline "Copied" feedback):
         ├── Copy new (N) -> copies turns since last Copy new or Option-Control-V;
         │                   disabled when 0; advances shared cursor only on success
         ├── Copy all -> echo-filtered live transcript as You/Them Markdown turns
         ├── Open -> opens the active meeting in the Meetings pane
         └── Stop -> Stop & Transcribe
  -> Settings window may stay closed

Detected app leaves the microphone for about 3 s     // meetings.call-ended
  -> "Zoom left the call" capsule: Keep recording / Stop & transcribe
  -> No timeout; nothing stops automatically

SCStream loses its display mid-recording (monitor unplug / display sleep)
  -> Audio delivered before error -> restart SCStream on current display once
     ├── Display available -> keep recording into the same tracks
     └── No display (e.g. display sleep) -> stop capture and transcribe recorded audio
```

Starting a recording no longer opens the Settings window; failures still do.
Checks: `call_end_is_reported_once_per_sustained_absence`,
`paste_cursor_counts_coalesced_turns_and_advances_only_on_success` in
[meeting_tests.rs](../../src/meeting_tests.rs),
`meeting_delta_tracks_each_source_and_coalesces_turns` in
[parakeet.rs](../../src/parakeet.rs), and
`meeting_menu_offers_one_action_per_state` in
[status_item.rs](../../src/status_item.rs), and
`mid_meeting_capture_loss_proceeds_to_transcription_when_audio_was_recorded` in
[meeting_tests.rs](../../src/meeting_tests.rs). Native menu-bar and capsule clicks
are unverified.

### Transcribe

```ts
While recording                                      // meetings.live
  -> Selected language is English or Auto
     -> Live draft: Moonshine, one stream per track, update every 1 s
        ├── Packets carry their sample position in the WAV -> exact timeline
        │   └── Dropped packets -> up to 0.5 s of silence, the rest skipped in time
        └── Completed You line -> written once the system track is 1 s past it
            └── System track stalled -> written after 3 s more microphone audio
  -> Selected language is a specific other language, such as Spanish
     -> Live draft: the selected model, on audio the chunker has not committed
        ├── First text after about 2 s of that tail
        ├── Next text after about 3 s of new audio, or after as much audio as
        │   the previous pass took when that was longer
        └── A pass slower than its audio, or slower than 4 s, stops further
            drafts on that track
     -> The pane shows that draft on the same path as the English lines
     -> A rolling chunk replaces the draft for the audio it covers
     -> Option-Control-V pastes the final transcript of those committed chunks

While recording                                      // meetings.rolling
  -> Each track -> one-second blocks -> bounded queue (about 5 minutes)
  -> 16 kHz streaming resampler -> chunker + echo envelope
     ├── First pause after 20 s (pause segmenter) -> chunk boundary
     └── No pause by 30 s -> hard cut; next chunk overlaps 2 s
  -> Utility-QoS worker transcribes a chunk only while dictation is idle
  -> Timed words -> sentences (punctuation or a 600 ms pause)
  -> transcript.rolling.ndjson: per-track coverage line, then entries
     ├── You entries publish only once the system track is transcribed past them
     ├── Covered time replaces the draft, including You audio dropped as echo
     └── Queue full -> that track keeps its entries; the draft continues after
         them; rebuilt from its WAV after Stop

Stop & Transcribe
  -> Flush tails -> finish pending chunks           // usually seconds
  -> Any track that fell behind or failed -> stream its WAV in 1 s blocks
  -> Join on one timeline -> Echo suppression -> Final publication
```

Design choice: a separate session of the selected model at utility QoS, loaded
lazily on the first completed chunk, rather than a job class inside the dictation
inference worker. Before each chunk it waits (up to 20 s per chunk) while
`parakeet::dictation_busy()` reports an intentional capture or dictation
inference. A chunk already running can still delay a dictation job by roughly
one chunk's inference time, and the second model stays resident while recording.
For any other selected language that same session drafts the uncommitted tail
while no committed chunk is waiting and dictation is idle. It does not wait out
the 20 s dictation pause for a draft. English and Auto keep the Moonshine draft
and do not run this pass. Checks:
`draft_waits_until_two_seconds_then_every_three_seconds_of_new_audio` and
`a_slow_pass_stops_or_waits_for_enough_new_audio` in
[meeting_provisional.rs](../../src/meeting_provisional.rs). The tail check
`uncommitted_tail_is_audio_the_chunker_has_not_cut` compiles on macOS only.

Seams: segments starting before the middle of an overlap belong to the earlier
chunk, and repeated words (two to twelve) at the join are removed.

CPU: every Moonshine session (commands and the meeting draft) runs on its calling
thread (`MOONSHINE_ORT_SINGLE_THREAD=1`, set at launch unless already set).
ONNX Runtime's default per-session spinning thread pools were the main cost of
meeting mode. Mac Studio, replayed in real time:

```ts
Meeting draft, two streams     113% of a core -> 31%
Command recognizer             115% of a core -> 22%
  └── update p50 21 -> 41 ms, p95 67 -> 127 ms (200 ms update interval)
Rolling Parakeet               well under 1 s of compute per minute of audio
```

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
Live draft (meeting_live::live_view): each Moonshine line on its own
├── Signal stamped by the live worker -> Bleed hidden, Local shown
├── Undecided, still in progress, overlapping remote speech -> hidden for now
└── Undecided and completed -> hidden when its words repeat remote speech
Selected-model tail draft: the same signal and word check, then the lines are
written after the committed rolling entries. Those drops are not counted in
echo_suppressed_utterances.
Rolling microphone entries are already judged; not re-judged by words
Applied in: rolling snapshot, final transcript, live view, Option-Control-V
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
(`meeting_echo_replay`, Mac Studio). Echo-like = at least 60% of the words occur
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
[meeting_live.rs](../../src/meeting_live.rs),
`StreamingResampler` in [dictation.rs](../../src/dictation.rs). Echo checks:
`speaker_bleed_is_classified_as_bleed`, `talking_over_the_remote_side_is_local`,
`speech_during_remote_silence_is_local`, `headphones_make_every_microphone_sound_local`,
`late_muffled_bleed_is_bleed_and_speech_over_it_is_local`,
`a_short_phrase_inside_a_long_bleed_segment_is_not_bleed`,
`the_signal_overrides_word_matching_both_ways`,
`synthetic_speaker_bleed_is_dropped_and_double_talk_kept`,
`a_local_sentence_inside_an_echo_utterance_is_kept`,
`microphone_entries_wait_until_the_system_track_covers_them`,
`rolling_coverage_hides_draft_echo_that_rolling_dropped`,
`completed_microphone_lines_wait_for_the_system_audio_that_judges_them`,
`live_view_hides_microphone_twins_of_remote_speech`,
`an_undecided_draft_line_waits_while_the_remote_side_speaks`,
`live_view_keeps_rolling_microphone_entries_that_rolling_already_judged`, and
`timed_words_become_sentences_at_punctuation_and_pauses` (synthetic signals).
Other checks:
`continuous_speech_is_hard_cut_with_overlap_and_covers_every_sample`,
`chunks_end_at_the_first_pause_after_the_target_length`,
`overlapping_chunks_join_at_the_middle_of_the_overlap`, `seam_drops_repeated_words`,
`speaker_echo_of_remote_speech_is_dropped`, `word_level_echo_is_dropped_as_one_utterance`,
`distinct_or_distant_local_speech_is_kept`,
`live_timestamps_preserve_dropped_packet_gaps_and_match_final_origin`, and
`streaming_resampler_matches_whole_buffer_conversion`. They use synthetic audio
and transcripts, not model inference or real speaker echo.

Opt-in measurements (ignored tests in
[meeting_echo_replay_tests.rs](../../src/meeting_echo_replay_tests.rs); they
need a private recording and are never part of CI): `meeting_echo_replay`
(every layer, cached transcripts, hand labels), `meeting_live_cpu`,
`moonshine_command_update_latency`, and `dictation_latency`.

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
Recording capsule Copy new / Option-Control-V -> One shared cursor (`MeetingPasteCursor` in `meeting.rs`); copies or pastes turns added since the last successful invocation
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
both stay recoverable from the Trash. Sources: `render_meetings` in
[app_window.rs](../../src/app_window.rs), `trash` / `rename` in
[meeting.rs](../../src/meeting.rs). Checks:
`deleting_trashes_the_markdown_then_the_recording_and_refuses_while_recording`,
`renaming_updates_the_manifest_and_an_unedited_markdown_heading`,
`retitling_replaces_only_a_matching_first_heading`,
`meeting_search_delete_and_rename_work_on_the_selected_meeting`, and the GPUI
layout checks `meeting_turns_alternate_sides_inside_the_detail` and
`meeting_overflow_menu_opens_beside_the_visible_actions`.

Preview: `scripts/capture-preview.sh OUT meetings [--meeting-recording]
[--confirm-meeting-delete] [--open-meeting-actions] [--meetings-empty]`.

Checks: `markdown_lists_metadata_and_labeled_turns` and
`export_never_overwrites_and_uses_safe_names`.

### Titles And Summary (Apple Intelligence)

```ts
While recording, setting on, model available           // meetings.title.provisional
  -> meeting-title thread, utility QoS; checks the transcript every 10 s
     (rolling chunks + live draft, echo-filtered)
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

### Agent Access (MCP)

```ts
hex mcp [--meetings-dir <path>]                     // meetings.mcp; release builds
├── MCP over stdio; independent of the Meetings opt-in (read-only)
├── Dispatches before process logging, the instance lock, GUI, and permissions
│   └── Safe beside the running app; logs only to stderr (HEX_MCP_LOG)
├── Default folder: Application Support/voice-control/meetings
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
`recording (in progress)` with the rolling transcript plus the live draft
(`meeting_live::shown_while_recording`, echo-filtered), labeled provisional. Not held ->
the app's recovery rules apply without writing: published `transcript.ndjson`
is final, an unfinished draft is `interrupted`. Stale lock files from finished
recordings read as not held. A mirror cannot see the Pro's lock, so a meeting
still recording there appears `interrupted` with its draft so far. Unreadable
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
│   pro:Library/Application Support/voice-control/meetings/
│   -> ~/Library/Caches/hex-meetings-mirror/ (0700)
├── Skips *.wav, *.lock, *.tmp; --delay-updates (files land together at the
│   end); --delete-delay (meetings trashed on the Pro disappear)
└── Pro asleep / unreachable -> exit 0 silently; next run retries
Each MCP client -> local `hex mcp --meetings-dir <mirror>` (no SSH, no lock)
└── No lock in the mirror -> a meeting still recording on the Pro reads as
    `interrupted` with its rolling transcript + draft so far; final once published
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
- Calendar titles (EventKit) are not implemented.
- The live Moonshine draft needs the command model when the selected language
  is English or Auto. Without that model, an English meeting shows rolling
  chunks only as they complete (about every 20–30 s). Another language drafts
  the uncommitted tail with the selected model instead. Parakeet v3 still
  identifies language from the audio; it has no prompt input, the same as
  dictation. The cadence tests ran on Linux. A macOS recording of the tail
  draft was not run.
- Settings, capsule, and Meetings pane previews launched, but screenshot capture
  failed on the build host (`could not create image from window`; the Studio
  session was locked, `CGSSessionScreenIsLocked 1`). The redesigned pane is
  verified only by GPUI layout checks, not visually.
- Echo thresholds are tuned on two speaker-mode calls (2026-10-15 and
  2026-10-16), with six hand-labeled local utterances; other rooms, external
  speakers, and Bluetooth are unmeasured. The 2026-10-16 replay audio was lost
  with Studio's temporary directory, so those numbers cannot be re-run. Real
  NSFileManager trashing and the native rename field are unexercised in the app.
- The live draft judges a Moonshine line whole: a line that mixes bleed with
  the user's own sentence stays hidden until rolling covers it.
- The CPU change was measured on the Studio, not on the Pro during a call.
- Apple SpeechTranscriber for the live draft is evaluated, not implemented:
  [meeting-live-transcription.md](../plans/meeting-live-transcription.md).
- Why the rolling worker stalled for over a minute on the Pro is unknown; slow
  chunks (> 5 s) are now logged.
- Titles and summaries have not run after a real recording in the app; the CLI
  path exercised the real model, and the provisional-title thread, its manifest
  write, and the menu/pane refresh are unexercised in a live recording.
