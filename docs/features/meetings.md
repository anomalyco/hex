# Meetings (beta)

Record a call locally: Hex captures the other participants (system audio) and
you (microphone) as separate tracks, transcribes them on this Mac, and saves a
Markdown transcript with `You` and `Them` turns. Recording starts automatically
only for calls from a website or app set to Always record, and never stops
automatically. This map covers macOS; Linux has no meetings.

Linked checks are locators unless an executed result is recorded below.

## How To Get To It

```ts
Labs > Meetings switch (macOS)                       // meetings.opt-in; off by default
├── Off -> only the Labs switch -> Turn on -> meetings.enabled = true
├── On -> Meetings joins the sidebar; header switch Recordings | Settings;
│   menu-bar items, call detection, Option-Control-V
│   Settings view: Call recording, Calendar, Detected calls, Advanced ▸
│   ├── System Audio Recording                  // meetings.permission; asked only now
│   │   ├── Allow -> macOS prompt the first time (tour's permission_control)
│   │   ├── Decided before -> Privacy pane + drag guide ("System Audio Recording Only")
│   │   ├── Asked -> pulse; TCC preflight re-read on each poll, no reopen needed
│   │   └── Granted -> the Allow pill morphs into a green Allowed; moves to Advanced
│   ├── Advanced -> Show recording indicator (on by default; corner-snapping capsule),
│   │   Transcript folder (~/Documents/Hex Meetings; Reveal / choose; an error opens
│   │   Advanced), Summarize with Apple Intelligence (on by default)
│   ├── Calendar -> Connect calendar; reminders before meetings // meetings.calendar
│   └── Detected calls -> Ask / Always record / Never offer per provider // meetings.policy
│       ├── Slack huddles, Zoom, Google Meet, Microsoft Teams, FaceTime (always listed)
│       └── then each website or app chosen for from an offer's "…"
└── Off -> Controller idles; no microphone-activity inspection or shortcut;
           the pane leaves the sidebar

Debug builds -> Always in the sidebar (DEVELOPER_FEATURES_ENABLED)
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
  -> On the microphone for 2 s by the monotonic clock // meetings.detect
     │   Google Meet and Banshee (banshee.app) in a browser count as known
     │   services; a slow watcher loop never stretches the time
     └── Browser on a page that is not a known call site -> 20 s,
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
  -> Same call is snoozed by key and app until off the microphone for 60 s,
     offered or recorded; stopping an automatic recording does not restart it
     └── Absence counts from the last time the call app was on the microphone,
         a gap included (sleep, Meetings off, a recording): an app that left
         the microphone during a recording is not snoozed for another minute
  -> "Left the call" prompt after the call app is off the microphone for 3 s
  -> Times are wall-clock runs: observations more than 10 s apart (sleep, a
     stalled loop) start a stability run again instead of counting the gap

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
[context.rs](../../src/context.rs) reads the active tab of Brave's front window,
even if Brave is behind another window. Background tabs are not searched: a
call open in one says nothing about which tab holds the microphone, and a
voice note elsewhere would otherwise get a known service's 2 s threshold or
trigger Always record. (A call in a background tab is therefore judged by the
active tab's page, which is the unknown-page 20 s for a new site.) That query is
an AppleScript round trip, so the watcher starts it on its own
thread (at most one at a time, about once a second) and reads the newest answer,
at most 5 s after it finished; a slow or timed-out query (the existing 2 s
limit) never holds up the loop. Answers and the remembered page are dropped
while Hex records, Meetings is off, the browser is snoozed, or no browser holds
the microphone; a query in flight at that moment is not repeated beside itself,
and its answer is discarded. When a browser's page is found for a run already
under way (the first pass had no answer yet), the run keeps counting, now
toward the known service's 2 s. When no answer names a page, the last page a browser showed while
it held the microphone names the call for 60 s (`BrowserPageMemory`), so the
site survives a failed query or Hex being in front. Safari, Chrome, Firefox and
Edge expose no URL, so their calls are unnamed unless the foreground context
says otherwise. Banshee has no recognised room path, so any `banshee.app` page
counts; it is not in Settings' always-listed providers, but appears under
Detected calls once its choice is set from an offer's "…". Not observed: the
new tab query against a live Brave with a Banshee call (the script compiles, and
the thread and memory are unit-tested with fake queries); the Banshee URL shape
was not available, so the host rule is a guess at "any page".

On 2026-10-21 Brave opened the microphone for 3-6 s at a time on
problems.kitlangton.com (board voice notes) and each burst got a "Browser call
call detected" offer; that is what the 20 s browser threshold prevents.
A second Banshee call on 2026-10-26 got no offer in the 30 s before Kit started
recording by hand: the "about 20 s" threshold was 40 loop observations, and each
loop took about 0.9 s (500 ms sleep plus the browser inspection), so an unknown
browser page needed about 37 s. The thresholds are now clock time.
Checks: `a_slow_loop_still_offers_an_unknown_browser_call_after_about_20_seconds`,
`a_slow_loop_still_ends_a_snooze_after_a_minute`,
`a_stalled_loop_does_not_count_the_gap_as_time_on_the_microphone`,
`banshee_is_offered_after_about_two_seconds`,
`the_site_is_resolved_while_hex_is_in_front`,
`a_page_seen_minutes_ago_no_longer_names_the_call`,
`a_remembered_page_belongs_to_its_browser_and_its_call`,
`finding_the_page_keeps_the_run_toward_the_known_threshold`,
`a_voice_note_on_another_site_is_not_a_banshee_call`,
`a_gap_with_the_app_absent_counts_as_absence`,
`a_recorded_call_that_left_the_microphone_is_not_snoozed_for_another_minute`,
`a_slow_tab_query_never_holds_up_the_loop`,
`a_reset_drops_the_answer_in_flight_without_starting_a_second_query`,
`an_answer_is_dated_by_when_its_query_finished`,
`a_cold_tab_query_then_its_answer_leads_to_a_banshee_offer` and
`a_snoozed_or_non_browser_application_is_not_queried` (meeting_watcher.rs),
`the_meeting_tab_query_reads_only_the_active_tab` (context.rs),
`a_browser_voice_note_is_not_offered_but_a_sustained_browser_call_is`,
`offer_titles_name_the_service_site_or_browser_without_repeating_call`,
`choices_apply_to_the_site_when_known_and_the_app_otherwise`,
`always_record_starts_after_the_same_stability_time_as_an_offer`, and
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
--bin hex capsule_snapshots` renders the offer, its choices, the
calendar reminders, and the automatic-recording states in light and dark on a
Mac with a GPU.

### Calendar Reminders

```ts
Meetings > Settings > Calendar                       // meetings.calendar (macOS)
├── Connect calendar -> macOS full-access prompt, on this click only
│   ├── Granted -> meetings.calendar.connected = true; reminders on, 1 min lead
│   │   └── trusts the prompt's answer: macOS keeps reporting "not determined"
│   │       to the asking process until it relaunches
│   └── Denied / Restricted / write-only -> "Calendar access is off"
│       └── Open settings -> Privacy & Security › Calendars
└── Connected
    ├── Remind me before meetings -> on by default
    ├── Remind me -> At start / 1 / 2 / 5 min
    ├── Calendars -> All calendars, or "2 of 3 calendars"; each switched in the menu
    └── Next 3 meetings: title, "Today · 2:30 PM", provider icon when it has a call link

Meeting = not all-day, declined, free, or cancelled, AND has other attendees or a
          Zoom / Google Meet / Teams / Slack huddle / Webex link (URL, location, notes)

Lead time reached (or Mac wakes within 5 min of the start)   // meetings.reminder
  -> Reminder capsule, offer-capsule placement, focus-free
     ├── "Design sync" · "Starts in 1 min · Zoom"
     │   ├── Not now -> close; this call is not offered when detected
     │   ├── Join & record (only with a link) -> open the link, then record
     │   └── Record -> recording titled "Design sync" with its attendees
     └── Service set to Always record: "Starts in 1 min · Hex records automatically"
         ├── Don't record -> close; the detected call neither offers nor auto-records
         └── Join (only with a link) -> open the link; Always record starts on detection
  -> Closes 60 s after the start (at least 30 s after showing); withdrawn if the
     meeting is cancelled, moved, or reminders are turned off. One at a time: an
     overlapping meeting's reminder waits until this one is answered or closes.
     Waits while Hex records; a recording that starts closes it.
Detected call during a calendar meeting on the same service
  -> recording takes the calendar title and attendees
```

Settings persist as `meetings.calendar` (`connected`, `excluded_calendars`,
`remind`, `lead_minutes`) in `settings.json`; missing means not connected, so
files from older builds stay off. Every calendar is included unless excluded.
Hex never asks for calendar access at launch: only Connect calendar calls
`requestFullAccessToEvents` (`NSCalendarsFullAccessUsageDescription`). With
Meetings off there is no Calendar card, no EventKit read, and no reminder.

The controller thread reads EventKit (one bounded query, 48 hours ahead, at most
200 events) when the store's change notification bumps its counter or once a
minute by the wall clock, so a wake rereads; each loop is then a cheap
scheduler pass. Reminders key each occurrence by event ID and start, so a moved
meeting reminds again and each occurrence reminds once. Settings reads its own
snapshot on a reader thread while the window shows, rereading when the
settings, access, or (once connected) the store change, and every 30 s; the
window's thread never calls EventKit. No network, never on the audio or UI
thread. Recording from a reminder (or dismissing it) snoozes detected calls
from the link's service until the meeting ends; without a link it covers only
the next detected call, and only until 5 min after the start (or the end, if
sooner). The manifest's `calendar` field and the
Markdown's Attendees line carry the calendar title and attendees, and the notes
prompt gets them as context.

Checks: `meetings_need_other_people_or_a_call_link`,
`all_day_declined_free_and_cancelled_events_are_skipped`,
`excluded_calendars_are_left_out_and_meetings_sort_by_start`,
`call_links_are_found_in_the_url_location_or_notes`,
`other_links_and_lookalikes_are_not_calls`,
`a_link_after_a_bare_http_in_the_same_word_is_found`,
`the_url_field_wins_over_the_notes`,
`an_overlapping_meeting_waits_for_the_showing_reminder`,
`a_late_reminder_stays_up_at_least_half_a_minute`,
`a_reminder_shows_once_at_the_lead_time`, `at_start_reminds_when_the_meeting_begins`,
`waking_shortly_after_the_start_still_reminds_but_not_much_later`,
`a_showing_reminder_closes_when_its_meeting_is_cancelled_or_long_started`,
`a_moved_meeting_is_reminded_at_its_new_time`, `back_to_back_meetings_each_remind`,
`reminded_occurrences_are_forgotten_after_they_end`,
`turning_reminders_off_withdraws_the_showing_one`, and
`a_detected_call_belongs_to_the_current_meeting_on_its_service` in
[calendar.rs](../../src/calendar.rs);
`a_call_answered_from_its_calendar_reminder_is_not_offered_again` and
`a_reminder_without_a_call_link_covers_only_the_next_call` in
[meeting_detection.rs](../../src/meeting_detection.rs);
`no_reminder_shows_over_a_recording` and
`a_dismissed_reminder_without_a_link_snoozes_only_around_its_start` in
[meeting_watcher.rs](../../src/meeting_watcher.rs);
`calendar_reminders_stay_off_until_connected_and_survive_older_files` in
[app_settings.rs](../../src/app_settings.rs);
`a_calendar_meeting_gives_its_title_and_attendees_as_context` and
`markdown_lists_the_calendar_attendees`; the reminder capsule checks in
[meeting_capsules.rs](../../src/rune_ui/meeting_capsules.rs)
(`a_reminder_with_a_link_joins_and_records_on_one_click`,
`a_reminder_without_a_link_only_records`,
`dismissing_a_reminder_tells_the_controller`,
`an_always_record_reminder_says_so_and_never_offers_record`,
`the_reminder_counts_down_to_the_start`); and the Linux pane tests
`the_calendar_asks_for_access_only_when_connect_is_pressed`,
`a_connected_calendar_saves_reminders_and_lists_meetings`, and
`denied_calendar_access_links_to_privacy_settings`; the answer checks
`a_grant_is_full_access_even_while_the_status_is_stale` and
`a_refusal_is_a_denial_unless_the_request_failed` in
[calendar.rs](../../src/calendar.rs), and
`a_granted_calendar_is_saved_connected_and_stays_connected`,
`a_failed_save_shows_its_error`, `a_denied_calendar_is_not_saved_and_stays_blocked`,
and `a_failed_request_keeps_connect_and_shows_the_error` in
[macos_host.rs](../../src/rune_ui/macos_host.rs). Each step of Connect calendar
logs a `calendar:` line to `process.log`. Scenes:
`meetings-settings` (disconnected), `meetings-calendar`, and
`meetings-calendar-denied`.

Not yet observed: the EventKit bridge against a real calendar, the macOS
permission prompt, and a reminder on screen. The controller loop that turns a
reminder click into a recording has no automated test; it shares the manual
Record path. Sources: [calendar.rs](../../src/calendar.rs),
[calendar.swift](../../src/calendar.swift), `Reminders` in
[meeting_watcher.rs](../../src/meeting_watcher.rs), `CalendarFeed` in
[macos_host.rs](../../src/rune_ui/macos_host.rs), and
[platform_settings.rs](../../src/rune_ui/platform_settings.rs).

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
     ├── Compact: pulsing red dot · title · mm:ss, plus "+N" when turns wait
     ├── Hover -> widens toward the screen with the actions below; pointer
     │   leaving -> eases back to compact (a held press keeps it open)
     ├── Drag -> follows the pointer; release -> snaps to the corner nearest
     │   its centre and saves recording_indicator_corner (window settings too)
     ├── Copy new (N) -> copies the turns not yet copied or pasted, through
     │   the click (see meetings.copy-through-now)
     │   ├── Shares Option-Control-V's cursor: either one advances both
     │   ├── N = speaker turns the ticker has seen; always enabled while
     │   │   recording, since the latest speech may not be counted yet
     │   ├── "Transcribing…" (disabled) while it waits, then copies; "Copied" for 1.5 s
     │   └── The wait runs on its own thread, never on the main thread
     ├── Copy all -> the in-call transcript so far as speaker-turn Markdown;
     │   disabled until there is text
     │   └── Both read the Parakeet in-call transcript (TranscriptReader) once a
     │       second on the ticker thread, never on the main thread
     ├── Open -> activates Hex, then opens the active meeting in the Meetings pane
     └── Stop -> Stop & Transcribe
  -> Settings window may stay closed

Detected app leaves the microphone for about 3 s     // meetings.call-ended
  -> "Zoom left the call" capsule: Keep recording / Stop & transcribe
  -> No timeout; nothing stops automatically

Capture sources (meeting_capture.rs)                // meetings.capture
├── Microphone -> dictation's input (saved device, else System default) via cpal, resampled to 16 kHz
│   ├── System default changes, the Settings microphone changes, or a missing
│   │   saved device returns, mid-meeting
│   │   -> New input opens before the old drops; same track, no retry gap
│   │      // macOS only (meetings are macOS); a failed open keeps the old input
│   └── Saved device listed but would not open -> System default stands in
│       -> tried again after 1, 2, 4, 8, 16, 30, 30, 30 s, then only on a change
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
corner, compact until hovered, dragged between corners) are ported. Copy new
and Copy all are checked by
`the_recording_capsule_copies_the_in_call_transcript` and
`copy_new_copies_the_transcript_read_through_the_click`, and rendered by
`capsule_snapshots` (`recording-empty`, `-live`, `-transcribing`, `-copied`,
`-open`, `-left`); they have not been clicked on a real Mac. Hover and the corner snap
are checked by `the_recording_capsule_rests_compact_and_opens_under_the_pointer`,
`dragging_the_recording_capsule_moves_it_and_snaps_to_a_corner`, and
`a_released_capsule_snaps_to_the_corner_nearest_its_centre`; neither has been
tried with a real pointer. A "Never offer" or "Always record" choice is saved and
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
     ├── No speech detected (Silero, once installed) -> empty transcript, no
     │   inference; coverage still advances
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
     provisional titles (TranscriptReader)

Copy new or Option-Control-V while recording       // meetings.copy-through-now
  -> The capture writer sends its partial blocks, then the request, in
     capture order, so the request covers every sample received
  -> The rolling worker answers it before any later audio:
     ├── Queued chunks as usual (published to the snapshot)
     ├── Each track's audio not yet in a chunk (no 6 s minimum), into a copy
     │   of its transcript: the snapshot and later chunks are unaffected
     ├── Waits for dictation like every meeting chunk; never a second model
     └── The same echo and You-waits-for-Them rules as the snapshot
     ├── Every sample up to the request, even 100 ms past a hard cut; the
     │   system track counts as transcribed through all its audio
  -> Each track remembers where the copy ended; its sentence ends there
     (between the words whose middles lie either side, as boundaries move
     when a later chunk hears more), so the copied part and the rest of it
     publish as separate entries and the paste cursor neither repeats nor
     drops either. A track rebuilt from its WAV after Stop keeps the cuts
  -> A copy pressed as capture ends is still answered; quitting ends the wait
  -> Up to 3 s (COPY_TRANSCRIBE_WAIT): usually under 1 s; past that, or with
     dictation holding the model, copies the in-call transcript as it is and
     logs a warning
  -> After Stop, or in another process: the in-call or final transcript

Stop & Transcribe
  -> Flush tails -> finish pending chunks           // usually seconds
  -> Any track that fell behind or failed -> stream its WAV in 1 s blocks
  -> Join on one timeline -> Echo suppression -> Final publication

Hex quits while recording                           // meetings.quit
  -> Capture stops; WAVs and manifest finalized; no chunk starts, no model loads
  -> Interrupted with its rolling draft + final.pending beside the manifest
  -> Next launch, Meetings on, no recording, once the dictation engine is up
     (or 30 s without one) -> full-WAV final pass -> Complete + Markdown + notes
     ├── Quit again or model not installed -> stays pending
     └── Other failure -> stays Interrupted with the error and its draft
```

Quitting gives its workers 5 s before `_exit`, and the final pass of a long
meeting can take longer, or load a model once the dictation engine is gone.
Sources: `record`, `finish_pending` in [meeting.rs](../../src/meeting.rs),
`PendingFinals` in [meeting_watcher.rs](../../src/meeting_watcher.rs). Check:
`quitting_mid_meeting_loads_no_model_and_finishes_in_time`. No repeatable
driver quits a real recording and relaunches.

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

Speechless chunks: models invent text for silence and noise ("Thank you",
"Grazie a tutti", or the recognition hints), and room or keyboard noise passes
the -60 dB silent-chunk check, so every chunk that passes it also goes through
the Silero speech detector used by dictation (see
[dictation](dictation.md#ongoing-jobs-and-output)). A chunk with no speech of at
least 160 ms becomes an empty transcript without inference, in rolling
transcription, the Stop & Transcribe fallback, and `meeting echo`. In a probe
outside the tree, run on each microphone entry of one 63-minute Italian call,
the detector found speech in 1 of 152 invented entries (a "Grazie a tutti.")
and in 32 of 33 real ones (the miss: a 40 ms "Certo." at a chunk boundary), and
scanned the call in 1.6 s on the CPU. Invented text inside a
chunk that also holds speech is not addressed. Without the detector only the
-60 dB check applies, as before. A Whisper chunk that returns only the recognition
hints is emptied too ([models](README.md#get-ready)).

In-call text was a Moonshine draft until 2026-10-22: the pane showed Parakeet
only for time rolling had covered, and Option-Control-V and the GPUI capsule's
Copy new pasted the Moonshine draft alone, which garbled names and jargon. The
meeting now loads no Moonshine; short Parakeet chunks replace it. Always-on
streaming Parakeet was rejected because one loaded model cannot serve it:
transcribe-cpp (0.2.4, and still 0.4.1) allows one active stream per model
across all sessions, and refuses any other run or stream with `Busy` while it is open
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
`speechless_chunks_skip_the_model_and_still_advance_coverage`,
`a_speechless_chunk_after_an_overlap_keeps_earlier_entries`,
`a_sentence_across_a_hard_cut_keeps_every_word`,
`rolling_publishes_only_entries_a_later_seam_cannot_replace`,
`a_capture_gap_restarts_the_track_after_it_instead_of_dropping_it`,
`a_copy_includes_speech_up_to_the_click`,
`repeated_copies_neither_repeat_nor_drop_words_as_later_chunks_overlap_them`,
`repeated_copies_cut_tracks_that_start_at_different_capture_times`,
`a_copy_just_after_a_hard_cut_keeps_you_while_them_is_silent`,
`a_copy_cut_holds_when_a_later_chunk_moves_the_words_slightly`,
`a_track_rebuilt_from_its_wav_after_stop_keeps_the_copies_cuts`,
`a_copy_waiting_when_capture_ends_is_still_answered`,
`a_copy_stops_waiting_when_hex_quits`,
`a_copy_that_runs_out_of_time_gets_no_reply`,
`a_copy_waits_out_dictation_and_a_preempted_chunk`,
`a_copy_reaches_only_the_recording_this_process_runs`,
`the_feed_forwards_a_copy_after_the_audio_before_it`,
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
Meetings pane -> Copy icon on a completed row's hover, the card's Copy transcript icon, Reveal Markdown (or Reveal Files when none)
Menu bar (idle) -> Copy Last Meeting Transcript
Option-Control-V -> `MeetingPasteCursor` in `meeting.rs` over the transcript
│   through the press (meetings.copy-through-now)
├── Pastes turns added since the last successful paste
├── Runs in order with dictation output: a dictation finished during its
│   wait (at most 3 s) pastes after it
├── Remembers, per source, the capture time spans it pasted (spans within 1.5 s
│   merge); an entry is new when its midpoint is outside them, so nothing
│   repeats or drops when the final transcript replaces rolling after Stop, and
│   text the final pass recovers for audio rolling skipped still pastes
└── Resets when Hex restarts
```

### Meetings Pane

```ts
Header: Start recording (the pane's one Primary button) / red dot, elapsed
        time, and Stop & transcribe (Secondary), grouped at the right
List (on the title's edge, no divider; grouped Today / Yesterday / date)
├── Search meetings (title, app, notes, final transcript) above the rows
└── Row: title (generated when present), time on the right; meta line
    "Recording · Zoom" / "Failed · 2 min" / "Zoom · 38 min" (red while
    recording or failed); no hover fill; a completed row shows a Copy
    icon in the time's place on hover (pointer only; "Copied" as a check)
Detail (a raised card beside the list)
├── Title, status chip (not when Ready), date · time · duration · app
├── Ghost icons top right: Copy transcript, Rename (finished only), "…" menu
│   └── Copy summary (stays open to show "Copied"), Reveal Markdown, Summarize /
│       Regenerate summary ("Summarizing…" disabled while running), separator,
│       Delete… (finished meetings only for the last three)
├── Errors at the top of the scroll area
├── Summary: one paragraph, at most two sentences
├── Chapters (when present): start, name, line, and a Copy section icon
└── Transcript: stacked turns, no bubbles; "You" in text colour, "Them"
    faint, each with its clock; merged turns; 30 s+ pause lines;
    "Listening…" while recording
Empty: "No meetings yet." with a one-line hint; "No meetings match." while
searching

Rename -> inline field; Enter or Save          // meetings.rename
  -> manifest title; Markdown heading only while it reads the old title
  -> file name unchanged; empty title refused
Delete… (overflow menu) -> "Trash recording & Markdown" (or "Trash recording")
  / Cancel, under the title in place of the icons until answered
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
     ├── Heading -> final title, only while it still reads Hex's last title
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

### Live Chapters (Labs)

Part of [Labs](labs.md), with its own switch, off by default: Live chapters
on its Labs row (`labs.live_chapters`, read when a recording starts). Off, or
with Labs off, nothing below starts, and meetings behave as without it.

```ts
Recording starts, Labs on                             // meetings.chapters.live
  -> meeting-chapters thread, utility QoS; checks the rolling transcript every 5 s
     (nothing read while Apple Intelligence is unavailable or titles are off)
  -> ≥ 3 min of new meeting time and ≥ 40 new words -> waits (≤30 s) for dictation
  -> ONE call over only the new lines (≤3,600 chars, newest kept, "[…]" before)
     ├── Prompt: vocabulary terms (≤30, vocabulary experiment) -> new lines ->
     │   "Current chapter: X"
     └── Answer: a 2–5 word name, one line, and "same topic or not"
  -> Rust decides, not the model's flag alone
     ├── First name -> chapter 1 at 00:00
     ├── Name sharing > half its words with this chapter's last 3 names, or any
     │   word when the model says "same" -> stays (clears a pending departure)
     ├── Otherwise a departure; it opens a chapter, dated at its window's first
     │   line, only when the next pass departs too and the current chapter has
     │   lasted ≥ 90 s. A second departure naming yet another topic waits for
     │   its own confirmation
     └── Refused passage -> counted as read; failure -> retry after 1, 2 … 5 min
  -> Recorder stores chapters in the manifest -> title, menu bar, pane
Stop
  -> A departure still waiting for its second pass opens (≥ 90 s rule applies)
  -> A pass finishing after Stop is discarded
After final publication, inside the notes thread       // meetings.chapters.refine
  -> One call per boundary, all within 30 s: lines (not merged turns) within
     3 min of the live boundary, "Chapter before/after: name (line)"
  -> Model lists the L numbers about the chapter after; Rust moves the boundary
     to the split that disagrees with the fewest labels, only if the labels
     cover at least half the lines after the live boundary and the new start
     stays after the previous chapter's
  -> chapters_refined = true; a failed call keeps its boundary
  -> Each call first waits (≤30 s) for dictation; a stop or quit ends the pass
     with nothing saved, so the next notes run redoes it
Copy section (pane, per chapter; menu bar "Copy Section" while recording;
`hex meetings show --chapter N`)
  -> "## <name>\n\n<line>\n\n" + the transcript lines that start inside the
     chapter, joined into turns only within it (one shared function)
```

Titles: with Labs on, the manifest records `title_source` (`default`,
`call`, `calendar`, `user`); off, it is written only to a manifest that already
has it or has chapters. Unless it is `user`, the shown title is the base title
plus up to three of the longest chapters in meeting order, once there are two:
"Team sync: image storage, motion lint, chat app name" (≤100 chars; a name that
would pass that is left out). While recording, the last chapter runs until now.
A title the user typed, including a rename while recording or transcribing, is
never extended. Manifests without the field read as `user` when their title is
not the default.

Read surfaces: `hex meetings chapters <id> [--json]`, `hex meetings show <id>
--chapter N`, a `## Chapters` list in `show`, `chapters` in `show --json`
(JSON stays version 1), and MCP `get_chapters`.

Sources: [meeting_chapters.rs](../../src/meeting_chapters.rs) (worker, decisions,
boundary pass, title), `GeneratedChapter` / `GeneratedChapterLines` in
[apple_intelligence.swift](../../src/apple_intelligence.swift), the record loop
and `TitleSource` in [meeting.rs](../../src/meeting.rs). Checks in
`meeting::chapters::tests` (fake model): pass plan and back-off, first chapter,
two-passes-away, minimum length, name overlap and the model's flag, refusals,
prompt order, pass window, confirming on another topic, Stop opening a waiting
departure, the boundary pass (one call per boundary, unsupported answers kept,
partial failures, a stopped pass saving nothing), title building and the
recording title, section Markdown, and the worker;
`meeting::tests::chapters_extend_every_title_but_one_the_user_typed`;
`copy_section_matches_the_menu_bar_and_the_cli` (macOS host: pane, menu bar,
and CLI give the same section when one speaker's turn crosses a boundary);
`copy_section_copies_one_chapter` (Linux-only Rune tests). The menu bar
submenu, the recorder writing chapters during a real recording, and the
`meetings-chapters` scene on a Mac screen are unexercised.

Real model (debug CLI): `hex meeting chapters <file.md>` runs the live passes
as a recording would, then the boundary pass, without saving.

Observed (2026-10-23, Mac mini, debug build) on a synthetic 15-minute, three
topic transcript (true changes at 05:32 and 11:02), 5 runs: passes 1.0–2.4 s;
boundary pass 1.8–3.4 s (2–3 calls, ~1,500 prompt chars each). All runs found
all three topics; 2 of 5 added a spurious "cdn cache headers" chapter at 03:42.
The second change landed at 11:02 in 3 runs and one line late (11:24) in 2; the
first at 05:32 in 2 runs and stayed at the live 06:38 in 3. The model's
same-topic flag was close to random ("same" for unrelated names, "new" for the
current one), which is why Rust's name overlap decides.

### Agent Access (MCP)

The release CLI also supports `hex meetings list`, `show <id|latest>` (with
`--from`/`--to` offsets or `--chapter N`), `chapters <id>`, and `search <query>`
through this query layer. See the [transcript CLI map](transcript-cli.md)
and [versioned JSON contract](../transcript-cli.md).

```ts
hex mcp [--meetings-dir <path>]                     // meetings.mcp; release builds
├── MCP over stdio; independent of the Meetings opt-in (read-only)
├── Dispatches before process logging, the instance lock, GUI, and permissions
│   └── Safe beside the running app; logs only to stderr (HEX_MCP_LOG)
├── Default folder: Application Support/hex/meetings (hex on older builds)
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
    │   ├── merged You/Them turns "[mm:ss · HH:MM:SS] Them: …" + next_cursor
    │   └── start/end, or the same as from/to (own schema fields; not both):
    │       entries overlapping the range; a start not before the end (an
    │       omitted start is 0) or past the meeting's end is an error
    ├── get_meeting_notes(meeting_id)
    │   └── stored title + two-sentence summary + "read the transcript for
    │       details, decisions, and action items", or an explicit
    │       "No Apple Intelligence notes are stored" + get_transcript hint
    └── get_chapters(meeting_id)                      // Labs
        └── "N. mm:ss–mm:ss · name" + line each, whether boundaries were
            corrected, a get_transcript start/end hint; or "No chapters are
            stored" ("No chapters yet" while recording); never generates any
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
over expanded terms: exact 1.0, prefix 0.5, edit distance 1 (2 from 8 letters)
0.35, Soundex 0.2 for 3-5 letter words (`Dacks` -> `Dax`); partial-term
coverage weighted the same way, and adjacent-term bonuses. Passages matching
every term exactly rank above all others. Edit and Soundex fallbacks apply only
to a term with fewer than three exact passages, and skip words in more than
0.5% of the passages searched (at least five) unless the term has no exact hit
and the word is exactly one edit away. This keeps `made`, `blog`, `Bob`, and
`believe` out of a `media`/`blob` search once those words appear exactly in
three passages, while a typo of a rare word still finds it. A term's match in a
passage is its exact stem when present, then the best score, kind, and
alphabetically first stem, so ties are deterministic. `"Quoted words"` are
required exact (stemmed) phrases; `fuzzy: false` keeps only exact stems.
`hex history --search` shares this ranker, counting an entry's raw and final
text as one document.

Semantic search is not shipped. On-device candidates are Apple NaturalLanguage
`NLContextualEmbedding` (asset download, macOS 14+) or the older static
`NLEmbedding` sentence model, through a Swift bridge like `apple_speech`, with a
cache under `~/Library/Caches/hex/meeting-index` keyed by meeting
fingerprints. Full-text with stemming and fuzzy matching answered the real
queries below; embeddings wait until a query fails for lack of paraphrase.

Sources: [meeting_query.rs](../../src/meeting_query.rs) (`meeting::query`,
queries and rendering) and [meeting_mcp.rs](../../src/meeting_mcp.rs) (`rmcp`
3.5 adapter); `run_mcp` in [main.rs](../../src/main.rs). Checks (synthetic
fixtures in `tests/fixtures/meetings` and `tests/fixtures/meetings-ranking`):
`meeting::query::tests::*` (listing and skipped folders, filters, misspelling
tolerance and ranking, stemming, accents, required phrases, context, drafts,
cursor paging, notes, held lock, `queries_never_write_to_the_meetings_folder`,
time and strict offset parsing, offset ranges on final, rolling, and
in-progress transcripts (including the "so far" error),
`exact_stems_outrank_loose_matches_to_common_words`,
`passages_matching_every_term_exactly_rank_first`,
`an_all_exact_passage_outranks_a_higher_scoring_partial_one`,
`a_term_found_in_fewer_passages_still_reaches_its_neighbours`,
`a_misspelled_rare_word_still_finds_its_passage`, and
`spelling_fallbacks_need_rare_terms_and_uncommon_candidates`) and
`tests/meetings_mcp.rs` (initialize with protocol 2025-06-18, tools/list with
schemas (including `from`/`to`) and read-only annotations, tools/call including
`from`/`to` ranges beside null `start`/`end` and their errors, resources/read,
stdout carries only JSON-RPC, no Application Support state created).

Observed (2026-10-23, Mac Studio, debug build, live meetings folder, read-only):
`hex meetings search "images database blob storage media"` dropped from 315 to
140 matching passages, with no fuzzy-only hit in the top 25 (before: four,
through `blog`, `blow`, `believe`, `made`, and `motto`); the top hit remains the
in-progress meeting's 115:02 passage, and 6 of the top 10 fall in its
113:03–120:43 storage discussion. `show <id> --from 113:03 --to 120:43` returned
that section's 168 rolling entries as Markdown and JSON.

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
│   pro:Library/Application Support/hex/meetings/
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
- Copying through the click is checked with a fake model only; its real-Mac
  latency and how often the 3 s wait runs out are unmeasured. Handing a
  copy's cuts from the rolling worker to the WAV rebuild in `transcribe` has
  no test of its own; the rebuild with cuts does. The copy waits
  for the next audio packet to arrive, so with both sources stopped it runs
  out of time and copies the in-call transcript.
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
- Live chapters (Labs) have not run in a real recording: the worker,
  manifest writes, title updates, menu bar Copy Section, and the boundary pass
  after publication ran only in unit tests and the debug CLI over one synthetic
  transcript. The thresholds (3 min passes, 90 s minimum, name overlap) are
  tuned on that transcript; the model's same-topic flag is unreliable, and a
  short detour can still become its own chapter. The boundary pass runs only
  when meeting notes run, after publication.
