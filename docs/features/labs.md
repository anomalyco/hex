# Labs

Labs holds features we're unsure about, **especially features people asked
for that we built**: they land in Labs, off by default, instead of shipping
on. Each feature's settings field, CLI name, **group**, **audience**, OSes, and
join date live in one descriptor table in `src/labs/feature.rs`. Its
**audience** (`labs::Feature::audience`) is either `Everyone` (ships in every
build) or `Dev` (`dev_build::is_dev_build`: debug, local `HEX_LOCAL_BUILD`,
test `HEX_TEST_BUILD`, and dev-channel `HEX_BUILD_CHANNEL=dev` builds). A
stable build never lists, accepts (`hex labs on`, `hex settings set`), or
runs a Dev feature: `available()` drops it, its gate reads false even when
`settings.json` says on, and the saved setting is kept untouched. In dev
builds a Dev row carries a quiet "Dev" tag, and `hex labs` prints each
feature's audience. Every feature below is `Everyone` today unless marked
`(Dev)`. Its **group** (`labs::Feature::group`) is one of Dictation, Output (paste, formatting,
spoken punctuation), History, Meetings, Annotate, Shortcuts, or Models. There
is no master switch. **Labs** is always the last item in the sidebar (Up and
Down reach it from the last pane; pressing it while it shows keeps it open) and
lists every Labs feature this OS has as a card (name, one-line description, toggle)
under its group's collapsible header, each **off by default**, plus Send
feedback and Run setup again. A card's title and line are
`labs::Feature::title` and `summary` (`src/labs/card.rs`), translated by the
window, and its OSes `labs::Feature::oses`.

**On the web.** Every `Everyone` lab (not Dev ones, nor the labs that only
matter once Annotate is on: Screenshots, Annotate last 15 seconds, Record the
screen under the pointer, Copy a capture as one file, Agent tools for captures,
Dictate while annotating)
has a page at `anoma.ly/hex/labs/<id>` (`dev.anoma.ly` for
the dev site), listed newest first at `/hex/labs`, with a 1200 × 630 social
card. The pages come from `site/src/labs.json`, which only the registry
writes:

```ts
Add or change a lab
├── HEX_UPDATE_LABS_JSON=1 scripts/check labs::web -> rewrites site/src/labs.json from Feature::ALL
├── commit it               -> labs::web's test fails while the JSON is stale
└── push to dev (site/**)   -> the dev site's build writes labs/<id>.html, labs/<id>.png, labs.html, and the sitemap entries
Page
├── Open in Hex             -> hex://labs/<id> (Mac labs only: only the Mac app registers hex://)
├── Get Hex                 -> the home page when the stable release's src/labs.rs names the lab,
│                              else Get Hex Dev (/hex/download/dev, and dev links for its other OSes)
└── hex labs on <id>        -> the same switch from a terminal, every OS
```

Checks: `labs::web` (Rust, the JSON and which labs it leaves out),
`every_lab_card_string_has_every_language` (i18n), and `site/labs.test.ts`
(`bun test`: links, meta, order). The site's build runs in the deploy only,
not on pull requests; the live page is unverified until a deploy.

```ts
Feature             Pane  Toggle (settings.json, hex settings)   On: what it does
├── Meetings (beta) yes   meetings.enabled                       Meetings joins the sidebar: call detection, recording, transcripts
├── Voice Filter (Dev) yes experimental.hex_microphone (macOS, Linux) Voice Filter joins the sidebar; feeds the Hex Microphone (macOS driver, Linux PipeWire source)
│                                        a stable build never feeds it; with the macOS driver installed it keeps the pane with Uninstall alone
├── Playground (Dev) yes  labs.playground                        a Playground sidebar item and ⇧⌘D open the story browser
├── Modes editor    yes   labs.modes_editor                      Modes joins the sidebar right after Output, with every mode's
│                                        applications, corrections, OpenCode rewrite, and transformations, as the old editor
│                                        did before Output replaced it; off, `hex window show --pane modes` opens Output
│                                        // checks: the_modes_editor_lab_brings_modes_back_to_the_sidebar (rune_ui/tests.rs)
├── Live text       no    labs.live_text                         provisional words beside the HUD while dictating; its card says when the selected speech model can't stream (Linux: or Speech engine is CPU), so no words would show
├── Faster live text no    labs.faster_live_text                 live words sooner, and the final text from the same shorter
│                                        lookahead for Parakeet buffered streaming, chosen from the model's own menus
│                                        (Unified English 70/7/7: 1.12 s instead of 2.08 s), at about 1.6 times the
│                                        encoder work; a change re-probes while idle, and an off-menu env value falls back
├── Stop phrase     no    labs.stop_phrase + labs.stop_phrase_text ("stop dictation", every OS)
│                                        saying the phrase at the end of a hands-free dictation finishes it, as if the
│                                        shortcut were pressed again, and the phrase never reaches the pasted text;
│                                        needs a speech model that streams on a GPU, as Live text does
├── Failure notices no    labs.failure_notices                   when a dictation fails, its explanation sits beside the HUD for about 6 s, or until the next capture starts
├── Live typing     no    labs.live_typing                       (Linux Wayland) provisional words as an underlined input-method preedit in the focused field; the final text is committed there instead of pasted; on a compositor that offers no input method (GNOME) its card is disabled with a note and `hex labs` says so
├── Appearance      no    labs.appearance + labs.appearance_theme (system/light/dark, default system)  the main window is forced light or dark instead of following the system; the dictation HUD (including its onboarding and Sounds previews) and meeting recording pill remain dark. Other overlays follow the OS. On Windows the title bar follows too, through its background and light/dark mode; Windows 10 has no caption colour, so DWM keeps its own light or dark caption background (builds before 19041 keep the OS caption) while its text and buttons follow
├── Live chapters   no    labs.live_chapters                     chapters a meeting while it records, boundary pass after Stop
├── Words on screen no labs.screen_vocabulary         (macOS)   names and terms in the focused window become temporary vocabulary for each dictation; read locally through Accessibility, never saved, password fields skipped
├── Learn from corrections no labs.learn_from_corrections (macOS) after a dictation is pasted, watch only that field for up to 5 minutes and add up to 5 names you corrected to Vocabulary; password fields, long text, and rewrites are skipped, and nothing leaves the Mac
├── Rewrite reads nearby text no labs.cursor_context (macOS) the mode's OpenCode rewrite reads the focused text element's text around the caret as context for names, terms, and where the result lands; password fields skipped
├── Switch model with keyboard language  no  keyboard_model_switching.enabled  (macOS, Windows, X11)
│                                        the input source picks its mapped model (#216); General shows the mappings while on
├── Mouse shortcuts no    labs.mouse_shortcuts         (macOS)   mouse buttons record as shortcuts (#209)
├── Also pause the browser no pause_browser_media      (macOS)   pause a playing tab too, with Pause media (#301); General shows its permission row
├── Share dictation shortcut no share_dictation_shortcut (Windows) apps such as Discord see the shortcut too (#176)
├── Free memory when idle no labs.unload_idle + labs.unload_idle_minutes (5/15/30/60, default 15)
│                                        the engine frees the speech model after that long unused (#442)
├── Keep mic open after dictating no labs.keep_microphone_open + labs.keep_microphone_open_seconds (30/60/180/300/1800, default 60)
│                                        with Turn off the mic when not dictating on, the microphone stays open for that
│                                        long after a capture, so the next dictation starts at once
├── Voice isolation no    labs.voice_isolation            (macOS) dictation's microphone opens through Apple's voice
│                                        processing, so Control Center's Mic Mode can offer Voice Isolation and background
│                                        noise is reduced; meetings, captures, the level meter, the local API's host
│                                        audio, and named opens stay on the raw cpal path; an open stream is replaced only while capture is
│                                        idle; an actual voice-processing failure (an open, a wrong device, a stall, or
│                                        more than three engine reconfigurations in 30 s) falls back to normal capture
│                                        until the lab is toggled or the resolved microphone changes
│                                        // checks: voice_isolation_is_its_own_bit_and_off_by_default (labs.rs);
│                                        // a_voice_isolation_change_reopens_only_while_capture_is_idle (audio/recovering/tests.rs);
│                                        // not run on a Mac, and no hardware confirmed Control Center offers the mode for Hex
├── Keep dictation on clipboard no labs.keep_on_clipboard (every OS) a pasted dictation stays on the clipboard; the previous one is not restored (#200)
├── Signature       no    labs.signature + labs.signature_text ("Sent from Hex", every OS)
│                                        every normal dictation pastes with the signature on its own line at the end, and
│                                        History and Paste Last keep that same text; not added to an empty transcript, one
│                                        that already ends with it as its own line, raw, printed, or API output
├── Copy dictation only no labs.copy_only (macOS, Windows)    a dictation goes to the clipboard and is never pasted; Paste last dictation still pastes (#629)
├── YOLO mode no labs.yolo_mode (every OS) press Return after pasting a shortcut dictation in a chosen app (none chosen = everywhere); Escape after release skips Return on macOS
├── Return to app   no    labs.return_to_app           (macOS)   a dictation pastes into the app it started in, switching back to it first when another app came forward
├── Rewrite by voice    no    labs.voice_prompt + labs.voice_prompt_phrase ("rewrite this", every OS)
│                                        a spoken phrase inside a dictation hands the mode's OpenCode model a one-off
│                                        instruction for that dictation; without one, processing is unchanged
│                                        // checks: the tests in dictation/voice_prompt.rs (whole-word matching, the last
│                                        // occurrence, the outcome table, phrase validation); a_spoken_instruction_replaces_
│                                        // the_rewrite_and_carries_context and a_voice_prompt_with_nothing_to_ask_pastes_
│                                        // nothing_and_runs_no_transformations (dictation/processor.rs);
│                                        // voice_prompt_is_its_own_atomic_and_carries_its_phrase (labs.rs);
│                                        // a_voice_prompt_phrase_needs_two_words_and_six_letters (control/settings.rs);
│                                        // an_invalid_voice_prompt_phrase_loads_the_default_and_keeps_its_switch
│                                        // (app_settings/persistence/tests.rs); voice_prompt_shows_its_phrase_on_its_card_while_on
│                                        // (rune_ui/tests/panes.rs); not observed with a real dictation, and no request
│                                        // ran against a live model
├── Spoken formatting no labs.spoken_formatting      spoken "new line"/"line break", "new paragraph", "open/close quote", "quote … unquote" become line breaks and quotation marks; Spanish "nuevo párrafo", "nueva línea", "abre/cierra comillas", … too (#630, #1040)
├── Spoken symbols  no    labs.spoken_symbols          spoken "hashtag", "underscore", "comma", … become symbols joined to their words: "#release", "my_variable", "done, thanks"; Spanish "arroba", "guion bajo", "coma", … too (#678, #1040)
├── Remove filler words no labs.filler_words          English dictations lose um, umm, uh, uhh, erm, er, ah, with no model (#631)
├── Dictation style no    labs.dictation_style + labs.dictation_style_choice (polished/casual/notes/verbatim, polished by default, every OS)
│                                        each dictation is written as Polished, Casual, Notes, or Verbatim like the iPhone app,
│                                        without an AI model
├── Clean up with Apple Intelligence no labs.clean_up_with_apple_intelligence (macOS)
│                                        one short on-device pass after the corrections fixes clearly misheard words and
│                                        resolves spoken self-corrections ("tuesday, no wait, wednesday" → "wednesday");
│                                        a mode that rewrites with OpenCode keeps its model, numbers-only dictations are
│                                        left alone, and an unavailable, late, refused, or over-reaching answer pastes the
│                                        corrected text; its card says why Apple Intelligence can't run here, and the
│                                        switch still turns off
├── Neutral voice   no    labs.neutral_voice             (macOS)
│                                        a mode that turns it on gets each dictation rewritten into plain, standard, neutral
│                                        wording on this Mac, instead of the OpenCode rewrite and the Apple Intelligence
│                                        cleanup, so the neutral pass itself never sends the text to OpenCode; the mode's
│                                        transformations still run afterwards and may use the network if you wrote them that
│                                        way, and Voice Action still uses OpenCode. Numbers, code, links, paths, and quoted
│                                        text must survive verbatim or the corrected transcript is pasted; Modes shows the
│                                        per-mode switch while on
├── Second dictation shortcut no labs.second_dictation_shortcut + second_dictation_hotkey
│                                        another shortcut dictates like the main one (#627); General shows its recorder while on
├── More dictation shortcuts no labs.more_dictation_shortcuts + extra_dictation_hotkeys (macOS)
│                                        up to four more shortcuts that each dictate like the main one, each with its own
│                                        mode; General shows a numbered row with a recorder, a shortcut mode, and Remove for each while on
├── Terminal paste shortcut no labs.terminal_paste_shortcut + terminal_paste_hotkey (Windows, Linux)
│                                        another shortcut dictates like the main one and always pastes with Ctrl+Shift+V; General shows its recorder while on
├── Caps Lock dictation no labs.caps_lock_dictation + labs.caps_lock_light (macOS)   a Caps Lock press starts and stops hands-free dictation; its light waits for the microphone, then shows recording (steady or slow blink), processing, and off
├── Smart spacing   no    labs.smart_spacing           (macOS)   a dictation pasted right after a word gets one leading space (#632)
├── Left and right keys no labs.sided_modifier_chords (macOS) the recorder keeps each modifier's side in a modifier-only chord: R⌘ R⌥ (#710)
├── Tap or hold      no    labs.tap_or_hold             (macOS, Linux) in Toggle mode, a press held past 300 ms dictates until it lifts; a short tap still locks (#711, #1144)
├── Double-tap Option, Command, or Globe no labs.double_tap_modifiers (macOS) Double-tap shortcut mode takes a shortcut without a key (one or more modifiers), such as Option, Globe, or ⌃⌥; two complete taps start hands-free, one tap or a hold does nothing
├── Hold through other keys no labs.hold_through_keys (macOS, Windows) macOS: a key a second or more into a held shortcut with no key no longer ends the dictation (clicks were already ignored after 300 ms); Windows: keys and clicks. They reach the app in front and the release still finishes; inside the first second a chord still discards it
├── Sort and filter models no labs.picker_sort_filter   the Local transcription picker sorts by recommendation, size, or name and can show only models for the saved language (#645)
├── Model test details no  labs.benchmark_details        picker rows show raw WER/CER (lower is better), language, dataset, source, and conditions; incomparable "Most accurate" badges drop (#647)
├── Parakeet Ultra  no    labs.parakeet_ultra          the picker offers Moondream's Parakeet Ultra (pinned Q8_0, CC BY 4.0) with a details view; stays on while it dictates (#648)
├── Special keys alone no labs.standalone_special_keys (Linux, Windows) Linux: Menu, Pause, Scroll Lock may be a shortcut's key, alone or with modifiers (#767); Windows: Insert, Home, End, Page Up, Page Down may stand alone (#794)
├── § key alone no  labs.section_key_alone       (Linux, macOS) the key left of 1 on ISO keyboards, § or a dead key such as ^, may be a shortcut with no modifier (#1050)
├── Microphone key alone no labs.microphone_key        (macOS) the microphone (dictation) key in a laptop keyboard's top row, always reported as Fn with key code 176, may be a shortcut with no modifier (#181)
├── Dictation stats no    labs.history_stats           History's Stats button opens a summary of retained dictations, with headline tiles, a typing-speed slider and time saved, the busiest-hour strip, a daily chart, app and Output bars, and Output time; an owner-only daily rollup (counts only, no text or app names) outlives retention, so the dialog also charts a 53-week heatmap and its headline totals cover all time; Clear all history deletes the rollup even with no entries; hex history stats prints it (#790)
├── Stats on History no  labs.history_stats_strip    History draws the Dictation stats dialog's six headline numbers (dictations, words, time saved, words per minute, daily streak, busiest hour) as one compact strip above its list; needs Dictation stats and changes nothing else
├── Copy recent dictations no labs.recent_dictations (macOS) the menu bar's Copy Recent Dictation submenu copies one of the last five dictations (#877)
├── Skip long pauses no    labs.skip_pauses             (Linux, Windows) a whole-clip dictation run hears only the head and tail of each long pause (#899)
├── Fix results in Output no labs.output_fixes           Output's Try card offers Vocabulary and Fix this, which copies an agent prompt naming the wrong result (#989)
├── Process API transcripts no labs.api_processing       (every OS) apps using Hex's local API can add process=global to
│                                        POST /transcriptions and POST /streams/{id}/finish to get the text after Global
│                                        mode's replacements, corrections, rewrite, and transformations; off, responses
│                                        stay raw transcripts as before
│                                        // checks: local_api::processing route tests (409, 403, 400, 429, processed, absent);
│                                        // not observed on macOS or Windows, and no /transcriptions success without a model
├── Timestamps in API transcripts no labs.api_timestamps (every OS) apps using Hex's local API can add
│                                        timestamps=segment|word to POST /transcriptions for the speech model's own
│                                        segment or word times (startMs, endMs); off, a timed request answers 403
│                                        timestamps-disabled and responses stay as before
│                                        // checks: local_api::timestamps (parse, 403 before the body, response fields,
│                                        // clamping, empty arrays), transcription::service (the 422 decision), and
│                                        // gguf_session (chunk offsets); no real-model transcript with times
├── Use model files where they are no labs.model_in_place the picker adds a model file without copying it, so accounts can share one (#941)
├── Smaller model files no labs.k_quant_models         (macOS) adding a model file also accepts Q6_K, Q5_K, and Q4_K weights, smaller than Q8_0 and unverified
│                                        // checks: k_quants_are_known_only_where_the_caller_runs_them,
│                                        // k_quant_tensors_are_measured_in_their_ggml_block_sizes (gguf_header);
│                                        // a_k_quant_import_needs_the_lab_and_falls_back_without_it (custom_models);
│                                        // smaller_model_files_is_its_own_switch_and_macos_only (labs.rs); not observed on Metal
├── Downloaded models only no labs.picker_downloaded   the picker's All speech models view can show only downloaded models, with the space Hex's copies take (#1018)
├── Transcribe dropped audio no labs.drop_transcribe   (every OS) a WAV, MP3, AAC, M4A, MP4, MOV, AIFF,
│                                        CAF, FLAC, OGG, Opus, WebM, or MKV file dropped anywhere on
│                                        Hex's window queues
│                                        behind any file already being transcribed, one at a time, each with the
│                                        dictation model and language in use;
│                                        it is read in ≤30 s pieces in this process and saved as `<stem>.txt` beside
│                                        it. A taken name gets " 2", " 3", and so on; a file in the OS temp folder,
│                                        or one whose folder Hex can't write, has its transcript saved in Downloads
│                                        instead. A drag with no file Hex reads says what it reads in one list on
│                                        every OS; Cancel stops the running file and clears the queue, and the
│                                        file's toast says Cancelling… until its worker answers, so nothing else
│                                        starts meanwhile; the toast names the file and its progress, and says Saved
│                                        <name> with the path and Show in folder. The lab's card offers Transcribe a
│                                        file…, which opens the same queue's file dialog, for Wayland, where nothing
│                                        can be dropped
│                                        // checks: the_drop_surface_shows_nothing_at_rest,
│                                        // a_dropped_file_does_nothing_with_the_lab_off,
│                                        // a_dropped_file_hovers_then_queues_a_job,
│                                        // dropped_files_queue_one_at_a_time,
│                                        // cancel_stops_the_running_file_and_the_queue,
│                                        // a_drop_after_cancel_waits_for_the_cancelled_worker,
│                                        // turning_the_lab_off_cancels_the_running_file_and_waits_for_it,
│                                        // a_cancel_after_the_last_check_still_says_saved,
│                                        // a_file_of_unknown_length_says_transcribing_without_a_percentage,
│                                        // a_file_hex_cannot_read_is_refused,
│                                        // a_saved_transcript_says_where_it_is_and_dismisses,
│                                        // the_labs_card_offers_a_file_choice_only_with_its_lab_on (rune_ui/tests/drop_transcribe.rs);
│                                        // the_first_free_name_numbering_counts_up, a_transcript_never_overwrites_an_earlier_one,
│                                        // a_file_beside_its_source_keeps_its_own_folder,
│                                        // a_writable_source_folder_needs_no_downloads_folder,
│                                        // an_unwritable_folder_sends_the_transcript_to_downloads,
│                                        // a_file_in_the_temp_folder_sends_the_transcript_to_downloads,
│                                        // a_temp_folder_reached_through_a_symlink_still_sends_the_transcript_to_downloads,
│                                        // still_wanted_refuses_a_cancelled_file,
│                                        // a_truncated_wav_is_refused_before_anything_is_transcribed,
│                                        // a_checked_wav_still_reads_from_its_first_sample,
│                                        // only_the_formats_this_build_reads_are_accepted,
│                                        // every_offered_extension_is_named_in_the_copy,
│                                        // every_extension_on_offer_reads_a_fixture_under_that_name,
│                                        // every_labs_line_names_the_formats_this_build_reads,
│                                        // an_unsupported_file_is_refused_before_anything_loads,
│                                        // a_file_without_audio_and_an_unknown_codec_say_what_is_wrong,
│                                        // the_bundled_mp3_decodes_to_sixteen_kilohertz_mono (transcription/file_transcript.rs);
│                                        // drop_transcribe_is_its_own_switch_and_everywhere (labs.rs); no model
│                                        // transcribed a dropped file, and nothing ran on Wayland (no file drags)
├── Keep recordings no    labs.keep_recordings         History entries keep their dictation audio; Play hears it, and Transcribe again runs it through another installed model (#1032)
├── Copy dictation audio no labs.copy_dictation_audio  History entries with a kept recording gain Copy audio, and the menu bar or tray gains Copy last dictation audio: one WAV file on the clipboard, ready to attach. Needs Keep recordings and stores no audio itself
├── Hide menu bar icon no labs.hide_menu_bar_icon      (macOS) the menu-bar icon hides while no meeting is under way; shortcuts keep working, reopening Hex shows Settings (#1064)
├── Pause from menu bar no labs.pause_menu             (macOS) the menu bar's action block offers Pause Hex; while paused every capture shortcut and the secure-input hotkeys are inert, the event tap passes keys through, and Resume waits for a neutral keyboard (#82)
├── Upcoming meetings in menu bar no labs.calendar_menu (macOS, with Meetings on and a calendar connected) the status menu starts
│                                        with an Upcoming section: up to five meetings from the reminder poll's own filtered list, soonest
│                                        first, each a local start–end time and title, "Now · " while in progress, "Tomorrow " or the
│                                        short weekday when it starts on a later day, and "No upcoming meetings" when none are left;
│                                        choosing a row with a call link joins it through the menu's own path, which opens
│                                        the link, records that occurrence so no reminder pops (closing a showing one), and
│                                        does not record (a provider set to always record may still record once the call
│                                        starts); a row without a link is informational
│                                        // checks: an_in_progress_meeting_reads_now, meetings_on_later_days_read_tomorrow_or_their_weekday,
│                                        // ended_meetings_are_left_out, at_most_five_meetings_show_soonest_first, an_empty_section_has_no_rows,
│                                        // a_stale_snapshot_never_shows, publishing_keeps_at_most_32_and_none_clears,
│                                        // the_section_hides_until_visible_and_fresh, the_menu_read_wants_a_connected_calendar,
│                                        // a_local_date_parses_the_iso_date_part (calendar_menu.rs);
│                                        // joining_elsewhere_before_the_lead_time_never_reminds,
│                                        // joining_elsewhere_while_showing_withdraws_it_for_good (calendar.rs);
│                                        // only_reminders_keep_the_titling_and_reminder_list,
│                                        // a_menu_only_read_and_a_new_lab_both_force_an_immediate_read (meeting/watcher.rs);
│                                        // calendar_menu_is_its_own_switch_and_macos_only (labs.rs);
│                                        // the_menu_reads_upcoming_then_meetings_then_actions_then_the_app (macos_rune/status_item.rs);
│                                        // not observed on a real Mac, in a real menu, or opening a real call link
├── Select all calendars no labs.calendar_bulk_select  (macOS, with Meetings on) Meetings › Settings › Calendar puts one
│                                        Select all / Deselect all button beside the Calendars select: Select all clears
│                                        meetings.calendar.excluded_calendars, and Deselect all sets it to the IDs shown now,
│                                        so calendars added later are still included; hidden with no calendars, and each
│                                        calendar's own switch in the menu is unchanged; off, the row is exactly as before
│                                        // checks: calendar_select_all_is_its_own_switch_and_macos_only (labs.rs);
│                                        // the_select_all_calendars_lab_saves_every_calendar_at_once (rune_ui/tests/panes.rs)
├── Notch indicator no    labs.notch_indicator         (macOS) on a screen with a notch, the HUD widens the notch sideways in the Dynamic Island's compact style at either visible position: an orange mic and level bars, the macOS spinner, a green checkmark; Hidden still hides it, and flat screens keep the Top/Bottom setting
├── HUD height      no    labs.hud_height              (macOS) with the HUD position Bottom, the plate keeps labs.hud_bottom_gap points (0–400, 24 by
│                                        default) above the Dock or the screen's bottom edge; a gap of 24 pt or less keeps the
│                                        window's bottom on the visible bottom, and a larger gap raises the window by gap − 24;
│                                        the plate only moves up inside it once the raised window would reach the visible top
│                                        (a gap above the visible height minus 104 pt), which within 0–400 only happens on
│                                        a very short visible frame; placement clamps the plate under the menu bar or notch;
│                                        off, the fixed 24 pt is exactly as before
│                                        // checks: hud_height_is_its_own_bit_and_off_by_default (labs.rs);
│                                        // the_default_bottom_gap_places_exactly_as_before, a_small_bottom_gap_sits_inside_the_window,
│                                        // a_tall_bottom_gap_raises_the_window_and_keeps_the_gap_exact,
│                                        // a_gap_that_reaches_the_visible_top_keeps_the_plate_inside,
│                                        // a_raised_bottom_gap_works_on_a_display_below_the_primary,
│                                        // a_huge_bottom_gap_keeps_the_plate_inside_the_visible_frame,
│                                        // a_bottom_gap_never_drops_the_window_below_the_visible_bottom (rune_hud/hud_placement.rs)
├── Import audio    no    labs.meeting_import           (macOS)   Meetings offers Import audio…: a WAV, M4A, MP3, AIFF, or CAF
│                                        file is decoded locally, transcribed with the selected model, saved as a normal
│                                        meeting (Speaker turns), exported as Markdown, and given notes as a finished recording
├── Stop every call recording when the call ends no labs.stop_any_call_recording (macOS, with Meetings on) an offered,
│                                        calendar, or by-hand call recording stops 30 s after its call app leaves the
│                                        microphone, even with Stop when the call ends off; a recording started by hand
│                                        watches the one native call app holding the microphone when it starts
│                                        // checks: the_lab_stops_on_request_recordings_and_the_setting_keeps_always_record,
│                                        // a_recording_kind_knows_whether_it_watches_a_call_app,
│                                        // one_native_call_app_is_watched_and_a_browser_or_two_are_not
│                                        // (meeting/watcher/call_end.rs); not observed on a real Mac
├── Restore clipboard on GNOME no labs.gnome_clipboard_restore (Linux) on a Wayland desktop without data-control (GNOME), a paste snapshots and restores
│                                        the clipboard through XWayland, only while this user's Xwayland already runs; never starts it (#1111)
├── Transform typed text no labs.typed_text + typed_text_hotkey (Linux) Settings › Output shows a Typed text section whose shortcut is unbound by
│                                        default; the shortcut or `hex typed-text` takes the clipboard (adopting a paste's pending restore), clears it,
│                                        sends Ctrl+A then Ctrl+C, runs the focused app's Output steps on the copied text (skipping spoken
│                                        formatting, spoken symbols, and filler words), clears and re-copies to confirm the selection still holds
│                                        that text, then pastes the result over it and restores the clipboard 500 ms later unless a newer copy
│                                        replaced it. A failed paste restores it too. Select-all replaces the whole field; on GNOME text is read
│                                        through wl-paste and cleared through wl-copy --clear. A clear that leaves text behind aborts.
│                                        It sends Ctrl+A/Ctrl+C, so it is not meant for terminals, and refuses before any key while
│                                        Terminal paste is on, because its Ctrl+V would become Ctrl+Shift+V.
│                                        // checks: typed_text_is_its_own_bit_and_off_by_default (labs.rs), the shortcut's rules
│                                        // (app_settings.rs), typed_text_fires_only_while_idle_and_changes_no_capture_state (linux/input.rs),
│                                        // typed_text_skips_the_dictation_only_labs (dictation/processor.rs),
│                                        // typed_text_joins_the_commands_only_while_its_lab_is_on (linux/gnome_shortcuts.rs), the fake-backed
│                                        // typed-text job tests and a_changed_second_copy_aborts_the_replacement (linux/typed_text.rs),
│                                        // a_new_clipboard_job_adopts_the_pending_restore and
│                                        // a_failed_dictation_paste_leaves_the_transcript_on_the_clipboard (linux/paste.rs),
│                                        // the_gnome_shortcut_key_tracks_typed_text (linux/app.rs); not observed with a real shortcut
│                                        // and target application
├── Call recording controls on GNOME no labs.gnome_meeting_capsules (Linux) on a Wayland desktop without layer-shell (GNOME), the recording offer and
│                                        recording capsule open on XWayland (Rune xwayland_input: override-redirect, clicks in the input shape, never keyboard
│                                        focus), decided at each show, only while this user's Xwayland already runs; never starts it. Off, an offer opens
│                                        the Meetings pane (#1271), whose note names the lab (#1274)
│                                        // checks: floats_only_by_opt_in_*, looks_for_nothing_while_the_lab_is_off (linux/meeting_capsules.rs),
│                                        // gnome_meeting_capsules_is_its_own_bit_*; Rune's xwayland pointer tests; not run on a GNOME session
├── Annotate on GNOME no   labs.gnome_annotate          (Linux)   on a Wayland desktop without layer-shell (GNOME), Annotate's recording capsule,
│                                        ink, and picker open on XWayland (xwayland_input: override-redirect, pointer in the input shape, never keyboard focus).
│                                        The ink is made with pointer regions and no keys (Rune refuses KeyPolicy::OnDemand on XWayland) and keeps its
│                                        regions while not drawing, since an XWayland window made click-through can't take the pointer later. Off, a
│                                        capture records but shows no capsule or ink. Decided at each show, only while this user's Xwayland already runs;
│                                        never starts it. Esc can't stop drawing there, so the capsule's pen button does, as does letting go of a
│                                        held Annotate key (a tap of it stops the recording)
│                                        // checks: gnome_annotate_is_its_own_bit_* (labs.rs), meeting capsule xwayland decision reused
│                                        // (linux/meeting_capsules.rs); Rune's xwayland tests; not run on a GNOME session
├── Record the screen under the pointer no labs.annotate_pointer_screen (Linux) on Wayland, a Screen capture asks the compositor which output
│                                        the pointer is on (ext-image-copy-capture-v1 pointer cursor sessions, no surface, no frames) and remembers a
│                                        screen-cast choice per output (`screencast-token-<output>`), so each monitor is asked once; off, one
│                                        remembered screen for all, as before. See capture.linux
│                                        // checks: entered (linux/pointer_output.rs), token_file_name and
│                                        // recorded_output (crates/capture/src/linux/portal.rs); not run on a compositor exporting the protocol
├── Screenshots     no    labs.screenshots + screenshots.hotkey (default ⌘⇧4) + screenshots.record_hotkey (default ⌘⇧5) + screenshots.folder
│                                        + screenshots.scale_to_1x/max_resolution/frame_rate/quality/export_size
│                                        + screenshots.record_kind/record_microphone/record_system_audio (macOS)
│                                        the shortcut picks an area (Space: a window); the PNG goes on the clipboard, into the folder (Desktop), and into a corner thumbnail;
│                                        while the Lab is on, the record shortcut and Annotate's capture shortcuts never share a key
├── Annotate last 15 seconds no labs.annotate_replay + capture.replay_hotkey (macOS 27+)
│                                        keeps a rolling 15 s SCClipBufferingOutput screen buffer for hex capture replay and the replay shortcut;
│                                        macOS keeps its screen-recording dot lit the whole time it is on
├── Copy a capture as one file no labs.copy_capture_zip (macOS, Linux, Windows)
│                                        a finished Annotate capture goes on the clipboard as one file, <stem>.scrawl.slim.zip, written
│                                        inside its bundle: the manifest, summary (with the bundle's path written as `.`), contact sheet,
│                                        and keyframes, without recording.mp4/.mov, audio.wav, frames-full/, contact-sheet-full.png, the
│                                        media and delivery markers, or an earlier copy. Stored, with no compression; the zip is renamed
│                                        into place, retention removes it with the bundle, and the SSH light-files copy to an agent host
│                                        leaves it out. macOS puts a file URL on the clipboard,
│                                        Windows CF_HDROP, Linux text/uri-list (the X11 target, or wl-copy --type text/uri-list). A failed
│                                        build or clipboard write logs and copies the capture as before; the capsule's Paste and Send
│                                        are unchanged, and off, the normal copy is unchanged. A terminal paste gets no text (the
│                                        clipboard holds only the file), and scrawl.json and summary.md still name media the zip
│                                        leaves out, because they describe the whole capture
│                                        // checks: slim_zip (CRC-32 vector, central-directory read-back, excludes, header rewrite,
│                                        // a second copy); a_file_is_one_percent_encoded_uri_list_line (linux/paste.rs);
│                                        // copy_capture_zip_is_its_own_switch_and_everywhere (labs.rs); not observed pasting the
│                                        // zip into a real chat or agent app, and the Windows clipboard write is cross-compiled only
├── Agent tools for captures no labs.annotate_agent_tools (every OS)
│                                        `hex annotate inspect [<capture>] [--marks] [--json]`,
│                                        `zoom [<capture>] (--at <M:SS|secs|phrase> | --mark <n|id> [--frame N]) [--span secs]`,
│                                        `frames [<capture>] --from --to [--fps N]`, and `skill` read a .scrawl bundle in the
│                                        CLI process, with Hex closed: summary.md, the marks, or scrawl.json; a dense sheet around a
│                                        moment or a mark with each cell's time and what was said; PNGs across a range; or the
│                                        hex-annotate skill an agent installs with npx skills add. zoom and frames write only into
│                                        <bundle>/zoom/. Off, the four commands refuse naming hex labs on annotate-agent-tools
│                                        // checks: capture_host::annotate_read (mark lines, both JSON shapes, a sheet's printed
│                                        // lines, every refusal), control::cli::annotate_read's clap conflicts, and
│                                        // agent_tools_for_captures_is_its_own_switch_and_everywhere (labs.rs); the video decode
│                                        // path is unverified on a real bundle, and no capture was recorded for this change
├── Dictate while annotating no labs.dictate_while_annotating (every OS)
│                                        while an Annotate capture records, the dictation shortcut dictates as usual: hold,
│                                        toggle, or lock it and the text pastes into the app in front, History and Paste Last keep
│                                        it, and the capture goes on recording and keeps the dictated words in its own track
│                                        (both copies of the same microphone timeline). Off, a press during a capture starts
│                                        nothing, as before
│                                        // checks: dictate_while_annotating_is_its_own_switch_and_everywhere (labs.rs);
│                                        // a_dictation_during_a_capture_shows_at_the_bottom_clear_of_the_capsule (rune_hud);
│                                        // not observed with a real Mac, Linux session, or Windows PC recording while dictating
├── Install agent skill from Annotate no labs.annotate_agent_install (every OS)
│                                        Annotate gains an Agents section: one row per coding agent writes the
│                                        hex-annotate skill into its own skills folder (Claude Code, OpenCode; Codex and
│                                        Pi use the shared ~/.agents/skills), a row copies npx skills add
│                                        anomalyco/hex --skill hex-annotate, and on macOS Command-line tool links hex
│                                        onto PATH (/opt/homebrew/bin, /usr/local/bin, else ~/.local/bin), never
│                                        replacing another program's hex and never linking an App Translocation copy.
│                                        Off, the pane is exactly as before
│                                        // checks: agent_install's unit tests (finding, installing, an outdated file,
│                                        // other files left alone, the shared copy, a symlinked skill, a dangling Hex
│                                        // link, App Translocation, a foreign hex, the link folder); rune_ui::scrawl's
│                                        // the_agents_section_needs_its_lab, installing_writes_each_checked_agents_skill,
│                                        // an_agents_row_checks_only_that_agent, a_linked_skill_has_no_switch; not run on
│                                        // a real Mac, and no agent read a capture after this installed the skill
├── Terminal paste  no    paste_with_shift             (Windows) paste with Ctrl+Shift+V (#477/#478; Linux keeps it in General);
│                                         Java games (javaw/java, e.g. Minecraft) still get Ctrl+V (#1362)
│                                         // check: terminal_paste_sends_plain_control_v_to_java_games (windows/paste.rs)
├── OpenCode in WSL no    labs.opencode_in_wsl          (Windows) Modes rewriting, the Modes model picker, and Annotate's capture sends
                                         use an OpenCode service running in the default WSL distribution
                                         ahead of a Windows one when it answers; every lookup also looks in WSL
                                         even when a Windows service is healthy, waiting up to 3 s, and that can
                                         start WSL; the Windows service is used when WSL answers nothing
                                         // checks: the_passwd_line_for_the_uid_names_its_home, both_wsl_prefixes_name_the_distribution,
                                         // a_distribution_or_home_windows_cant_reach_is_refused, a_zero_budget_or_cancellation_skips_the_lookup,
                                         // the_cache_follows_the_default_distribution_and_uid, one_lookup_serves_every_caller_and_a_short_wait_stops,
                                         // a_joiner_does_not_wait_for_a_lookup_older_than_its_cap, a_timed_out_lookup_defers_the_next_one,
                                         // the_workspace_is_created_somewhere_windows_can_write_it,
                                         // the_automount_section_gives_the_root_and_can_be_off,
                                         // windows_drives_become_paths_under_the_automount_root,
                                         // a_capture_folder_follows_the_automount_read_with_the_home,
                                         // a_wsl_service_gets_the_workspace_its_linux_server_can_use,
                                         // a_wsl_service_maps_a_capture_folder_into_its_distribution,
                                         // a_wsl_service_wins_with_the_lab_on_and_the_windows_one_is_the_fallback,
                                         // a_wsl_lookup_is_capped_only_when_windows_already_answered,
                                         // a_failed_wsl_request_forgets_the_workspace, a_not_found_wsl_request_forgets_the_workspace,
                                         // a_wsl_request_failure_reads_as_the_capsule_expects,
                                         // opencode_in_wsl_is_its_own_bit_and_windows_only; not run on a real Windows PC with WSL
├── Vocabulary files no  labs.vocabulary_files           (every OS) the Vocabulary sheet offers Import… and Export…
│                                        beside Add term; Import merges a CSV or TXT file's terms (Teams' custom dictionary
│                                        columns) into the list and saves it, and Export writes the list as a CSV file. Off,
│                                        the sheet is exactly as before
│                                        // checks: dictation::vocabulary::csv tests (header, quotes and embedded newlines,
│                                        // an unclosed quote, BOM, CRLF, blank rows, merge, duplicate terms, dropped
│                                        // spellings, caps, a folder, export→import round trip), merge_status and the
│                                        // sheet's import and export in rune_ui/vocabulary.rs,
│                                        // the_vocabulary_sheet_offers_files_only_with_its_lab_on (rune_ui/tests.rs),
│                                        // vocabulary_files_is_its_own_switch_and_everywhere (labs.rs); the native
│                                        // dialogs were not opened in a desktop session
├── Vocabulary menu no  labs.vocabulary_menu             (macOS) the menu bar's Vocabulary ▸ submenu, right under
│                                        Speech Model ▸, offers Add term… and Open vocabulary… and lists the last five
│                                        saved terms newest first; off, the row is hidden and the menu is exactly as
│                                        before (#2540)
│                                        // checks: vocabulary_lists_the_last_five_terms_newest_first (status_item.rs,
│                                        // macOS only), the_vocabulary_menu_is_a_macos_dictation_lab_off_by_default
│                                        // (labs.rs), adding_a_term_from_the_menu_opens_the_sheet_with_one_blank_row_last
│                                        // (rune_ui/vocabulary.rs); the native menu was not opened on a Mac
└── Only on speakers no   labs.speakers_only            (every OS) While dictating (Mute, Lower audio, Pause
                                         media) applies only while the default output is speakers; wired or Bluetooth
                                         headphones leave the output and media alone for that recording or capture, and
                                         anything Hex can't classify counts as speakers
                                         // checks: bluetooth_outputs_are_headphones,
                                         // a_built_in_output_on_its_headphone_jack_is_headphones,
                                         // apples_wired_headphone_device_is_headphones, everything_else_is_speakers,
                                         // an_analog_sink_is_headphones_only_on_a_headphone_port, a_headset_port_is_headphones,
                                         // bluetooth_sinks_are_headphones_unless_they_say_speakers,
                                         // a_sink_the_sound_server_does_not_name_is_unknown,
                                         // a_non_string_property_does_not_hide_the_others,
                                         // cpal_headphone_types_are_headphones (audio/output_route.rs);
                                         // speakers_only_is_its_own_bit_and_off_by_default (labs.rs);
                                         // not observed on a real Mac or a real Linux or Windows desktop with headphones

Settings › Labs                          // labs.page, a store (labs/store.rs)
├── Header -> "Labs", Search N labs (⌘F focuses it), Discover | Turned on · N
├── Group chips with counts -> All, Dictation, Output, History, Meetings, Annotate, Shortcuts, Models
│   on one line; More… lists only the groups that don't fit. Choosing one brings its chip onto the row.
│   (labs/group.rs; a group this OS has no lab in hides)
├── Discover, no chip or search -> New in Labs: the two newest labs, then every other lab
│   under its group's header; a lab added on the newest date among those shown carries New
├── Group header (key group-<id>) -> name, card count, chevron; a group with no card here hides
│   ├── Press -> folds or unfolds it, saved at once in labs.groups (settings.json, not a hex settings key)
│   ├── Never pressed -> open while it holds a lab that's on, folded otherwise
│   └── Turning off a group's last lab -> the group stays open (it is saved open first)
├── A chip, a search (name, description, group), or Turned on -> only the matching cards,
│   still under their headers; a chip or search shows every match whatever the folds,
│   under a plain heading; none -> "No labs match" / "Nothing turned on yet"
│   // checks: the_labs_search_clear_button_empties_the_field_and_brings_every_card_back,
│   // labs_groups_fold_and_remember_it, labs_groups_follow_each_os (tests/panes.rs);
│   // labs_search_clears_across_the_titlebar_button_and_keeps_typing (labs/store.rs):
│   // macOS runtime, top/middle/bottom presses with the native titlebar band, both
│   // appearances, typed and linked searches, restored cards, retained focus and typing;
│   // observed failing without the clear callback and passing with it; no AppKit window
│   // every_group_has_a_feature_and_round_trips_its_id (labs/group.rs);
│   // goldens mac-labs (light, dark), windows-labs, linux-labs, mac-labs-link,
│   // rune-whats-new and rune-whats-new-newest (light, dark)
├── Card -> neutral icon tile, name with New / Dev tags on one line, up to two lines of description, a switch;
│   pressing the card itself does nothing (there is no drawer)
│   ├── The switch (key lab-<id>) -> flips the lab, saved at once
│   ├── A pane feature on -> its pane is a normal top-level sidebar item; off -> hidden entirely
│   └── Voice Filter while its driver is installed, or a managed or locked lab -> its switch is disabled
├── A lab's own settings show on its card while it's on -> Free memory when idle: Unload after (5/15/30/60 min);
│   Keep mic open after dictating: Keep open for (30 s, 1/3/5/30 min, in an inline menu);
│   Appearance: Theme (System, Light mode, Dark mode);
│   Rewrite by voice: Phrase (a text field; an invalid draft says why under it while the saved phrase keeps working);
│   Screenshots: its shortcut, Record screen, Saves to (Reveal and the folder's own chooser, which a
│   failed save or chooser answers under the row), with Annotate on a Recording group (Scale Retina
│   to 1x, Max resolution pop-up, Frame rate, Quality), and the macOS-takes-this-shortcut note with
│   Open Keyboard Settings.
│   The Annotate last 15 seconds shortcut lives in Annotate's pane
│   // checks: unload_when_idle_shows_its_minutes_on_its_card_while_on,
│   // keep_microphone_open_shows_its_window_on_its_card_while_on (its control stays inside its card, its last choice reachable),
│   // appearance_shows_its_theme_on_its_card_while_on; scenes mac-labs-settings, mac-labs-screenshots
├── Opened by a link (macOS) -> hex://labs opens this page; ?tab=on, ?category=<group id> and ?search=<text>
│   set the store; hex://labs/<lab> also selects that lab's group chip (unless ?category names one), scrolls to its card,
│   and gives the card a focus border for 1.5 s (Store::flash). It never turns a lab on. An unknown
│   or absent name -> the page opens and process.log notes it. hex://settings/<pane>
│   (general, output, modes — only while Labs › Modes editor shows it — history,
│   meetings, voice-filter) opens a main pane; one Hex doesn't show -> General.
│   Links over 2048 bytes are ignored; a test build's scheme is hex-test://
├── Annotate is listed as a lab (Annotate group) wherever Hex has the Annotate pane; its switch flips capture.enabled
└── Footer -> "Unfinished features. They may change or break.", Run setup again (replays the onboarding,
    below), Send feedback -> https://github.com/anomalyco/hex/issues/new

hex labs                                 // labs.cli; edits settings.json, no running Hex
├── (list) -> every feature this build has: name, on/off, audience, description, its hex settings key
├── link <feature> -> macOS: prints hex://labs/<feature> (hex-test:// in a test build), which opens Labs on its group and card
├── on <feature> | off <feature> -> a running Hex saves and applies it (hex settings set); otherwise settings.json
├── off voice-filter with the driver installed -> refused: uninstall the driver (dev builds; stable has no voice-filter)
└── hex experiments … -> points at hex labs

hex settings set labs.live_text true     // macOS: through the running Hex
```

Checks: `rune_ui::deep_link` tests (parser, query, every feature's link reads
back); the labs store tests in tests/panes.rs (links, card toggles, a lab's own settings on its card, the stable build's missing Dev labs,
filters, ⌘F, a lone card's width, group folds and each OS's groups) and the
`mac-labs`, `mac-labs-link`, `windows-labs`, and `linux-labs` goldens (Linux);
`rune-whats-new` and `rune-whats-new-newest` (light and dark) move too, since
the feed lists every new lab.
Scenes: `mac-labs`, `mac-labs-on`, `mac-labs-category`, `mac-labs-search`,
`mac-labs-settings`, `mac-labs-screenshots`, `mac-labs-link`, `windows-labs`, `linux-labs`. Not yet observed: the store in a running
Hex.

Settings saved before this model may carry `labs.enabled`; it is read and
ignored, and every feature keeps its own toggle. `hex status` lists the
features on (`Labs` line on macOS, `labs` array in the macOS and Linux JSON,
`labs\t…` on Windows).

Every toggle applies at once, from the window or `hex labs on|off` (issue
568, from @maharshi365): Live text is asked as each dictation starts and its
plate is always installed (prewarmed on macOS whatever the setting, so a
plate turned on after launch never flashes the last dictation's words,
#1075; elsewhere only when Live text is on at launch);
the window always attaches the playground with Rune's own ⇧⌘D off, and its
sidebar item, ⇧⌘D, the macOS View menu, and story links open it only while
the toggle is on. Live typing is read at the hold threshold (or the Toggle
press). Live chapters are read when a recording starts. Nothing
starts at launch for a feature that is off. Limit: the macOS window's fixed
size is chosen when it is created, so it becomes resizable for the
playground only after Hex restarts.

Gate a Labs feature where its work starts (the worker, thread, or capture),
not only where its UI appears: `labs::live_text()`, `labs::live_chapters()`,
`labs::playground()`,
`labs::learn_from_corrections()`, `labs::microphone_linger()`,
`labs::voice_prompt()`, `labs::stop_phrase()`,
`app_settings::meetings_enabled()`, or
`app_settings::hex_microphone_enabled()`.
Add a new feature to `labs::Feature` (name, settings key, description, audience), give
it a row in `rune_ui/labs.rs` and a `hex settings` key, and default it off.
When a feature graduates, remove it from Labs.

Words on screen (`screen-vocabulary`, macOS): while on, each dictation
reads the focused window's visible text once, on a background thread, as the
capture becomes real: the same point the page refresh is asked for (a hold
past its threshold, a control start, or a released microphone opening). The
read is bounded to 1,500 elements, 40 levels below the window, 32 KB, and a
soft 150 ms deadline checked between elements, with the existing 0.25 s
messaging timeout; each value is capped at 4 KB, and a text value over that
reads only its `AXVisibleCharacterRange` (or is skipped when the app names
none), so a document or a terminal's scrollback never fills the budget.
`AXSecureTextField` elements and their subtrees are skipped, and Hex's own
process is never read (`accessibility::focused_window_text`).
`dictation::screen_vocabulary` extracts at most 24 names or technical terms:
mixed-case and CamelCase words, letters with digits, 2–6 letter acronyms, and
capitalized words away from a sentence's start; plain lowercase words,
sentence-start capitals, common window labels, hexadecimal ids, ordinals,
times, units, and random-looking tokens are dropped, then frequency and first
appearance order them. The terms attach to that capture's context
(`ContextSnapshot::screen_terms`) and join its transcription vocabulary after
the mode's own terms and the shared list, inside the existing 64-term bias cap
and prompt character budgets (`dictation::vocabulary::for_capture`). Parakeet
reads them only with a boost other than Off;
Whisper and Qwen3-ASR read them as a prompt; Apple Speech and other models
that take no vocabulary never read them. A capture with terms for a model
that reads them takes its whole clip after release rather than a live stream's
text, because a stream fixes its vocabulary when it starts; that costs a
little latency. Meetings, the local API, and OpenCode rewriting never see the
terms, and nothing is saved or logged except the term count and read time at
debug level. Off, or for a model that ignores the terms, no Accessibility
read or thread starts. Checks: `dictation::screen_vocabulary::tests` (the
Qdrant example, sentence-start exclusion, acronyms, long all-caps words,
stoplist and log levels, hashes, ordinals, times, units, random tokens, the
24-term cap, dedup against the saved list),
`a_captures_screen_terms_come_last_and_leave_with_it` and
`parakeet_terms_need_a_boost` in
[dictation/vocabulary.rs](../../src/dictation/vocabulary.rs),
`a_cancelled_capture_leaves_no_stale_screen_terms` and
`one_screen_read_runs_at_a_time` in
[dictation/screen_vocabulary.rs](../../src/dictation/screen_vocabulary.rs),
`screen_vocabulary_is_its_own_switch_and_macos_only` in
[labs.rs](../../src/labs.rs), and `a_missing_attribute_is_none` and
`hex_never_reads_its_own_accessibility_tree` in
[accessibility.rs](../../src/accessibility.rs). Not observed: a real Mac
reading a real window, or a dictated word corrected by what was on screen.

Learn from corrections (`learn-from-corrections`, macOS): while on, after a
dictation is pasted, Hex watches only the field it pasted into, reading it
about every 100 ms for up to 5 minutes. The first read retries for up to a
second until the app shows the pasted text, since the app handles ⌘V after
Hex's paste returns. What it compares is what Hex actually inserted (the
continuation join's capitalization and leading space, or smart spacing,
included), never the transcript. It stops on a focus change, an empty or
vanished field, a newer paste, or the timeout; the lab turned off stops it
too, and that stop learns nothing. A read that a newer paste overtook is
discarded, so a superseded watch learns only from text read before that
paste. It compares the final text to what it pasted and adds up to 5
name-like corrected words to Vocabulary: capitalized mid-sentence, all caps,
mixed case, letters with digits, and short phrases of those. A word after a
line break, after a colon, or after a sentence end (even behind a closing
quote or bracket) is a sentence start, so capitalizing there teaches
nothing, and a word like I'm is never name-like. It skips password fields,
fields over 20,000 characters (a field whose app names its character count is
measured before its value is read), text over 400 words, rewrites (more than
3 inserted words, and over 60% of them, changed), plain lowercase words, and
terms already in Vocabulary. Only a Paste that left text in a field is
watched: a copy, a Service or Print output, a YOLO send, and Hex's own window
never start one, while every other paste into an app (Paste Last, a meeting
turn, a capture's sheet and summary) moves the paste generation the watch
reads. Each learned term is saved the way `hex vocabulary add` saves one
(`vocabulary::add` on the saved list, then one `AppSettings::update` that
writes `settings.json` and applies the list to the running app), and an open
window adopts it. The one event it logs holds the number of terms only, never
the field or a term. Nothing leaves the Mac. Checks:
`learn_from_corrections_is_its_own_switch_and_macos_only` in
[labs.rs](../../src/labs.rs), `a_tick_stops_in_the_order_the_watch_reads`,
`a_reading_a_newer_paste_overtook_is_discarded`,
`only_the_lab_turning_off_discards_a_correction`,
`only_a_paste_the_person_can_still_fix_is_watched`,
`learned_terms_join_the_list_the_way_hex_vocabulary_add_does`, and
`a_full_list_keeps_its_terms_and_takes_no_more` in
[correction_watch.rs](../../src/dictation/correction_watch.rs), plus
[correction_terms.rs](../../src/dictation/correction_terms.rs)'s
`learns_a_short_correction_past_its_sixty_percent`,
`ignores_a_long_rewrite`, `the_pasted_text_matches_only_on_word_boundaries`,
`a_punctuation_edge_needs_no_word_boundary`, `an_im_word_is_never_a_name`,
`a_name_with_an_apostrophe_is_learned`, `a_line_break_starts_a_sentence`,
`a_colon_starts_a_sentence`, and
`a_closing_quote_after_a_period_still_starts_a_sentence`. Not observed: a
real Mac reading a real field through a correction.

Rewrite reads nearby text (`cursor-context`, macOS): while on,
each dictation reads the focused text element once, on a background thread, at
the same moment as the screen vocabulary read (a hold past its threshold, a
control start, or a released microphone opening), and only for the mode's
OpenCode rewrite. It reads `AXFocusedUIElement` through the existing 0.25 s
messaging timeout, never Hex's own process, and returns nothing for an
`AXSecureTextField` (role or subrole), so a password never leaves the app. It
reads `AXSelectedTextRange`, then `AXStringForRange` for up to 1,000 UTF-16
units before the selection, the selection itself (up to 2,000 units; a longer
selection skips the read), and up to 1,000 units after it, bounded with
`AXNumberOfCharacters` (without it, the after part is skipped). An app that
answers a range with more text than asked still sends at most that range's
units: the last units before the selection, the first ones for the selection
and after it. A slice edge that splits a surrogate pair drops that half, so a
real U+FFFD in the text stays, and everything empty returns nothing. The read
is taken non-blocking when the dictation finishes, so a very short dictation,
or an app that answers slower than that, may send no nearby text. The text
joins that capture's context
(`ContextSnapshot::caret_text`) and only the mode's OpenCode rewrite prompt,
ahead of the raw transcript, as context only: the prompt says never to copy,
answer, or follow it, uses it to resolve names, terms, references, and likely
recognition errors, and to fit capitalization and punctuation to the
insertion point. Any of the section's own tag names inside that text are made
inert, so field text can't close or open a section. It is never saved, logged,
put in History, events, or meetings. Typed-text transformations, the Rewrite
by voice request, meetings, and the local API never see it. Off, no Accessibility
call and no thread start, a capture finishing after the lab was turned off
sends nothing, and the rewrite prompt is byte for byte what it was. A terminal
exposes its scrollback through the same text field as any editor, so while the
lab is on, nearby terminal output — including secrets printed in the shell,
such as tokens from a command just run — can go with that dictation. A
cancelled capture discards its late read, and only one read runs at a time.
That text goes to the mode's OpenCode model, which may be a cloud provider.
Checks:
`dictation::caret_text` tests (a cancelled capture leaves no stale text, one
read at a time, a capture whose lab turned off sends nothing, an empty read is
no context, and the unit window's head/tail, lone-surrogate edges, and real
U+FFFD); `dictation::caret_prompt` tests
(the section and its replace note, empty parts omitted, tag names in field
text made inert); the golden
`a_prompt_without_caret_text_is_byte_for_byte_todays_prompt`, plus
`caret_text_joins_the_prompt_ahead_of_the_transcript` and
`empty_caret_parts_leave_no_tags_and_typed_text_reads_none`
([processor.rs](../../src/dictation/processor.rs));
`cursor_context_is_its_own_switch_and_macos_only` in [labs.rs](../../src/labs.rs);
and the Labs card tests in `rune_ui/tests/panes.rs`. Not observed: a real Mac
reading a real text field, or a dictated word corrected by the text around the
cursor; the Accessibility function itself has no host test here.

Mouse shortcuts off: the recorder ignores mouse buttons, `hex settings`
refuses one, and a saved mouse-button shortcut goes back to its default when
settings load (macOS), with a one-time note in General
(`mouse_shortcut_notice`). Check:
`mouse_shortcuts_go_back_to_their_default_while_their_labs_toggle_is_off` in
[app_settings.rs](../../src/app_settings.rs).

Clicks and drags reach Hex's filtering (event-consuming) session tap only
while some shortcut is a mouse button; otherwise they go to the listen-only
tap, as chord evidence only. A filtering tap on mouse events, even one that
passes every event through, broke dragging windows between displays and
Spaces (#1215). The tap thread's 5 s health check moves them when a mouse
shortcut is bound or cleared, so for up to 5 s after that a newly bound button
still reaches the app. Check:
`mouse_events_reach_the_filtering_tap_only_while_a_mouse_shortcut_is_bound` in
[suppression/tests.rs](../../src/suppression/tests.rs) (macOS only). Not yet
observed: a real two-display Mac dragging windows with Hex running.

Free memory when idle (`unload-idle`, issue 442 from @theoneand33): while
on, its card shows "Unload after" (5, 15, 30, or 60 minutes; default 15).
`transcription/engine`'s worker unloads a loaded GGUF model once no command
has reached it for that long and nothing records (`audio::idle_sleep::held()`: a
dictation past its hold or a meeting; a macOS capture from its press via
`dictation_busy`), freeing its RAM and VRAM; between commands no request
runs. With the lab on, a launch loads no GGUF model: the first dictation,
local API request, meeting chunk, or model switch loads it, so memory stays
free even when nobody dictates. On Linux, a model Settings has just prepared
for the listener starts loaded and unloads after the idle period. Until it
loads, whether it can stream comes from the model's definition.
Apple Speech stays loaded. The unloaded model still counts as held
(`TranscriptionEngine::holds`), so dictation, meetings, and the local API send
their work to the engine as before, and the worker reloads it before the
first command that needs it: audio fed meanwhile waits in the queue, so
nothing recorded is lost. A capture's stream opens after the reload and
catches up, falling back to the whole clip if it can't finish in time; the
HUD shows its transcribing state while a released clip waits on the reload,
with no look of its own. A failed reload fails that job and is retried by the
next. Check: `unload_idle_carries_its_minutes` in [labs.rs](../../src/labs.rs);
`a_parked_engine_holds_its_selection_and_reloads_it_before_the_first_job` in
[transcription/engine.rs](../../src/transcription/engine.rs) covers a parked
start and its first job. The unload itself has no automated check.

Keep mic open after dictating (`keep-microphone-open`): while on, its
card shows "Keep open for" (30 s, 1, 3, 5, or 30 minutes; default 1 minute)
in an inline menu, the same control General's rows use: five segments needed
about 332 points inside the card's 273-point copy column, so 30 min ran past
the card's edge. The values and the saved setting are unchanged. It applies
only while General's Turn off the mic when not dictating
(`release_microphone_while_idle`) is on: with that off the microphone is
always open, so the lab has nothing to do. When both are on, a capture that
finishes or is cancelled no longer closes the microphone at once. One
`audio::linger::MicrophoneLinger` holds the window for every listener: the
first idle pass with nothing recording arms it, a pass with something
recording clears it, and closing waits until a later idle pass has passed the
deadline with still nothing recording: no dictation, no local API dictation,
no release tail, and no capture copying the microphone (`capture_host`'s
`ListenerTimeline`). The window therefore also starts after an Annotate
capture-sink session (the sink's microphone stays for the window once it
closes) and after a local-API dictation (`POST /dictations`), which count as
captures too. A press inside the window takes the ordinary warm path: the
microphone is already open, so the capture starts at once and keeps its
pre-roll (up to 450 ms; none while "While dictating" mutes, pauses, or lowers
audio) from the warm stream, instead of the cold open's asynchronous start
with no pre-roll that happens today with the release setting on. A
digital-silence finish (the stream runs but delivers only zeros, #1122) ends
the window and closes the microphone on macOS and Linux, so the next press
opens a fresh stream; on macOS it does not close while a capture sink is
copying the microphone, so Annotate's capture gets no gap, and on Windows,
which has no silence reopen, a zero-only stream can linger for the window as
it can in always-open mode. On macOS a
stream interruption, or a stray reopen with no press waiting, closes the
microphone directly as it always did, which ends the window there early.
Turning the lab off during the window releases the microphone at the next
idle pass, the same release the check always made, without the wait; turning
the release setting off leaves the microphone open, as that setting always
did. macOS reads the gate as `dictation/audio`'s owner reconciles the input
(its loop ticks every 10 ms while the input is active, so the deadline is
observed); Windows checks it once per pass before its idle wait, and Linux
once per pass before it polls the microphone. On Windows and Linux the check
is one small macro the pass and the old event sites (a submitted finish, a
cancel, a discard before the microphone opened, a client dictation ending or
failing to start) share, so with the lab off a press handled in the same pass
still sees a closed microphone at once, as before. That check also closes a released
microphone an interrupted recording left open, which the old event sites
never did, with the lab off too. The window has no room in the Labs state
word beside Free memory when idle's minutes, so the switch holds the word's
last free bit and the window lives in its own atomic, both written by
`labs::apply`. Checks: the `audio::linger` tests
(`the_microphone_closes_only_after_the_window`,
`a_new_capture_clears_the_window_and_its_end_starts_a_fresh_one`,
`no_window_keeps_the_plain_release_while_idle_close`,
`a_cleared_window_waits_for_the_next_capture_to_end`),
`keep_microphone_open_is_its_own_bit_and_carries_its_window` and
`a_window_outside_the_choices_turns_the_linger_off` in
[labs.rs](../../src/labs.rs), the persistence tests in
[persistence/tests.rs](../../src/app_settings/persistence/tests.rs),
`a_microphone_linger_window_is_seconds_in_settings_json` in
[control/settings.rs](../../src/control/settings.rs), and
`a_lingering_silent_stream_closes_instead_of_reopening` in
[linux/dictation/silence.rs](../../src/linux/dictation/silence.rs). The Labs
page goldens were regenerated for the new card. Not yet observed on a real
Mac, Windows PC, or Linux desktop: no capture has been run with the lab on.

Voice isolation (`voice-isolation`, macOS only): while on, dictation's warm
microphone opens through Apple's voice processing instead of cpal's raw
input stream, so Control Center's Mic Mode can offer Voice Isolation for Hex
and background noise is reduced. The voice-processed microphone stays open
while dictation is idle, exactly like the raw warm microphone, so macOS's
ducking applies whenever Hex is listening and not only while a dictation
records; with General's Release microphone while idle on, it closes after a
capture and the ducking ends with it. `OpenRequest` gains
`voice_processing` (true only from `RecoveringAudioInput` while
`labs::voice_isolation()` is on) and a remembered
`voice_processing_fallback`; meetings, Annotate captures, the microphone
level meter (`open_dictation_input`), `open_named_input`,
`open_meeting_input`, the local API's host-audio and stream transcription
(which never opens a microphone), and every other OS stay on the cpal path.
The Swift bridge (`src/voice_processing.swift`) resolves the cpal device's
UID to a CoreAudio device
(`kAudioHardwarePropertyTranslateUIDToDevice`), sets
`kAudioOutputUnitProperty_CurrentDevice` on the input node's audio unit,
enables `setVoiceProcessingEnabled(true)`, connects the input through a
silent main mixer to the output (voice processing is duplex), and taps
channel 0 at the bus rate in a standard Float32 non-interleaved format with
`AVAudioTime.hostTime`; Rust converts the Mach ticks with
`CaptureInstant::from_mach_ticks` and feeds the same chunk and Hex
Microphone channels a cpal stream does. The bridge reads
`kAudioOutputUnitProperty_CurrentDevice` back after start. It observes
`AVAudioEngineConfigurationChange` only once start has returned, and reports
it through a separate error callback only while the engine has stopped; that
is a plain reopen, so the next open tries voice processing again. More than
three such reopens within 30 s count as a failure. An actual voice-processing
failure falls back to normal capture: an exception from the graph (the
Objective-C `@try/@catch` guard in `native/voice_processing_guard.m`), a
refused format, device, or start, a device that isn't the requested one after
start, or a stream that stalls or fails. `RecoveringAudioInput` remembers that
failure for the resolved microphone, so every later open uses the raw cpal
stream without reopening the engine, until Labs › Voice isolation is toggled
or the resolved microphone changes (the saved selection, the OS default, or a
stand-in). A configuration change reopens through voice processing again, so
it never disables the lab. A silence recovery's renegotiating open is raw too.
A gate change is applied by `DictationAudio::reconcile_input` only while
nothing records and no capture sink holds the microphone; the same device
is then reopened on the other path rather than kept as unchanged, and a
change during a capture waits for it to end. Off, every open is
byte-for-byte the existing cpal path. macOS may lower other audio while the
duplex unit runs: advanced ducking lowers it only while speech is heard,
and `.min` is the least ducking macOS offers. A Bluetooth headset may
switch to its call profile. Not verified on hardware: no Mac has run this
path; Control Center's Mic Mode offering Voice Isolation for Hex; the tap's
latency against the 200 ms release tail; and a meeting sharing the
microphone while dictation's voice-processed stream is open. Checks:
`voice_isolation_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs) and
`a_voice_isolation_change_reopens_only_while_capture_is_idle` in
[audio/recovering/tests.rs](../../src/audio/recovering/tests.rs). The macOS
Labs goldens were regenerated for the new card.

Restore shortcut default (`restore-shortcut-default`, issue 613) graduated:
Paste last dictation's row always offers Reset when its shortcut isn't the
default (see [dictation.md](dictation.md)). `labs.restore_shortcut_default`
is read and ignored, and `hex labs on restore-shortcut-default` says it
graduated.

Settings search graduated to the [window’s Find popover](README.md#find-a-setting-or-lab).
The former `settings-search` card and General filter field are gone. Older
settings files with `labs.settings_search` load without migration or an
immediate write; the unused key drops on a subsequent save. General’s ⌘F
opens Find. Labs’ own ⌘F, tabs, chips and local filter still work normally.

The sidebar footer's **What's new** opens a short feed of shipped features,
newest first. The feed follows the Labs registry: it lists the newest eight
labs this build offers (audience Everyone, on this OS, not locked), each with
its Labs card's date, title, description, and icon, so a lab that ships
appears here without another edit, plus the hand-written Sounds entry
(20261029) with the volume icon. Entries shipped on the same date share a
heading: Today, Yesterday, or 2–6 days ago by local calendar day; older
entries show the month and day, and the year outside the current year. A lab
entry opens and highlights its Labs card without changing the switch; Sounds
opens General's sound picker without selecting or playing a set.
`whats_new_opens_controls_without_changing_them` follows both kinds of link
with a fixture and compares the settings before and after, and
`whats_new_lists_the_newest_offered_labs` checks the newest offered lab is
listed first and no more than eight labs show.

Appearance (`appearance`, every OS): while on, its card shows "Theme"
(System, Light mode, Dark mode; default System). `labs::window_dark` resolves
the main window's paint from `labs.appearance_theme` and the OS appearance
for each view: System follows the OS live, Light mode always draws light, and
Dark mode always draws dark. Off, or with System chosen, the window is
exactly as before. Only the one main window is resolved; the onboarding HUD
preview and the Sounds sheet's HUD replay inside the window, and every
overlay (the HUD, meeting capsules, live text, capture capsule, screenshots,
and playgrounds), keep following the OS. On Windows the native title bar
follows the window's background and light/dark mode: Hex sets
`DWMWA_CAPTION_COLOR` and `DWMWA_USE_IMMERSIVE_DARK_MODE`, and a window
subclass re-applies that caption after a `WM_SETTINGCHANGE` while Appearance
forces a theme different from the OS, because winit repaints the frame from
the OS theme on each one while only reporting `ThemeChanged` for a real OS
theme change.
Windows 10 has no caption color attribute, so DWM keeps its own light or dark
caption background from the same flag; builds before 19041 do not recognize
attribute 20 and keep the OS caption. The caption's text and buttons follow
the forced mode.
Settings saves the choice at once, like the Caps Lock light.
Checks: `the_window_follows_the_os_unless_appearance_forces_it` in
[labs.rs](../../src/labs.rs), `the_main_window_paints_the_forced_appearance_not_the_os`
and `appearance_shows_its_theme_on_its_card_while_on` in
[tests/panes.rs](../../src/rune_ui/tests/panes.rs), and
`an_invalid_appearance_theme_resets_only_that_field` in
[persistence/tests.rs](../../src/app_settings/persistence/tests.rs);
`every_appearance_theme_string_has_every_language` in
[i18n.rs](../../src/desktop/i18n.rs) covers the three names in every
language. The Windows title bar change is only type-checked; no person has
seen it on Windows yet. Not yet observed in a real window.

Keep dictation on clipboard (`keep-on-clipboard`, issue 200, every OS): while
on, a pasted dictation stays on the clipboard. `paste::Paster`
reads `labs::keep_on_clipboard()` as each paste commits: it writes the
transcript without the nspasteboard.org transient markers (so clipboard
managers may record it), schedules no 500 ms restore, and drops any earlier
snapshot and pending restore (`ClipboardRestore::keep`), so a later restoring
paste never brings back an older clipboard. Into Chromium 155/156 or Windows
App, where Hex types the text instead of pasting, it also writes the
transcript to the clipboard after typing. On Windows,
`windows::paste::WindowsPaster` reads the
same gate as each paste commits: it writes an ordinary clipboard entry (not
marked out of clipboard history or cloud sync), calls its
`ClipboardRestore::keep`, sends Ctrl+V as usual, and schedules no restore, so
the dictation stays in Windows clipboard history (Win+V). On Linux,
`linux::paste::LinuxPaster::paste` reads the same gate as each paste commits:
it snapshots nothing, schedules no 500 ms restore, and calls its
`Restore::keep`, so a paste made while the lab was on leaves nothing for a
later paste with it off to bring back. A typed-text replacement, which saved
the clipboard before it copied the selection, and a capture's PNG still
restore; one with no readable format, or one whose clipboard changed while it
was read, has nothing to restore and falls back to the ordinary paste, so it
follows the toggle too. Off, pasting and
clipboard restore are exactly as before. Checks:
`a_kept_paste_leaves_nothing_to_restore` and
`a_kept_transcript_is_an_ordinary_clipboard_entry` in
[paste.rs](../../src/paste.rs) (macOS only);
`keeping_the_paste_cancels_a_pending_restore` in
[windows/paste.rs](../../src/windows/paste.rs) (Windows only);
`a_kept_paste_stands_down_the_pending_restore` in
[linux/paste.rs](../../src/linux/paste.rs); and
`contributed_options_are_labs_toggles_off_by_default` in
[panes.rs](../../src/rune_ui/tests/panes.rs). The ignored native Wayland test
also checks a kept paste on a real compositor, though it has not been run.
Not yet observed: a real paste on a Mac, Windows PC, or Linux desktop with the
toggle on.

Copy dictation only (`copy-only`, issue 629, macOS and Windows): while on, a
finished dictation (on macOS the dictation shortcut, Dictate without
processing, or `hex dictation` without `--print`; on Windows any dictation
job) is put on the clipboard and nothing is
sent to the focused app: no ⌘V or Ctrl+V, no typed text (Chromium 155/156
included), and no 500 ms restore, so selected text is never replaced. The
paste worker (`parakeet::finish_output`) reads `labs::copy_only()` as each
dictation's output commits and hands `paste::Paster` `PasteMode::CopyOnly`,
which writes the transcript as an ordinary clipboard entry (no
nspasteboard.org transient markers), drops any earlier snapshot and pending
restore (`ClipboardRestore::keep`), and ends the continuation, so the next
dictation is not joined to it. On Windows, `windows::dictation` reads the same
gate as each job's output commits and calls
`windows::paste::WindowsPaster::copy_only`, which writes an ordinary entry,
calls its `ClipboardRestore::keep`, ends its continuation, and sends no
keystroke, so the dictation appears in Windows clipboard history (Win+V).
Windows reports it as `copied`, not `pasted` (so does a dictation whose paste
target was the desktop or a taskbar; see
[dictation.md](dictation.md#windows-paste-target)).
History and Paste last dictation
update as for a paste, and Paste last dictation still pastes. With Labs › YOLO mode on, a dictation YOLO mode sends (its
chosen apps, or every app with none chosen) is pasted and sent anyway; a
rescued microphone-loss dictation on macOS is always only copied. Off, output
is exactly as before. Checks:
`a_copy_only_dictation_is_left_on_the_clipboard_with_nothing_to_restore` in
[paste.rs](../../src/paste.rs) (macOS only);
`keeping_the_clipboard_is_everywhere_and_copy_only_is_macos_or_windows` in
[labs.rs](../../src/labs.rs);
`contributed_options_are_labs_toggles_off_by_default` in
[panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real
dictation on a Mac or Windows PC with the toggle on.

YOLO mode (`yolo-mode`, every OS): while on, a
dictation started by a dictation shortcut (hold, double-tap lock, Toggle, a
second or extra shortcut, Caps Lock dictation on macOS) presses Return once
after Hex pastes its nonempty final text. A one-second “yep” sends just like
a longer message. The ordinary 300 ms hold threshold still discards short
taps. Silence, cancellation, empty output, and a failed insertion never
press Return. Old saved durations are ignored. The card's Only in row
(`labs.yolo_mode_apps`, empty by default: every app) limits it to
chosen apps, picked with the Modes pane's application picker
(`modes::PickerTarget::YoloMode`; a popover over the cards below its +,
closed by a pick, a click outside, or Escape) and matched as a Mode's
applications are (`ContextSnapshot::application_is`) against the frontmost
app captured with the dictation, under its own name or, on macOS, the name
Finder shows for its bundle (`Rex Beta.app` runs as `Rex`); with apps chosen, a dictation whose app is
unknown (Wayland without foreign-toplevel) never sends. It never applies to Dictate
without processing, `hex dictation` or local API captures, printing stops,
Live typing's input-method commits, meetings, Annotate
captures, or Paste last dictation. It wins over Copy dictation only (macOS
and Windows): a dictation it sends is pasted rather than only copied. Each OS decides as the output commits
(`yolo_mode::sends` reads the lab then): macOS `parakeet::finish_output`
picks `PasteMode::Send` (⌘V, and the continuation ends), then
`send_unless_escaped` waits until `yolo_mode::ESCAPE_WINDOW` (500 ms) after
the job's submission (release plus the 200 ms tail) and `SETTLE` after the
paste, whichever is later, and presses a bare Return (`PasteMode::Return`,
`keyboard::post_return`) only if the listener saw no Escape since submission
(`yolo_mode::SendGate`, a count `note_escape` bumps). An Escape after the
release leaves the text pasted and unsent, and its History entry is a
dictation. Pastes usually land 0.25–0.5 s after submission and Return went
100 ms after them, so the window adds at most about 0.25 s; the log has `YOLO mode pressed Return` with
`after_submit_ms`, or `YOLO mode pasted without Return`. Windows
`process_dictation_job` follows its paste with
`WindowsPaster::press_return` (VK_RETURN through `SendInput`); Linux
`DictationOutput::deliver` follows it with `LinuxPaster::press_return`
(`Chord::Return`: XTest on X11, `wtype -k Return` or the RemoteDesktop
portal on Wayland), with no Escape window. A sent dictation's History entry is kind Send. On Linux
a desktop keybinding that runs `hex dictation toggle` (Wayland without the
GlobalShortcuts portal, GNOME custom shortcuts) is a command, so it never
sends. Checks: `yolo_sends_yep_only_after_a_successful_paste`,
`chosen_apps_limit_where_it_sends`, `escape_after_submission_closes_the_gate`,
and `return_waits_for_the_window_and_the_settle` (yolo_mode.rs);
`escape_after_release_pastes_a_yolo_dictation_without_return` (parakeet.rs);
`old_yolo_durations_leave_the_switch_and_apps_alone` (settings persistence);
`return_sends_no_modifier` (linux/paste.rs);
`yolo_mode_is_its_own_switch_and_everywhere` (labs.rs). Not yet
observed: a real dictation sending a message on any OS.

Return to app (`return-to-app`, macOS): while on, a dictation pastes into the
app its capture started in. `handle_hotkey_action`'s Start (shared by the
dictation shortcuts, Caps Lock dictation, and `hex dictation start`) records
the frontmost application's pid once the capture starts
(`paste::return_to_app::capture_start`, which reads `labs::return_to_app()`
and never Hex's own pid), and the job carries it through inference and
processing to `parakeet::finish_output`. Right before a Paste or RawPaste
dictation commits, `paste::return_to_app::bring_to_front` compares that pid
with the app in front now: the same app pastes as usual, and a different one
is activated (`NSRunningApplication`,
`NSApplicationActivationOptions::ActivateIgnoringOtherApps`) and polled every
10 ms for up to 500 ms until it is frontmost, then given 50 ms so its key
window takes the paste. When the app is gone or focus can't be confirmed in
time, nothing is pasted: the job fails with "couldn't switch back to the app
you started in; use Paste last dictation" (error tone and HUD as any failure),
the text becomes the last transcript so Paste last dictation pastes it unless
the job was cancelled, and History, which records only pasted output, records
nothing. Paste last dictation, meetings, and Labs › Copy dictation only never
switch.
Off, no pid is read and output is exactly as before. Checks:
`only_a_different_recorded_app_needs_a_switch` and
`only_the_recorded_pid_is_the_app_that_arrived` in
[paste/return_to_app.rs](../../src/paste/return_to_app.rs) cover the pure
decision and poll (macOS only), and `return_to_app_is_its_own_bit_and_macos_only`
in [labs.rs](../../src/labs.rs). Not yet observed: a real Mac with another app
taking focus mid-dictation. The `mac-labs` and `mac-labs-link` goldens moved
with the Labs and Output card counts and were regenerated on Linux; the card
itself sits in a folded group there.

Rewrite by voice (`voice-prompt`, every OS): for giving one dictation a one-off
instruction for the mode's OpenCode model. While on, `Profiles::process_cancellable`
reads `labs::voice_prompt()` and the phrase as each dictation's processing starts,
after speech preparation and replacements have run, and splits the transcript at
the phrase's last whole-word occurrence. Matching ignores case, punctuation,
and apostrophes and hyphens inside a word, so "re-write this" matches "rewrite
this" and "don't" matches "dont", while "write this", "overwrite this", and
"this's" don't; there is no fuzzy matching. The text before the phrase and the
instruction after it keep their own punctuation and quotes; only whitespace and
joins next to the phrase (`, ; :` and en/em dashes) leave.

```ts
Dictate "we ship friday. rewrite this, make it formal."   // labs.voice_prompt + labs.voice_prompt_phrase
├── Phrase absent -> the mode's rewrite and transformations run exactly as before
├── Text before + instruction after -> one request applies the instruction to the text
├── Text before only (nothing after, or only thanks / thank you / please ...) -> rewrite the text clearly and concisely
├── Instruction after only (nothing before) -> write what the instruction asks for
├── Neither (just the phrase, or thanks or please after it) -> no request, nothing pasted
└── Afterwards -> the mode's selected transformations still run, except after Neither
```

The one request replaces the mode's normal rewrite, including while the mode's
rewrite switch is off, and uses that mode's model, variant, and deadline; no
model means Hex's ranked default list. With the mode's rewrite on and a prompt
saved, its prompt rides along as background instructions that the spoken one
overrides. The vocabulary (the mode's terms before the shared list) and the
foreground application and browser host are the same ones the rewrite carries,
and the protocol still asks for paste-ready text only. A failed, empty, or
timed-out request pastes the text before the phrase, with the punctuation it
was dictated with; with no such text it pastes the whole corrected transcript.

An organization's DisableOpenCodeProcessing lock reads as off: the card shows it
managed, the phrase is not split, that mode processes as usual, and
`hex settings set labs.voice_prompt_phrase` is refused as managed. The browser
host is read for every voice prompt: on Linux and Windows `ModeRuntime::reads_page`
turns the page on while the lab is on even when no mode rule or rewrite would,
and macOS's `reads_browser` does the same, which can bring up the Automation
prompt for the browser. macOS also shows the Processing stage even when the mode
has no rewrite or transformations.

Nothing pasted really is nothing. On macOS, an empty processed result or the
flag skips the paste, last dictation, and History in `finish_output`
([parakeet.rs](../../src/parakeet.rs)). The Neither outcome carries
`Processed::nothing`, so the Windows and Linux listeners finish quietly without
a failure, a paste, a last dictation, a done tone, or a `Pasted`/`Transcript
Completed` event; their activity log records a discard and the HUD closes the
job as cancelled, the same style as a discarded capture
([windows/dictation.rs](../../src/windows/dictation.rs),
[linux/dictation.rs](../../src/linux/dictation.rs)). An ordinary empty result
still fails there as before. A printing stop answers `no-speech` instead of an
empty transcript. The local API's `process=global` returns an empty `processed`
and never pastes or records
([local_api/processing.rs](../../src/local_api/processing.rs)); the Neither
outcome also drops the mode's transformations, so one can't turn the empty
result into text.

The phrase is `labs.voice_prompt_phrase` ("rewrite this" by default), also
`hex settings set labs.voice_prompt_phrase "<phrase>"`. It needs at least two
words and at least six letters counted after normalization, so one common word
can't start it. An invalid value is an `invalid-value` error; settings load
falls back to the default without changing the saved file. While the lab is on,
its card shows Phrase: a draft that fails validation stays in the field with the
reason under it in danger color, and the saved phrase keeps working; a valid
draft saves once typing pauses, like the window's other text fields. A phrase
saved elsewhere updates the field without resetting its focus, and an invalid
draft leaves when the lab is turned off, whether from its card or `hex settings`.
Off, processing is byte-for-byte unchanged. Checks:
`the_phrase_matches_whole_words_in_any_case`,
`write_this_and_overwrite_this_never_match`,
`hyphens_and_apostrophes_inside_words_are_ignored`,
`the_edges_keep_their_punctuation_and_quotes`,
`quotes_brackets_and_dashes_that_set_the_phrase_off_leave`,
`the_last_occurrence_splits_the_dictation`,
`the_outcome_table_covers_every_piece`, and
`a_phrase_needs_two_words_and_six_letters` in
[dictation/voice_prompt.rs](../../src/dictation/voice_prompt.rs);
`a_spoken_instruction_replaces_the_rewrite_and_carries_context`,
`a_failed_voice_prompt_pastes_its_text_or_the_whole_transcript`,
`text_only_and_prompt_only_requests_ask_for_the_right_thing`,
`a_closed_or_locked_voice_prompt_gate_leaves_processing_alone`,
`a_voice_prompt_that_asks_nothing_ends_through_process_cancellable`, and
`a_voice_prompt_with_nothing_to_ask_pastes_nothing_and_runs_no_transformations`
in [dictation/processor.rs](../../src/dictation/processor.rs);
`a_voice_prompt_is_locked_by_opencode_processing` and
`the_opencode_lock_turns_only_the_voice_prompt_off_and_keeps_it_saved` in
[policy.rs](../../src/policy.rs); `the_page_is_read_only_when_processing_uses_it`
in [dictation/modes.rs](../../src/dictation/modes.rs);
`voice_prompt_is_its_own_atomic_and_carries_its_phrase` in
[labs.rs](../../src/labs.rs); `a_voice_prompt_phrase_needs_two_words_and_six_letters`
in [control/settings.rs](../../src/control/settings.rs);
`an_invalid_voice_prompt_phrase_loads_the_default_and_keeps_its_switch` in
[persistence/tests.rs](../../src/app_settings/persistence/tests.rs); and
`voice_prompt_shows_its_phrase_on_its_card_while_on` in
[tests/panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real
dictation with the lab on, and a request against a live model; the Labs page
goldens were regenerated for the new card.

Stop phrase (`stop-phrase`, every OS): while a hands-free dictation records,
saying a configurable phrase at the end of your speech finishes it as if the
shortcut were pressed again (the Toggle press, or the press that ends a locked
Hold capture), and the phrase never reaches the pasted text. Only hands-free
captures listen: Toggle mode, or a Hold capture locked by the double-tap. An
ordinary Hold capture, where letting go is the end, is unaffected, and so are
meetings and the local API's `/dictations`. A `hex dictation start` capture,
printing or not, is hands-free too, so the phrase ends it like any locked one,
and a printing start answers its command with the words minus the phrase.

Detection uses the provisional words the streaming engine already produces for
Labs › Live text: with only Stop phrase on, the engine's `live_text` closure is
also true, so each dictation's live stream publishes `Partial`s (`stable` and
`tentative`) for the phrase matcher. It does not turn on the HUD's live-text
plate: `DictationCapture::begin_stream`'s `show_live_text` still reads only
`labs::live_text()` and Live typing's board. So the lab needs a speech model
that can stream on a GPU, exactly as Live text does; with no stream the words
never arrive and the capture ends only by shortcut. The phrase fires as soon
as the model commits it (a partial's `stable` words end with it), or once two
observations in a row whose `stable` and `tentative` words together end with
it agree, so a flickering tentative word can't finish a dictation, and a
capture finishes at most once. Committed words matter because the engine
publishes only changes (`Partials::publish` drops a partial identical to the
last): a phrase the model committed and the speaker then left quiet is often
the capture's last observation, and waiting for it to be published twice
would never finish that dictation.

```ts
Dictate "we ship friday. stop dictation."       // labs.stop_phrase + labs.stop_phrase_text
├── Phrase absent -> the capture records until the shortcut ends it, as before
├── Phrase in the last words, hands-free capture -> it finishes as its press would;
│   its job strips the words -> pasted "we ship friday."
├── Only the phrase -> empty: nothing pasted, no History, no error tone
├── An ordinary Hold capture -> the release still ends it; the phrase is just words
├── /dictations client capture, meeting -> unaffected
└── A model that can't stream -> no partials, so it never fires
```

Matching is whole-word and case- and punctuation-insensitive, the same
`words()` tokenizer a voice prompt uses, so "Stop Dictation.", "stop
dictation!", and "stop dictation" match while "stops dictation", "stop
dictations", "unstoppable dictation", and "stop-dictation" don't (a hyphen
joins a word, so it isn't the two-word phrase). The phrase leaves the text with
the whitespace and joins a voice prompt drops next to its phrase (`, ; :` and
en/em dashes), so "we ship friday. stop dictation." leaves "we ship friday."
while "we ship friday — stop dictation" leaves "we ship friday"; the sentence's
own final `.?!` stays. A capture that is only the phrase leaves an empty
string, which pastes nothing.

The phrase is `labs.stop_phrase_text` ("stop dictation" by default), also
`hex settings set labs.stop_phrase_text "<phrase>"`. It follows the voice
prompt's rules: at least two words and at least six letters counted after
normalization, so one common word can't finish a dictation. An invalid value is
an `invalid-value` error; settings load falls back to the default without
changing the saved file and keeps the switch. While the lab is on, its card
shows Phrase: a draft that fails validation stays in the field with the reason
under it in danger color, and the saved phrase keeps working; a valid draft
saves once typing pauses, like the window's other text fields. A phrase saved
elsewhere updates the field without resetting its focus, and an invalid draft
leaves when the lab is turned off.

Every OS finishes the capture itself, never by injecting keys. Each
listener owns a `dictation::stop_phrase::Detector`, fresh for every capture,
and feeds it each live partial. When it fires the words are read once more and
travel with the job, and the capture ends through its own shortcut state
machine: macOS sends the fire (with its capture generation) through the audio
owner's events to `recognition`, whose `stop_phrase` module has the machine
recording hands-free finish itself (`DictationHotkey::finish_hands_free`: the
`HotkeyAction::Finish` its own press makes, then straight back to Idle, as the
Caps Lock finish and a suspend leave it, since no key is down and no stale-key
recovery is owed; then the ordinary `finish_hotkey_dictation`); Windows asks
the keyboard hook to emit that ordinary `HotkeyAction::Finish`
(`WindowsHotkeyMonitor::finish_hands_free` under `CALLBACK_STATE`), whose
words only that action takes; Linux sends
`ListenerCommand::FinishPhrase` into the queue its monitor already drains, and
`ShortcutState::finish_hands_free` answers with `HotkeyEvent::FinishPhrase`.
The words ride with the job (`Finishing`/`Job` on Linux and Windows,
`InferenceJob` on macOS) and `dictation::stop_phrase::strip_finished` removes
them from the final transcript before any processing, raw captures included.
A capture that is only the phrase reads as a plain discard in Activity on
every OS: not a paste, not History, no error tone, and not a voice prompt that
asked nothing.

A request that arrives after its capture ended is ignored: macOS drops it by
capture generation before the listener sees it, and both other platforms ask
the state machine, which finishes only a capture still locked.

Checks: `the_phrase_matches_the_last_words_in_any_case`,
`words_that_only_contain_the_phrase_never_match`,
`stripping_keeps_a_sentences_final_punctuation_and_drops_the_joins`,
`a_capture_that_is_only_the_phrase_strips_to_nothing`,
`stripping_leaves_a_dictation_without_the_phrase_alone`,
`a_committed_phrase_fires_the_detector_at_once`,
`a_tentative_phrase_fires_once_after_two_observations_in_a_row`,
`a_flickering_tentative_word_never_fires_the_detector`,
`an_ordinary_dictation_never_fires_the_detector`, and
`a_finished_job_strips_its_phrase_and_leaves_every_other_transcript_alone` in
[dictation/stop_phrase.rs](../../src/dictation/stop_phrase.rs);
`a_stop_phrase_finishes_a_locked_capture_and_leaves_the_next_press_free` in
[suppression/tests.rs](../../src/suppression/tests.rs);
`a_stop_phrase_finishes_only_a_hands_free_capture` and
`a_stop_phrase_request_finishes_only_a_locked_capture` in
[linux/input.rs](../../src/linux/input.rs), the first also in
[windows/input.rs](../../src/windows/input.rs);
`stop_phrase_is_its_own_atomic_and_carries_its_phrase` in
[labs.rs](../../src/labs.rs); `a_stop_phrase_needs_two_words_and_six_letters`
in [control/settings.rs](../../src/control/settings.rs);
`an_invalid_stop_phrase_loads_the_default_and_keeps_its_switch` in
[persistence/tests.rs](../../src/app_settings/persistence/tests.rs); and
`stop_phrase_shows_its_phrase_on_its_card_while_on` in
[tests/panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real
dictation with the lab on, and the Mac and Windows builds, which the Linux host
cannot compile.

Signature (`signature`, every OS): while on, every normal dictation is pasted
with `labs.signature_text` on its own line at the end. The line is added once,
at the one point each output stage's final text feeds the paste, History, and
Paste last dictation, so History shows exactly what was inserted and Paste
last reinserts it unchanged. macOS signs in `parakeet::finish_output` only a
`TranscriptionTarget::Paste` or `CopyOnlyPaste` (never raw, printed, or the
local API), Windows in `windows::dictation::output::process_dictation_job`
only when the job is neither printed nor a raw flavour, and Linux in
`linux::dictation::output::DictationOutput::deliver` behind the same gate.
Nothing signs meetings, typed text, Annotate, or `Paster::paste` itself.

The pure rule is `dictation::signature::append`: it appends only when the lab
is on, the text is not empty after trimming, the saved signature is not empty
after trimming, and the text does not already carry it on a line of its own.
That means the text is the signature, or the signature follows a line break
with nothing but spaces or tabs in between; a signature that merely ends a line
of prose, like "Hex" in "I love Hex", is appended. The saved signature is
trimmed, so its own edges add no blank line. A saved value must be one line of
at most 200 characters with no control characters; another value is an
`invalid-value` error, and settings load falls back to the default without
changing the saved file and keeps the switch. While the lab is on, its card
shows Text: a draft that fails validation stays in the field with the reason
under it in danger color, and the saved line keeps working; a valid draft saves
once typing pauses. A line saved elsewhere updates the field without resetting
its focus, and an invalid draft leaves when the lab is turned off.

A signed paste is standalone on every OS: it neither joins the dictation
before it nor records a continuation, so the next dictation is never glued onto
the signature line. macOS passes `PasteMode::Standalone`; Windows and Linux
pass `standalone` through their paster. Paste last dictation remembers with
its kept text whether that text was signed (`dictation::paste_last::LastDictation`)
and reinserts a signed one standalone as well, so it neither joins nor leaves a
continuation even after the lab is turned off or its line changes; an unsigned
one reinserts as a continuation. On Linux, Live typing commits the
signed text in its own final input-method step, so the provisional preedit never
shows the line and the text is typed exactly once; if that commit fails, the
ordinary paste path carries the same signed text.

```ts
Dictate "we ship friday"                  // labs.signature + labs.signature_text
├── Off -> pasted and kept exactly as before
├── On, "Sent from Hex" -> pasted "we ship friday\nSent from Hex"
│   -> History and Paste Last hold the same two lines
├── Text already ends with the line
│   └── it is the signature, or follows a line break -> left alone
├── Empty text, empty signature -> nothing appended
├── Raw (Dictate without processing), printed, API, meeting, typed text, Annotate -> unaffected
├── Paste Last of a signed text -> standalone too, even after the lab is off
└── The next dictation -> standalone: never continues the signature line
```

Checks: `the_signature_is_appended_once_on_its_own_line`,
`off_appends_nothing`, `empty_text_appends_nothing`,
`an_empty_or_whitespace_signature_appends_nothing`,
`text_that_already_ends_with_the_signature_is_left_alone`,
`a_signature_that_ends_a_line_of_prose_is_still_appended`,
`a_signature_after_a_line_break_is_left_alone`,
`a_saved_signature_is_trimmed_before_it_is_appended`,
`a_signature_must_be_one_line_of_at_most_the_limit`, and
`a_signature_refuses_line_and_control_characters` in
[dictation/signature.rs](../../src/dictation/signature.rs);
`a_signed_dictation_reinserts_standalone` and
`an_unsigned_dictation_reinserts_as_a_continuation` in
[dictation/paste_last.rs](../../src/dictation/paste_last.rs);
`paste_last_reinserts_a_signed_dictation_standalone` in
[parakeet.rs](../../src/parakeet.rs), macOS; and
`signature_is_its_own_atomic_and_carries_its_text` in
[labs.rs](../../src/labs.rs); `a_signature_is_trimmed_and_must_be_one_line` in
[control/settings.rs](../../src/control/settings.rs);
`an_invalid_signature_loads_the_default_and_keeps_its_switch` and
`an_empty_signature_stays_empty_and_appends_nothing` in
[persistence/tests.rs](../../src/app_settings/persistence/tests.rs). Nothing
pastes into a real app here; the three call sites are gated by the same pure
rule and their platform tests, and CI's clippy builds compile macOS and
Windows. Not yet observed: a real dictation with the lab on, and the next
dictation's continuation after a signed paste.

Spoken formatting (`spoken-formatting`, issue 630, every OS): while on,
`Profiles::process_cancellable` in
[dictation/processor.rs](../../src/dictation/processor.rs) reads
`labs::spoken_formatting()` as each dictation's processing starts and runs
`dictation::spoken_formatting::apply` before Global corrections, mode corrections,
OpenCode rewriting, and transformations on every OS. Whole phrases, ignoring
case and Spanish accents, are
replaced: "new line", "line break", "nueva línea", and "salto de línea" by
one line break, "new paragraph" and "nuevo párrafo" by two, "open quote",
"abrir comillas", and "abre comillas" by an opening `"`, and "close quote",
"cerrar comillas", and "cierra comillas" by a closing one. A whole-word
"quote" with a later "unquote" in the same dictation and words between them
becomes the same pair of marks ("He said quote hello unquote." becomes
`He said "hello".`, #1167); "quote" with no "unquote" after it, a lone
"unquote", "quotes"/"quoted"/"unquoted", and "quote unquote" said back to
back stay as they are. The commas,
periods, and spaces the model puts around a command go with it ("Hello. New
line. Next" becomes "Hello.\nNext"). Words such as "newline", "new liner",
"lineal", or "comillas" alone stay as they are. English and Spanish phrases
are recognized whatever the Language setting is (#1040); other languages
are not covered yet. Dictate without processing, meetings, and live text are
unchanged, and with the toggle off processing is exactly as before. Checks:
the tests in [dictation/spoken_formatting.rs](../../src/dictation/spoken_formatting.rs);
`spoken_formatting_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs);
`contributed_options_are_labs_toggles_off_by_default` in
[panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real
dictation with the toggle on; the Labs page goldens need regenerating on
Linux with the new row.

Spoken symbols (`spoken-symbols`, issue 678, every OS): while on,
`Profiles::process_cancellable` in
[dictation/processor.rs](../../src/dictation/processor.rs) reads
`labs::spoken_symbols()` as each dictation's processing starts and runs
`dictation::spoken_symbols::apply` right after spoken formatting and before filler
words, the mode's corrections, OpenCode rewriting, and transformations.
Whole English or Spanish words, ignoring case and Spanish accents, become
symbols joined to their neighbours: "hashtag" and "hash sign" become `#` and
"at sign" and "arroba" `@`, joined to the next word ("hashtag release" →
"#release"); "underscore" and "guion bajo" `_`, "hyphen" and "guion" `-`,
and "slash" `/` join both words ("my underscore variable" → "my_variable");
"comma" and "coma" `,`, "colon" and "dos puntos" `:`, and "semicolon" and
"punto y coma" `;` join the previous word and leave one space before the
next ("done comma thanks" → "done, thanks"). The longer Spanish phrase wins
("punto y coma" is `;`, not "punto y" + `,`). The commas, periods, and
spaces the model puts around a spoken symbol go with it. Words such as
"commas", "comas", "guiones", "hashtags", "slashed", or "colonel" stay as
they are. Other languages are not covered yet (#1040), and the word list is fixed (no per-user symbol
sets yet). Dictate without processing, meetings, and live text are
unchanged, and with the toggle off processing is exactly as before. Checks:
the tests in [dictation/spoken_symbols.rs](../../src/dictation/spoken_symbols.rs);
`spoken_symbols_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs);
`contributed_options_are_labs_toggles_off_by_default` in
[panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real
dictation with the toggle on; the Labs page goldens need regenerating on
Linux with the new row.

Remove filler words (`filler-words`, issue 631, every OS): while on,
`Profiles::process_cancellable` in
[dictation/processor.rs](../../src/dictation/processor.rs) reads
`labs::filler_words()` as each dictation's processing starts and runs
`dictation::filler_words::strip_if` right after spoken formatting and before
the mode's corrections, OpenCode rewriting, and transformations. It applies
only when that dictation's language is exactly English
(`filler_words::applies_to`): Auto and every other language are left
untouched, because "er" and "um" are ordinary German and Portuguese words.
The language comes from the submission, not the setting in force while the
text is processed: macOS reads the saved transcription selection when the
dictation is submitted, while Linux and Windows read the selection the
transcription engine holds at submission, falling back to the saved selection
when none is held. On Linux and Windows, a dictation finished while an
in-place model reload for a language change is still running uses the previous
language for this check. It is rule-based, with
no model and no added latency. The English fillers "um", "umm", "uh", "uhh",
"erm", "er", and "ah" are removed as whole words, lowercase or capitalised
only, so "umbrella", "ahead", "error", "Uhura", "uh-huh", and "ER" stay. The
comma or ellipsis that comes with a filler goes too, the spaces around it
close up, no space is left before punctuation, a capitalised filler that
started a sentence passes its capital to the next word ("Um, so I think"
becomes "So I think"), and line breaks stay. Dictate without processing,
meetings, and live text are unchanged, and with the toggle off processing is
exactly as before. Checks: `only_english_dictations_apply_the_filter`,
`enabled_english_dictations_strip_their_fillers`,
`every_other_language_or_a_disabled_switch_keeps_the_words`,
`fillers_leave_with_their_commas_and_spaces`,
`a_filler_that_started_a_sentence_hands_on_its_capital`,
`line_breaks_are_kept`, and `other_words_are_left_alone` in
[dictation/filler_words.rs](../../src/dictation/filler_words.rs);
`filler_words_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs);
`contributed_options_are_labs_toggles_off_by_default` in
[panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real
dictation with the toggle on; the Labs page goldens need regenerating on
Linux with the new row.

Dictation style (`dictation-style`, every OS, off by default): while on,
`Profiles::process_cancellable_with` in
[dictation/processor.rs](../../src/dictation/processor.rs) reads
`labs::dictation_style()` (the switch and `labs.dictation_style_choice`,
one of `polished`, `casual`, `notes`, or `verbatim`, Polished by
default) as each dictation's processing starts, and
[dictation/style.rs](../../src/dictation/style.rs) writes the text after the
Global and mode corrections and after any Apple Intelligence cleanup,
OpenCode rewrite, or voice-prompt rewrite, and before the selected
transformations. The styled text is therefore `Processed.text` when the
transformations run, and transformations still have the last word. Only a
heard dictation (`Source::Dictation`) is styled: Labs › Transform typed
text, the local API's `process=global` (which promises Global's
replacement, correction, rewrite, and transformation steps, not a dictation
style), meetings, and live text keep their text exactly. Dictate without
processing skips the processor entirely. With the toggle off, processing is
byte-for-byte as before.

The rules are the iPhone app's `TranscriptFormatter` (`ios/HEXShared/`),
ported without its spoken lists, spoken punctuation, insertion spacing, or
"End with a period" handling, which desktop already has as its own Labs or
does not have. Verbatim only collapses runs of spaces and tabs and trims the
ends. Polished trims fillers, writes the English informal contractions
("gonna", "wanna", "gotta", "kinda", "lemme") out except inside quotes or on
a name mid-sentence, capitalizes each sentence, writes the English pronoun
"I", and ends every line with "." or, after Chinese or Japanese, "。" (never
a second mark, and never after a lone address or a line that already ends a
sentence). Casual trims fillers, drops an opening English interjection's
comma ("Yeah, sounds good" becomes "yeah sounds good"), lowercases sentence
starts except acronyms, words with inner capitals, and addresses ("OK"
becomes "ok"), writes English "I" as "i", and drops the final full stop on a
short message (one line, up to three sentences). Notes trims fillers, turns
spoken "new line", "new paragraph", and "bullet"/"bullet point"/"next bullet"
into lines, makes several sentences a list, splits a short "Intro: a, b,
and c" into a colon line and bullets (English also splits the last "and" or
"or"), drops an opening English connective ("so", "yeah", "and then", and
the rest), removes a final full stop, and capitalizes the first letter.

Filler trimming follows the dictation's language as the iPhone app's
patterns do: English, Spanish, German, and Norwegian/Danish/Swedish have
their own hesitations, and any other language trims nothing ("er" is "is" in
Norwegian, "um" is "a" in Portuguese, and "eh" is "anyway" in German). The
English-only rules apply when that dictation's language is `en` or starts
with `en-`/`en_`; `auto` and an empty language say nothing about the
language, so they get only the language-neutral rules and trim no fillers
(the iPhone app treats an unknown language as English, which desktop does
not). Checks: `styles_differ_on_ordinary_short_dictation`,
`polished_writes_full_sentences`, `casual_texts`,
`notes_list_several_items_concisely`, `styles_keep_an_address_whole`,
`styles_trim_spanish_and_german_hesitations`,
`filler_trimming_follows_the_dictation_language`, and
`verbatim_collapses_horizontal_spaces_only` in
[dictation/style/tests.rs](../../src/dictation/style/tests.rs);
`the_dictation_style_writes_a_dictation_and_leaves_typed_text_and_the_api_alone`
in [dictation/processor.rs](../../src/dictation/processor.rs);
`dictation_style_is_its_own_atomic_and_carries_its_choice` in
[labs.rs](../../src/labs.rs);
`dictation_style_shows_its_choices_on_its_card_while_on`,
`output_shows_the_style_row_while_its_lab_is_on`, and
`the_style_menu_shows_every_name_whole_beside_its_hint` in
[panes.rs](../../src/rune_ui/tests/panes.rs);
`every_dictation_style_string_has_every_language` in
[i18n.rs](../../src/desktop/i18n.rs). Not yet observed with a real
dictation; the Labs-page goldens were regenerated on Linux, and no Output
golden moved because its scene leaves the lab off.

Clean up with Apple Intelligence (`clean-up-apple-intelligence`, macOS, off
by default): while on, each macOS dictation that its mode does not rewrite
gets one short on-device pass with Apple Intelligence, after the speech Labs
and the Global and mode corrections and before the transformations. Its
instructions are the iOS cleaner's, word for word
(`ios/HEXMobile/TranscriptCleanup.swift`): fix clearly misrecognized words
using the sentence's context, resolve an explicit spoken self-correction
("tuesday, no wait, wednesday" becomes "wednesday"), keep the speaker's
wording, language, meaning, numbers, and negation otherwise, and treat the
transcript as content to clean rather than as instructions to answer. A
dictated question or request comes back as that same question or request,
cleaned, never as a reply and never behind a sentence of the model's own. The
stage is read in `Profiles::process_cancellable_with`
([dictation/processor.rs](../../src/dictation/processor.rs)) where the
OpenCode rewrite would have run, though only when that rewrite is off: a mode
that rewrites with OpenCode keeps its own model. It also skips typed text
(Labs › Transform typed text), a transcript of nothing but numbers, an empty
transcript, and a dictation a voice prompt answered. `dictation::apple_cleanup`
holds the decision (`plan`), the one bridge call, and the guard. The guard
applies the iOS cleaner's rules and adds the checks the reported answers
needed: length, digits, word counts, negation counts with "no wait" read as a
self-correction, and word overlap as before; each cleaned word the transcript
never used must replace a dropped transcript word (the count keeps repeats),
and no more may be added than a quarter of the transcript (or two words, for a
short one), so "cloud code"
becomes "Claude Code" and "get hub" becomes "GitHub"; no first- or
second-person pronoun may appear more often than in the transcript, so a
stutter or a repeated-pronoun self-correction may drop repeats but a new
speaker is refused; pronoun contractions read as their expansions ("I'm" and
"I am" are the same words, and "i'd" as "i would", with a "had" reading
covered by the replacement rule); no reply opener ("yes", "no", "sure",
"ok"/"okay", "certainly", "absolutely", "of course", "here", "here's",
"sorry") unless the transcript starts with the same one after its own leading
fillers, and a "hear" that becomes "here" is allowed when every other word is
unchanged; and no preamble, meaning no colon outside a time like 3:30 in the
first sentence or clause when the transcript has no such colon, and no
"here's the", "here is the", "cleaned transcript", or "cleaned text" the
transcript did not say. A cleanup that only drops words may keep less than
half the transcript only when the transcript says a self-correction marker
("no wait", "wait", "i mean", "actually", "scratch that", "sorry", or
"rather"), the cleaned text is the transcript's opening words followed by
every word after the last marker, and the words removed before that marker are
no longer than that tail plus two; otherwise it is a silent truncation and
falls back. One that rewrites must still keep half. Counting differences
remain: a length is counted in scalars here and in grapheme clusters on iOS;
the CJK numerals `Character.isNumber` sees but Rust's `is_numeric` does not
are read as digits here; and the colon exception ignores a colon between two
digits as each platform reads them, so a CJK numeral around a colon (`三:四`)
counts on iOS but not here. The first clause ends at `.`, `!`, `?`, `;`
or a line feed. Swift also ends it at a `\r\n` pair as one character; Rust
ends at its `\n` and leaves the `\r` in the clause, which does not change
the colon check. The guard returns the reason (empty
text, much longer or many more words than the transcript, a changed number, an
added or dropped negation, added words the speaker did not say, a changed
speaker, a reply opener, a preamble, text about something else, or most of the
transcript dropped) and that exact reason is what History and the live log
record. The bridge
(`apple_intelligence::generate_text` → `hex_apple_intelligence_text`) runs
the plain-text responder at utility priority with a 2 s budget, temperature 0.2,
and 1024 response tokens; the answer is trimmed, loses one pair of wrapping
quotes only when the transcript didn't dictate its own, takes back the
transcript's leading and trailing whitespace (spoken formatting's line breaks),
and is judged by the guard. Whatever happens the dictation pastes: an
unavailable model (checked from the cached availability first, so it costs no
wait, and never read while the lab is off), a cancelled job, a timeout, a
refusal or guardrail block, an error, or an answer the guard refuses all keep
the corrected text. A long dictation can spend the whole 2 s budget and paste
uncleaned. With the lab off, or Apple
Intelligence unavailable, the observation stays absent exactly as before; a
pass that ran records `profile: "Clean up with Apple Intelligence"` with its
latency in the dictation's NDJSON `processing` field and in History's
Processing line, and a pass that fell back records the reason as `fallback`
("fell back: …" in History). The card shows the same reason Summaries shows
when this Mac can't run Apple Intelligence, and only locks the switch while it
is off, so it can always be turned back off again (an organization's Labs lock
keeps the managed row and its words instead). Dictate without processing
never reaches this stage on macOS: its captures skip the processor entirely.
The local API's `POST /transcriptions` with `process=global` (Labs › Process
API transcripts) runs the same shared pipeline on this Mac
(`local_api::processing::profiles_processor`), so a processed API transcript
gets this pass too while the lab is on; its cancellation flag stops the stage
before the bridge call, the same flag the pipeline's other stages read.
Checks: `misheard_words_and_spoken_self_corrections_are_accepted`,
`punctuation_and_case_changes_are_accepted`, `digit_changes_are_rejected`,
`a_lost_negation_is_rejected`, `an_added_negation_is_rejected`,
`runaway_length_is_rejected`, `an_empty_answer_or_transcript_is_rejected`,
`an_answer_with_little_overlap_is_rejected`,
`an_answered_request_is_rejected`,
`a_replied_to_sentence_is_rejected`,
`a_preamble_before_a_long_transcript_is_rejected`,
`a_preamble_phrase_the_transcript_never_said_is_rejected`,
`an_added_colon_in_the_first_clause_is_rejected`,
`words_the_speaker_never_said_are_rejected`,
`a_pure_insertion_is_rejected`,
`misheard_and_replaced_words_pass_the_added_word_rule`,
`stutters_and_repeated_pronoun_self_corrections_are_accepted`,
`a_self_correction_marker_explains_a_short_cleanup`,
`a_truncation_without_a_self_correction_is_rejected`,
`replaced_words_count_with_multiplicity`,
`pronoun_contractions_compare_as_their_expansions`,
`a_had_contraction_still_passes_as_its_expansion`,
`a_changed_person_is_rejected`,
`a_self_correction_may_drop_a_pronoun`,
`an_opener_the_speaker_said_is_kept`,
`fillers_and_ok_are_not_replies`,
`hear_became_here_with_everything_else_equal`,
`a_time_colon_is_not_a_preamble`,
`the_instructions_are_the_ios_cleaners_word_for_word`,
`only_a_dictation_with_words_to_clean_is_cleaned`,
`a_stage_that_cannot_run_changes_nothing`,
`a_cancelled_stage_starts_nothing`,
`quotes_go_only_when_the_transcript_did_not_dictate_them`,
`the_transcripts_own_whitespace_comes_back`,
`cjk_numerals_count_as_digits`, and
`words_keep_their_apostrophes` in
[dictation/apple_cleanup.rs](../../src/dictation/apple_cleanup.rs);
`a_plain_text_answer_reads_its_text` in
[apple_intelligence.rs](../../src/apple_intelligence.rs);
`typed_text_never_reaches_the_dictation_cleanup` and
`a_numbers_only_dictation_skips_the_cleanup` in
[processor.rs](../../src/dictation/processor.rs);
`clean_up_is_its_own_atomic_and_macos_only` in [labs.rs](../../src/labs.rs);
and `contributed_options_are_labs_toggles_off_by_default`,
`clean_up_says_why_apple_intelligence_cannot_run`, and
`a_managed_lock_holds_the_cleanup_lab_off` in
[panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real
dictation on a Mac with the lab on and Apple Intelligence available, and any
real-Mac run of the new Swift bridge — this host and CI compile it, but no one
has dictated through it yet. The Labs page goldens were regenerated on Linux
with the new row.

Neutral voice (`neutral-voice`, macOS, off by default): while on, a mode that
turns on its own **Neutral voice** switch gets each of its dictations
rewritten on this Mac into plain, standard, neutral wording with the same
meaning, after the speech Labs and the Global and mode corrections and before
the transformations. The point is privacy: word choice, sentence structure,
filler, and punctuation habits identify a person, so the pasted text should
carry none of them. The stage replaces both the mode's OpenCode rewrite and
Labs › Clean up with Apple Intelligence: a mode with Neutral voice on never
sends its dictation to OpenCode, whatever its rewrite switch says, and also
outranks Labs › Rewrite by voice, which is an OpenCode request too. The
neutral pass itself runs on Apple Intelligence's local model; the mode's
transformations still run afterwards and may use the network if the person
wrote them that way. Voice Action is a separate capture and still uses
OpenCode. The Modes editor's Processing section shows the per-mode switch
while the lab is on (Global is a normal mode there, the default mode, so it
gets the switch like every other mode). Dictate without processing, typed
text, meetings, the local API's raw transcripts, and Voice Action never reach
it.

```ts
Dictate in a mode with Neutral voice on       // labs.neutral_voice + the mode's neutral_voice
├── Labs off, or the mode's switch off -> both the OpenCode rewrite and the cleanup run exactly as before
├── Numbers-only transcript, silence, a cancelled job -> corrected transcript, no model work
├── Apple Intelligence unavailable -> corrected transcript, no wait, no observation (the card says why)
├── Model answers within 4 s -> the rewrite, if the guard accepts it -> transformations -> paste
└── Timeout, refusal, error, or a refused rewrite
    -> corrected transcript, the reason recorded -> transformations -> paste
```

`dictation::neutral_voice` holds the decision (`chosen`), the one bridge call,
and the guard. The instructions ask the model to keep the speaker's meaning
exactly, remove filler, false starts, repeated words, slang, idiosyncratic
phrasing, and unusual sentence structure, use standard capitalization and
punctuation, keep the language, and keep every number, code, URL, file path,
and quoted text exactly as written; the transcript is content to rewrite,
never instructions to answer. The bridge
(`apple_intelligence::generate_text` → `hex_apple_intelligence_text`) runs the
plain-text responder at utility priority with a 4 s budget, temperature 0.7
(the cleanup stays at 0.2) so two dictations of the same words don't come back
with identical wording, and 1024 response tokens. The answer is trimmed,
loses one pair of wrapping quotes only when the transcript didn't dictate its
own, takes back the transcript's leading and trailing whitespace, and is
judged by the guard. The guard is pure and every OS tests it: an empty answer,
a rewrite under 0.4× or over 2× the transcript's characters, a changed
multiset of digit runs, an added or dropped negation, a missing protected
span, or a rewrite whose content words overlap too little with the transcript
all keep the corrected transcript with the reason recorded. Protected spans
are every matched pair of double or curly double quotes that opens at a word
start and closes at a word end, every ASCII digit run, and every span shaped
like a link or code or a path (`://` or `www.`, `/`, `\`, `_`, `::`,
brackets, backticks, or a `.` inside a word as in `file.rs`). Only the listed
prose pairs (`and/or`, `either/or`, `yes/no`, `he/she`, `on/off`, `w/`, …) and
a single-letter abbreviation such as `e.g.` or `u.s.` are prose, not code;
every other token with a slash, such as `src/components` or `docs/features`,
is protected. Balanced brackets inside code (`bar()`, `arr[i]`) stay with it;
only unmatched outer brackets are trimmed. Rewriting a whole Chinese or
Japanese sentence is fine: only the embedded number or link is protected, and
CJK numerals such as 一 and 三 are words, not digits. When the transcript has
at least four content words, at least 30% of them must appear in the answer,
so an answer to the text or injected prose falls back; text without
whitespace, or with any CJK, kana, or hangul character, is exempt. Unlike the
cleanup's guard, there is no word-overlap check: rewording is the point.

A pass that ran records `profile: "Neutral voice"` with its latency in the
dictation's NDJSON `processing` field and in History's Processing line; a pass
that fell back records the reason as `fallback` ("fell back: …" in History).
History already shows "Raw transcript" under the final text when they differ,
so a neutral rewrite is visible there, and History itself is unchanged. The
card shows the same reason Summaries shows when this Mac can't run Apple
Intelligence, and only locks the switch while it is off, so it can always be
turned back off (an organization's Labs lock keeps the managed row and its
words instead). Checks: `a_reworded_sentence_is_accepted`,
`a_rewrite_that_keeps_the_protected_words_is_accepted`,
`a_dropped_link_file_or_quote_is_rejected`, `a_changed_number_is_rejected`,
`a_lost_or_added_negation_is_rejected`,
`a_runaway_or_empty_answer_is_rejected`,
`a_short_answer_inside_the_length_window_is_accepted`,
`only_heard_speech_in_an_opted_in_mode_takes_the_pass`,
`quotes_go_only_when_the_transcript_did_not_dictate_them`,
`the_transcripts_own_whitespace_comes_back`,
`an_unmatched_quote_protects_nothing`,
`only_a_quote_at_word_boundaries_pairs`,
`a_curly_quote_pair_protects_its_text`,
`a_failed_close_leaves_the_quote_open`,
`quotes_pair_only_with_their_own_kind`,
`brackets_around_a_path_are_not_part_of_it`,
`balanced_brackets_stay_in_the_token`,
`common_abbreviations_are_not_code`,
`a_slash_between_words_is_not_a_path`,
`folder_paths_stay_protected`,
`a_chinese_rewrite_keeps_or_drops_its_ascii_number`,
`cjk_text_skips_the_content_floor`,
`a_moved_number_is_accepted`,
`an_unrelated_answer_is_rejected_but_a_rewording_is_accepted`,
`protected_tokens_keep_code_and_quotes`,
`a_cancelled_stage_starts_nothing`,
`a_mode_switch_alone_never_reaches_the_stage` (Linux and Windows), and
`a_stage_with_the_lab_off_changes_nothing` in
[dictation/neutral_voice.rs](../../src/dictation/neutral_voice.rs);
`only_a_mode_that_asks_for_neutral_voice_carries_it` in
[processor.rs](../../src/dictation/processor.rs);
`numbers_and_number_words_are_numbers` in
[dictation/numbers.rs](../../src/dictation/numbers.rs);
`a_mode_without_neutral_voice_loads_it_off`,
`a_mode_keeps_its_neutral_voice_choice`, and
`an_off_mode_serializes_without_neutral_voice` in
[dictation_processing.rs](../../src/app_settings/dictation_processing.rs);
`neutral_voice_is_its_own_atomic_and_macos_only` in [labs.rs](../../src/labs.rs);
`neutral_voice_says_why_apple_intelligence_cannot_run` in
[tests/panes.rs](../../src/rune_ui/tests/panes.rs); and
`neutral_voice_shows_in_modes_only_with_its_lab_and_toggles_the_mode`
(macOS) in [tests.rs](../../src/rune_ui/tests.rs).
Not yet observed: a real dictation on a Mac with the lab on and Apple
Intelligence available, and any real-Mac run of the new Swift bridge
parameter — this host and CI compile it, but no one has dictated through it.
The Labs page goldens were regenerated on Linux with the new card.

Second dictation shortcut (`second-dictation-shortcut`, issue 627, macOS, Linux
and Windows): for a Globe/Fn shortcut on the laptop keyboard and another key on
an external keyboard. While on, General shows a "Second dictation shortcut"
recorder under Shortcut, unbound by default and clearable
(`second_dictation_hotkey`, also `hex settings set second_dictation_hotkey
f5`). It is refused when it shares a key with another Hex shortcut, and the
others' checks count it while the toggle is on. The gate is
`AppSettings::active_second_dictation_hotkey`: with the toggle on, a binding
saved, no overlap with another active shortcut, and, on Linux and Windows, a
key. On macOS `RuntimeHotkeys::second_dictation` carries it, so the event tap
suppresses its keys and the listener (`recognition::Hotkeys`) runs a second
shortcut machine with the dictation shortcut's Hold/Toggle, double-tap lock,
and double-tap-only settings. On Linux it joins the listener's
`HotkeyOptions` as its own `Shortcut`, so an X11 grab or a shortcuts-portal
entry (id `second-dictation`, description "Second dictation shortcut")
registers it only while on and bound; the desktop may grant a different
trigger, and Settings shows what it granted. On Windows the listener's
keyboard hook registers it as its own trigger and down flag
(`CallbackState::second_dictation`), so it starts, locks, double-taps, and
finishes a capture exactly as the dictation shortcut does, and the capture's
`DictationFlavour::Second` keeps it from the main, raw, or Terminal paste
machine. Linux and Windows register a key, so a binding without one never runs
there; Windows Settings refuses one without a key, the same rule Dictate
without processing and Terminal paste follow. On every OS it dictates and
pastes with processing, as the dictation shortcut does; Paste last and meeting
paste still come only from the main machine. The shortcut that starts a
capture finishes or cancels it, and the other stands down until its keys lift
while it records. Turning the toggle off or clearing the binding mid-capture
cancels that capture on macOS; on Linux and Windows a settings change applies
once the capture ends. Off, nothing is matched, suppressed, registered, or
checked for it, and Hex is unchanged. Checks:
`the_second_dictation_shortcut_needs_its_lab_and_its_own_keys` in
[app_settings.rs](../../src/app_settings.rs),
`the_shortcut_that_starts_a_capture_owns_it` in
[recognition.rs](../../src/recognition.rs) (macOS only),
`second_dictation_shortcut_owns_the_capture_it_starts` in
[linux/input.rs](../../src/linux/input.rs),
`second_dictation_is_registered_only_while_its_lab_is_on` in
[linux/dictation.rs](../../src/linux/dictation.rs), and the Windows
`the_second_dictation_flavour_owns_its_capture` (cross-compiled only). Not yet observed: a
real Mac with an external keyboard, a real Linux X11 or Wayland shortcut press,
or the recorder row's rendering; the Labs page goldens show the new row counted
on Linux, macOS, and Windows.

More dictation shortcuts (`more-dictation-shortcuts`, macOS): up to four
more shortcuts that each dictate like the main one, each with its own
activation mode, for a keyboard with more keys to spare. While on, General
shows one numbered row per saved extra under the second dictation shortcut's:
its recorder, a shortcut-mode select with the dictation shortcut's choices
(Hold or double-tap, Hold, Tap, Double-tap; Double-tap is left out while the
row's binding has no key, unless Labs › Double-tap Option, Command, or Globe is on,
exactly as it is for the main shortcut), and
a Remove button that drops the row. An Add shortcut row appears while fewer
than four rows exist, and a new row starts unbound. The rows persist as
`extra_dictation_hotkeys`, an array of `{"binding": ..., "mode": ...}` rows
in `settings.json` (also `hex settings set extra_dictation_hotkeys …`),
capped at four: loading keeps the first four and folds each row's mode to
one its binding can trigger. The gate is
`AppSettings::active_extra_dictation_hotkey`: only on macOS, with the toggle
on, a binding saved, and no overlap with another active shortcut does
`RuntimeHotkeys::extra_dictation` carry that row, so the event tap
suppresses its keys and the listener (`recognition::Hotkeys`) runs the row's
own shortcut machine with the switches its mode saves
(`ExtraDictationMode::switches`). Each row follows Labs › Double-tap on a
lone modifier exactly as the dictation shortcut does: with it on, a row on a
lone modifier keeps Double-tap, and the machine's own
`labs::double_tap_modifiers()` check admits it; off, the row folds to Hold or
double-tap as before, and turning the lab off folds the saved mode while the
dictation shortcut's `double_tap_only` is cleared. It dictates and pastes with processing, as
the dictation shortcut does; Paste last still comes only
from the main machine. The shortcut that starts a capture owns it, and the
others stand down until its keys lift while it records. Escape cancels a
recording row once it is locked, in Tap mode, or while a bare key or mouse
button is held, Toggle's cancel binding included; while a Hold row on a
modifier or chord binding is physically held, Escape carries those modifiers
and neither cancels nor is swallowed. Removing the row that records cancels
its capture while removing another row leaves it recording. Turning the
toggle off or clearing the binding mid-capture cancels that capture. Off,
the saved rows stay and nothing is matched, suppressed, or checked for them.
Checks:
`an_extra_shortcut_needs_its_lab_and_its_own_keys`,
`active_rows_do_not_share_a_key`,
`adding_stops_at_the_maximum_and_removing_frees_a_place`,
`extra_rows_round_trip_and_survive_their_lab_turning_off`,
`loading_truncates_and_folds_the_rows`,
`double_tap_folds_away_without_a_key_or_the_lab` and
`a_rows_double_tap_follows_the_lone_modifier_lab` in
[app_settings/extra_shortcuts.rs](../../src/app_settings/extra_shortcuts.rs);
`a_row_needs_a_key_or_the_lone_modifier_lab` in
[rune_ui/shortcut_mode.rs](../../src/rune_ui/shortcut_mode.rs);
`each_extra_shortcut_keeps_its_own_mode`,
`an_extra_shortcut_owns_the_capture_it_starts`,
`removing_another_row_keeps_a_recording_extra` and
`escape_cancels_a_locked_extra_that_records` in
[recognition.rs](../../src/recognition.rs); and
`an_extra_capture_keeps_its_cancel_from_the_app` and
`an_extra_mode_sets_its_trigger_switches` in
[suppression/tests.rs](../../src/suppression/tests.rs) (macOS only).
`extra_dictation_hotkeys_takes_up_to_four_checked_rows` covers
`hex settings set` in
[control/settings.rs](../../src/control/settings.rs). Not yet observed on a
real Mac: the recorder rows, the mode select, Add shortcut and Remove, each
row's own mode, and the Labs card (macOS builds only; no golden).

Terminal paste shortcut (`terminal-paste-shortcut`, Windows and Linux):
another dictation shortcut whose paste is always Ctrl+Shift+V, for a terminal
window that needs it, while the main dictation shortcut keeps following the
global Terminal paste setting. While on, General shows a "Terminal paste
shortcut" recorder under Shortcut, unbound by default and clearable
(`terminal_paste_hotkey`, also `hex settings set terminal_paste_hotkey
ctrl+shift+t`). It is refused when it shares a key with another Hex shortcut,
and the others' checks count it while the toggle is on. The gate is
`AppSettings::active_terminal_paste_hotkey`: only on Windows and Linux, with
the toggle on, a binding saved, and no overlap is it handed to the listener,
so X11 grabs or the shortcuts portal register it (id `terminal-paste`), and
the Windows hook matches it. Linux has no `hex` command for it, unlike
Dictate without processing's `hex dictation start|toggle --raw`, so without
X11 grabs or the portal it stays unregistered. Off, nothing is registered,
matched, or checked for it, and the saved binding stays. A capture it starts
is a normal dictation in every other way: the same Hold/Toggle and double-tap
lock, modes processing, History kind Dictation, last-dictation update, and
Escape, cancel, and short-tap discards. The shortcut that starts a capture
owns it:
the main dictation and Dictate without processing shortcuts neither finish it
nor pair a double tap with it, and its flavour survives the post-release tail
to the paste. Paste last dictation keeps the global Terminal paste setting.
On Windows, `javaw`/`java` still get plain Ctrl+V, as Terminal paste does.
Checks: `the_terminal_paste_shortcut_needs_its_lab_and_its_own_keys` in
[app_settings.rs](../../src/app_settings.rs),
`terminal_paste_shortcut_owns_the_capture_it_starts` in
[linux/input.rs](../../src/linux/input.rs), and the Windows
`a_second_dictation_shortcut_owns_the_capture_it_starts` in
[windows/input.rs](../../src/windows/input.rs) (cross-compiled only). Not yet
observed: a real terminal on either OS; the Windows tests have not run on a
Windows PC. The Labs page goldens (`windows-labs`, `linux-labs`) show the new
card.

Caps Lock dictation (`caps-lock-dictation`, macOS): while on, a physical Caps
Lock press toggles a hands-free dictation. From idle it starts a capture that
stays recording without a hold; the next Caps Lock press finishes it and hands
it to the normal transcription, processing, and paste flow. A hold capture,
a pending double tap, or another shortcut's recording is left alone. Escape
cancels the Caps Lock capture in every shortcut mode; Toggle mode's configured
cancel shortcut does not, only Escape does for a Caps Lock capture. The normal
dictation shortcut keeps working when nothing records (while a Caps Lock
capture runs it stands down, as a second shortcut does). Turning the lab off
mid-capture cancels that capture.

Caps Lock never changes typing: every press also resets the system Caps Lock
state off through `IOHIDSetModifierLockState`, so the next keys stay
lowercase. The key's light follows Hex's dictation state through the
keyboard's HID LED output element (usage page 0x08, usage 0x02). It comes on
only after the capture's microphone delivers its first audio, so a press whose
microphone is still opening (with Release microphone while idle, or a device
switch) leaves the light as it was. Once live, it is on while any capture
records, including a normal-shortcut or Annotate capture, blinking while an
accepted job transcribes or processes, and off when nothing is left or the
capture is discarded, cancelled, or failed. The listener builds each chunk's
meter once and feeds both the HUD (when one exists) and the light, so the light
never depends on a HUD; an Annotate capture feeds the light alone. The
recording light is steady by default; its Recording light option, Slow blink,
blinks at 700 ms on and off while recording, while transcribing keeps its
350 ms blink. Turning the lab off or quitting Hex clears the light and stops
forcing the lock off, so Caps Lock behaves normally again.

```ts
Labs › Caps Lock dictation on                        // labs.caps_lock_dictation
├── Caps Lock press, idle -> hands-free capture starts (no hold, no short-tap discard)
├── Caps Lock press, that capture running -> finishes and pastes normally
├── Hold capture / pending double tap / another shortcut recording -> ignored
├── Escape while it records -> cancels (the tap keeps Escape from the app)
├── Toggle mode's other cancel shortcut -> does not touch a Caps Lock capture
├── software Caps Lock -> off after every press; typing is unchanged
├── light -> stays off, or keeps blinking a pending job, until the microphone delivers audio
├── Recording light -> steady (default) solid recording, or slow blink at 700 ms on/off; transcribing always blinks at 350 ms
├── light -> off done/cancelled/failed
├── Labs off mid-capture -> cancels it, light off, normal Caps Lock returns
└── Hex quits -> light off, lock released
```

The gate is `labs::caps_lock_dictation()` in the listener
([recognition.rs](../../src/recognition.rs)), which starts and stops the
feature's worker each pass. Detection needs both halves of a physical press:
the raw HID tracker records Caps Lock usage 0x39 with its press timestamp
([suppression/physical_keys.rs](../../src/suppression/physical_keys.rs)), and
the event tap confirms it with an AlphaShift change edge within 300 ms
([suppression.rs](../../src/suppression.rs)). A key remapped to Control or
Escape never changes AlphaShift, so it never toggles dictation. Hex's own
lock-reset edge is marked and dropped before the press gate. A Caps Lock activation
delay configured above 300 ms would make those presses invisible; the default
has no delay. Without Input Monitoring the HID tracker is inactive, so an AlphaShift on edge is the
fallback press and off edges remain resets. Confirmed presses reach the
listener with the HID timestamp, before any IOKit work. The light model stays
on the listener and publishes its pattern through an atomic; the worker
([caps_lock_dictation/macos.rs](../../src/caps_lock_dictation/macos.rs)) owns
every IOKit call, re-enumerates keyboards on a bounded refresh, and runs the
blink timer. No IOKit work runs on the event-tap or audio threads. The pure
press gate, press decision, and light model live in
[caps_lock_dictation.rs](../../src/caps_lock_dictation.rs). The LED needs
Input Monitoring, which Hex already asks for; without it, dictation still
works and the light stays under macOS's control. Some keyboards, built-in ones
included, may not expose a Caps Lock LED output element, and macOS's own light
update after a reset can race Hex's write; neither changes dictation or the
lock reset. Checks:
`an_older_jobs_stage_event_never_interrupts_a_newer_capture`,
`a_stage_event_for_an_unknown_job_changes_nothing`,
`the_light_is_solid_recording_blinking_working_and_off_after`,
`the_light_waits_for_the_captures_first_audio`,
`a_meter_without_a_capture_turns_nothing_on`,
`ending_a_waiting_capture_clears_the_wait`,
`a_waiting_capture_keeps_earlier_jobs_blinking`,
`the_slow_blink_setting_only_changes_the_recording_light`,
`the_light_waits_for_every_accepted_job`,
`cancelling_or_failing_a_capture_turns_the_light_off`,
`a_hid_press_matches_an_edge_timestamp_from_before_the_callback`,
`an_edge_that_arrives_before_the_hid_press_still_matches`,
`a_fast_double_press_yields_two_presses`,
`a_hid_press_without_an_alpha_shift_edge_never_presses`,
`without_the_hid_tracker_an_on_edge_is_the_fallback_press`, and
`the_light_pattern_codes_round_trip` in
[caps_lock_dictation.rs](../../src/caps_lock_dictation.rs);
`meter_reports_rms_and_peak` and `a_dc_offset_never_reads_as_loudness` in
[indicator.rs](../../src/dictation/indicator.rs);
`a_meter_reaches_the_light_without_a_hud`,
`escape_cancels_a_caps_lock_capture_in_hold_mode`,
`caps_lock_does_not_start_over_an_annotate_recording`,
`the_dictation_shortcut_stands_down_while_caps_lock_records`,
`turning_the_caps_lock_lab_off_stops_its_capture`,
`caps_lock_never_starts_over_a_pending_double_tap`, and
`a_caps_lock_capture_keeps_escape_from_the_app_in_any_mode` in
[recognition.rs](../../src/recognition.rs) and
[suppression/tests.rs](../../src/suppression/tests.rs);
`caps_lock_dictation_is_its_own_bit_and_macos_only` and
`caps_lock_light_carries_its_slow_blink_choice` in
[labs.rs](../../src/labs.rs);
`contributed_options_are_labs_toggles_off_by_default` and
`caps_lock_light_shows_its_choices_on_its_card_while_on` in
[panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real Mac
with the lab on — no hardware run of the Caps Lock LED, the lock reset, the
blink, Escape cancellation, or the off/quit cleanup. The `mac-labs` and
`mac-labs-link` goldens were regenerated on Linux with the new row.

Smart spacing (`smart-spacing`, issue 632, macOS): while on, a dictation
pasted right after typed text gets one leading space, so words don't merge
("alreadyin progress"). `paste::Paster` reads `labs::smart_spacing()` as each
paste commits, only when no continuation join applies (a join already spaces
contiguous dictations) and not for Copy dictation only. It asks the
frontmost app's focused element through Accessibility
(`accessibility::char_before_caret`: `AXSelectedTextRange`, then
`AXStringForRange` for the one character before the cursor, surrogate-pair
aware, each call bounded by the 250 ms messaging timeout) and adds the space
when `paste::needs_leading_space` agrees: after a letter, digit, `. , ; : ! ?
…`, or a closing bracket or typographic quote (`) ] } ” ’ »`); never after
whitespace, opening punctuation, straight quotes, or symbols such as `@ $ # /
-`, and never before text that starts with whitespace or closing punctuation.
Selected text, a cursor at the start, or an app that doesn't expose its
cursor leaves the text unchanged, and both the clipboard paste and the
Chromium 155/156 typing path use the adjusted text. Hex's own window is
never asked: its tree belongs to the main thread that updates it (#1402).
Lowercasing the first letter is not done. Off, pasting is exactly as before.
Checks:
`smart_spacing_adds_a_space_only_where_one_belongs` in
[paste.rs](../../src/paste.rs) (macOS only);
`smart_spacing_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: which apps (Chrome, Safari,
Slack, VS Code, Notes) expose the cursor, or a real paste with the toggle
on; the row renders only in macOS builds, so no golden changes.

Left and right keys (`sided-modifier-chords`, issue 710, macOS): while
on, the shortcut recorder (`ChordCapture` in
[rune_ui/macos_host.rs](../../src/rune_ui/macos_host.rs)) records a
modifier-only chord of two or more keys with the side each modifier was
pressed on, so Right ⌘ + Right ⌥ saves as `right-option+right-command` and
the tray shows R⌥ R⌘; only those keys press it, and the left keys don't.
The gate is the recorder: `labs::sided_modifier_chords()` is read once when
a recording starts. Matching, the conflict checks, and saved shortcuts are
unchanged (chords already store and match sides; `hex settings set` could
always save one), a chord with a non-modifier key still records its
modifiers from either side, and the side picker stays for one-modifier
shortcuts only. Off, a chord such as ⌥⌘ records from either side exactly as
before. Checks: `with_sided_chords_each_modifier_keeps_its_side` and
`without_sided_chords_two_modifiers_record_from_either_side` in
[rune_ui/macos_host.rs](../../src/rune_ui/macos_host.rs) (macOS only);
`sided_modifier_chords_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: a real recording on a Mac;
the row renders only in macOS builds, so no golden changes.

Tap or hold (`tap-or-hold`, issue 711, macOS and Linux): one dictation shortcut
works as both tap-to-toggle and push-to-talk. It acts only in Toggle mode
(`shortcut_mode: toggle`). The gate is the macOS shortcut machine
(`DictationHotkey` in [suppression/hotkey.rs](../../src/suppression/hotkey.rs)), which reads
`labs::tap_or_hold()` at each release of a Toggle press. With it on, a press
released at least `MINIMUM_HOLD_DURATION` (300 ms, the hold threshold) after
it started finishes the capture on release, as Hold does, with the usual
200 ms tail. A shorter press locks the capture as plain Toggle does, and the
next press finishes it. Chord discards and the press that finishes a lock are
unchanged. It applies to every `DictationHotkey` in Toggle mode.
Off, or in Hold mode, nothing changes.

Linux (issue 1144): the gate is the listener's shortcut options
(`shortcuts` in [linux/dictation.rs](../../src/linux/dictation.rs) sets
`HotkeyOptions::tap_or_hold` only in Toggle mode with the toggle on; a
change re-registers the shortcuts once nothing records). In
`ShortcutState` ([linux/input.rs](../../src/linux/input.rs)) a Toggle press
released at least 300 ms (`TAP_OR_HOLD_THRESHOLD`) after it went down
finishes its capture, with the usual release tail; a sooner release locks as
Toggle does. The desktop's key repeats of a held press don't move its start,
and a release the desktop drops leaves the capture recording until the next
press finishes it, as before. It applies to the dictation shortcut and
Dictate without processing, each for the capture it started; `hex dictation`
commands are unchanged. X11 counts a release after its 50 ms auto-repeat
grace. Windows doesn't have it.

Checks: `tap_or_hold_locks_a_short_tap_and_finishes_a_long_hold_on_release`
and `tap_or_hold_still_discards_a_chord_and_off_keeps_plain_toggle` in
[suppression.rs](../../src/suppression.rs) (macOS only);
`tap_or_hold_finishes_a_held_press_and_locks_a_tap`,
`without_tap_or_hold_a_held_toggle_press_locks`,
`tap_or_hold_survives_repeats_and_dropped_releases`, and
`tap_or_hold_applies_to_raw_dictation_but_not_commands` in
[linux/input.rs](../../src/linux/input.rs);
`tap_or_hold_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: a real Mac or Linux desktop
(X11 grab or Wayland portal) with the toggle on; the Linux Labs page goldens
need regenerating with the new row.

Double-tap Option, Command, or Globe (`double-tap-modifiers`, macOS): Double-tap
shortcut mode works with any dictation shortcut that has no key: one or more
modifiers, such as Option, Globe, or ⌃⌥. The gate is the shortcut machine
(`DictationHotkey` in [suppression/hotkey.rs](../../src/suppression/hotkey.rs)),
which asks `labs::double_tap_modifiers()` as the mode is applied; with it on,
a shortcut without a key keeps `double_tap_only` instead of the mode falling
back to Hold or double-tap. The runtime gate,
`app_settings::double_tap_only()` (filled by `apply_runtime`), also accepts a
shortcut without a key while the lab is on. The state machine is the existing
one: the first complete tap arms the window silently; one tap or a hold
starts nothing; two complete taps inside 300 ms lock a hands-free capture; a
fresh press finishes it; Escape still reaches the app. For a shortcut without
a key, a press held 300 ms or longer is a hold, not a tap, so keeping the
modifier down and then tapping never locks. A key, mouse button, or a
modifier or side the binding doesn't cover is a chord and the machine goes
Dirty, as before: Option, Command, Option is a chord, and so is Left Option
between Right Option taps. The binding's own keys may go down and up one at a
time, so ⌃ then ⌥ (or one Option side then the other) never counts as a
different modifier. Modifier flag events are never suppressed, so a lone
Option or Globe press and release reach the focused app unchanged; macOS's
own "press Globe twice" system dictation shortcut may therefore also fire
while this lab is on. It applies to every `DictationHotkey` that takes the
mode (the primary shortcut, Dictate without processing, the second dictation
shortcut, and each Labs › More dictation shortcuts row). Off, the persisted
normalization clears `double_tap_only` for a shortcut without a key exactly
as a fresh load does, Double-tap is dimmed in the Shortcut mode modal, and
`hex settings set double_tap_only on` refuses it. Windows and Linux have no
such path for this lab: their runtime gates and `hex settings` validation are
exactly as before, and both keep the key requirement.

Checks: `double_tap_only_with_the_lab_starts_from_a_lone_modifier` (Option
and Fn), `double_tap_only_with_the_lab_still_discards_a_lone_modifier_chord`,
`double_tap_only_with_the_lab_does_not_take_a_held_first_press_as_a_tap`,
`double_tap_only_with_the_lab_discards_a_different_modifier_between_taps`,
`double_tap_only_with_the_lab_locks_a_keyless_control_option_chord`,
`double_tap_only_with_the_lab_locks_a_both_options_chord`,
`double_tap_only_with_the_lab_discards_the_other_option_side_between_taps`,
and `without_the_lab_double_tap_only_stays_off_for_a_lone_modifier` in
[suppression/tests.rs](../../src/suppression/tests.rs) (macOS only);
`a_binding_covers_only_its_own_modifiers_and_sides` in
[app_settings/hotkeys.rs](../../src/app_settings/hotkeys.rs);
`double_tap_only_on_a_lone_modifier_needs_the_lab` and
`turning_the_double_tap_modifiers_lab_off_falls_back_as_a_fresh_load` in
[app_settings/shortcut_repair.rs](../../src/app_settings/shortcut_repair.rs);
`double_tap_only_takes_a_lone_modifier_with_the_lab` in
[control/settings.rs](../../src/control/settings.rs);
`double_tap_modifiers_is_its_own_bit_and_macos_only` in
[labs.rs](../../src/labs.rs). Not yet observed: a real double-tap on Option
or Globe on a Mac; the `mac-labs` and `mac-labs-link` goldens change with the
new row, and `windows-labs` stays as it was.

Hold through other keys (`hold-through-keys`, macOS and Windows):
while you hold a modifier-only shortcut (a binding with no key) in Hold mode,
a key or mouse button joining it a second or more after the press is left to
the app in front instead of counting as another app's chord, and the capture
keeps recording. Releasing the shortcut still finishes it, with the usual
release tail, even while that key is still down. Inside the first second the
chord discards as before, so Option-1 or Alt-Tab still interrupts a press
meant for another app. The second counts from the press that started the
capture (`started_at`, `pressed_at_ms` on Windows), not from the hold
threshold.

macOS reads the gate in the shortcut machine (`DictationHotkey` in
[suppression/hotkey.rs](../../src/suppression/hotkey.rs)), which asks
`labs::hold_through_keys()` as each chord key arrives; the extra key was
never suppressed, so it still reaches the app. Mouse clicks are unchanged
there: the hold threshold already ignores them after 300 ms. Windows reads
it in the hook (`process_button` in
[windows/input.rs](../../src/windows/input.rs), through
[windows/input/hold_through.rs](../../src/windows/input/hold_through.rs)),
where the key or mouse button that joins a long hold no longer sets the
capture dirty or interrupts it: `CaptureState::holds_through_joining_keys`
keeps the press's tick count and wraps with it. Toggle captures and
shortcuts with a key are unaffected on both OSes, meetings and the local API
are too, and Linux has no such path. Off, every chord behaves exactly as
before.

Checks: `hold_through_keys_keeps_a_long_hold_through_a_joining_key` in
[suppression/tests.rs](../../src/suppression/tests.rs) (HID and visible keys
at 500 ms, 1 s, and 5 s, the release while the key is down, and off);
`a_long_hold_keeps_recording_through_a_joining_key_or_button` and
`toggle_and_a_wrapping_tick_count_keep_the_second` in
[windows/input/hold_through.rs](../../src/windows/input/hold_through.rs)
(they need Windows to compile and run, through Check Windows);
`hold_through_keys_is_its_own_bit_and_macos_and_windows_only` in
[labs.rs](../../src/labs.rs). The regenerated goldens that changed against
`origin/dev` are `mac-labs` and `mac-labs-link` (light and dark),
`windows-labs` (light), and `whats-new-yolo` (light and dark; since replaced
by `whats-new-newest`). Not yet
observed: a real keyboard on either OS.

Sort and filter models (`picker-sort-filter`, issue 645, every OS): while
on, the speech model picker's Every model view shows one row between its
header and the list: a sort (By recommendation, the default and the catalog's order; By
size, smallest model file first with unknown sizes last; By name) and a
switch, "Only models that support <saved language>" ("Only models that
detect the language automatically" when the language is Auto), which keeps
the models whose `supports_language` accepts it. Both are view state in
`rune_ui/picker.rs`, reset each time the picker opens; they never save
settings, send a request, or change the language or the active model. When
the switch hides the model being prepared, or the active one, a line outside
the results names it and its state (its preparation label or Active), with
the preparation's Cancel. The gate is the picker itself
(`sort_filter_on`); the feature does no other work. Off, the picker is
exactly as before. Speed and accuracy sorts, and filters for English-only,
multilingual, specific languages, size, streaming, and recognition hints,
are not built yet. Checks: `recommended_keeps_the_catalogs_order_and_the_others_reorder_it`,
`the_language_toggle_keeps_only_models_for_the_saved_language`,
`a_hidden_preparing_or_active_model_is_named`, and
`browsing_never_saves_or_asks_for_a_model` in
[picker.rs](../../src/rune_ui/picker.rs). Not yet observed: the row rendered
in a real window; there is no `--snap` scene for the picker with the Lab on,
and the Labs page goldens need regenerating on Linux with the new row.

Model test details (`benchmark-details`, issue 647, every OS): while on,
each row in the speech model picker's Every model view that has a benchmark
record (`transcription::models::benchmark`; Apple Speech has none) adds one
wrapping line under its facts: the raw figure, such as "7.57% CER", then the measured language,
the dataset and split, the source as plain text (with its source-record
revision), the conditions (utterances, batch size, timestamps, hardware), and
"Speed: upstream figure; hardware and method unknown". Anything the source
didn't record reads "Dataset unknown", "Source unrecorded", "Hardware
unknown", or "Conditions unknown". Qwen3-ASR 0.6B (7.57% CER) and SenseVoice
Small (10.12% CER) carry transcribe.cpp's legacy-published Mandarin FLEURS
test records (945 utterances, batch 8, no timestamps, source revision
7045d1c; run IDs and scoring or artifact revisions unknown); the catalog's
7.64 and 10.11 stay as the alternate legacy figures the picker shows with the
toggle off. QwenCleo-ASR 1.7B names its author's 3,699-utterance Egyptian
Arabic test. ivrit.ai Whisper large-v3-turbo carries Hex's own run of
transcribe.cpp's WER harness on the FLEURS Hebrew test (18.45% WER, 792
utterances, batch 1, no timestamps, Radeon 8060S over Vulkan, source revision
7045d1c), NB-Whisper Medium Hex's run on the FLEURS Norwegian test (6.68%
WER, 357 utterances, same conditions), and Granite Speech 5.0 Hex's run on
the FLEURS English test (4.77% WER, 647 utterances, same conditions);
KB-Whisper Medium names KBLab's model card for the FLEURS Swedish test
(6.6% WER, its default checkpoint, with utterances, batch, hardware, and
scorer unrecorded); every other figure is English WER with dataset and source
unrecorded. A "Most accurate" badge stays only when its record is comparable
(`BenchmarkRecord::comparable`: same metric, language, and a known, equal
dataset) with every other recommended model's for the saved language, so
today it drops everywhere. The gate is the picker (`benchmark_details_on`);
the feature does no other work. Off, the picker is exactly as before.
Checks: `benchmark_details_show_raw_rates_and_mark_unknowns`,
`most_accurate_needs_comparable_figures_only_with_benchmark_details`, and
`benchmark_details_are_off_by_default` in
[picker.rs](../../src/rune_ui/picker.rs);
`only_figures_on_the_same_metric_language_and_dataset_compare` in
[models.rs](../../src/transcription/models.rs). Not yet observed: the rows
rendered with the toggle on; there is no `--snap` scene for it, and the Labs
page goldens need regenerating on Linux with the new row.

Use your own speech model (`custom-models`) graduated: adding a model file
and models found on this computer are always on now
([own-models.md](own-models.md)). `hex labs on|off custom-models` still
answers with that and exits 0 (`labs::GRADUATED`); the saved
`labs.custom_models` key is ignored.

Parakeet Ultra (`parakeet-ultra`, issue 648, every OS): a pinned catalog
entry, `parakeet_ultra`, listed and runnable only while the toggle is on
(`ModelDefinition::available` reads `labs::parakeet_ultra`).

```ts
Picker row -> "Parakeet Ultra" + Labs tag; 25 languages, offline, Token     // labs.parakeet-ultra
│   timestamps; v3's "Detects language from audio" note; never recommended
├── Download -> handy-computer/parakeet-ultra-gguf@a1539be1, parakeet-ultra-Q8_0.gguf,
│   740,363,168 bytes, SHA-256 007a5976…0a59 (430 F32, 225 Q8_0, 48 F16 tensors)
├── Details -> languages; peak memory measured on apollo (~0.7 GB GPU, ~0.2 GB
│   RAM, Radeon 8060S over Vulkan; not measured elsewhere); "Experimental: Hex
│   has run it only on Linux (Vulkan) so far."; the credit word for word ("Parakeet
│   Ultra by Moondream, based on NVIDIA Parakeet TDT 0.6B v3. GGUF conversion and
│   quantization by handy-computer. Creative Commons Attribution 4.0 International
│   (CC BY 4.0)."); the publisher's NOTICE; Source and License links
├── "Published benchmarks; not measured in Hex" (folded) -> conditions
│   (FLEURS test, Q8_0, CUDA, batch 8, each language's hint, no timestamps,
│   transcribe.cpp 021aa17d; file digest, dataset revision, scorer unknown), then
│   25 FLEURS WERs from transcribe.cpp 7045d1c's catalog/parakeet-ultra.json
├── Accuracy 98.2% / Labs › Model test details: LibriSpeech test-clean 1.80%
│   WER, the same published source (2,620 utterances, batch 8, CUDA)
└── Toggle off while Ultra is the dictation model -> refused (row locked,
    `hex labs off`, `hex settings set`); a saved Ultra with the toggle off some
    other way stays saved and the listener runs a downloaded fallback
```

Observed on apollo (Linux, Radeon 8060S over Vulkan, 2026-11-01): the file
matched its pinned size and SHA-256 (`hex model install` verified it); `hex
model transcribe` with Ultra, passed and saved, transcribed FLEURS German and
English and JFK correctly; Ultra ran a FLEURS English clip at 191x against
Parakeet v3's 211x in transcribe-cli; turning the toggle off while Ultra was
selected was refused. Not observed: Metal, Windows, the picker in a real window
(the `picker-parakeet-ultra` scene was rendered once, not as a golden, because
it lists Ultra only where the saved settings turn the Lab on).
Checks: `parakeet_ultra_is_offered_only_with_its_lab_and_a_saved_choice_survives`
in [models.rs](../../src/transcription/models.rs) and
`parakeet_ultras_published_scores_cover_its_languages_once` in
[model_details.rs](../../src/transcription/model_details.rs).

Special keys alone (`special-keys`, issue 767, Linux): for holding Caps Lock
to dictate on GNOME Wayland by remapping it to Menu (GNOME's "Caps Lock is
also a Menu" keyboard option), or using Pause or Scroll Lock.

```ts
hex settings set dictation_hotkey menu        // labs.special-keys
├── Toggle on (Linux) -> saved; any shortcut may use menu, pause, scrolllock,
│   alone like a function key or with modifiers (ctrl+pause)
│   ├── X11 -> grabs keysym Menu 0xff67, Pause 0xff13, Scroll_Lock 0xff14
│   └── Wayland portal -> preferred trigger Menu, Pause, Scroll_Lock
├── Toggle off -> "menu needs Labs › Special keys alone: hex labs on special-keys"
├── Windows -> Menu, Pause, Scroll Lock always allowed, alone too; the toggle
│   instead lets Insert, Home, End, Page Up, Page Down stand alone ("home alone
│   needs Labs › Special keys alone" while off), and off puts a saved one back
├── macOS -> no toggle; a bare one still needs a modifier, as before
├── Window recorder (Linux) -> records them, alone too, while on (#1187);
│   refuses them while off ("Add a modifier")
│   Check: `recorder_takes_special_keys_alone` in `rune_ui/settings.rs`
└── Toggle turned off (row, hex labs off, hex settings), or settings load with
    it off -> a Linux shortcut on one goes back to its default (Dictate without
    processing unbound), so the listener never registers it
```

The gates are `AppSettings::special_keys_allowed` in `control/settings.rs`'s
`apply` and the window's `settings::validate`, and
`AppSettings::repair_special_keys` (`app_settings/shortcut_repair.rs`) where
settings load and where the toggle turns off. Off, Hex is unchanged: the
added keysym names are reached only by a saved shortcut on one of these keys.
Checks: `menu_alone_needs_its_labs_toggle` in
[control/settings.rs](../../src/control/settings.rs) and
[rune_ui/settings.rs](../../src/rune_ui/settings.rs),
`special_keys_have_portal_and_x11_names` in
[linux/input.rs](../../src/linux/input.rs), and
`special_key_shortcuts_go_back_to_their_default_while_their_labs_toggle_is_off`
in [app_settings/shortcut_repair.rs](../../src/app_settings/shortcut_repair.rs).
Not yet observed: a real GNOME Wayland session granting Menu through the
portal, or an X11 grab of it; the Labs page goldens need regenerating on Linux
with the new row.

§ key alone (`section-key`, issue 1050, Linux and macOS): for
push-to-talk on the ISO section key, left of 1 above Tab, without a
modifier. Layouts label it § (U.S. ISO) or a dead key: on a German Mac the
same key types `^`, and many layouts use `´` or `` ` ``.

```ts
Settings › General › Shortcut, press the section key alone  // labs.section-key
├── Toggle off -> saving the recording is refused: "Add a modifier", as before
├── Toggle on  -> saved as § or the dead key's base character (^, ´, `)
│   ├── Linux: hex settings also takes `section`; Settings shows §
│   │   ├── X11 -> grabs the keycode that types XK_section 0xa7
│   │   └── Wayland portal -> preferred trigger `section`
│   └── macOS: key code 10 (kVK_ISO_Section) alone; hex settings takes `^` on
│       a German layout and resolves it by scanning the layout's base labels
├── macOS recorder -> AppKit gives no characters for a dead key, so the key
│   is named from the active layout with UCKeyTranslate and
│   kUCKeyTranslateNoDeadKeysBit: key 10 is saved as {code: 10, label: "^"};
│   the recorder takes it alone either way, and `settings::validate` refuses
│   it while the toggle is off, as Linux's recorder does
├── hex settings set dictation_hotkey §, off -> "section needs Labs › § key
│   alone: hex labs on section-key"
└── Toggle turned off (row, hex labs off, hex settings), or settings load with
    it off -> a Linux or macOS shortcut on the section key alone goes back to
    its default (Dictate without processing unbound), so the listener never
    registers it
```

While bound, Hex takes the section key from every app (an X11 grab, a portal
shortcut, or the macOS session tap holds its key), so typing it needs another
key. The window's `settings::recorded_binding` (Linux; macOS uses
`ChordCapture::key_down` in rune_ui/macos_host.rs) and `control/settings.rs`'s
`parse_hotkey` let a lone section key through on Linux and macOS; the gates are
`AppSettings::section_key_allowed` in the window's `settings::validate` and in
`control/settings.rs`'s `apply` (`special_key_refusal`), and
`AppSettings::repair_section_key` (`app_settings/shortcut_repair.rs`) where
settings load and where the toggle turns off. On macOS, matching is by key
code: `HotkeyKey::is_section` also accepts code 10 whatever its label, and
`RuntimeHotkey::matches_key_press` compares `key_code` (saved from
`HotkeyBinding::runtime()` with code 10 for a non-letter label), so
`{code: 10, label: "^"}` with Option matches and the session tap swallows it
like any keyed shortcut. Off, Hex is unchanged: a lone section key is refused
everywhere, and the added `section` keysym names are reached only by a saved
section-key shortcut (with a modifier it now also resolves on X11 and the
portal, where it was refused as an unsupported key). Windows doesn't have it.
Checks: `section_alone_needs_its_labs_toggle` in
[control/settings.rs](../../src/control/settings.rs) and
[rune_ui/settings.rs](../../src/rune_ui/settings.rs),
`a_dead_key_section_binding_needs_its_labs_toggle_on_macos` in
[rune_ui/settings.rs](../../src/rune_ui/settings.rs),
`the_section_key_has_portal_and_x11_names` in
[linux/input.rs](../../src/linux/input.rs),
`a_lone_section_key_goes_back_to_its_default_while_its_labs_toggle_is_off` and
`a_dead_key_label_is_the_section_key_only_on_macos` in
[app_settings/shortcut_repair.rs](../../src/app_settings/shortcut_repair.rs),
and `a_dead_key_records_its_layout_label` and
`the_section_key_records_alone` in
[rune_ui/macos_host.rs](../../src/rune_ui/macos_host.rs) (macOS-only tests).
Not yet observed: a real ISO keyboard on X11 or a Wayland portal granting
`section`, and a German Mac keyboard recorded end to end.

Microphone key alone (`microphone-key`, issue 181, macOS): for push-to-talk on
the microphone (dictation) key in a laptop keyboard's top row. Every press of
it is a normal keyDown/keyUp for virtual key code 176 with SecondaryFn always
set (observed flags `0x800100`), so it has no character to record and the
recorder refused it as "Unsupported key"; a binding that implied Fn would also
never match. The lab makes this one key alone a shortcut.

```ts
Settings › General › Shortcut, press the microphone key  // labs.microphone_key
├── Toggle off -> saving a shortcut on it is refused: "Unsupported key", as before
├── Toggle on  -> saved as {code: 176, label: "Mic"}; hex settings also takes
│   `mic` or `microphone`
├── macOS recorder -> ChordCapture::key_down lets it stand alone like a
│   function key; `settings::validate` refuses it with any modifiers while the
│   toggle is off
├── macOS session tap (microphone-key tap, only while this lab is on)
│   ├── Toggle on + a Hex shortcut on the key -> press and release are Hex's:
│   │   fed to the listener like any key and consumed at the session
│   │   boundary, which is where handy-keys also blocks macOS's Dictation
│   ├── Toggle on + the window records a shortcut -> press and release are
│   │   consumed and the press is fed to the window's ChordCapture, so the
│   │   recorder sees a key its own key events never get
│   ├── Toggle on + the key is the screenshot shortcut -> refused: a shot
│   │   reads its keys at the annotated session, after this tap
│   └── Toggle off, Hex paused, or no shortcut on the key -> passed through:
│       macOS's Dictation and the app keep the key untouched
├── hex settings set dictation_hotkey microphone, off -> "the mic key needs
│   Labs › Microphone key alone: hex labs on microphone-key"
└── Toggle turned off (row, hex labs off, hex settings), or settings load with
    it off -> any macOS shortcut on the microphone key, with or without a
    modifier (Dictation, Paste last, Cancel, Screenshots, capture, and the
    Labs rows), goes back to its default (optional ones unbound), so the
    listener never registers it; the session tap is removed too
```

The key joins F1–F20, the arrows, and the navigation keys, whose always-set
SecondaryFn the matcher already ignores (`key_reports_function_flag` in
[app_settings/hotkeys.rs](../../src/app_settings/hotkeys.rs)), so 176 with the
observed `0x800100` matches a `{code: 176, label: "Mic"}` binding, and Shift
added does not. Fn is never stripped globally, so it stays significant for
every other key. The gates are `AppSettings::microphone_key_allowed` in
the window's `settings::validate` and in `control/settings.rs`'s `apply`
(`special_key_refusal`), and `AppSettings::repair_microphone_key`
([app_settings/shortcut_repair.rs](../../src/app_settings/shortcut_repair.rs))
where settings load and where the toggle turns off; it resets the cancel and
screenshot shortcuts itself because `reset_shortcuts` doesn't own them. Off,
Hex is unchanged: any shortcut on the microphone key is refused everywhere, a
saved one is repaired, as with the section key, and no tap is created.
macOS matches its own Dictation shortcut (a symbolic hot key) before the app
and before Hex's annotated-session key tap, so this one key is read at a
session tap ([suppression/dictation_key.rs](../../src/suppression/dictation_key.rs))
that exists only while the lab is on: it takes a press and its release only
while the key is one of Hex's shortcuts or a shortcut is being recorded, and
hands the press to the listener or to the window's recorder through the same
`ChordCapture`. Everything else passes through, so macOS's Dictation still
works while the key is not Hex's. Whether macOS's own Dictation still opens on
a press Hex takes is unobserved on hardware: if it does, change its shortcut in
System Settings › Keyboard › Dictation. The microphone key is refused for the
Screenshots shortcuts, whose keys are read at the annotated session, after this
tap has taken the press. The decision is `MicrophoneKey::take` in
[app_settings/microphone_key.rs](../../src/app_settings/microphone_key.rs),
which remembers the one press it took so its release is taken too while that
tap is still there, even if the binding or the recording changed in between;
turning the lab off removes the tap and forgets it.
Windows and Linux don't have it.
Checks: `the_microphone_key_needs_its_labs_toggle` in
[control/settings.rs](../../src/control/settings.rs) and
[rune_ui/settings_microphone_key_tests.rs](../../src/rune_ui/settings_microphone_key_tests.rs),
`a_microphone_key_shortcut_goes_back_to_its_default_while_its_labs_toggle_is_off`
in [app_settings/shortcut_repair.rs](../../src/app_settings/shortcut_repair.rs),
`the_microphone_key_is_hexs_only_while_its_lab_wants_it` and
`a_recorded_press_keeps_its_release` in
[app_settings/microphone_key.rs](../../src/app_settings/microphone_key.rs),
`the_microphone_key_records_from_the_listener_tap` and
`the_microphone_key_records_alone` in
[rune_ui/macos_host.rs](../../src/rune_ui/macos_host.rs),
`the_microphone_key_taken_at_the_session_tap_starts_and_finishes_a_dictation`
and `the_microphone_key_matches_with_the_fn_flag_macos_adds` in
[suppression/tests.rs](../../src/suppression/tests.rs) (the rune_ui, suppression,
and tap checks are macOS-only; the microphone-key and repair checks run on
every OS). Not yet observed: a real laptop microphone key recorded end to end
and a real press suppressed before macOS's Dictation; stale-key recovery for it
(whether `CGEventSourceKeyState(176)` reports it held, and whether SecondaryFn
lingers after its release); and whether a session tap sees the key while
macOS's Dictation shortcut is on for every macOS version.

Dictation stats (`history-stats`, issue 790, every OS): while on, History's
header shows a Secondary "Stats" button beside its retention select once
anything is retained, or once the daily rollup exists. It opens a
centered "Dictation stats" dialog (Escape, a scrim press, or Close dismisses
it, and focus returns to Stats) with six headline tiles (dictations, words,
time saved, words per minute, the daily streak, and the busiest hour, each a
grouped number over a muted label), a Typing speed slider under them, a bar
chart of dictations per local day over the last 30 days with empty days left
at zero and today's bar in the accent colour, a compact 24-bar hourly strip
with the busiest hour in the accent colour, a 53-week × 7-day heatmap of
dictations per local day (four accent levels over faint empty days), the top
five apps as bars measured against the busiest one, Output's share of the text
as a meter, and each mode's Output time (`median 0.9 s · slowest 3.2 s`, the
four most run modes).
Time saved is `words / typing_wpm` minutes less recorded audio, floored at
zero (`history/stats.rs::time_saved_ms`); the slider runs from 10 to 200 words
a minute, starts at 40, saves as `typing_wpm`, and recomputes the tile from
its current value. The busiest hour is the earliest local hour on a tie, and
the tile and the strip always read the same hours: retained entries, or the
whole daily rollup while it has a day. Output time is a mode's whole
Output processing, rewrite plus any transformations that ran after it; runs
that fell back (a failure or a deadline) and runs that only applied the
person's transformations are left out. A section with nothing to show is left
out; the meter still reads 0%, and the body scrolls while the window is short
so the title and Close stay put. The dialog is built in
[rune_ui/history/stats.rs](../../src/rune_ui/history/stats.rs) with the big
number as `controls::STATS_NUMBER`; `hex history stats [--since] [--until]
[--json]` prints the same totals ([transcript-cli.md](../transcript-cli.md)),
and its JSON adds the typing speed, time saved, hourly series and busiest
hour, the daily series, per-mode Output time, and a `yearly` summary
of the rollup. It sums every entry within
retention (`History::stats`, `history/stats.rs`), not the pane's 200-entry
search, so it ends with what retention removes. The headline totals and hours
come from the rollup instead whenever it has a day (all time); streak
notifications and per-replacement counts are not built. The gates are
`labs::history_stats()`, read when the
pane reloads (off, it reads nothing for stats, and sums again when the entries,
the typing speed, or the toggle changes, or once when the dialog closes because
the stats emptied), as each entry is recorded (the rollup's write), and where
`hex history stats` starts (off, it fails naming
`hex labs on history-stats` before History is read), and the window's own
setting for the button and dialog. Closing the window, opening History again
from the menu bar or `hex window show history`, turning the switch off,
emptying retained history and the rollup, or leaving History closes the
dialog, and Command-1…9 pane shortcuts wait while it is open. Off, History is
exactly as before. The rollup is `usage.json` beside `history.json` in the
support folder: while History retention is not Off, each recorded entry adds
its local day, dictations, words, recorded milliseconds, and 24 hourly counts,
with no text and no application names. The first write seeds itself from the
retained History entries and then adds the new entry once, so the dialog's
totals don't drop when the lab turns on. The file is owner-only from its
staging file (0600; `durable::write_private` after
`crate::app_paths::create_private_dir`), atomic, keeps at most 400 days, and a
failed write is logged and ignored so capture and paste never wait on it. A
damaged or unreadable rollup is moved aside as `usage.json.corrupt` before a
fresh one starts, as History does, so a year is never silently overwritten;
display reads may still fall back to empty. History retention Off records
nothing, here or in History; Clear all history removes the rollup too, even
with no retained entries; off, the lab writes and reads nothing. The recording
site is `HistoryStore::record` (`history/usage.rs::count`). The retired Yearly
stats lab (`yearly-stats`) merged into this one: the `yearly-stats-merge`
migration reads a saved `labs.yearly_stats` into `labs.history_stats` once, so
someone who had it on keeps the heatmap and all-time totals, and a later
choice sticks. Checks: the tests in
[history/stats.rs](../../src/history/stats.rs) (words, speed, changes, time
saved and its floor, hours and the busiest hour tie, top apps, streak across a
gap, the daily series' zero days and today, Output time medians for odd and
even counts, fallback and transformation-only runs left out, the JSON's field
names, expired entries skipped past the search cap);
`history_stats_parse_sum_retained_entries_and_need_the_labs_toggle` and
`history_stats_json_includes_the_yearly_rollup_with_dictation_stats` in
[control/transcript_cli_tests.rs](../../src/control/transcript_cli_tests.rs);
rollup accumulation, bounding, damage preservation, and owner-only round trips
in [history/usage.rs](../../src/history/usage.rs); the record site, seeding,
the retention-Off skip, and clearing with no entries in
[history.rs](../../src/history.rs); the grid's Monday start, accent levels,
and single-source hours in
[rune_ui/history/heatmap.rs](../../src/rune_ui/history/heatmap.rs);
`history_stats_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs); `retired_yearly_stats_becomes_dictation_stats_once`
in [app_settings.rs](../../src/app_settings.rs); and
`history_stats_button_opens_and_closes_the_dialog` and
`history_stats_dialog_hides_sections_with_nothing_to_show` in
[rune_ui/tests.rs](../../src/rune_ui/tests.rs). `history-stats` in
[rune_ui/scenes.rs](../../src/rune_ui/scenes.rs) renders the button, dialog,
heatmap, and hourly strip in both themes, and its fixture is the worst case
(five apps, four modes, long names, a six-digit word count, a year of
heatmap). Not yet observed: the dialog in a real window, its slider dragged by
hand, and a real dictation written to `usage.json`.

Stats on History (`history-stats-strip`, every OS): while it and Dictation
stats are both on and History has stats, History draws one compact row above
its list with the dialog's six headline numbers. Both draw
`stats::headline` from `stats::headline_inputs`, so the strip and the tiles
can't drift; the Stats button and dialog are unchanged, and with the strip off
History is exactly as before. It is inserted into both History views
(`rune_ui/history.rs` and `rune_ui/history/unified.rs`) and reads no data of
its own: the gate is `labs::history_stats_strip_enabled(&settings)`, which requires
both switches, this build and OS, and no managed lock, on top of
`HistoryPane::has_stats`. Checks:
`history_stats_strip_requires_dictation_stats_and_no_policy_lock`
([labs.rs](../../src/labs.rs)),
`history_stats_strip_needs_both_labs_and_stats`,
`history_stats_strip_shows_the_dialogs_six_headline_numbers`, and
`history_stats_button_and_dialog_still_work_with_the_strip_on`
([rune_ui/tests.rs](../../src/rune_ui/tests.rs)); `history-stats-strip` in
[rune_ui/scenes.rs](../../src/rune_ui/scenes.rs) renders it in both themes. Not
yet observed in a real window.

Copy recent dictations (`recent-dictations`, issue 877, macOS): while on,
the menu-bar menu shows a Copy Recent Dictation submenu right under Paste
Last Dictation. It lists the five newest History entries within retention,
newest first, skipping printed (`hex dictation stop --print`) and empty ones
as Paste Last does, each on one line (whitespace and line breaks collapsed,
cut to 48 characters with "…"). Clicking one puts its full final text on the
clipboard as an ordinary entry; nothing is pasted. The gate is the menu's
delegate (`menuNeedsUpdate:` in
[macos_rune/status_item.rs](../../src/macos_rune/status_item.rs)): History is
read only as the menu opens and only with `labs::recent_dictations()` on;
off, with History retention Off, or with nothing to list, the item stays
hidden and the menu is exactly as before. The copy itself
(`StatusItemAction::CopyRecentDictation`) looks the entry up by ID again and
logs a warning if it has gone. Checks:
`recent_lists_the_newest_reusable_entries_within_retention` in
[history.rs](../../src/history.rs);
`recent_dictations_show_on_one_short_line` in
[macos_rune/status_item.rs](../../src/macos_rune/status_item.rs) (macOS
only); `recent_dictations_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: the submenu on a real Mac;
the row renders only in macOS builds, so no golden changes.

Skip long pauses (`skip-pauses`, issue 899, Linux and Windows): for slow
CPUs and older integrated GPUs, where a dictation is transcribed as a whole
clip (no stream, or the fallback after a failed or late stream) and every
second of silence goes through the model. While on, the engine's whole-clip
run (`TranscriptionEngine::finish` in
[transcription/engine.rs](../../src/transcription/engine.rs)) reads
`labs::skip_pauses()` as it starts and transcribes the clip as a dictation
stream hears it (`skip_long_pauses` in
[engine/streaming.rs](../../src/transcription/engine/streaming.rs), the same
`PauseCap`): each pause at the noise floor keeps its first 2.5 s and last
0.3 s, a trailing pause only its first 2.5 s, and shorter pauses pass whole.
Unified English's trailing silence is appended after that cut, so the trailing
pause cannot trim it.
A clip in which no frame rose above the noise floor is left whole, as the
stream does for #841. The job deadline, History's duration, and the silence
discard still use the full clip; process.log adds a `skipped long pauses
before whole-clip transcription` line with `pauses_dropped_ms`. macOS,
streams, meetings, captures, and the local API are unchanged, and off, the
pause cut leaves the clip untouched. Checks:
`skipping_pauses_cuts_a_whole_clips_long_pauses_to_their_ends`,
`skipping_pauses_leaves_short_pauses_and_quiet_only_clips_whole`, and
`with_skip_pauses_off_the_whole_clip_is_untouched` in
[transcription/engine.rs](../../src/transcription/engine.rs);
`skip_pauses_is_its_own_bit_and_off_by_default` in [labs.rs](../../src/labs.rs).
Not yet observed: a real dictation with the toggle on in Hex (the reporter
measured their own branch on an i5-8259U); the Labs page goldens need
regenerating on Linux with the new row.

Fix results in Output (`output-fixes`, issue 989, every OS): for a Try
result in Settings › Output that comes out wrong. While on, the Try card
adds a row under the sentence with a Vocabulary button, which opens the same
Vocabulary sheet as General's Vocabulary row, and, once a sentence is typed,
Fix this, which copies Edit with agent's prompt ending `What I want: When I
say "<sentence>", Hex pastes "<result>". It should paste: ` for the person to
finish in their agent; it reads Copied until the sentence changes or the pane
is left. The result is what the Try card shows (corrections and built-in
steps; scripts and rewriting run only when dictating). Nothing is saved:
the agent changes the steps or vocabulary as for Edit with agent. The gate is
the Try card (`try_card` in [rune_ui/output.rs](../../src/rune_ui/output.rs)
reads `labs.output_fixes`), and both actions check it again. Off, the pane is
exactly as before. Checks:
`output_fixes_show_only_with_their_lab_and_fix_this_copies_for_the_sentence`
in [rune_ui/tests.rs](../../src/rune_ui/tests.rs) and
`fix_this_ends_edit_with_agents_prompt_with_the_sentence_and_its_result` in
[rune_ui/output.rs](../../src/rune_ui/output.rs). Not yet observed: the row
in a real window; the Labs page goldens need regenerating on Linux with the
new row (the Output goldens keep the toggle off and are unchanged).

Use model files where they are (`model-in-place`, issue 941, every OS): for
one speech model file shared by several accounts. While on, the speech model
picker's Every model header adds "Use model file where it is…" beside "Add
model file…"; the chosen GGUF is checked, hashed once, and recorded with its
path (`custom_models::add_in_place`), never copied
([own-models.md](own-models.md#use-a-model-file-where-it-is-labs)). The gate
is `model_import::in_place_on`, read when the button draws and again when the
dialog answers, before the worker starts. Off, the picker is exactly as
before. Checks:
`a_file_added_in_place_is_recorded_by_path_copied_nowhere_and_forgetting_leaves_it`,
`hex_s_own_copy_chosen_through_another_path_is_never_deleted`, and
`an_independent_identical_file_replaces_hex_s_copy`
in [transcription/custom_models.rs](../../src/transcription/custom_models.rs);
`every_feature_is_off_by_default_and_round_trips` in
[labs.rs](../../src/labs.rs). Not yet observed: a real window or a second
macOS account; the Labs page goldens need regenerating on Linux with the new
row.

Smaller model files (`k-quant-models`, macOS): for an imported speech model
quantized smaller than Q8_0. While on, "Add model file…" and "Use model file
where it is…" accept Q6_K, Q5_K, and Q4_K tensors too, through the one header
check (`custom_models::profile`, given the types this build can run:
`gguf_header::TensorTypes`). The import's manifest records `k_quants`, and a
saved choice of one while the lab is off stays saved while the listener runs
the default model, with `hex status` saying "its Labs toggle is off"
(`FallbackReason::LabsOff`, the path Parakeet Ultra uses); `hex model use` on
one is refused and names `hex labs on k-quant-models`. Off, the rule and its
message are exactly F32, F16, and Q8_0. Linux and Windows never run these
files: their Vulkan builds embed shaders only for the catalog's types, so the
lab is not listed there and the check refuses the file either way. The tensor
sizes the header check uses come from ggml's block sizes (256 values in 144,
176, and 210 bytes), so a cut-short file is still refused. Checks:
`k_quants_are_known_only_where_the_caller_runs_them`,
`k_quant_tensors_are_measured_in_their_ggml_block_sizes` in
[gguf_header.rs](../../src/transcription/gguf_header.rs),
`a_k_quant_import_needs_the_lab_and_falls_back_without_it` in
[custom_models.rs](../../src/transcription/custom_models.rs), and
`smaller_model_files_is_its_own_switch_and_macos_only` in
[labs.rs](../../src/labs.rs). Not yet observed: a real K-quant file transcribed
on Metal (macOS), or the row in a real window.

Downloaded models only (`picker-downloaded`, issue 1018, every OS): for
seeing which speech models take disk space and deleting unused ones. While
on, the speech model picker's All speech models view shows one row under its
header (below Sort and filter models' row when that Lab is on too): "Downloaded
models take <size>", the sum of the pinned sizes of every downloaded catalog
model and import whose file is in Hex's own models folder (files another app
downloaded, and imports added where they are, aren't counted), and a switch,
"Only downloaded models". The switch keeps only the rows the picker reads as
downloaded (catalog models, imports, and found catalog models in use) and
leaves out "On this computer"; each keeps its usual Delete (Forget for a file
used in place) with its confirmation. It is view state in
`rune_ui/picker.rs`, reset each time the picker opens; it saves nothing and
sends no request. When it hides the model being downloaded, a line outside
the results names it with its progress and Cancel. The gate is the picker
(`downloaded_on` in `shown_models`, the row, and `list::body`); off, the
picker is exactly as before. Not built: a button that shows the models folder
in the file manager, or choosing another folder for models. Checks:
`only_downloaded_models_keeps_the_installed_ones`,
`the_downloaded_total_adds_up_hexs_own_files`, and
`without_downloaded_models_only_the_picker_is_unchanged` in
[picker.rs](../../src/rune_ui/picker.rs);
`every_feature_is_off_by_default_and_round_trips` in
[labs.rs](../../src/labs.rs). Not yet observed: the row in a real window; the
Labs page goldens need regenerating on Linux with the new row.

Keep recordings (`keep-recordings`, issue 1032, every OS): for hearing a
dictation again, through the output device or a different model.

```ts
Labs › Keep recordings on                     // labs.keep-recordings
├── Dictation saved to History -> its 16 kHz mono clip is written as a 16-bit
│   WAV, history-audio/<id>.wav beside history.json (folder 0700, file 0600)
│   ├── The gate: `Recording::keep`, read as each capture goes to the engine
│   │   (parakeet::submit on macOS, submit_capture on Windows and Linux)
│   ├── History Off, the local API's dictations -> no entry, no audio
│   └── Entries from before the toggle was on -> no audio
├── Retention, the 10,000-entry and text caps, Delete, Clear all -> the
│   entry's WAV is deleted at the next save (`recordings::sweep`)
├── Over 512 MiB of audio -> the oldest entries lose their audio, keep their text
├── History card, entry with audio -> "Saved recording" + Play + "Transcribe again" menu
│   ├── Play -> the clip through the default output device, on a worker; one
│   │   playback at a time: Play on another entry, Stop, selecting another
│   │   entry, leaving History, turning the lab off, or closing the window
│   │   stops it. Stop shows while it plays and Play returns when it ends or
│   │   fails (history/playback.rs)
│   ├── Failed -> "Could not play: <why>" (the file is gone, no output device)
│   ├── Lists every available model installed for the language it would use
│   ├── A model that takes a language hint (not Parakeet v3/Ultra, GigaAM,
│   │   Granite, Bark) -> "<model> · Language: <saved language>" menu (Auto when it
│   │   detects, then its languages by name) + Transcribe; the language applies
│   │   to that run only, never to settings (#1068)
│   ├── Choose one -> "Transcribing with <model>…" on a worker: its own
│   │   transcription service, sharing the dictation engine's model when it is
│   │   the one chosen, yielding to dictation; otherwise loads a copy for the run
│   ├── Done -> "Transcribed again with <model>", the text, Copy, Replace
│   │   └── Replace -> the entry's text becomes it (processing record dropped),
│   │       with its model and chosen language; the audio stays; the old text
│   │       becomes the newest of at most 5 "Earlier versions" (#1177)
│   ├── Earlier versions -> each with its model, language, and age, Copy and
│   │   "Use this version" (swaps it back; the current text becomes a version).
│   │   Stored in the entry: they count toward the 16 MiB text cap and leave
│   │   with it (retention, Delete, Clear all); hidden while the toggle is off
│   └── Failed -> "Could not transcribe again: <why>"
└── Toggle off -> nothing new is saved and the menu and Play hide; existing
    audio stays until it expires or History is cleared
```

`hex history --json` names each kept recording's absolute path in `recording`
while the lab is on and the file is still there; every other row omits the
field. `hex history` reads the saved `labs.keep_recordings` switch with
`AppSettings::read` instead of the process's runtime switches, which that
read-only command never sets; a managed lock and the build's audience keep it
off as `labs::encode` does. Turning the lab off, leaving the entry or History,
or closing the window stops playback; closing stops it directly in `on_close`,
since a hidden window only polls when woken. Off, History and dictation are
exactly as before: no copy of the clip is made. Checks:
`a_kept_recording_is_an_owner_only_wav_that_reads_back`,
`recordings_leave_with_their_entries`,
`the_recordings_byte_cap_drops_the_oldest_audio_but_keeps_the_text`,
`a_recording_another_process_has_not_saved_yet_survives_a_sweep`,
`replacing_the_text_keeps_the_entry_and_its_recording`, and
`retranscribing_a_missing_recording_says_so` in
[history.rs](../../src/history.rs);
`a_kept_recording_decodes_into_its_own_clip` and
`a_worker_answer_returns_the_card_to_play_and_keeps_a_failure` in
[history/playback.rs](../../src/history/playback.rs);
`keep_recordings_follows_the_saved_switch_not_the_process` and
`history_json_names_a_recording_only_where_one_is_kept` in
[transcript_cli_tests.rs](../../src/control/transcript_cli_tests.rs);
`transcribe_again_shows_only_with_its_lab_and_replace_updates_the_entry`,
`transcribe_again_offers_a_language_only_for_models_that_take_one`, and
`leaving_an_entry_or_turning_off_keep_recordings_stops_playback` in
[tests.rs](../../src/rune_ui/tests.rs);
`keep_recordings_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs); the tests in
[history/versions.rs](../../src/history/versions.rs) (newest first, five at
most, switch back, text cap, Clear all). Not yet observed: a real dictation kept
and played or transcribed again with another model in a window, on any OS.

Copy dictation audio (`copy-dictation-audio`, every OS, off by default): the
kept WAV as a file on the clipboard, ready to attach in an app that takes
files. It needs Keep recordings, and it stores no audio of its own: it only
copies a file that lab already wrote. Both switches must be on for anything
to show.

```ts
Labs › Copy dictation audio on, Keep recordings on   // labs.copy-dictation-audio
├── History card, entry with a kept recording -> "Saved recording" gains Copy audio beside Play
│   ├── Copy audio -> the entry's history-audio/<id>.wav goes on the clipboard as one file,
│   │   on a worker like Transcribe again, so the window never waits:
│   │   macOS an NSURL through Scrawl's handoff::copy_file_to_clipboard, Windows CF_HDROP
│   │   through win::clipboard::set_file, Linux text/uri-list through the process's
│   │   one LinuxPaster (linux::paste::with_paster, wl-copy on Wayland, the X11 selection)
│   │   ├── While it runs -> "Copying…"; Done -> "Copied" (Copy's own check clears)
│   │   └── Failed (file gone, clipboard refused) -> "Could not copy the audio: <why>"
│   ├── No kept recording (Keep recordings off, or the entry predates it) -> nothing to press
│   └── The lab off, or Keep recordings off -> History is exactly as before
├── Menu bar (macOS) and the Linux and Windows tray -> Copy last dictation audio
│   ├── The newest History entry within retention that kept its audio and still has its file
│   │   (HistoryStore::newest_recording(now); a missing or expired entry is skipped, not copied)
│   ├── Listed only while the lab and Keep recordings are on, dimmed while there is nothing to copy
│   └── Failed -> logged, as the menu's other copy actions do; the status menu and tray have
│       no notice to show, so nothing appears there
└── It never records, keeps, or deletes audio: Keep recordings owns every file
```

Checks: `the_newest_kept_recording_skips_entries_without_audio_or_without_the_file`
in [history.rs](../../src/history.rs), which covers a file deleted behind the
store, retention's cutoff, and an expired entry;
`copy_dictation_audio_is_its_own_switch_and_everywhere` in
[labs.rs](../../src/labs.rs); `copy_audio_shows_only_with_its_lab_and_keep_recordings`
in [tests.rs](../../src/rune_ui/tests.rs); the tray menu's ids and the dimmed
`copy-last-dictation-audio` item in
[tray_menu.rs](../../src/desktop/tray_menu.rs); the status menu's row order in
[macos_rune/status_item.rs](../../src/macos_rune/status_item.rs). Not yet
observed: a file pasted into a real app from History, the macOS menu bar, or
the Linux or Windows tray; the Labs page goldens are regenerated with the new
History row.

Hide menu bar icon (`hide-menu-bar-icon`, issue 1064, macOS): for people
who only use the shortcut and want the menu-bar space back. The status item
stays installed and is only hidden (`NSStatusItem` `setVisible`), so
dictation, the shortcuts, and the listener are unchanged. The gate,
`labs::hide_menu_bar_icon()`, is read when the item is installed and on each
menu-bar refresh (`status_item::update_meeting`, about twice a second), so
turning it on or off applies at once. A meeting starting, recording, or
transcribing brings the icon back, so a recording stays visible and
stoppable from the menu bar, and it hides again once the meeting ends; an
Annotate capture keeps its own capsule and doesn't bring it back. With the
Dock icon off too, a launch stays quiet, and opening Hex again from Finder
or Spotlight (the reopen Apple event) opens Settings, which has Quit Hex.
Off, the icon is exactly as before. Checks:
`a_hidden_icon_comes_back_while_a_meeting_is_under_way` in
[macos_rune/status_item.rs](../../src/macos_rune/status_item.rs) (macOS
only); `hide_menu_bar_icon_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: the icon hiding on a real
Mac, or macOS restoring an item's remembered visibility; the row renders
only in macOS builds, so no golden changes.

Upcoming meetings in menu bar (`calendar-menu`, macOS, with Meetings on and a
calendar connected): the status menu starts with an Upcoming section while
the lab is on — a disabled "Upcoming" header, up to five meetings from the
same filtered list the reminders read, soonest first, and a disabled "No
upcoming meetings" when nothing is left — then a separator. Each row is its
local start–end time and title (`10:30–11:00  Design sync`), prefixed
`Now · ` while the meeting is in progress and `Tomorrow ` or its short
weekday when it starts on a later local day; ended meetings are left out.
Choosing a row with a call link sends the occurrence to the meeting
controller's `ControllerCommand::JoinFromMenu`, the menu's own join path
([meeting/watcher.rs](../../src/meeting/watcher.rs)): it opens the link,
records that occurrence as already reminded so its reminder never pops, and
sends `ReminderWithdrawn` when that reminder is currently showing so its
capsule closes. A moved meeting is a new occurrence and may remind again. The
click itself starts no recording, though a provider set to Always record may
still record once the call is detected. A row without a link is informational
and disabled. The gate, `labs::calendar_menu()`, is read when the meeting
controller decides to read its calendar, on the status item's 500 ms refresh,
and as the menu opens.

The meeting controller's reminder poll publishes what it reads
(`calendar_menu::publish`) and the status item only formats that snapshot on
the main thread, so EventKit is never touched there. The read stays one
bounded query each minute or on a store change; while the lab is on, a
connected calendar is also read with Reminders switched off, since the section
does not depend on them. That menu-only read never becomes the controller's
titling and reminder list, so a detected call is not named from the calendar
while reminders are off; with reminders on the read behaves exactly as
before. Turning the lab on, or turning reminders back on after a menu-only
read, reads at once instead of waiting for the next minute. With no calendar
connected nothing is read for the menu. The snapshot
keeps at most 32 meetings, and the section hides when no calendar is
connected, when Meetings are off, and when the snapshot is over 10 minutes
old (a stopped controller); without access the read fails and clears it, so
nothing shows until access is granted. Clearing an already empty snapshot
takes only a read lock, so the lab on with Meetings off costs no write per
loop. Off, the menu is identical to today, with every Upcoming row hidden.

Checks: `an_in_progress_meeting_reads_now`,
`meetings_on_later_days_read_tomorrow_or_their_weekday`,
`ended_meetings_are_left_out`, `at_most_five_meetings_show_soonest_first`,
`an_empty_section_has_no_rows`, `a_stale_snapshot_never_shows`,
`publishing_keeps_at_most_32_and_none_clears`,
`the_section_hides_until_visible_and_fresh`,
`the_menu_read_wants_a_connected_calendar`, and
`a_local_date_parses_the_iso_date_part` in
[calendar_menu.rs](../../src/calendar_menu.rs);
`joining_elsewhere_before_the_lead_time_never_reminds` and
`joining_elsewhere_while_showing_withdraws_it_for_good` in
[calendar.rs](../../src/calendar.rs);
`joining_from_the_menu_withdraws_its_reminder_and_starts_nothing`,
`only_reminders_keep_the_titling_and_reminder_list`, and
`a_menu_only_read_and_a_new_lab_both_force_an_immediate_read` in
[meeting/watcher.rs](../../src/meeting/watcher.rs);
`calendar_menu_is_its_own_switch_and_macos_only` in
[labs.rs](../../src/labs.rs); and
`the_menu_reads_upcoming_then_meetings_then_actions_then_the_app` in
[macos_rune/status_item.rs](../../src/macos_rune/status_item.rs) (macOS only).
Not yet observed: the section in a real Mac's menu bar, a real EventKit read
reaching it, a click opening Zoom or Meet, and the menu with calendar access
denied. The rows render only in macOS builds, so no golden changes.

Select all calendars (`calendar-select-all`, macOS, with Meetings on): off by
default, and off the Calendars row is exactly as before. On, Meetings ›
Settings › Calendar puts one small secondary button beside the Calendars
select. While any calendar is left out it reads "Select all" and clears
`meetings.calendar.excluded_calendars`, so every calendar is included; while
every calendar is included it reads "Deselect all" and sets that list to the
IDs in `ui.platform.calendar.calendars` as shown now. The setting stores
exclusions, so a calendar added later is still included. The button hides
while there are no calendars, and each calendar's own switch in the menu stays
as it is. The lab is UI only: it has no runtime gate or state-word bit, and the
row reads `Feature::CalendarSelectAll` with `shipped`, no organization lock,
and `on`, as `in_place_on` in
[model_import.rs](../../src/rune_ui/model_import.rs) reads its lab. A save goes
through `ui.update`, like the per-calendar toggle. Off, nothing is drawn.

Checks: `calendar_select_all_is_its_own_switch_and_macos_only` in
[labs.rs](../../src/labs.rs) and
`the_select_all_calendars_lab_saves_every_calendar_at_once` in
[rune_ui/tests/panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed:
the button on a real Mac. The row renders only in macOS builds, and the lab is
off by default, so no golden changes.

Pause from menu bar (`pause-menu`, issue 82, macOS): for people whose game or
another app collides with Hex's shortcuts and who don't want to quit. With the
lab on, the menu bar's action block starts with Pause Hex; pressing it pauses
Hex and the row reads Resume Hex. A pause is runtime-only and never saved, so a
restart starts unpaused. While paused every capture shortcut is inert:
dictation (main, second, extra, Caps Lock, and raw), Paste Last Dictation,
Annotate and Screenshots, and the Carbon hotkeys registered under
Secure Event Input. The event tap passes every key event through unmodified,
exactly as while a new binding is recorded, because every suspension site reads
`app_settings::shortcuts_suspended()` (a recording binding or a pause) instead
of `hotkey_capture_active()` alone. Pausing cancels only an active dictation
capture: the listener suspends every shortcut machine as the pause begins
(`Hotkeys::suspend_when`, once per suspension) and cancels the capture it was
recording, while accepted transcription and paste jobs still finish and a
capture a command starts during the same pause keeps recording. An Annotate or
Screenshots recording already under way is not cancelled by a pause: it keeps
recording and stops from its own capsule, while only its shortcuts go inert. A
Caps Lock dictation ignores the key while shortcuts are suspended, so a pause
neither starts a dictation nor forces Caps Lock off. Hiding the menu bar icon
(Labs › Hide menu bar icon) leaves no Resume Hex row at all, so with the icon
hidden turning the lab off is what resumes.

```ts
Paused     -> every capture shortcut inert; tap passes keys through
           -> the listener cancels only the dictation capture it is recording
           -> Caps Lock starts nothing and the lock stays as it is
Resume     -> the row still reads Resume Hex: shortcuts stay suspended until
              no key or modifier reads as held for 100 ms -> live again
Lab off    -> resumes at once and the row hides; a restart starts unpaused
```

Choosing Resume Hex again while a resume already waits leaves it waiting; it is
never re-paused or restarted.

Resuming does not re-arm shortcuts on a held keyboard: the listener's
`shortcut_pause::tick` settles the pause on each pass and only goes live after
100 ms of a neutral keyboard (`STALE_KEY_NEUTRAL_DURATION`'s window in
[src/suppression.rs](../../src/suppression.rs)), so a key held while the user
clicks Resume can never start a capture. That check,
`suppression::keyboard_is_neutral`, reads the HID modifier flags and every
keyboard key code below 128 through `CGEventSourceKeyState` (the combined
session state and HID state must agree); Caps Lock's own key code is skipped
because macOS reports it down while it is merely toggled on, and the Fn flag is
masked out because arrow-key metadata can leave SecondaryFn set after release
(the pause has no modifier history to tell that apart from a held Fn, so it
always ignores the bit), the same way `recover_stale_keys_with` drops it once
its own history says Fn is up. Only a pending resume reads the keyboard and
takes the pause machine's lock (once); a running or paused Hex reads a lock-free
atomic mirror instead. A pending resume logs one warning naming the held key
code after five seconds, and the pause, resume request, and live-again
transitions log at info. The gate, `labs::pause_menu()`, is read when the status
item refreshes (about twice a second) and as the menu opens; `labs::apply`
resumes at once when the lab turns off. Explicit CLI commands (`hex dictation
start`) and the menu's own actions are not keyboard shortcuts and keep working,
though a capture `hex dictation start` begins during a pause can only be
finished with `hex dictation stop`: its reply still offers the shortcut, which
is inert until Resume.
Checks: `pause_suspends_and_resume_only_follows_a_neutral_keyboard`,
`a_neutral_keyboard_past_the_window_resumes`,
`choosing_resume_while_a_resume_waits_leaves_it_waiting`,
`a_pending_resume_reports_how_long_it_has_waited`,
`turning_the_lab_off_resumes_at_once`, `settling_leaves_a_running_hex_alone`, and
`a_resume_ignored_while_running_does_not_suspend` in
[shortcut_pause.rs](../../src/shortcut_pause.rs);
`a_pause_cancels_once_and_leaves_a_command_capture_alone` in
[recognition/hotkeys.rs](../../src/recognition/hotkeys.rs);
`the_pause_row_reads_pause_or_resume` and
`the_menu_reads_upcoming_then_meetings_then_actions_then_the_app` in
[macos_rune/status_item.rs](../../src/macos_rune/status_item.rs);
`pause_menu_is_its_own_bit_and_macos_only` in [labs.rs](../../src/labs.rs);
`a_toggled_caps_lock_still_reads_neutral`, `a_held_key_reads_not_neutral`,
`a_held_modifier_reads_not_neutral`, and
`a_stuck_function_flag_still_reads_neutral` in
[suppression/tests.rs](../../src/suppression/tests.rs).
Not yet observed: a real pause and resume on a Mac, especially against a held
modifier and a game's colliding key; the suppression, secure-input, listener,
and status-item changes are compiled and clippy-linted by the macOS CI build but
no permission-granted keyboard test ran. The pause machine's checks run on
Linux too, while the tap, secure-input, listener, and menu tests, and the
neutral-keyboard rules, run only in macOS builds. The row renders only in macOS
builds, so no golden changes.

Notch indicator (`notch-indicator`, macOS only): for MacBooks with a
notch, a recording indicator in the Dynamic Island's compact style that
reads as part of the Mac. It is not Dot scope: the ears widen the notch by
`hud_placement::EAR_WIDTH` (34 pt) on each side, with fully rounded bottom
corners, and grow from the housing's own height to the menu bar's, so open they
end on the bar's bottom edge and folded back they are the notch again. They
draw one SF Symbols–style glyph beside the camera on each side in Apple's dark
system colours (`ears` in
[rune_hud.wgsl](../../src/rune_hud.wgsl)):

```ts
Recording   -> orange mic.fill (the colour macOS and iOS give a microphone in use)
               + five orange capsule bars that move with the voice (√ level)
Working     -> the macOS 12-spoke spinner + the bars at rest with light running through them
Done        -> a green checkmark pops in, held 700 ms (Dot scope's done stays 100 ms)
Failed      -> red exclamationmark.circle.fill
Then        -> the ears fold back into the notch; glyphs scale and unblur in and out with them
Reduce Motion -> the spinner, bars' wobble, and light hold still (Pose::clock stays 0)
```

The gate, `labs::notch_indicator()`, is read each time the HUD is placed on
the pointer's screen (`Hud::place_on_pointer_screen` in
[rune_hud/macos.rs](../../src/rune_hud/macos.rs)), so it applies from the
next dictation. `hud_placement::notch_placement` places `ears` only on a
screen whose `NSScreen` reports a notch, at either visible Settings position:
Top and Bottom both grow the island, while Hidden still hides the HUD. A flat
external display keeps honoring Top and Bottom, and with the lab off HUD
Lab's placement stands. Linux and Windows have no row,
`hex labs on notch-indicator` doesn't know it there, and `hex settings set
labs.notch_indicator` answers `unsupported` (as any `labs.` switch for
another OS now does). Checks: `ears_widen_the_notch_sideways_only`,
`the_ears_reach_the_menu_bars_bottom_edge`, and
`the_notch_indicator_wins_at_top_or_bottom_but_still_needs_a_notch`
in [rune_hud/hud_placement.rs](../../src/rune_hud/hud_placement.rs);
`the_ears_grow_from_the_housing_to_the_menu_bar` in
[rune_hud/tests.rs](../../src/rune_hud/tests.rs);
`the_notch_indicator_holds_done_for_its_checkmark` in
[dictation/indicator.rs](../../src/dictation/indicator.rs);
`a_labs_switch_for_another_os_is_refused` in
[control/settings.rs](../../src/control/settings.rs);
`notch_indicator_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs); its frames render through the real shaders
with `HEX_HUD_EARS=1` and `rune_hud_frames`. Not yet observed: a real
dictation through the ears on a Mac with the lab on (`hex dev hud` drives
them in a debug build), and the failure glyph, which no render covers yet; the
row renders only in macOS builds, so no golden changes.

HUD height (`hud-height`, macOS only): at the Bottom position, how far
above `visibleFrame.y` (the Dock's top or the screen's edge) the plate
sits. `labs.hud_bottom_gap` is 0–400 points with 24 by default, so
`hex settings set labs.hud_bottom_gap 40` raises it and 0 puts it on the
edge; the card's slider, `{} pt` readout, and one-point Lower and Raise
buttons set the same value. The gate, `labs::hud_bottom_gap()`, is read
each time the HUD is placed (`Hud::place_on_pointer_screen` in
[rune_hud/macos.rs](../../src/rune_hud/macos.rs)), so a change applies
from the next dictation. A gap of 24 pt or less keeps the window's bottom
on the visible bottom and moves `Layout.center` (24 pt is exactly the old
placement); a larger gap raises the window by gap − 24 with the plate at
its usual offset inside it, keeping its shadow padding, so the visible gap
stays exact. The plate only moves up inside the window once the raised
window would reach the visible top (a gap above the visible height minus
104 pt), which within 0–400 only happens on a very short visible frame.
Placement clamps the gap so the plate's top never passes the visible
frame's top, below the menu bar or notch, and the window never crosses
below the visible bottom. Top, Off, HUD Lab's menu bar and
notch placements, and Linux and Windows are unchanged. Linux and Windows
have no row, `hex labs on hud-height` doesn't know it there, and
`hex settings set labs.hud_height` answers `unsupported` (as any
`labs.` switch for another OS now does). Checks:
`the_default_bottom_gap_places_exactly_as_before`,
`a_small_bottom_gap_sits_inside_the_window`,
`a_tall_bottom_gap_raises_the_window_and_keeps_the_gap_exact`,
`a_gap_that_reaches_the_visible_top_keeps_the_plate_inside`,
`a_raised_bottom_gap_works_on_a_display_below_the_primary`,
`a_huge_bottom_gap_keeps_the_plate_inside_the_visible_frame`, and
`a_bottom_gap_never_drops_the_window_below_the_visible_bottom` in
[rune_hud/hud_placement.rs](../../src/rune_hud/hud_placement.rs);
`hud_height_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: a real dictation at a
raised gap on a Mac, and the card's controls with every language.

Screenshots (`screenshots`, macOS only, group Annotate): for people who
miss CleanShot's ⌘⇧4. While on, the shortcut (`screenshots.hotkey`,
default ⌘⇧4; the card's recorder changes it and refuses one another Hex
shortcut uses) opens a second copy of Annotate's capture picker
(`PickerOverlay::install_as` in
[capture_capsule/picker.rs](../../src/rune_ui/capture_capsule/picker.rs)):

```ts
⌘⇧4            -> the screen dims under a crosshair cursor, even while another app is frontmost;
│                 a drag draws the area with its W × H beside the pointer, outside the area
├── Space      -> window mode: a camera replaces the pointer and the window under it highlights;
│                 Space again -> area
├── Click      -> the whole display ("Full screen · Click to capture")
├── Escape     -> closes it, nothing shot; Escape reaches no other app or Hex shortcut;
│                 a permission reply for that opening cannot reopen it or change a later pick
└── Release    -> shot at the display's own pixels (ScreenCaptureKit), Hex's overlays,
                  the picker, and the pointer left out
                  ├── PNG (pHYs at the display's scale) -> "Hex Screenshot 2026-11-04 at 14.03.22.png"
                  │   in screenshots.folder (the Desktop when unset); a second shot that second gets " (2)"
                  ├── clipboard -> the PNG and the file's URL, one pasteboard item
                  └── a card bottom right, 16 pt in, its corner concentric with the display's
                      (30 pt, a design constant), staying until it's closed (no timer):
                      every shot starts as exactly the rectangle captured (the area, the window, the
                      whole display; a recording, the area it recorded), flat and square with no
                      fade, and flies into its corner slot on critically damped springs: place and
                      size together, its corner rounding and its shadow coming in as it lifts; a
                      new shot mid-flight retargets it with its velocity kept; any card leaves past
                      the right edge (Reduce Motion: a fade either way); ✕ shows inside its
                      top-left corner on hover
                      a dictation while cards show -> pastes as usual, being no shot's talk-over
                      a failure -> the error tone and, for a failed save, the folder and why
                                   on Labs › Screenshots' card; there is no success sound
                                   (Hex has no shutter cue)
Another app holds Secure Event Input -> ⌘⇧4 and the record shortcut still open the picker
                  (Carbon hotkeys while it lasts; see dictation.md's Shortcuts Under Secure Input);
                  Space and Escape don't arrive, so window mode and Escape don't work then
```

Where shots go is the card's Saves to row, not a fixed folder. It reads the
folder in full with the home folder as `~` ("~/Desktop" until another is
chosen), and its two Secondary buttons are Reveal, which opens that folder
in Finder (making it first, as a save would), and the folder itself, which
opens the one native folder chooser (`desktop/file_dialog`, the seam Labs ›
Import audio uses) at the folder in use. A pick counts only once
`screenshots.folder` is saved, applies live to the next shot or recording,
and clears the last save's note; a failed settings save reads as a danger
note instead. A cancel changes nothing, and a click while a chooser is
open opens no second one.

A danger note under the row says why: the chooser or Reveal couldn't make
the folder, or the last save failed ("Couldn't save to ~/…: <why>"). The
note keeps the folder that failed with the error
(`screenshots::save_error` is an `Option<(PathBuf, String)>`, bounded at
300 characters per message), so a folder changed since through
`hex settings` or `settings.json` never makes the note name the wrong one.
`take` records a shot that can't be written, a recording's save records
its own, and the editor's Save records a shot's PNG or a trimmed
recording's movie the same way; any later save that works clears it,
including one whose clipboard copy then fails. Those happen on their own
threads, and a window's poll that reports nothing redraws nothing, so
`screenshots::save_error_generation` counts each recorded or cleared error
and the card's poll reports the difference from what it last drew: the
visible window's 250 ms tick then draws the note, with no extra wake
kept.

```ts
Saves to  -> the folder, home-shortened          Reveal  -> Finder on it, made first
          -> a chosen folder -> screenshots.folder (settings.json, live at once) -> clears the note
          -> a settings save that fails -> its reason under the row, old folder kept
          -> a cancel        -> nothing
          -> a failed chooser or Reveal, or a failed save (the folder it meant and why)
             -> one danger note under the row, until a save works or a pick is saved
             the save's error changes on its own thread; the card's poll reports the
             generation change, and only that makes the next tick redraw
// check: a_chosen_screenshots_folder_is_saved_and_says_why_a_chooser_failed
//        (tests/panes.rs): a pick saves and shows, a cancel leaves it, a chooser failure
//        says why, a save failure names the folder it failed into, and a pick clears it
```

The cards stack in the corner as CleanShot's Quick Access Overlay does
(`screenshot_thumbnail.rs`, drawn by `screenshot_thumbnail/card.rs`):

```ts
cards
├── a new capture  -> nearest the corner; the older cards rise above it, 12 pt apart, on the
│                     HUD's spring (Reduce Motion: they move at once); once the stack would pass
│                     the display's top, the oldest close. The stack follows the newest capture
│                     to its display
├── click          -> the card itself grows into the editor (see below); closing flies it
│                     back into its slot, wherever the stack has moved it
├── drag           -> the file, into any app
├── ✕              -> closes that card; the ones above it move down into its place
├── right-click    -> Open in Editor, Copy, Show in Finder, Move to Trash, Close
└── keys, while the pointer is on a card (the session tap takes them; nothing gets focus)
    ├── ⌘C         -> the file on the clipboard (a shot's picture with it); "Copied" on the card
    ├── ⌘S         -> the file, already saved, shown in Finder
    ├── ⌘E         -> the editor
    ├── ⌘W, Escape -> closes it
    └── Return     -> keeps it (it's saved) and closes the card
```

CleanShot's ⌘S saves the capture; every Hex capture is already saved,
so ⌘S shows it in Finder instead.

Every shot and recording the Lab saves, and every annotation and replay
Annotate wrote to its captures folder, is also listed in the Captures
pane ([captures.rs](../../src/rune_ui/captures.rs)), which sits under
History in the sidebar while the Lab is on. It is its own pane rather than
a part of History: its rows are pictures and movies with their own
actions, and History's retention menu and Clear all don't apply to them.

```ts
Captures
├── search      -> every word in what was said over it, its file name, or its kind
│                  (an annotation: its words, app, the session it went to)
├── rows        -> newest first, the last 30 days, only files still where they were saved:
│                  thumbnail · the words said over it (else the file's name) ·
│                  "Screenshot · 2880 × 1800", "Screen recording · 0:42",
│                  "Annotation · 0:18", or "Annotation · Video expired" · its age
├── click       -> the editor (a shot with its words; a recording, or an
│                  annotation's movie with its timed words, to trim; one whose
│                  model gave no word times (GigaAM v3) has its text spread
│                  over the recording as estimated; an annotation whose movie
│                  expired: its first keyframe)
├── drag        -> the file (an annotation's movie, else its keyframe)
├── hover       -> Copy, Show in Finder, Move to Trash in the age's place
│                  (Copy: a check only once the clipboard has it, else the
│                  error tone; an annotation copies as Annotate does; Trash
│                  moves an annotation's whole bundle)
└── nothing yet -> "No captures yet." and what shows here
```

The list reads an owner-only index in Application Support
(`captures/index.json`, 0600 in a 0700 folder) with a 240 px PNG thumbnail
per entry beside it ([history/captures.rs](../../src/history/captures.rs)),
never the files themselves. A shot is remembered as it saves, a recording
once its movie is saved, and each accepted dictation over a shot appends to
that shot's entry. Closing or replacing its editor before release returns
the dictation to ordinary paste. After release, its words stay attached to
the original shot even if the editor closes; they never land on a different
shot or finish a newer dictation. Escape cancels only that narration request.
Pure request tests live in `screenshot_thumbnail/editor/narration.rs` and
`screenshots/picker.rs`; these are not native shortcut or permission proof.
Entries past 30 days or past 500 entries drop with
their thumbnails the next time one is added; each keeps at most 4 KiB of
words. Move to Trash (a card's or the pane's) forgets the entry. The
editor's Save makes a new file that isn't listed. Writes are atomic, and a
malformed index is kept aside as `index.json.corrupt`.

Annotations come from the captures folder itself
([history/annotations.rs](../../src/history/annotations.rs)), never an
index: while the pane shows, a `hex-captures-scan` thread lists the
`*.scrawl` bundles at most every 5 s, skips any whose name dates it past 30
days, and reads only the `scrawl.json` files that changed since the last
scan. Annotate's replays are written as ordinary bundles, so they list as
annotations too.

The last ten cards closed (by ✕, a key, the menu, opening the editor, or
the stack reaching the top) are kept in memory. The menu bar's Restore
Last Capture, shown while the Lab is on and dimmed until a card was
closed, brings back the newest of them, sliding in from the right edge
into the corner; one whose file is gone is skipped, and Move to Trash
keeps none. They don't outlast Hex: after a relaunch the Captures pane is
the way back to a closed card's capture.

The card's overlay is the whole display under the pointer, click-through
outside the card, so the card can start anywhere a shot was taken from.

A click on the thumbnail opens the editor
([screenshot_thumbnail/editor.rs](../../src/rune_ui/screenshot_thumbnail/editor.rs)), a panel
centred on the display under the pointer, the capsule's corners and
shadow. The card's own picture grows from the card into its place in the
panel, losing the card's corner and shadow as it goes, and the panel
fades in around it once it has nearly landed; the card waits unseen
meanwhile, so the shot never shows twice. Closing reverses it: the panel
fades, the shot flies back into its card and dissolves into it, and a
close mid-zoom turns straight back with its velocity kept. A shot whose
card has gone shrinks a little and fades where it is (Reduce Motion: a
cross-fade). Its tools,
✕ and microphone are the kit's icon buttons (a picked tool filled, as a
capsule's expanded one is) and Copy, Save and Send to… are the kit's
buttons, so it reads as the rest of Hex. The shot
shows at no more than its own points. It stays until Escape or ✕:

```ts
editor
├── Pen, Arrow, Box, Text, Crop -> a drag on the shot draws in Annotate's red;
│                                   Text: a click, then type, Enter or Escape ends it;
│                                   Crop: a drag shades what's left out, a click clears it
├── ⌘Z            -> the text being typed, else the last mark, else the crop
├── Copy (⌘C)     -> the edited PNG on the clipboard
├── Save (⌘S)     -> the native save panel, above the editor, in the folder Save or Export
│                    last saved to (screenshots.export_folder; screenshots.folder at first),
│                    offering the shot's own name (PNG only); Cancel saves nothing and
│                    changes nothing; a chosen path is written whole or not at all
│                    (.partial-<name>, then renamed); choosing the shot's own file
│                    replaces it and closes the editor, else the note says "Saved"
├── Send to…      -> a list of open OpenCode sessions (typing searches title, folder, id,
│                    as Annotate's; ↑↓ Enter); the PNG goes to the session as an image,
│                    with the words said over it as its text ("Screenshot" without any);
│                    picked while a dictation goes, it waits for the words
├── dictation (the shortcut, or the microphone under the shot, which starts and finishes
│   one like `hex dictation toggle`)
│                 -> talks over the shot: the words show under it (3 lines, then "…"),
│                    each dictation's added; never pasted; ✕ beside them clears them
├── a drag on the panel's edge or the toolbar's gap -> moves it, kept on the display
└── Escape        -> cancels a dictation over the shot (its words and a waiting send never
                     land; the panel stays), else ends the text, else closes the list,
                     else closes the editor
```

Talking over a shot
([screenshots/narration.rs](../../src/screenshots/narration.rs)) is the
shortcut's own dictation, on the loaded model and through the mode's
processing; only where its text lands changes. The listener asks at a
dictation's start whether the shot's editor is open, and at its finish hands
the transcript to that shot instead of pasting it, as
`hex dictation stop --print` does. The corner card never takes words: while
only it shows, a dictation pastes as usual. A short, silent, cancelled, or
failed clip lands nothing. Dictation never takes Escape, so the editor's
Escape cancels it through the listener.

The edited shot is drawn on the GPU at the shot's own pixels, the marks
as the editor draws them (an offscreen Rune view), then cropped; without
marks it's a plain crop. A note in the toolbar says Copied, Saved, or Sent
to…, or what failed. Sending writes the PNG to the temp folder and posts it
through the router's `send_image_to_session`, which Scrawl's copy of the
router doesn't have yet.

Left to do: no blur or highlight tool, colour or stroke choice, moving or
deleting a single mark, redo, or zoom; a shot larger than the GPU's
largest texture exports without its marks; Send lists sessions only, not
Annotate's rules; no Save as or drag out of the editor.

Recording the screen
([screenshots/recording.rs](../../src/screenshots/recording.rs)) needs
Annotate on (`capture.enabled`): a recording is an Annotate capture
(`CaptureCommand::Clip`) on capture-core's recorder, narrated from the
dictation timeline. Its shortcut (`screenshots.record_hotkey`) is ⌘⇧5 by
default, set or cleared on the drawer's Record screen row (a key is
required; the trash button clears it); without Annotate the row says "Needs
Annotate". macOS's own ⌘⇧5 (symbolic hot key 184, Keyboard Shortcuts ›
Screenshots › "Screenshot and recording options") is matched in the window
server first, so while it's on the drawer shows the same "macOS takes this
shortcut first" note as for ⌘⇧4 (`system_shortcut.rs`).
The record shortcut never takes Annotate's capture or replay chord. Annotate
can't take the record shortcut's chord while the Lab is on; with the Lab off
it can (every Mac has ⌘⇧5 saved, which mustn't hold Annotate's key), and
turning the Lab on then clears the record shortcut rather than leave it dead
while Annotate's whole-screen recording takes the chord. A file saved with the
two sharing a chord has its
record shortcut cleared when it loaded, with a warning logged: Annotate's
capture shortcut isn't filtered the way the record one is, so the chord would
still record the whole screen, and a record saved on the replay shortcut
turned replay off with the Lab off. Other shortcuts only see the record
key while a recording is available (the Lab and Annotate on). Without
Annotate, the row still shows a saved shortcut with its clear button, so the
key it holds stays visible and can be freed.

The record shortcut opens the recording toolbar
([screenshots/toolbar.rs](../../src/screenshots/toolbar.rs), its view in
[capture_capsule/toolbar.rs](../../src/rune_ui/capture_capsule/toolbar.rs)),
as macOS's own ⌘⇧5 bar: a capsule at the bottom of the display under the
pointer, never focused and left out of every capture. Each choice is saved
as it's made, off the UI thread.

```ts
record shortcut  -> the toolbar: ✕  [Area | Window | Full screen ▾]  [sound ▾] [Options ▾]  ● Record
│                   icons only (names in tooltips and for VoiceOver), groups spaced, no dividers
│                   while an Annotate capture records -> nothing; while recording -> stops
│                   without Screen Recording -> Annotate's permission helper, no toolbar
├── kind         -> Area (default), Window, or Full screen; remembered (screenshots.record_kind)
├── display      -> only with two or more displays: a chevron inside the chosen Full screen
│                   segment, or a second click on it, lists their macOS names, the one under the
│                   pointer first chosen; picking one moves the toolbar there. Not remembered
├── sound ▾      -> Microphone: None, Same as dictation (default), or any input but Hex Microphone
│                   (screenshots.record_microphone: "none", "default", or the device's name);
│                   Record computer audio (screenshots.record_system_audio, off). The button shows
│                   a microphone (crossed out for None) and a waveform while computer audio is on;
│                   with a microphone chosen, three bars beside it show what it hears, dim and
│                   low while silent (screenshots/toolbar_level.rs)
├── Options ▾    -> Scale Retina to 1x, 30 or 60 fps, High · Medium · Low (the Recording settings)
├── Record / Return -> Area: the picker on an area; Window: the picker on windows;
│                      Full screen: records the chosen display at once
└── Esc, ✕, or the record shortcut again -> closes; the shot shortcut does nothing meanwhile
// check: the_toolbar_takes_escape_return_and_its_shortcut_and_leaves_the_shot (screenshots.rs)
// check: a_choice_shows_before_the_controller_hears_it, the_sound_button_names_what_it_keeps,
//        the_level_moves_in_whole_steps (capture_capsule/toolbar.rs)
// not run: the toolbar on a real display, popups, and a display move; no golden or --snap scene shows it
```

The sound a recording keeps: Same as dictation copies the dictation
timeline, as every Annotate capture does, so no second stream of that
device opens. Another device opens for that recording alone
([capture_host/microphone.rs](../../src/capture_host/microphone.rs)), on
its own thread, closed at stop, cancel, or quit (waiting at most 300 ms
for the tail); a device that fails is opened again every second and what
it missed is a gap, and one that won't open at all falls back to the
timeline. A device dictation already has open is copied from the
timeline. None keeps no microphone.

The toolbar's bars read dictation's level while dictation's microphone is
the chosen one and delivering audio. Otherwise (Release microphone while
idle, or another device chosen) the toolbar opens the chosen input for
the meter alone while it shows: no samples are kept, dictation's stream
is never touched, and it closes when the toolbar closes, Record takes
over, or another microphone is chosen. macOS shows its orange microphone
indicator meanwhile. The toolbar redraws only when the level crosses one
of eight steps. While a Screenshots recording with a microphone runs, the
recording capsule shows the same three bars after its timer
(`capture_host::recording_level`: the dictation timeline's level, or the
recording's own input's); Annotate captures and None show none. Checks:
`a_metered_recording_makes_room_for_its_level` (capture_capsule/surface.rs);
the stills `renders_the_toolbar` and `renders_the_recording_level` are
`#[ignore]`d and write only with `HEX_TOOLBAR_RENDERS` /
`HEX_CAPSULE_RENDERS`. Not yet observed: the bars on a real display with
a live microphone.

Computer audio is ScreenCaptureKit's
own stream audio with Hex's own sounds left out, written beside the movie
as `system.m4a` under the Screen Recording grant the recording already
has. When the movie saves, `scrawl-helper mux` mixes the microphone
(`audio.wav`, 16 kHz mono, skipped when it is all silence) and computer
audio into one AAC track (48 kHz stereo, 192 kbit/s) with the video
copied as it is, each placed by its start on capture-core's clock and cut
at the movie's end; the bundle keeps the separate files. A failed mix
saves the plain movie.

```ts
record shortcut  -> the toolbar (above); Area and Window use the same picker as a shot
├── recording    -> Annotate's capsule shows it recording, as any capture; talk to narrate
│   └── Area or Window -> every display dims but what's recorded, outlined as the picker
│                         outlined it (untinted), fading in once the screen records and out
│                         at stop or cancel; a Window's hole follows the window as it moves,
│                         as the movie does; click-through, never focused, left out of the
│                         movie unless capture.show_overlays_in_recordings; Screen dims nothing
├── record shortcut again (or the capsule's stop) -> stops; the capsule folds unless it failed
│                   ├── the movie saves itself: "Hex Recording 2026-11-04 at 14.03.22.mp4" in
│                   │   screenshots.folder (the Desktop when unset), " (2)" on a clash, as a shot's
│                   │   PNG does; a clone of Annotate's copy, so instant on one volume. A failed save
│                   │   plays the error tone, the card shows Annotate's copy, and Labs › Screenshots'
│                   │   card names the folder and why
│                   ├── no clipboard, no router: nothing is sent or copied on its own
│                   └── the corner card, from the first frame, its length in a pill
│                       click -> the editor; drag -> the saved movie; a dictation never lands on it
└── editor       -> the saved movie's frame above a filmstrip of 12 frames; a drag on the strip
                    moves the nearer end of the span kept (never under 0.5 s) and shows the frame
                    there; ▶ beside the strip (or Space) plays the span with sound from the
                    playhead, a white line kept inside the handles, and stops at its end; the words said inside the span under it, read-only (one whose model
                    gave no word times has its text spread over the movie as estimated, never
                    "Nothing was said"); no marks
                    ├── size     -> "Original · 93 MB ▾" (or "1080p · ~56 MB ▾"), a pop-up of Original,
                    │               1080p, 720p, each with its estimate for the span kept; the pick
                    │               is remembered (screenshots.export_size) and saved off the UI thread
                    ├── Copy (⌘C) -> the movie FILE on the clipboard (pastes as an attachment in
                    │               Slack, Messages, Finder): the saved movie itself when nothing was
                    │               cut at Original, else a copy in the temp folder's hex-screenshots
                    ├── Export   -> the native save panel, above the editor, in the folder Export
                    │               last saved to (screenshots.export_folder; screenshots.folder at
                    │               first), offering the movie's own name, MPEG-4 only; Cancel
                    │               exports nothing and changes nothing; the size and span go to
                    │               the path chosen, and only then does the button show progress
                    └── Send to… -> a contact sheet of up to 4 frames in the span as the image,
                                    the span's words and "Screen recording (m:ss): <movie>" as text,
                                    the saved movie itself when nothing was cut at Original
                    each at Original is copied whole or cut without re-encoding
                    (`scrawl-helper trim`, which snaps to keyframes); at 1080p or 720p it is
                    re-encoded (`scrawl-helper shrink`, HEVC at the recording's quality, utility
                    priority, off the UI thread) with its percentage on the pressed button; one
                    export at a time
```

The recording's bundle stays Annotate's (`~/Library/Application
Support/scrawl/captures/<id>.scrawl`), and Annotate's retention may trash
its movie; the saved copy is the one the card, the editor, and a drag use.
Annotate's own captures are not saved anywhere new.

The movie is made as Labs › Screenshots' Recording settings say
(`MovieQuality::Full(MovieSize)`, read at each start): Scale Retina to 1x
(`screenshots.scale_to_1x`, on: one pixel a point), Max resolution
(`screenshots.max_resolution`: Original, 1440p, 1080p, 720p by long edge;
with 1x on, the smaller of the two wins), Frame rate
(`screenshots.frame_rate`: 30 or 60), and Quality (`screenshots.quality`:
High, Medium, Low, HEVC constant quality 0.50, 0.30, 0.20). Defaults: 1x on,
Original, 30 fps, High; Full size is one switch away (1x off). The Recording
group shows only while Annotate is on, since only then can the lab record.
Measured on a 46 s 1920×1016 clip with nearly every pixel moving (a worst
case), VideoToolbox HEVC gave High 92.6 MB (16.0 Mbit/s), Medium 35.2 MB
(6.1), Low 21.3 MB (3.7), against 330.9 MB (57.3) at the old 0.7; 60 fps
costs about 1.27× 30 fps; below 0.45 changing digits ghost, and 0.2 smears.
Then capture-core's
`Encoder::Hevc` writes hardware HEVC Main at that constant quality with
VideoToolbox's real-time mode off, Rec. 709 tags, no B-frames, a keyframe at
least every 2 s (so a cut at Original, which starts at the keyframe before
it, lands close; 2 s measured 5% larger than 5 s on that busy clip at High,
1 s 8%), and a frame only when the picture changes, up to 60 fps. An
area above 8192×4320 pixels (4320×8192 tall), the hardware limit, is scaled
to fit. Annotate's own captures stay sized for agents: H.264 from
`SCRecordingOutput` with a 1280 px long edge (2560 for a whole display). On a
3200×1800 test animation of Retina text this measured 43.4 dB luma PSNR
against 18.8 dB for the old 1280 px movie, at 56 MB/min while busy and
8 MB/min while still; a 5K display busy is about 96 MB/min.

```ts
the record shortcut's movie  -> Recording settings (1x, max size, 30/60 fps, HEVC 0.50/0.30/0.20), Rec. 709, frames only on change
an Annotate capture's movie  -> 1280 px long edge (2560 for a display), system H.264
// check: a_screenshots_recording_follows_its_settings_and_a_capture_stays_compact (capture_host.rs)
// check: a_full_quality_movie_records_hevc_at_the_size_asked, one_x_and_a_long_edge_take_the_smaller (crates/capture/src/capture/macos.rs)
// check: an_export_estimate_follows_the_span_kept_and_the_size, a_recording_has_no_marks_and_copies_its_movie (editor/tests.rs)
// check: playing_moves_the_playhead_through_the_span_and_stops_at_its_end (editor/tests.rs)
// check: save_offers_the_recordings_own_name_and_a_cancel_changes_nothing, the_path_chosen_gets_the_recording (editor/tests.rs)
// check: a_save_replaces_the_file_chosen_and_leaves_nothing_beside_it (screenshots/save.rs)
// check: the_dim_waits_for_the_screen_and_ends_with_its_recording, only_an_area_or_a_window_is_dimmed (screenshots/dim.rs)
// stills: HEX_CAPSULE_SNAPS writes recording-dim-{area,window}-{light,dark}.png (capture_capsule/tests/motion.rs);
//         HEX_SCREENSHOT_SNAPS writes editor-recording-{saving,saved}-*.png (editor/tests.rs).
//         Not observed on a real Mac: the save panel itself, and the dim over a real window being dragged
// check: hevc_records_native_pixels_up_to_the_hardware_limit (crates/capture-core/src/plan.rs)
// check: writes_a_tagged_hevc_movie_from_frames (crates/capture-core/src/video_writer.rs)
```

Checks: `the_record_shortcut_records_and_never_while_the_picker_shows` in
[screenshots.rs](../../src/screenshots.rs),
`a_granted_screenshots_permission_reopens_its_picker` in
[capture_capsule/flow.rs](../../src/rune_ui/capture_capsule/flow.rs),
`a_clip_capture_asks_for_the_screenshots_picker` in
[capture_host/permissions.rs](../../src/capture_host/permissions.rs),
`a_failed_screenshots_recording_asks_for_its_picker` in
[capture_capsule/feed.rs](../../src/rune_ui/capture_capsule/feed.rs),
`the_record_shortcut_and_annotates_capture_shortcuts_never_share_a_key` in
[app_settings.rs](../../src/app_settings.rs),
`a_saved_record_shortcut_sharing_annotates_chord_is_cleared_on_load` in
[app_settings.rs](../../src/app_settings.rs),
`annotate_will_not_take_a_saved_record_shortcut` in
[rune_ui/scrawl.rs](../../src/rune_ui/scrawl.rs),
`the_screenshot_lab_and_annotate_never_share_a_shortcut` in
[control/settings.rs](../../src/control/settings.rs),
`a_recording_saves_beside_the_shots_and_never_over_one` in
[screenshots/recording.rs](../../src/screenshots/recording.rs),
`a_stopped_screenshot_recording_folds_the_capsule` in
[capture_capsule/feed.rs](../../src/rune_ui/capture_capsule/feed.rs), the
trim, words, sheet, and no-marks tests and the `trim` editor golden in
[screenshot_thumbnail/editor/tests.rs](../../src/rune_ui/screenshot_thumbnail/editor/tests.rs),
and the `recording` thumbnail golden. Not yet observed: a real recording
on a Mac, its card, trimming and saving a movie, and a session receiving
a sent recording, or playback with sound. Left to do: no audio-only or GIF
export, and the recording shows no countdown.

The shortcut is taken in Hex's session tap (`screenshots::key`, called
from [suppression.rs](../../src/suppression.rs) before the listener's
own handling), never by the listener, and its press and release stay with
it. With the lab off the tap returns at once and nothing is made: no
picker window, no thumbnail, no work. Screen Recording is Annotate's: a
press without it shows the same permission helper instead of the picker.
Restart relaunches Hex; when the grant arrives without a relaunch, the
helper's Ready button reopens the Screenshots picker in the mode the
shortcut asked for — a shot, or a recording — never Annotate's whole-screen
recording. A record pick that starts a clip (`CaptureCommand::Clip`) reports
the same way, and a clip that fails after starting keeps that picker origin
for its helper. Annotate's own helper still records once it's ready.

The picker is a non-key overlay, so Hex is usually not the frontmost app
while it shows, and macOS ignores a background app's cursor. Hex builds
Rune with `macos-background-cursor`, which sets WindowServer's private
`SetsCursorInBackground` property so the picker's crosshair (and Window
mode's hidden arrow under the drawn camera) shows over any app. It is a
private API, acceptable because Hex isn't sold through the App Store; if a
macOS release drops the property, Rune skips it and the cursor shows only
while Hex is frontmost. Checks: `the_badge_stays_on_the_display` in
[capture_capsule/picker.rs](../../src/rune_ui/capture_capsule/picker.rs).
Not yet observed: the cursor over another frontmost app on a real Mac.

macOS matches its own screenshot shortcuts (Keyboard Shortcuts ›
Screenshots, ⌘⇧4 among them) in the window server before any app's event
tap, so while that system shortcut is on, Hex never sees the press. Hex
reads `com.apple.symbolichotkeys` (`screenshots::system_shortcut`) and,
when the system takes Hex's chord, the card says so with Open Keyboard
Settings, which opens that pane; turning macOS's shortcut off, or
choosing another chord, hands it to Hex.

`hex screenshot` shoots the whole display under the pointer and prints the
file's path; `hex screenshot --pick` opens the picker instead
([cli.md](../cli.md)). With the lab off it answers `disabled`; Linux and
Windows answer `unsupported`.

Checks: `the_shortcut_opens_the_picker_and_its_release_stays_with_it`,
`a_held_shortcut_opens_one_picker`,
`escape_and_space_belong_to_the_open_picker_only`,
`other_keys_and_other_chords_pass`, `recording_a_shortcut_takes_nothing`
and `a_large_shot_shrinks_for_its_thumbnail` in
[screenshots.rs](../../src/screenshots.rs); the file name, the second
file in one second, and the PNG's density in
[screenshots/save.rs](../../src/screenshots/save.rs); the system shortcut
read in [screenshots/system_shortcut.rs](../../src/screenshots/system_shortcut.rs);
`thumbnail_goldens` (rest, hover, a recording's length, and three stacked,
light and dark), a card staying until closed, the stack's slots, the
oldest closing at the top, keys reaching only the hovered card, and
Restore bringing back the card closed last but never a trashed one in
[screenshot_thumbnail/tests.rs](../../src/rune_ui/screenshot_thumbnail/tests.rs);
`a_hovered_card_takes_its_keys_and_only_then` in
[screenshots.rs](../../src/screenshots.rs);
`editor_goldens` (marks, the Send to… list, and words while a dictation
records, light and dark), the layout and the words row's height, words
landing only while transcribed, Escape dropping them and a waiting send, a
waiting send going once they land, each tool's drag, text, undo, the
list's search, and both exports
(`an_export_draws_the_marks_over_the_shot` renders on a GPU, skipping
without one) in [screenshot_thumbnail/editor/tests.rs](../../src/rune_ui/screenshot_thumbnail/editor/tests.rs),
and the mark model in [screenshot_thumbnail/editor/marks.rs](../../src/rune_ui/screenshot_thumbnail/editor/marks.rs);
the flights in [screenshot_thumbnail/flight.rs](../../src/rune_ui/screenshot_thumbnail/flight.rs)
(no overshoot, the same flight at 60 and 120 Hz, retargeting keeps its
velocity) and the handoffs between card and editor in
[screenshot_thumbnail/editor/zoom/tests.rs](../../src/rune_ui/screenshot_thumbnail/editor/zoom/tests.rs)
(`the_shot_never_jumps_between_the_card_and_the_editor`, a close mid-zoom,
a gone card, a new shot mid-zoom; `zoom_frames` renders the clips with
`HEX_SCREENSHOT_MOTION=<dir>` and does nothing without it);
`screenshots_is_its_own_bit_and_off_by_default` in [labs.rs](../../src/labs.rs);
the index's order, reopen, owner-only modes, search, window, caps, forgetting,
words, and a malformed file in [history/captures.rs](../../src/history/captures.rs);
the annotation listing's window, order, expired movies, search, and rescan in
[history/annotations.rs](../../src/history/annotations.rs);
the Captures pane (with an annotation row) as the `captures` golden (light and dark) and
`--scene captures-empty`;
its card renders as `--scene mac-labs-screenshots`; the card's entrance
and the stack's motion are `thumbnail_frames` (`HEX_SCREENSHOT_FRAMES`
writes them at 60 fps).
Not yet observed: the card keys and right-click menu on a real Mac, the
Captures pane over a real index (its thumbnails decoding, a drag out, Copy,
and Move to Trash), a
real ⌘⇧4 on a Mac (with macOS's shortcut off), a shot of each kind, the
clipboard paste, the thumbnail's drag, the editor on a real display
(its keys, Copy, Save, and a session receiving a sent shot), talking over a
shot with a real shortcut and microphone, the card growing in from a real
capture rectangle, and whether a session
tap can ever see ⌘⇧4 while macOS's shortcut is on (inferred from how
symbolic hotkeys are dispatched, not tested), and the Saves to row's real
folder panel (the test applies a chooser's answer and never opens one). The
row renders only in
macOS builds, so the settings goldens don't change.

Annotate last 15 seconds (`annotate-replay`, macOS 27+ only, group Annotate): keeps
a rolling 15-second display buffer through `SCClipBufferingOutput` (resolved
at run time through Objective-C, so macOS 26 and earlier neither link nor
list it) while both `labs.annotate_replay` and `capture.enabled` are on and
Screen Recording is granted. Because an active `SCStream` feeds `replayd`,
macOS shows its purple screen-recording indicator the whole time the lab is
on, which the Labs store card states plainly. `hex capture replay` (or a tap
on `capture.replay_hotkey`, unbound by default and recordable in
Settings › Annotate) exports the last 15 seconds into a normal
`.scrawl` bundle; holding `capture.replay_hotkey` (≥ 300 ms) freezes the clip
at the press and records microphone narration shifted onto the bundle clock
after it. Details, checks, and live macOS 27.0 measurements:
[capture.md](capture.md) (`capture.replay`) and
`annotate_replay_is_its_own_bit_and_off_by_default` in [labs.rs](../../src/labs.rs).
The row renders only on macOS 27+, so Linux goldens don't change.

Choose meeting summary model (`meeting-summary-model`, issues 1107 and 1160)
graduated: Meetings › Settings › After a meeting › Summaries picks Off, On
this Mac, or OpenCode, with OpenCode's Model nested under it
([meetings.md](meetings.md)). Its bit (1 << 24) stays unused.

Process API transcripts (`api-processing`; every OS; its row is Output's): for
a host app that wants what a person would paste, not just the raw transcript.
While on, the authenticated local API's `POST /transcriptions` and
`POST /streams/{id}/finish` accept `process=global` (`true` and `1` are
aliases); the response keeps its existing fields and adds `processed`, the
text after exactly what a paste with no matching app or website mode gets:
global replacements, Global mode corrections, the optional OpenCode rewrite,
and Global's selected transformations
([streams.md](../streams.md#post-streamsidfinish),
[local-transcription-service.md](../specs/local-transcription-service.md)).
The context is `ContextSnapshot::default()`, so no app or website mode
applies, and the transform `source` is `dictation`. It never pastes, writes
History, updates Paste Last, or touches the clipboard, and like a paste it is
best-effort: a failed stage keeps its input and the call still succeeds.

An absent parameter leaves the response byte-for-byte as before. Any other
value, or a repeated parameter, is `400 invalid-request`. A process with no
installed processor, such as `hex service` or `--embedded`, answers `409
processing-unavailable` even while the lab is off; with a processor installed
and the lab off, `process` answers `403 processing-disabled`. These are decided
before `/transcriptions` reads the audio body and before `/finish` finishes
the stream; a refused `/finish` leaves the stream running. Processing runs one
at a time across the local API, so a request that arrives while another holds
the slot answers `429 queue-full`; the slot is released when the run ends,
fails, or is interrupted. `GET /capabilities` always includes `outputProcessing`,
true only with the lab on and a processor installed. `API_VERSION` stays `2`.

Processing stops at its next stage boundary when Hex quits: the API's shutdown
flag is passed through as the pipeline's cancellation flag. On macOS, custom
transformations are skipped until Hex finishes setup and its listener starts,
because they use the recognition loop's transformation client; this is
best-effort like every other processing stage, so the response still succeeds
with the text the stages before them produced.

The running app installs one processor built from its dictation pipeline:
macOS reads the live profiles snapshot in
[dictation/config.rs](../../src/dictation/config.rs) and the recognition
loop's transformation client; Linux and Windows read the listener's live
`ModeRuntime` and transformation client. Both are the objects settings saves
already update, so edits and the personal-commands approval gate apply
without a restart, and processing keeps its existing deadlines. The seam is
`Processing` in [local_api/processing.rs](../../src/local_api/processing.rs),
passed through `LocalApiOptions` like `control` and `dictation_control`.
`hex service` installs none. Checks: `local_api::processing` route tests cover
the 409 for a missing processor and the 403 before the body, invalid and
repeated values, the aliases, `429 queue-full` while one run holds the slot,
`processed` present with the existing fields unchanged, absent responses, and
`outputProcessing`; `api_processing_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not observed: a real `/transcriptions` success
needs an installed model, so its `processed` field is covered through the
shared seam and the `/streams` finish route rather than an end-to-end
transcription; nothing ran on macOS or Windows.

Timestamps in API transcripts (`api-timestamps`; every OS; its row is
Output's, beside Process API transcripts): for a host app that needs to know
when each phrase or word was said, for captions, search, or syncing. While on,
the authenticated local API's `POST /transcriptions` accepts
`timestamps=segment|word`; absent means `none`, the response as it was
([local-transcription-service.md](../specs/local-transcription-service.md)).
`segment` adds `timestampGranularity: "segment"` and `segments`; `word` adds
`timestampGranularity: "word"`, `segments`, and `words`. Each span is `text`,
`startMs`, and `endMs` in whole milliseconds from the start of the submitted
audio.

The times are the model's own native output. Nothing estimates them from text
length or chunk boundaries: `Transcript` (already the meeting and capture
result) carries them, Linux and Windows chunk long clips at the model's own
window and add each chunk's offset once from its sample position, and macOS
chunks a GGUF clip the same way. Apple Speech gives segments. An end that the
minimum clip duration or a model's trailing silence pushed past the decoded
audio is clamped to `durationMs`. A GGUF model answers a clip with no speech
with `200`, an empty `transcript`, and empty arrays; Apple Speech reports no
speech as `500 transcription-failed`, with or without timings.

An unknown value, or a repeated parameter, is `400 invalid-request`. With the
lab off, `segment` and `word` answer `403 timestamps-disabled` before the
audio body is read, and `none` is unaffected. The running app reads the switch
live; a `hex service` process without the app reads the saved
`labs.api_timestamps` once (the embedded helper as it starts), so a
change reaches a running helper after it restarts.

When the selected model cannot
produce that level natively — a word request on a model whose
`capabilities.max_timestamp_kind` is segments, or Apple Speech for words — the
answer is `422 unsupported-timestamps`, decided after the model resolves and
before inference, on both the shared dictation engine and the service's own
copy. A word request is never downgraded to segments. With `process=global`,
the timings describe the raw transcript and `processed` stays separate.
`/streams` and `/dictations` keep their own timing and do not offer this
parameter. Checks: `local_api::timestamps` route tests cover the absent and
`none` defaults, each level, invalid and repeated values, the 403 before the
body, the response fields (and no new field without a request), clamping, and
empty arrays; `transcription::service`'s
`a_timed_request_is_answered_only_at_the_models_own_level` covers the 422
decision; `transcription::gguf_session`'s
`a_chunks_times_move_onto_the_clips_timeline_by_sample_position` covers chunk
offsets; `api_timestamps_is_its_own_bit_and_off_by_default` and
`every_feature_is_off_by_default_and_round_trips` in
[labs.rs](../../src/labs.rs) cover the switch. Not observed: a real model
answering with times, on any OS, and the macOS `/transcriptions` path.

Transcribe files (`transcribe-files`) graduated: `hex transcribe <file>`
always works now, reads common audio and video formats, and starts its own
embedded service when Hex isn't running ([control-cli.md](control-cli.md)).
`hex labs on|off transcribe-files` still answers with that and exits 0
(`labs::GRADUATED`); the saved `labs.transcribe_files` key is ignored.

Meeting platform icons (`meeting-platform-icons`, issue 1290) graduated:
each recording in the Meetings list always shows its call service's icon, or
a neutral one ([meetings.md](meetings.md)). `hex labs on|off
meeting-platform-icons` still answers with that and exits 0
(`labs::GRADUATED`); the saved `labs.meeting_platform_icons` key is ignored.

Ten meeting features left Labs together. Follow live transcript,
More call apps, Vocabulary in meeting notes, Meeting finished event, Subtitle
export, and Movable meeting capsules graduated: they always work now
([meetings.md](meetings.md)), and a panel capsule thrown on release glides
into the corner it heads for. Stop when the call ends, Expire meeting audio
separately, Descriptive meeting file names, and Separate notes file became
Meetings › Settings rows: Calls › Stop when the call ends
(`meetings.stop_when_call_ends`), After a meeting › Keep audio
(`meetings.audio_retention`), and Export › Descriptive meeting file names and
Separate notes file (`meetings.export.descriptive_filenames`,
`meetings.export.notes_file`), shown once Meetings is on. The one-time
`meeting-labs-settings` migration copies each old switch into its setting and
keeps a chosen audio window only where Expire meeting audio separately was on.
`hex labs on|off` for any of the ten answers where it went and exits 0
(`labs::GRADUATED`); the old `labs.*` keys are read and ignored.

Normal features, not Labs: dictation, Annotate (every build; its own pane
and switch), lowering audio while dictating (#210), Dot scope, chord shortcuts, QuickJS transformations, clipboard
restore (Labs › Keep dictation on clipboard skips it on every OS), the
vocabulary list itself (rewriting and search), and Linux's Terminal paste.

## Features

- Agent tools for captures: `hex annotate inspect`, `zoom`, `frames`, and
  `skill` read a capture for a coding agent (every OS; off by default,
  `labs.annotate_agent_tools`, in the Annotate group). They run in the `hex`
  process itself, never over the running Hex's control socket, so they work
  with Hex closed; they only read the bundle and decode its video, and write
  nothing but `<bundle>/zoom/`. `inspect [<capture>]` prints `summary.md`;
  `--marks` lists every mark as `#2 ✎② 0:09 circled · m2` (or `No marks`), and
  `--json` prints `scrawl.json` pretty (`--marks --json`: the marks as JSON).
  `zoom [<capture>] (--at <M:SS|secs|phrase> | --mark <n|id> [--frame N])
  [--span secs] [--frames N] [--full]` writes a sheet at
  `<bundle>/zoom/zoom-<from>-<to>.png` and prints its path, the window, each
  cell's time, and `Said: “…”` when anything was said; for a mark it prints
  the mark's line first. `frames [<capture>] --from <M:SS|secs> --to
  <M:SS|secs> [--fps 4]` writes one PNG per frame under
  `<bundle>/zoom/range-*/` and prints each path. `skill` prints the
  `skills/hex-annotate/SKILL.md` guide this Hex ships, which an agent installs
  with `npx skills add anomalyco/hex --skill hex-annotate`. Every command
  resolves its capture as `hex capture list` does (bundle name, path, or
  `latest`, in `capture.folder`) and refuses an unknown one with
  `not-found`. While the lab is off all four print how to turn it on and exit
  1.
- Install agent skill from Annotate: the Annotate pane gains an **Agents**
  section (every OS; off by default, `labs.annotate_agent_install`, in the
  Annotate group). Off, the pane is exactly as before. One row per coding
  agent says whether its folder exists and whether it already has this Hex's
  skill: Claude Code at `$CLAUDE_CONFIG_DIR` or `~/.claude` plus
  `skills/hex-annotate/SKILL.md`, Codex at
  `~/.agents/skills/hex-annotate/SKILL.md` (found with `~/.codex` or
  `$CODEX_HOME`; `$CODEX_HOME/skills` is the legacy place),
  OpenCode at `$XDG_CONFIG_HOME` or `~/.config` plus
  `opencode/skills/hex-annotate/SKILL.md`, and Pi at
  `~/.pi/agent/skills/hex-annotate/SKILL.md` (`$PI_CODING_AGENT_DIR` or
  `~/.pi`). `npx skills` writes Codex and Pi skills to
  `~/.agents/skills`, so a copy there counts for either: read by content,
  never duplicated, and an outdated shared copy is the one installing updates.
  A found agent without the skill reads Not installed and starts checked; a
  different file reads Update available. **Install** writes the same embedded
  `skills/hex-annotate/SKILL.md` that `hex annotate skill` prints into each
  checked agent's folder — directories created, a staging file renamed into
  place, no other file touched — and a not-found agent has no switch, is never
  checked, and gets no folder; Install is disabled while nothing is checked.
  A symlinked `SKILL.md` or `hex-annotate` folder is managed elsewhere: the
  row says Managed elsewhere, it has no switch, and nothing is written through
  the link. Beside it, `npx skills add anomalyco/hex --skill
  hex-annotate` has Copy, for people who manage skills themselves. On macOS,
  **Command-line tool** links `hex` (this app's executable) into
  `/opt/homebrew/bin` or `/usr/local/bin` when one exists and is writable,
  else `~/.local/bin`, and never replaces a `hex` another program owns; a
  link whose target is gone counts only when it names a Hex app, a Hex running
  from App Translocation refuses to link and says to move Hex to Applications
  first, and a `~/.local/bin` link `$PATH` doesn't name says so. While Labs ›
  Agent tools for captures is off, a note names that switch for the skill's
  `zoom`/`inspect`/`frames` commands.
- Vocabulary files: the Vocabulary sheet gains **Import…** and **Export…**
  beside Add term (every OS; off by default, `labs.vocabulary_files`). Off,
  the sheet is exactly as before. Import opens one CSV or TXT file, refuses
  one over 1 MiB or not UTF-8, and reads Microsoft Teams' custom dictionary
  columns: column 1 is the term and column 2 its comma-separated "sounds
  like" spellings, with a first header row naming the column "Term" or
  "Term (required)" (any case) left out; further columns are ignored.
  Quoted fields keep commas, quotes, and line breaks, and CRLF or LF ends a
  row. Blank rows and rows without a term are left out. A file that isn't a
  regular file (a folder, a device, a FIFO), is over 1 MiB, isn't UTF-8, or
  ends inside a quoted field is refused with that reason, so nothing opens a
  FIFO and stalls the window.
  Lines split and merge as the sheet's own fields do: a term already listed
  in any case keeps its entry and gains the spellings it lacks, and new terms
  append in file order. A term this file added never also counts as updated,
  and a term counts as updated once however many rows name it. Skipped counts
  each refused term once, whether it is over 64 characters or a new term
  past the list's 500, and each spelling the limits drop, whether it is over
  64 characters or past the 16th for its term. The merged rows then save
  through the same draft path typing uses; a failed save says so instead of
  reporting success. The sheet says "Added 12 terms", "Added 3 terms,
  updated 2", "Updated 2 terms", "Added 3 terms, updated 2, skipped 1", "No
  new terms in that file" only when nothing changed at all, "No terms found
  in that file", or what failed.
  Export opens a save dialog named `hex-vocabulary.csv` (starting in
  Downloads), appends `.csv` only when the name has no extension and
  `<name>.csv` doesn't already exist, and writes the header plus one row per
  entry, its spellings joined by ", " and two empty cells, quoting what needs
  it. The file is written atomically
  (`durable::write`), and the sheet says "Exported N terms" or the error.
  The CSV parser, merge, and writer are `dictation::vocabulary::csv`, with
  no new dependency; the save dialog is the same native
  `desktop::file_dialog` wrapper as open, over Rune's
  `FileDialogs::save`.
- Vocabulary menu: the menu-bar menu gains **Vocabulary ▸** right under
  **Speech Model ▸** (macOS; off by default, `labs.vocabulary_menu`, in the
  Dictation group). Off, the row is hidden and the menu is exactly as before.
  **Add term…** opens Settings on the Vocabulary sheet with a new focused term
  row at its end, which the list glides to, autosaved through the sheet's own
  draft path like Add term; a list that is empty already opens with one focused
  blank row, so no second row is made. **Open vocabulary…** opens the sheet
  alone. Between them, up to the last five saved terms, newest first, as
  disabled informational rows on one line each (whitespace collapsed and cut
  like a meeting title); the separator and the rows are left out while the list
  is empty. The saved list is read only as the menu opens, and only with
  `labs::vocabulary_menu()` on, from
  `dictation::vocabulary::entries()`; the row, submenu, and actions
  (`StatusItemAction::AddVocabularyTerm`, `OpenVocabulary`,
  `refresh_vocabulary`) are in
  [macos_rune/status_item.rs](../../src/macos_rune/status_item.rs), and the
  sheet opens through `rune_ui::vocabulary::open` / `open_new_term` from
  `Pump::status_action`. Checks:
  `vocabulary_lists_the_last_five_terms_newest_first`
  ([macos_rune/status_item.rs](../../src/macos_rune/status_item.rs), macOS
  only), `the_vocabulary_menu_is_a_macos_dictation_lab_off_by_default`
  ([labs.rs](../../src/labs.rs)), and
  `adding_a_term_from_the_menu_opens_the_sheet_with_one_blank_row_last`
  ([rune_ui/vocabulary.rs](../../src/rune_ui/vocabulary.rs)). Not yet observed:
  the submenu on a real Mac; the menu-bar row exists only in macOS builds.
- Paste Last picker: tapping Paste Last pastes the newest ready dictation or
  finished annotation, and holding it (⌥⇧V) opens a picker of recent rows, or
  all of History; History then lists dictations, annotations, screenshots and
  recordings together with one search and a type filter (macOS; Dev-audience,
  off by default). Its switch is `labs.paste_last_picker`, and the gate,
  `labs::paste_last_picker()`, is where the work starts: the picker and its
  gesture, History's unified list and the rows it reads, and the words a
  deliberate cancel keeps. It applies live, without a restart. Off, Paste
  Last and History behave exactly as before. Checks:
  `toggling_the_lab_after_start_changes_what_holding_paste_last_does` in
  [recognition/paste_picker.rs](../../src/recognition/paste_picker.rs); `paste_last_picker_is_its_own_switch_and_macos_only` in
  [labs.rs](../../src/labs.rs); the picker's `paste::browse`,
  `paste::recent`, `paste::gesture` and `paste::capture` tests; History's
  `history::` tests; `one_history_filters_and_searches_the_real_owners_without_enlarging_paste_last`
  in [history/unified/proof.rs](../../src/rune_ui/history/unified/proof.rs);
  and `a_held_shortcut_browses_real_rows_and_clicks_choose_the_original_item`
  in [paste_picker/proof.rs](../../src/rune_ui/paste_picker/proof.rs), a native
  Mac proof that needs a login session and an explicit `--ignored` run. The
  unified History exists only in macOS builds, so its `--snap` scenes
  (`mac-history`, `mac-history-capture`, `mac-history-filtered`,
  `mac-history-types`, `mac-history-search`, `mac-history-search-empty`,
  `mac-history-empty`) render on a Mac and have no Linux golden. Not yet
  observed: a real held shortcut in a desktop login, a receiving app's paste
  of every row kind, and the picker over a real display.
- Live chapters: chapter a meeting's transcript live while it records
  (macOS, Apple Intelligence; see
  [Live Chapters](meetings.md#live-chapters-labs)). Its own switch is
  `labs.live_chapters` (off by default). The gate,
  `labs::live_chapters()`, is where the work starts: the recorder's chapter worker, recording
  chapters in the manifest, and the boundary pass after Stop.
- Run setup again (not a feature toggle; every build): replays Hex's one
  onboarding, the first-run story every new install gets
  ([setup.tour](README.md)). Labs' **Run setup again** and
  `hex onboarding reset` (macOS control socket, the Linux service socket,
  and a named event on Windows) forget that onboarding finished
  (`desktop::onboarding::reset_at`) and open it from the start without
  touching settings, history, models, or permissions; ready pages pass at
  once. Checks: `labs_run_setup_again_opens_the_first_run` and
  `run_setup_again_opens_the_first_run_on_linux_and_windows` in
  [panes.rs](../../src/rune_ui/tests/panes.rs).
- Live text: a streaming dictation shows what you have said so far while
  you dictate, as one click-through line just past the recording HUD (every
  OS; see [Streaming Finish](dictation.md#streaming-finish)). Off macOS the
  line is placed with the HUD's own edge placement (`OverlayHandle::place`),
  so with the HUD at the bottom it sits above the pill and the Windows
  taskbar rather than under the pill (#797; not yet observed on Windows).
  Its toggle
  is `labs.live_text` (off by default), asked as each dictation starts. The
  gate, `labs::live_text()`, is where the work starts: with it off, the engine gives a hotkey capture's
  live stream no partial slot and never reads the model's text between
  feeds, and no host adds the line's overlay. Each dictation's `finished
  live stream` line in process.log counts its text changes (`partials`);
  `RUST_LOG=hex=debug` adds a `live text` line per change with the audio
  fed and the word counts, never the words. Under its description, its card
  adds a line when no words can show: the selected model's catalog
  `ModelDefinition::streams` is false (the line names the streaming models
  from the catalog that take the saved language, and with none for it says
  only that the model doesn't show live words), or, on Linux, Speech engine
  is explicitly CPU (which never streams). An imported model gets no line:
  its catalog entry says it can't stream, but its GGUF is only checked when it
  loads. A resolved CPU on Auto (#856's slow-iGPU verdict) isn't known to the
  window, so that case has no card line. When a capture wants live text and
  no stream opens yet, process.log names the reason once per capture (the
  model doesn't stream, it isn't streaming on this device, or no streaming
  speech model is loaded), and a stream begun while the loaded model can't
  stream logs `a live stream was requested but this model can't stream on its
  device`, or says no speech model is loaded when none could load.
  Checks:
  `a_dictation_stream_publishes_its_text_after_each_feed_with_live_text`,
  `without_live_text_a_dictation_stream_publishes_nothing`,
  `each_dictation_stream_has_its_own_text`, and
  `a_failed_feed_clears_the_text_and_stops_publishing` in
  [transcription/engine.rs](../../src/transcription/engine.rs), and
  `a_stream_refusal_names_the_model_or_the_device` there;
  `live_text_says_which_models_show_live_words`,
  `live_text_suggests_no_model_for_another_language`,
  `live_text_never_judges_an_imported_model`, and
  `live_text_says_when_the_cpu_engine_stops_the_words` in
  [rune_ui/labs.rs](../../src/rune_ui/labs.rs);
  `live_text_follows_the_recording_and_never_outlives_it` in
  [dictation.rs](../../src/dictation.rs); the three tests in
  [dictation/live_text.rs](../../src/dictation/live_text.rs) (a recording's end clears only its
  own text); `a_spot_places_the_plate_just_past_the_hud`,
  `a_short_line_hugs_its_words_in_the_middle`,
  `a_long_line_stays_inside_the_overlay_on_one_line`, and
  `no_words_draw_nothing` in
  [rune_ui/live_text.rs](../../src/rune_ui/live_text.rs), whose opt-in
  `snapshots` renders the line in light and dark; and
  `live_text_hangs_just_past_the_plate_wherever_it_sits` in
  [rune_hud/hud_placement.rs](../../src/rune_hud/hud_placement.rs). The line has no golden
  and no `--snap` scene (overlays have none), and nothing headless checks
  how it appears and hides on screen.
- Failure notices: when a dictation fails, the same click-through plate that
  carries Live text shows why for about 6 seconds, or until the next capture
  starts. It reuses the Live text overlay's window, placement
  (`rune_hud::live_text_place`), and plate styling; the HUD's own coral failure
  flash and its 600 ms stay exactly as they were. The reason travels with the
  HUD's failure event, so no old failure is replayed from the event log at
  launch. Known microphone causes use the same actionable wording as Settings'
  listener card; any other failure keeps its logged message, truncated to one
  line. The gate is `labs::failure_notices()` (toggle
  `labs.failure_notices`, off by default), read when each failure reaches the
  HUD; with it off, no notice is drawn. Every OS shows a notice for a failed
  accepted job. Windows and macOS also show one when the microphone fails to
  open. On Linux, notices cover failed transcriptions only: a microphone-open
  failure never reaches its HUD today. A job failure that arrives while a newer
  capture records is skipped. Checks: `a_failure_notice_lasts_six_seconds`,
  `a_new_capture_dismisses_a_failure_notice_at_once`,
  `only_a_notices_own_deadline_dismisses_it`,
  `a_failure_arriving_after_a_queued_words_end_stays_visible`, and
  `a_failure_notice_takes_the_plate_from_the_live_words` in
  [rune_ui/live_text.rs](../../src/rune_ui/live_text.rs), and
  `failure_notices_is_its_own_bit_and_off_by_default` in
  [labs.rs](../../src/labs.rs). Proof limit: the timing and Labs gate are
  unit-tested; the overlay was not observed on screen on macOS or Windows.
- Live typing: on a wlroots-family Wayland session, a hotkey dictation's
  provisional words are an underlined input-method preedit in the focused
  text field, replaced on each update, and its final text is committed
  through `zwp_input_method_v2` instead of the clipboard paste
  (`labs.live-typing`; the toggle is `labs.live_typing`, off by default, Linux
  only, and its card is hidden in an X11 session). On Wayland, the card asks
  the compositor's registry once at window load
  (`linux::paste::wayland_offers("zwp_input_method_manager_v2")`); without
  that global (GNOME) it stays listed, disabled while off, with a line under
  its description that this desktop offers no input method it needs and
  dictations paste normally, and `hex labs` prints the same note in its list
  and after `hex labs on live-typing` (never refusing the toggle). An
  organization's managed lock keeps its own note, and the CLI adds none. It is
  read at the hold threshold, or at the Toggle press, never for a Local API
  dictation, `/streams`, or Voice Action; the final text still goes through
  Output's processing, and History and Paste Last update as they do for a
  paste. Hex creates the input method when the capture becomes intentional
  (the hold threshold, or at once in Toggle) and destroys it at Finish or
  Cancel, including post-release processing, so short taps and chords create
  none and another input method keeps the seat while Hex is idle. A newer
  capture's Begin takes over from an older job, which then pastes. A CPU
  engine or a model that cannot stream shows no provisional words while
  speaking; the final text still commits through the input method. Hex's own
  first-run practice field on Linux is an ordinary focused field, so it
  receives the preedit and commit too while Live typing is on. It falls back
  to today's paste when the compositor has no
  `zwp_input_method_manager_v2` (KWin and Mutter do not offer it, so KDE and
  GNOME paste), another input method holds the seat (`unavailable`), the
  field has no active text input, the final text is multi-line or over 4000
  bytes, or the 350 ms decision passes without a commit: a caller that wins
  the abandonment waits up to 250 ms for the preedit to clear and then
  pastes even if the compositor never answered, while a commit the owner
  already claimed is given up to 750 ms and is never pasted over. After an
  abandoned Finish, Live typing starts no new captures and pastes instead
  until Hex restarts; a session that had already begun still finishes and
  cancels. The claim is one atomic Pending → Committing or
  Pending → Abandoned change, so only one side wins. A residual Wayland race
  can still let a `done` event arrive after the serial is read; that
  preedit or commit is then ignored by the compositor. A commit ignored for
  a stale serial has already returned “inserted”, so that dictation is lost
  rather than pasted (wlroots does not enforce the serial today). The
  plate stays off unless Labs › Live text itself is on: a Live typing capture
  takes `dictation::live_text::TypingDemand` at that same intentional start,
  before the stream opens, so the engine sees it but the plate does not, and
  the plate decision is stored once when the stream opens. Checks in
  [linux/live_typing.rs](../../src/linux/live_typing.rs):
  `provisional_words_become_one_preedit_line`,
  `a_preedit_stops_at_a_character_boundary`,
  `only_one_line_text_within_the_limit_commits_through_the_input_method`,
  `the_cursor_sits_after_the_preedit`,
  `done_events_number_the_commit_serial_and_apply_activation`,
  `the_owner_wins_the_commit_before_queueing_it`,
  `a_waiting_caller_wins_abandonment_before_the_owner_claims`,
  `a_started_session_cancels_when_dropped`,
  `a_session_that_never_started_sends_nothing`,
  `finishing_a_session_never_sends_cancel`,
  `only_the_newest_preedit_waits_and_it_wakes_once`,
  `a_full_control_queue_stops_new_sessions_but_not_old_ones`, and
  `a_stopped_owner_still_finishes_and_cancels_a_session_that_began`;
  `a_typing_demand_feeds_the_engine_without_wanting_the_plate` in
  [dictation/live_text.rs](../../src/dictation/live_text.rs); and
  `the_plate_follows_the_decision_made_when_the_stream_opened` in
  [dictation.rs](../../src/dictation.rs); and
  `live_typing_says_when_the_desktop_cannot_use_it` in
  [rune_ui/tests/panes.rs](../../src/rune_ui/tests/panes.rs). **Not verified
  on a real compositor**: only unit tests and the build, plus the Linux Labs
  golden's new card.
- Playground: Rune's playground in the window (every OS), `labs.playground`
  (off by default, applied at once). While on, a **Playground** sidebar item,
  ⇧⌘D (View › Playground on macOS), or Ctrl+Shift+D swaps the window for a list of
  folded groups, nested by path:

  ```ts
  Prototypes        stories that aren't the window
  ├── Onboarding    the options and First run
  ├── Meetings      Live notes: a canned call's chapters and notes under a
  │                 draggable capsule, hidable, copied whole, new, or by
  │                 chapter (nothing real runs; live_notes.rs)
  ├── HUD           (macOS) each placement × look × ending, Processing, Your voice + live HUD Lab Dot scope sliders & Save
  └── Capture capsule   (macOS) whole flow and each state × flow, notch, family
  Window            every macOS-window fixture scene by pane, under a readable name
  ├── General       Settings, Microphone missing, Shortcut/…, Updates/…
  ├── Output, Modes … HUD Lab, Activity, Transcription model, Shortcut explainer, Onboarding
  Linux & Windows   the portal and Windows fixtures
  ```

  Every group starts folded; opening a story, from the list or a link,
  unfolds the groups above it, and folds hold until Hex quits. The list
  keeps only its own keys (arrows, Return, J/K, a story's trigger letters,
  Tab, Esc, `/` to filter) and ⇧⌘D; every other key, ⌘-chords and digits
  included, reaches the open story. Each
  story starts from a fresh fixture; the real window keeps running
  underneath. With it on at launch, the macOS window resizes, so the stage
  beside the playground's columns has room. Off, no shortcut, menu item,
  sidebar item, or link opens it.
  Headless renders take `--playground [story]` while it is on, e.g.
  `hex app -- --snap out.png --playground "Window/General/Settings"`. Checks: `every_scene_but_live_has_a_path_under_a_group`,
  `the_playgrounds_keys_work_with_a_story_open`, and
  `a_scene_opened_from_the_list_hears_command_digits` in
  [playground.rs](../../src/rune_ui/playground.rs).
  First run's side controls tune rise, leave, travel, stagger and keyboard
  arrival; Quick motion and Gentle motion are presets. The real first run
  uses the default values from
  [landing.rs](../../src/rune_ui/onboarding_kit/landing.rs): zero-bounce
  vertical entrances/exits, with each stagger delaying the whole entrance.
  The demonstration's second send removes its input field in the same layout
  change, so the card contracts once while the old field fades inside it.
  Polling follows the display clock; the scripted HUD samples between 60 Hz
  frames, the download HUD advances one retained simulation, and the microphone
  bars follow the level continuously. A real press keeps the waiting HUD open;
  completion collapses it until another press. The compositor draws a waveform
  per effect slot, retaining an outgoing page's waveform through its fade. The
  `the_hud_advances_between_sixty_hertz_frames` regression checks different
  half-frame poses, and
  `pressing_keeps_the_waiting_hud_open_and_completion_stays_gone` checks the live
  HUD on a virtual clock. A headless story recording does not prove native display
  pacing or a real permission grant.
  On macOS a story opens from a link or the CLI:

  ```ts
  open 'hex://story/prototypes/hud/notch?variant=pulse-fail&play=1'   // hex-test:// in Hex Test.app
  open 'hex://story/prototypes/capture-capsule/suggestions?variant=toast'
  hex window story Prototypes/HUD/Processing [--variant 2] [--play]
  ├── the window opens on the story: its path, name, or slug, `/` or `-` between groups and name
  ├── a link from before the groups nested (`hud/processing`, `General/mac-settings`, `mac-settings`) → that story now
  ├── variant=<n|name> → that variant
  ├── play=1 → its `r` trigger runs, as pressing R does
  ├── playground off → only the window opens; process.log says why, the CLI answers feature-off
  └── another link, or longer than 2 KiB → ignored and logged
  ```

  An unknown story opens the list and an unknown variant the story; Rune
  logs which to stderr. Checks: `reads_story_links`, `links_from_before_the_list_nested_still_open_their_story`, `ignores_other_links`
  in [story_link.rs](../../src/rune_ui/story_link.rs);
  `every_hud_story_and_variant_opens_from_its_link` and
  `every_capsule_story_and_flow_opens_from_its_link` in
  playground.rs open every HUD and capsule link headlessly. The URL
  handler and the replay have no automated end-to-end check.
- Voice Filter: the Voice Filter pane (macOS and Linux), the only home of
  the Hex Microphone (macOS's driver, Linux's PipeWire virtual source), its
  filter modes, and Try it
  ([Hex Microphone](README.md#experimental-hex-microphone-and-voice-filter)).
  Its toggle is `experimental.hex_microphone` (off by default). Off, the pane
  is hidden and Hex neither reads the driver's status nor starts its sink
  supervisor. While the driver is installed the Voice Filter runs (apps using
  the Hex Microphone keep their audio), its pane shows, and its Labs row is
  on and disabled: "On while the Hex Microphone driver is installed.
  Uninstall the driver in Voice Filter to turn it off." Checks:
  `voice_filter_shows_only_while_its_labs_toggle_is_on` and
  `an_installed_driver_keeps_the_voice_filter_on` in
  [panes.rs](../../src/rune_ui/tests/panes.rs);
  `the_setting_or_an_installed_driver_turns_the_voice_filter_on_and_labs_never_does`
  in [app_settings.rs](../../src/app_settings.rs).
- Meetings (beta): see [Meetings](meetings.md). Its toggle is
  `meetings.enabled`; on, Meetings is a top-level sidebar item (debug builds
  keep it available regardless).
- Stop every call recording when the call ends (macOS, with Meetings on): the
  recorders Hex did not start on its own stop the way Always record's do
  under Stop when the call ends. Its toggle is
  `labs.stop_any_call_recording` (off by default), and it needs nothing from
  `meetings.stop_when_call_ends`: that switch keeps governing Always record,
  and this one governs everything else. On, a recording accepted from a
  detection offer and a recording started from a calendar reminder stop 30 s
  after the call app it watches leaves the microphone and goes quiet, with the
  same 3 s absence, Keep recording, and call-comes-back logic
  ([Meetings › Stop Other Call Recordings](meetings.md#stop-other-call-recordings-labs)).
  A recording started by hand (Meetings' Start recording, the menu bar, `hex
  meetings record`) takes its call app from the microphone at start: the one
  native call app holding it right then, by the canonical bundles detection
  uses, with browsers left out so no page is read and none or several leaving
  the recording unwatched. The recording keeps its own title, app, and
  capture; only its stop is associated. The absence rule is all it adds: a
  browser that never releases the microphone between calls (Arc) is still
  ended only for a recording Hex started itself. Turning the lab off while a
  stop counts drops it. Checks:
  `the_lab_stops_on_request_recordings_and_the_setting_keeps_always_record`,
  `a_recording_kind_knows_whether_it_watches_a_call_app`, and
  `one_native_call_app_is_watched_and_a_browser_or_two_are_not`
  ([meeting/watcher/call_end.rs](../../src/meeting/watcher/call_end.rs));
  `only_an_automatic_browser_call_with_a_code_keeps_reading_its_page`
  ([meeting/watcher.rs](../../src/meeting/watcher.rs)). Not observed on a real
  Mac or a real Slack huddle.
- Import audio (macOS): Meetings offers **Import audio…** beside Start
  recording. A local WAV, M4A, MP3, AIFF, or CAF file is decoded to 16 kHz
  mono float WAV by the AVFoundation bridge, then transcribed with the
  selected speech model as a normal meeting: its turns are labelled Speaker,
  it gets the user Markdown export, and notes and summaries run afterward when
  the meetings setting asks for them. Its toggle is `labs.meeting_import`
  (off by default). The button, and the watcher before it spawns a worker,
  both read `labs::meeting_import()`, which includes an organization's Labs
  lock; with the lab off the button hides and an import request is refused
  with Audio could not be imported, so no decoder or worker runs. While an
  import runs the button stays visible, and another choice is refused until it
  finishes. CLI/MCP query output, chapter prompts, notes input, the internal
  transcript.md, and Copy Last Transcript still say You/Them for imported
  meetings; only the pane and the user Markdown export say Speaker. Quitting
  cancels the decode and removes its folder; a crash or a quit past the drain
  leaves import.pending with no manifest, which the next launch's sweep
  removes once its lock is free. Checks:
  `meeting_import_is_its_own_bit_and_off_by_default`
  ([labs.rs](../../src/labs.rs));
  `only_the_listed_audio_files_are_imported`,
  `the_title_is_the_file_name_without_its_extension`,
  `decodes_a_stereo_float_wav_to_mono_16_khz`, and
  `decodes_a_stereo_16_bit_wav_to_mono_16_khz`
  ([meeting/import.rs](../../src/meeting/import.rs), macOS);
  `only_abandoned_import_folders_are_removed`
  ([meeting/sweep.rs](../../src/meeting/sweep.rs));
  `imported_meetings_keep_their_flag_and_older_manifests_read_as_recorded`
  ([tests.rs](../../src/meeting/tests.rs));
  `an_empty_track_transcribes_to_nothing` and
  `imported_meetings_label_their_one_track_speaker`
  ([transcript.rs](../../src/meeting/transcript.rs)); the Linux-only pane test
  `an_imported_meeting_labels_its_one_track_speaker`
  ([rune_ui/meetings.rs](../../src/rune_ui/meetings.rs)). The decoder tests run
  in Check macOS. Not verified on a real Mac: the native picker and quitting
  at each mid-import moment have no automated end-to-end check.
- Words on screen (macOS): each dictation may read the focused window's
  visible text through Accessibility as temporary vocabulary. Off by default
  (`labs.screen_vocabulary`). Parakeet reads the terms only with Vocabulary
  help on and a boost other than Off; Whisper and Qwen3-ASR read them as a
  prompt; Apple Speech and models that take no vocabulary read nothing. A
  capture with terms for such a model uses its whole-clip run rather than a
  live stream's text, so the text arrives after release with a little more
  latency. Its read bounds and extraction are above, under "Words on
  screen".
- Learn from corrections (macOS): after a dictation is pasted, Hex watches
  only the field it pasted into, about every 100 ms for up to 5 minutes. The
  first read retries for up to a second until the app shows the pasted text,
  and the comparison uses what Hex actually inserted, not the transcript. It
  stops on a focus change, an empty or vanished field, a newer paste, or the
  timeout; the lab turned off stops it too and learns nothing, and a read a
  newer paste overtook is discarded. It adds up to 5 name-like corrected
  words to Vocabulary (capitalized mid-sentence, all caps, mixed case,
  letters with digits, and short phrases of those; a word after a line break,
  colon, or sentence end is never name-like, and neither is I'm); password
  fields, fields over 20,000 characters, text over 400 words, rewrites (more
  than 3 inserted words and over 60% changed), plain lowercase words, and
  terms already in Vocabulary are skipped. Only a real Paste starts a watch
  (never a copy, a Service or Print output, or a YOLO send); other pastes
  into an app (Paste Last, a meeting turn, a capture's sheet) still move the
  paste generation. Each learned term is saved as `hex vocabulary add` saves
  it, which an open window adopts. The one logged event holds the term count
  only. Nothing leaves the Mac. Off by default
  (`labs.learn_from_corrections`).
- Terminal paste (Windows): paste with Ctrl+Shift+V instead of Ctrl+V for
  apps that ignore a synthetic Ctrl+V (issue 477 / PR 478). Its toggle is the
  shared `paste_with_shift`; Linux keeps it as a General › Advanced row.
- Terminal paste shortcut (Windows, Linux): another dictation shortcut whose
  paste is always Ctrl+Shift+V, whatever Terminal paste says. Its own switch
  is `labs.terminal_paste_shortcut`, and General shows its recorder while on.
- OpenCode in WSL (Windows): Modes rewriting, the Modes model picker, and
  Annotate's capture sends use an OpenCode service running inside the default
  WSL distribution. With the lab on, a WSL service that answers is used ahead
  of a Windows one; the Windows service is used when WSL answers nothing. WSL2
  forwards loopback, so Hex dials the registered URL as usual; what it cannot
  see without this lab is the registration, which lives in the distribution's
  home.

```ts
Settings › Labs › OpenCode in WSL on            // labs.opencode_in_wsl
├── a WSL registration answers /api/info -> it wins over a Windows one; the Windows service
│   is used when the WSL wait answers nothing, so Windows comes back only after that wait
├── Windows already confirmed a service -> the WSL lookup waits at most min(the caller's
│   remaining budget, WSL_LOOKUP_CAP 3 s), so a cold WSL can't hold a rewrite or Annotate's send
├── each search -> read HKCU\…\Lxss\DefaultDistribution, DistributionName and DefaultUid on every lookup
│   ├── a cached home is reused only when DistributionName and DefaultUid still match; otherwise
│   │   rebuild it from passwd: \\wsl.localhost\<distribution>\etc\passwd, else \\wsl$\<distribution>\etc\passwd
│   │   on older Windows 10 (at most 1 MiB) -> the line whose uid is DefaultUid -> its (one trailing / trimmed) home
│   ├── only the default distribution is opened; opening it can start the WSL VM too
│   ├── state dir -> <home>/.local/state/opencode; same service.json order, cap, loopback, /api/info, password
│   ├── found -> Hex's workspace is created at <home>/.local/share/hex/opencode once, and every
│   │   x-opencode-directory names the Linux path <home>/.local/share/hex/opencode
│   └── the cache is dropped when its state dir can't be read, when the default distribution or
│       uid changes, and on the next search that runs with the lab off
├── one lookup runs at a time on a shared helper thread, with its own WSL_LOOKUP_CAP (3 s) budget.
│   Its starter keeps its full budget; a joiner waits at most min(its own budget, the cap minus the
│   lookup's age) and returns "OpenCode isn't running" at once once the lookup is older than the cap.
│   Each wait checks Escape in WAIT_SLICE slices; a timeout or cancel never aborts the others
├── a lookup that ran into its cap without a service pauses new lookups for 10 s, so a hung WSL
│   doesn't make every rewrite wait out its deadline; a later try finds it once it starts
├── no WSL registration, no home, or a wait that ran into its cap -> the Windows service when one
│   already answered; with neither, OpenCode isn't running, or the update notice when one answered
│   /api/info with 404
├── Annotate's capture send asks the same lookup first (src/capture_host/routing/opencode.rs)
│   ├── a WSL service answers the router's local requests; when WSL answers nothing, a native Windows
│   │   one leaves the router's own registration path unchanged
│   └── the prompt's "Recorded at" line names the capture folder as the WSL server sees it:
│       X:\a\b -> /mnt/x/a/b, following [automount] root = (one trailing /, else /mnt/);
│       enabled = false, a UNC share, or a relative path leaves the Windows text. The
│       summary header keeps the Windows path
└── Labs off -> nothing reads the registry or any WSL path; Hex is exactly as before
```

The pure parts (the passwd line for a uid, a Linux home to its UNC path, the
Linux workspace string, `wsl.conf`'s `[automount]` (read once with the
home) and a drive path under its root, the cache identity, the shared lookup
and its skip window) live in
[desktop/opencode_service/wsl.rs](../../src/desktop/opencode_service/wsl.rs);
the registry and `\\wsl.localhost\`/`\\wsl$\` reads are Windows-only. Nothing
runs `wsl.exe`, and the passwd contents and password are never logged. A zero
budget is refused before any registry or UNC read. A scoped request to a WSL
service that fails (a transport error or 404) forgets the workspace, so the
next request retries `create_dir_all`.

Limits: `XDG_STATE_HOME` set inside WSL is ignored (Hex always reads
`<home>/.local/state/opencode`); a home that isn't in `/etc/passwd`, such as
one from LDAP or SSSD, isn't found; Annotate's recorded-at line maps only
Windows drive paths, so a UNC or relative capture folder keeps its Windows
text (the summary header always keeps it).
Checks: `the_passwd_line_for_the_uid_names_its_home`,
`both_wsl_prefixes_name_the_distribution`,
`a_distribution_or_home_windows_cant_reach_is_refused`,
`a_zero_budget_or_cancellation_skips_the_lookup`,
`the_cache_follows_the_default_distribution_and_uid`,
`one_lookup_serves_every_caller_and_a_short_wait_stops`,
`a_joiner_does_not_wait_for_a_lookup_older_than_its_cap`,
`a_timed_out_lookup_defers_the_next_one`,
`the_workspace_is_created_somewhere_windows_can_write_it`,
`the_automount_section_gives_the_root_and_can_be_off`,
`windows_drives_become_paths_under_the_automount_root`, and
`a_capture_folder_follows_the_automount_read_with_the_home` in
[wsl.rs](../../src/desktop/opencode_service/wsl.rs);
`a_wsl_service_gets_the_workspace_its_linux_server_can_use`,
`a_wsl_service_maps_a_capture_folder_into_its_distribution`,
`a_wsl_service_wins_with_the_lab_on_and_the_windows_one_is_the_fallback`,
`a_wsl_lookup_is_capped_only_when_windows_already_answered`,
`a_failed_wsl_request_forgets_the_workspace`, and
`a_not_found_wsl_request_forgets_the_workspace` in
[opencode_service.rs](../../src/desktop/opencode_service.rs);
`local_requests_ask_the_hosts_opencode_first`,
`a_local_request_falls_back_when_the_host_has_no_opinion`,
`a_hosts_refusal_keeps_its_http_status_and_what_opencode_said`, and
`a_sent_prompt_names_the_folder_as_the_hosts_server_sees_it` in
[crates/router/src/opencode.rs](../../crates/router/src/opencode.rs);
`a_wsl_request_failure_reads_as_the_capsule_expects` in
[src/capture_host/routing/opencode.rs](../../src/capture_host/routing/opencode.rs);
`opencode_in_wsl_is_its_own_bit_and_windows_only` in
[labs.rs](../../src/labs.rs). Not yet observed: a real Windows PC with a
running OpenCode inside WSL, including whether the UNC read starts the
distribution and whether Annotate's send reaches it. The
`windows-labs` golden shows the new card.

Only on speakers (`speakers-only`, every OS)
puts "While dictating" behind the default output. While on, each dictation
session and each screen-capture session asks the route as it starts, on the
recording environment's worker thread (macOS) or the audio-control worker
(Linux and Windows), never the event tap's or the dictation loop's, and with
headphones it holds no audio effect for that session: no mute, no lower, no
media pause. On macOS an overlapping session that shares an already-held
effect reuses the decision the first session made when it took it. Dictation
still prevents idle sleep, and screen captures never do. Speakers, and
anything the route can't classify, behave exactly as before, and with the lab
off every recording behaves as it always did, and nothing reads the output at
all: macOS never asks CoreAudio, Linux never runs
`pactl`, Windows never asks cpal, and a session with "While dictating" set to
Do nothing never asks either. The macOS backend
([audio/output_route.rs](../../src/audio/output_route.rs)) reads
the default output's CoreAudio transport type, its output-scope data source
(`hdpn` on the built-in jack), and its UID (`BuiltInHeadphoneOutputDevice`):
Bluetooth and Bluetooth LE are headphones whatever they are, a built-in
device is headphones while the jack source is selected. Linux reads `pactl
get-default-sink` and `pactl -f json list sinks` with the same helper its
microphone route uses: the default sink is headphones while its active port
names headphones or a headset (`analog-output-headphones`, `[Out] Headset`),
or its `device.form_factor` is headphone or headset, and a Bluetooth sink
(`device.api` `bluez5`) is headphones unless its form factor says speaker.
Windows reads the default output's WASAPI form factor through cpal: headphones,
headset, earpiece, and handset are headphones, speakers are speakers, and
anything else is unknown. Limits:
Bluetooth speakers read as headphones on macOS (transport alone) and on Linux
unless PipeWire names them speakers; an external USB headphone DAC reads as
speakers; a Linux session with only `wpctl` and no `pactl`, or an older
`pactl` without `get-default-sink`, reads unknown, so "While dictating"
applies; and the hotkey pre-roll follows the While dictating setting, not the
route, so a skipped recording keeps the setting's pre-roll. Checks:
`bluetooth_outputs_are_headphones`,
`a_built_in_output_on_its_headphone_jack_is_headphones`,
`apples_wired_headphone_device_is_headphones`,
`everything_else_is_speakers`,
`an_analog_sink_is_headphones_only_on_a_headphone_port`,
`a_headset_port_is_headphones`,
`bluetooth_sinks_are_headphones_unless_they_say_speakers`,
`a_sink_the_sound_server_does_not_name_is_unknown`,
`a_non_string_property_does_not_hide_the_others`, and
`cpal_headphone_types_are_headphones` in
[output_route.rs](../../src/audio/output_route.rs);
`speakers_only_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: a real Mac with wired and
Bluetooth headphones and the lab on, and the Linux and Windows backends on a
real desktop.

Transcribe dropped audio (`drop-transcribe`, every OS): while on, a file
dropped anywhere on Hex's window, or chosen with the lab card's "Transcribe a
file…", joins one queue the window runs one file at a time.

The drop target is the window root
([rune_ui/drop_transcribe.rs](../../src/rune_ui/drop_transcribe.rs)):
`on_drop_files` takes the files whose extension this build reads
(`file_transcript::EXTENSIONS`), `on_drop_hover` shows the hint over the whole
window while a file drag is on it, and a drag with nothing Hex reads says what
it reads instead ("Hex can transcribe WAV, MP3, AAC, M4A, MP4, MOV, AIFF,
CAF, FLAC, OGG, Opus, WebM, or MKV files") through `on_drop_rejected_hover`,
and again as a toast if it is dropped. With the lab off the window sets no
drop handler at all, so nothing of this draws or runs. A file's toast shows
its name, a ring and percentage of
the audio decoded (the backend's progress callback; a spinner and
"Transcribing…" when the file's length is unknown), how many files wait
behind it, and Cancel, which stops the running file, empties the queue, and
leaves that file's toast saying "Cancelling…" with no Cancel of its own until
its worker answers. The finished toast says "Saved <name>", the full path, and
Show in folder (the host's meeting `Reveal` request, as the Meetings pane
reveals its Markdown, so a preview or headless window records it instead of
opening a file manager); a failure shows the `FileTranscriptError`'s own
words. Both dismiss with their ✕, and the next file starts on its own.

The queue runs on one named worker per file (`hex-transcribe-file`) with the
dictation model and language in use when the file starts, so a drop never
waits on the window; the window wakes on the worker's channel, as History's
Transcribe again does. Files keep their order and exactly one worker runs at a
time: Cancel clears the files waiting behind the running one and marks it
cancelling, and the job stays tracked until its worker reports Done or its
channel closes. Only then does the next queued file start, so a drop right
after Cancel — or the lab turned off and on again — queues behind the old
worker instead of starting a second one and a second `TranscriptionService`
while the first still loads or runs. A file's own gate
(`labs::drop_transcribe`) is checked again inside the backend when it starts;
turning the lab off while a file runs cancels it from the window, but a worker
that still saves writes its transcript without a toast saying so.

The backend
([transcription/file_transcript.rs](../../src/transcription/file_transcript.rs))
starts its own `TranscriptionService` on the calling thread, so it must run on
that worker, never the window thread. A file's kind is checked against this
build's extensions before a model loads, and every container decodes through
the shared pieces reader
([transcription/pieces.rs](../../src/transcription/pieces.rs)): a WAV is read
whole once before the model loads, as `hex transcribe` does, so a truncated or
corrupt file is refused up front, and compressed audio decodes once, as it
transcribes, with damage found later named where it is (`12:34` into the
file). The drop target and the card's dialog offer the same common
extensions on every OS, one set per container the shared reader takes, which
detects the format from the file's
contents rather than the name: `wav`/`wave`, `mp3`, `aac`, `m4a`/`m4b`,
`mp4`/`m4v`/`3gp`, `mov`, `aiff`/`aif`/`aifc`, `caf`, `flac`, `ogg`/`oga`,
`opus` (an Ogg file), `webm`/`weba`, and `mkv`/`mka`. Every
format is downmixed to mono, resampled to 16 kHz, cut at most every 30 s, and
each
piece is sent to the service in order, which skips one the speech detector
hears nothing in; the pieces' text joins with one space. A piece runs on the dictation engine's
loaded copy when that copy is the model the file asks for and dictation stays
idle for up to `SHARED_WAIT` (20 s,
[service.rs](../../src/transcription/service.rs)); otherwise the service
loads its own copy, so a file keeps moving while dictation is busy or the
engine holds another model. A piece that holds the engine can delay a
dictation released while it runs, for at most that piece (30 s); a dictation
that stays locked past `SHARED_WAIT` makes the service load its own copy of
the model for the file instead. The whole transcript is written as
`<stem>.txt` in the source's folder, never replacing an existing file
(`<stem> 2.txt`,
`<stem> 3.txt`, …), through a reserved name and an atomic rename. A file in the
OS temp folder or in an unwritable folder has its transcript written to
Downloads; a writable source folder needs no Downloads at all. Cancelling
stops between pieces, every 100 ms while one waits for its transcript, and
once more before the transcript is saved, so a cancel before the final check
writes nothing.
The worker answers only after the piece already decoding or transcribing
finishes, and after a cold load of the service's own model, so the next file
starts only then. Progress is the fraction of the file decoded; a compressed
file whose header gives no duration reports no fraction, which the toast shows
as "Transcribing…" with a spinner rather than 0%.

Entry points: a file drag on the window (macOS, Windows, and Linux X11), and
Labs › Transcribe dropped audio's card button (every desktop, including
Wayland, which reports no file drags). Nothing is pasted, saved to History,
or processed.

Checks: `a_dropped_file_does_nothing_with_the_lab_off`,
`a_dropped_file_hovers_then_queues_a_job`,
`dropped_files_queue_one_at_a_time`,
`cancel_stops_the_running_file_and_the_queue`,
`a_drop_after_cancel_waits_for_the_cancelled_worker`,
`turning_the_lab_off_cancels_the_running_file_and_waits_for_it`,
`a_cancel_after_the_last_check_still_says_saved`,
`a_file_of_unknown_length_says_transcribing_without_a_percentage`,
`a_file_hex_cannot_read_is_refused`,
`a_saved_transcript_says_where_it_is_and_dismisses`,
`the_labs_card_offers_a_file_choice_only_with_its_lab_on`, and
`the_drop_surface_shows_nothing_at_rest` in
[drop_transcribe.rs](../../src/rune_ui/tests/drop_transcribe.rs) (headless
Linux, with a stand-in worker, so no model loads); the goldens `drop-hover`
and `drop-running` in both appearances; `the_first_free_name_numbering_counts_up`,
`a_transcript_never_overwrites_an_earlier_one`,
`a_file_beside_its_source_keeps_its_own_folder`,
`a_writable_source_folder_needs_no_downloads_folder`,
`an_unwritable_folder_sends_the_transcript_to_downloads`,
`a_file_in_the_temp_folder_sends_the_transcript_to_downloads`,
`a_temp_folder_reached_through_a_symlink_still_sends_the_transcript_to_downloads`,
`still_wanted_refuses_a_cancelled_file`,
`a_truncated_wav_is_refused_before_anything_is_transcribed`,
`a_checked_wav_still_reads_from_its_first_sample`,
`only_the_formats_this_build_reads_are_accepted`,
`every_offered_extension_is_named_in_the_copy`,
`every_extension_on_offer_reads_a_fixture_under_that_name`,
`every_labs_line_names_the_formats_this_build_reads`,
`an_unsupported_file_is_refused_before_anything_loads`,
`a_file_without_audio_and_an_unknown_codec_say_what_is_wrong`, and
`the_bundled_mp3_decodes_to_sixteen_kilohertz_mono` in
[file_transcript.rs](../../src/transcription/file_transcript.rs);
`drop_transcribe_is_its_own_switch_and_everywhere` in
[labs.rs](../../src/labs.rs); and the pieces tests in
[pieces.rs](../../src/transcription/pieces.rs).

Proof limits: the drop tests drive Rune's headless `drag_files`/`drop_files`,
so a real Finder, Explorer, or file-manager drag is unobserved; Wayland
reports no file drags, so there only the card's dialog works, and its native
picker is unobserved here too; no model transcribed a dropped file, so no
dropped file ran through a model end to end (each offered extension's fixture
was read by the shared reader only); on macOS, writing a transcript beside a
file in Documents, Desktop, or Downloads may trigger that folder's access
prompt, not observed; and no transcript saved by the toast was opened by a
person.

Checks: `labs_is_one_flat_list_of_toggles_off_by_default`,
`contributed_options_are_labs_toggles_off_by_default`,
`the_browser_labs_toggle_turns_on_and_asks_for_system_audio`,
`keyboard_model_switching_is_a_labs_toggle_wherever_layouts_are_observable`,
`voice_filter_shows_only_while_its_labs_toggle_is_on`, and
`an_installed_driver_keeps_the_voice_filter_on` in
[panes.rs](../../src/rune_ui/tests/panes.rs);
`app_model_switching_is_a_labs_toggle_on_macos_and_windows`,
`a_mode_picks_an_installed_speech_model_and_language`, and
`the_speech_row_needs_the_lab_and_a_capable_os` in
[speech_model_tests.rs](../../src/rune_ui/speech_model_tests.rs);
`every_feature_is_off_by_default_and_round_trips` and
`the_old_master_switch_is_read_and_ignored` in [labs.rs](../../src/labs.rs);
`a_dev_feature_in_a_stable_build_is_unlisted_refused_and_off` in [labs.rs](../../src/labs.rs) (a stand-in for a Dev feature, since none exists yet); `labs_parse_as_a_read_command` in [main.rs](../../src/main.rs);
`status_reads_for_a_person` and
`status_reports_labs_and_reads_older_replies_without_it` in
[control/cli.rs](../../src/control/cli.rs);
`every_persisted_setting_has_a_cli_key` in
[control/settings.rs](../../src/control/settings.rs). Passed on Linux on
2026-10-30; the macOS build was not checked, and the Windows build was
type-checked only.
