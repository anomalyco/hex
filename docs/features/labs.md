# Labs

Labs holds features we're unsure about, **especially features people asked
for that we built**: they land in Labs, off by default, instead of shipping
on. Each feature also has an **audience** (`labs::Feature::audience`, one
`match` beside `on_this_os`, so a new feature must choose): `Everyone` ships
in every build; `Dev` exists only in dev builds (`dev_build::is_dev_build`:
debug, local `HEX_LOCAL_BUILD`, test `HEX_TEST_BUILD`, and dev-channel
`HEX_BUILD_CHANNEL=dev` builds). A stable build never lists, accepts
(`hex labs on`, `hex settings set`), or runs a Dev feature: `available()`
drops it, its gate reads false even when `settings.json` says on, and the
saved setting is kept untouched. In dev builds a Dev row carries a quiet
"Dev" tag, and `hex labs` prints each feature's audience. Every feature
below is `Everyone` today unless marked `(Dev)`. Each feature also has a
**group** (`labs::Feature::group` in `src/labs/group.rs`, one exhaustive
`match`, so a new feature must choose): Dictation, Output (paste, formatting,
spoken punctuation), History, Meetings, Annotate, Shortcuts, or Models. There
is no master switch. **Labs** is always the last item in the sidebar and lists
every Labs feature this OS has as a card (name, one-line description, toggle)
under its group's collapsible header, each **off by default**, plus Send
feedback and Run setup again.

```ts
Feature             Pane  Toggle (settings.json, hex settings)   On: what it does
├── Meetings (beta) yes   meetings.enabled             (macOS)   Meetings joins the sidebar: call detection, recording, transcripts
├── Voice Filter (Dev) yes experimental.hex_microphone (macOS, Linux) Voice Filter joins the sidebar; feeds the Hex Microphone (macOS driver, Linux PipeWire source)
│                                        a stable build never feeds it; with the macOS driver installed it keeps the pane with Uninstall alone
├── Playground (Dev) yes  labs.playground                        a Playground sidebar item and ⇧⌘D open the story browser
├── Live text       no    labs.live_text                         provisional words beside the HUD while dictating
├── Live chapters   no    labs.live_chapters           (macOS)   chapters a meeting while it records, boundary pass after Stop
├── Vocabulary biasing no labs.vocabulary_biasing                Parakeet transcription biased toward the vocabulary (#452/#457)
├── Switch model with keyboard language  no  keyboard_model_switching.enabled  (macOS, Windows, X11)
│                                        the input source picks its mapped model (#216); General shows the mappings while on
├── Mouse shortcuts no    labs.mouse_shortcuts         (macOS)   mouse buttons record as shortcuts (#209)
├── Also pause the browser no pause_browser_media      (macOS)   pause a playing tab too, with Pause media (#301); General shows its permission row
├── Share dictation shortcut no share_dictation_shortcut (Windows) apps such as Discord see the shortcut too (#176)
├── Unload model when idle no labs.unload_idle + labs.unload_idle_minutes (5/15/30/60, default 15)
│                                        the engine frees the speech model after that long unused (#442)
├── Restore shortcut default no labs.restore_shortcut_default   Paste last dictation's row offers its default back (#613)
├── Keep dictation on clipboard no labs.keep_on_clipboard (macOS) a pasted dictation stays on the clipboard; the previous one is not restored (#200)
├── Copy dictation only no labs.copy_only (macOS)    a dictation goes to the clipboard and is never pasted; Paste last dictation still pastes (#629)
├── Spoken formatting no labs.spoken_formatting      spoken "new line"/"line break", "new paragraph", "open/close quote", "quote … unquote" become line breaks and quotation marks; Spanish "nuevo párrafo", "nueva línea", "abre/cierra comillas", … too (#630, #1040)
├── Spoken symbols  no    labs.spoken_symbols          spoken "hashtag", "underscore", "comma", … become symbols joined to their words: "#release", "my_variable", "done, thanks"; Spanish "arroba", "guion bajo", "coma", … too (#678, #1040)
├── Remove filler words no labs.filler_words          um, umm, uh, uhh, erm, er, ah leave dictations, with no model (#631)
├── Subtitle export no    labs.subtitle_export         (macOS)   hex meetings show --srt|--vtt writes a meeting as timed subtitles (#633)
├── Second dictation shortcut no labs.second_dictation_shortcut + second_dictation_hotkey (macOS)
│                                        another shortcut dictates like the main one (#627); General shows its recorder while on
├── Smart spacing   no    labs.smart_spacing           (macOS)   a dictation pasted right after a word gets one leading space (#632)
├── Sided modifier chords no labs.sided_modifier_chords (macOS) the recorder keeps each modifier's side in a modifier-only chord: R⌘ R⌥ (#710)
├── Tap or hold      no    labs.tap_or_hold             (macOS, Linux) in Toggle mode, a press held past 300 ms dictates until it lifts; a short tap still locks (#711, #1144)
├── Sort and filter models no labs.picker_sort_filter   the Local transcription picker sorts by recommendation, size, or name and can show only models for the saved language (#645)
├── Benchmark details no  labs.benchmark_details        picker rows show raw WER/CER (lower is better), language, dataset, source, and conditions; incomparable "Most accurate" badges drop (#647)
├── Parakeet Ultra  no    labs.parakeet_ultra          the picker offers Moondream's Parakeet Ultra (pinned Q8_0, CC BY 4.0) with a details view; stays on while it dictates (#648)
├── Special keys alone no labs.standalone_special_keys (Linux, Windows) Linux: Menu, Pause, Scroll Lock may be a shortcut's key, alone or with modifiers (#767); Windows: Insert, Home, End, Page Up, Page Down may stand alone (#794)
├── Section key alone no  labs.section_key_alone       (Linux) the § key, left of 1 on ISO keyboards, may be a shortcut with no modifier (#1050)
├── Dictation stats no    labs.history_stats           History shows a summary of retained dictations; hex history stats prints it (#790)
├── Copy recent dictations no labs.recent_dictations (macOS) the menu bar's Copy Recent Dictation submenu copies one of the last five dictations (#877)
├── Skip long pauses no    labs.skip_pauses             (Linux, Windows) a whole-clip dictation run hears only the head and tail of each long pause (#899)
├── Stop when the call ends no labs.stop_when_call_ends (macOS) a recording Hex started on its own stops 30 s after its call app leaves the microphone and goes quiet (#835)
├── More call apps no    labs.more_call_apps             (every OS with Meetings) Signal, Telegram, WeChat, LINE, Webex, Element, and Phone calls are offered,
│                                        and calls on Google Voice, Messenger, Teams, FaceTime, GroupMe, and Webex pages are named
├── Fix results in Output no labs.output_fixes           Output's Try card offers Vocabulary and Fix this, which copies an agent prompt naming the wrong result (#989)
├── Model in use first no labs.picker_active_first     the speech model picker shows the model in use first: Compare's first column, Every model's first row (#1019)
├── Use model files where they are no labs.model_in_place the picker adds a model file without copying it, so accounts can share one (#941)
├── Downloaded models only no labs.picker_downloaded   the picker's All speech models view can show only downloaded models, with the space Hex's copies take (#1018)
├── Keep recordings no    labs.keep_recordings         History entries keep their dictation audio; Transcribe again runs it through another installed model (#1032)
├── Hide menu bar icon no labs.hide_menu_bar_icon      (macOS) the menu-bar icon hides while no meeting is under way; shortcuts keep working, reopening Hex shows Settings (#1064)
├── Notch indicator no    labs.notch_indicator         (macOS) with the indicator at Top, the notch widens sideways in the Dynamic Island's compact style: an orange mic and level bars, the macOS spinner, a green checkmark; flat screens keep it below the menu bar
├── Movable meeting capsules no labs.movable_meeting_capsules (macOS) panel capsules (Meeting transcript ready, Recording automatically, offers, reminders) drag to another corner and show in the last one, shared with the recording capsule (#1093)
├── Vocabulary in meeting notes no labs.meeting_notes_vocabulary (every OS with Meetings)
│                                        final meeting titles and summaries (and Regenerate summary) read the vocabulary ahead of the transcript (#1154)
├── Meeting finished event no labs.meeting_finished_event (every OS with Meetings)
│                                        the local API's GET /meetings/events sends each finished meeting's id once its final transcript is written (#964)
├── Follow live transcript no labs.follow_live_transcript (every OS with Meetings)
│                                        a recording meeting's transcript card keeps its newest line in view while scrolled to
│                                        the bottom; scroll up to read, back to the bottom to follow again (#1332)
│                                        // check: only_a_recording_transcript_follows_and_only_with_its_lab (rune_ui/meetings.rs,
│                                        // Linux and Windows); not observed on a real call; Labs page goldens need regenerating
├── Restore clipboard on GNOME no labs.gnome_clipboard_restore (Linux) on a Wayland desktop without data-control (GNOME), a paste snapshots and restores
│                                        the clipboard through XWayland, only while this user's Xwayland already runs; never starts it (#1111)
├── Floating meeting capsules on GNOME no labs.gnome_meeting_capsules (Linux) on a Wayland desktop without layer-shell (GNOME), the recording offer and
│                                        recording capsule open on XWayland (Rune xwayland_input: override-redirect, clicks in the input shape, never keyboard
│                                        focus), decided at each show, only while this user's Xwayland already runs; never starts it. Off, an offer opens
│                                        the Meetings pane (#1271), whose note names the lab (#1274)
│                                        // checks: floats_only_by_opt_in_*, looks_for_nothing_while_the_lab_is_off (linux/meeting_capsules.rs),
│                                        // gnome_meeting_capsules_is_its_own_bit_*; Rune's xwayland pointer tests; not run on a GNOME session
├── Screenshots     no    labs.screenshots + screenshots.hotkey (default ⌘⇧4) + screenshots.folder (macOS)
│                                        the shortcut picks an area (Space: a window); the PNG goes on the clipboard, into the folder (Desktop), and into a corner thumbnail
├── Annotate 15 s replay no labs.annotate_replay + capture.replay_hotkey (macOS 27+)
│                                        keeps a rolling 15 s SCClipBufferingOutput screen buffer for hex capture replay and the replay shortcut;
│                                        macOS keeps its screen-recording dot lit the whole time it is on
└── Terminal paste  no    paste_with_shift             (Windows) paste with Ctrl+Shift+V (#477/#478; Linux keeps it in General);
                                         Java games (javaw/java, e.g. Minecraft) still get Ctrl+V (#1362)
                                         // check: terminal_paste_sends_plain_control_v_to_java_games (windows/paste.rs)

Settings › Labs                          // labs.page, a store (labs/store.rs)
├── Header -> "Labs", Search N labs (⌘F focuses it), Discover | Turned on · N
├── Group chips with counts -> All, Dictation, Output, History, Meetings, Annotate, Shortcuts, Models
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
│   // checks: labs_groups_fold_and_remember_it, labs_groups_follow_each_os (tests/panes.rs);
│   // every_group_has_a_feature_and_round_trips_its_id (labs/group.rs);
│   // goldens mac-labs (light, dark), windows-labs, linux-labs, mac-labs-link
├── Card -> neutral icon tile, name with New / Dev tags on one line, up to two lines of description, a switch;
│   pressing the card itself does nothing (there is no drawer)
│   ├── The switch (key lab-<id>) -> flips the lab, saved at once
│   ├── A pane feature on -> its pane is a normal top-level sidebar item; off -> hidden entirely
│   └── Voice Filter while its driver is installed, or a managed or locked lab -> its switch is disabled
├── A lab's own settings show on its card while it's on -> Unload model when idle: Unload after (5/15/30/60 min);
│   Screenshots: its shortcut, Saves to, and the macOS-takes-this-shortcut note with Open Keyboard Settings.
│   Annotate 15 s replay's shortcut lives in Annotate's pane
│   // checks: unload_when_idle_shows_its_minutes_on_its_card_while_on; scenes mac-labs-settings, mac-labs-screenshots
├── Opened by a link (macOS) -> hex://labs opens this page; ?tab=on, ?category=<group id> and ?search=<text>
│   set the store; hex://labs/<lab> also selects that lab's group chip (unless ?category names one). It never turns a lab on. An unknown
│   or absent name -> the page opens and process.log notes it. hex://settings/<pane>
│   (general, output, history, meetings, voice-filter) opens a main pane; one Hex doesn't show -> General.
│   Links over 2048 bytes are ignored; a test build's scheme is hex-test://
├── Annotate is listed as a lab (Annotate group) wherever Hex has the Annotate pane; its switch flips capture.enabled
└── Footer -> "Unfinished features. They may change or break.", Run setup again (replays the onboarding,
    below), Send feedback -> https://github.com/anomalyco/hex/issues/new

hex labs                                 // labs.cli; edits settings.json, no running Hex
├── (list) -> every feature this build has: name, on/off, audience, description, its hex settings key
├── link <feature> -> macOS: prints hex://labs/<feature> (hex-test:// in a test build), which opens Labs on its group
├── on <feature> | off <feature> -> a running Hex saves and applies it (hex settings set); otherwise settings.json
├── off voice-filter with the driver installed -> refused: uninstall the driver (dev builds; stable has no voice-filter)
└── hex experiments … -> points at hex labs

hex settings set labs.live_text true     // macOS: through the running Hex
```

Checks: `rune_ui::deep_link` tests (parser, query, every feature's link reads
back); the labs store tests in tests/panes.rs (links, card toggles, a lab's own settings on its card, the stable build's missing Dev labs,
filters, ⌘F, a lone card's width, group folds and each OS's groups) and the
`mac-labs`, `mac-labs-link`, `windows-labs`, and `linux-labs` goldens (Linux).
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
the toggle is on. Live chapters are read when a recording starts. Nothing
starts at launch for a feature that is off. Limit: the macOS window's fixed
size is chosen when it is created, so it becomes resizable for the
playground only after Hex restarts.

Gate a Labs feature where its work starts (the worker, thread, or capture),
not only where its UI appears: `labs::live_text()`, `labs::live_chapters()`,
`labs::playground()`, `labs::vocabulary_biasing()`,
`labs::more_call_apps()`, `app_settings::meetings_enabled()`, or
`app_settings::hex_microphone_enabled()`.
Add a new feature to `labs::Feature` (name, settings key, description, audience), give
it a row in `rune_ui/labs.rs` and a `hex settings` key, and default it off.
When a feature graduates, remove it from Labs.

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

Unload model when idle (`unload-idle`, issue 442 from @theoneand33): while
on, its card shows "Unload after" (5, 15, 30, or 60 minutes; default 15).
`transcription/engine`'s worker unloads a loaded GGUF model once no command
has reached it for that long and nothing records (`audio::idle_sleep::held()`: a
dictation past its hold or a meeting; a macOS capture from its press via
`dictation_busy`), freeing its RAM and VRAM; between commands no request
runs. Apple Speech stays loaded. The unloaded model still counts as held
(`TranscriptionEngine::holds`), so dictation, meetings, and the local API send
their work to the engine as before, and the worker reloads it before the
first command that needs it: audio fed meanwhile waits in the queue, so
nothing recorded is lost. A capture's stream opens after the reload and
catches up, falling back to the whole clip if it can't finish in time; the
HUD shows its transcribing state while a released clip waits on the reload,
with no look of its own. A failed reload fails that job and is retried by the
next. Check: `unload_idle_carries_its_minutes` in [labs.rs](../../src/labs.rs);
the unload and reload themselves have no automated check.

Restore shortcut default (`restore-shortcut-default`, issue 613): while on,
General › Advanced › Paste last dictation shows a Restore default shortcut
button (`restore-paste-last`) whenever the shortcut is not its default
(Option-Shift-V on macOS, Ctrl+Alt+V elsewhere), and a cleared shortcut's
row reads "Default shortcut: …". Pressing it saves the default through the
same checks a recorded shortcut meets; if another Hex shortcut already uses
it, nothing changes and the recorder opens with "Already in use". Off, the
row is exactly as before. The gate is the row itself (the feature does no
other work). Check:
`restore_default_brings_back_paste_last_only_with_its_lab_on` in
[tests.rs](../../src/rune_ui/tests.rs).

Keep dictation on clipboard (`keep-on-clipboard`, issue 200, macOS): while
on, a pasted dictation stays on the clipboard. `paste::Paster` reads
`labs::keep_on_clipboard()` as each paste commits: it writes the transcript
without the nspasteboard.org transient markers (so clipboard managers may
record it), schedules no 500 ms restore, and drops any earlier snapshot and
pending restore (`ClipboardRestore::keep`), so a later restoring paste never
brings back an older clipboard. Into Chromium 155/156, where Hex types the
text instead of pasting, it also writes the transcript to the clipboard after
typing. Off, pasting and clipboard restore are exactly as before. Checks:
`a_kept_paste_leaves_nothing_to_restore` and
`a_kept_transcript_is_an_ordinary_clipboard_entry` in
[paste.rs](../../src/paste.rs) (macOS only);
`contributed_options_are_labs_toggles_off_by_default` in
[panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real paste
on a Mac with the toggle on.

Copy dictation only (`copy-only`, issue 629, macOS): while on, a finished
dictation (the dictation shortcut, Dictate without processing, or
`hex dictation` without `--print`) is put on the clipboard and nothing is
sent to the focused app: no ⌘V, no typed text (Chromium 155/156 included),
and no 500 ms restore, so selected text is never replaced. The paste worker
(`parakeet::finish_output`) reads `labs::copy_only()` as each dictation's
output commits and hands `paste::Paster` `PasteMode::CopyOnly`, which writes
the transcript as an ordinary clipboard entry (no nspasteboard.org transient
markers), drops any earlier snapshot and pending restore
(`ClipboardRestore::keep`), and ends the continuation, so the next dictation
is not joined to it. History and Paste last dictation update as for a paste,
and Paste last dictation and meeting pastes still paste. Off, output is
exactly as before. Checks:
`a_copy_only_dictation_is_left_on_the_clipboard_with_nothing_to_restore` in
[paste.rs](../../src/paste.rs) (macOS only);
`contributed_options_are_labs_toggles_off_by_default` in
[panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real
dictation on a Mac with the toggle on.

Spoken formatting (`spoken-formatting`, issue 630, every OS): while on,
`Profiles::process_cancellable` in
[dictation/processor.rs](../../src/dictation/processor.rs) reads
`labs::spoken_formatting()` as each dictation's processing starts and runs
`dictation::spoken_formatting::apply` before the mode's corrections, OpenCode
rewriting, and transformations (on Windows and Linux, after the Global
replacements). Whole phrases, ignoring case and Spanish accents, are
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
`dictation::filler_words::apply` right after spoken formatting and before the mode's
corrections, OpenCode rewriting, and transformations. It is rule-based, with
no model and no added latency. The English fillers "um", "umm", "uh", "uhh",
"erm", "er", and "ah" are removed as whole words, lowercase or capitalised
only, so "umbrella", "ahead", "error", "Uhura", "uh-huh", and "ER" stay. The
comma or ellipsis that comes with a filler goes too, the spaces around it
close up, no space is left before punctuation, a capitalised filler that
started a sentence passes its capital to the next word ("Um, so I think"
becomes "So I think"), and line breaks stay. Dictate without processing,
meetings, and live text are unchanged, and with the toggle off processing is
exactly as before. Checks: the tests in
[dictation/filler_words.rs](../../src/dictation/filler_words.rs);
`filler_words_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs);
`contributed_options_are_labs_toggles_off_by_default` in
[panes.rs](../../src/rune_ui/tests/panes.rs). Not yet observed: a real
dictation with the toggle on; the Labs page goldens need regenerating on
Linux with the new row.

Subtitle export (`subtitle-export`, issue 633, macOS like Meetings; its row
shows wherever Meetings does): while on, `hex meetings show <id> --srt` or
`--vtt` writes the meeting's transcript as SubRip or WebVTT subtitles, one cue
per transcript entry (speaker turn) labelled You or Them, timed by the entry's
offsets from the meeting's start rather than word timings, and limited by
`--from`/`--to` or `--chapter` as the other outputs are
([transcript-cli.md](../transcript-cli.md)). The gate,
`labs::subtitle_export()`, is read where the command starts: off, `--srt` and
`--vtt` fail naming `hex labs on subtitle-export` before any meeting is read,
and every other output is exactly as before. There is no export button in the
window yet. Checks:
`subtitles_render_speaker_turns_as_srt_and_vtt_cues` and
`subtitles_follow_the_range_and_need_the_labs_toggle` in
[control/transcript_cli_tests.rs](../../src/control/transcript_cli_tests.rs);
`subtitle_export_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: a real meeting exported and
loaded in a player; the Labs page goldens need regenerating on Linux with the
new row.

Second dictation shortcut (`second-dictation-shortcut`, issue 627, macOS):
for a Globe/Fn shortcut on the laptop keyboard and another key on an
external keyboard. While on, General shows a "Second dictation shortcut"
recorder under Shortcut, unbound by default and clearable
(`second_dictation_hotkey`, also `hex settings set second_dictation_hotkey
f5`). It is refused when it shares a key with another Hex shortcut, and the
others' checks count it while the toggle is on. The gate is
`AppSettings::active_second_dictation_hotkey`: only on macOS, with the toggle
on, a binding saved, and no overlap does `RuntimeHotkeys::second_dictation`
carry it, so the event tap suppresses its keys and the listener
(`recognition::Hotkeys`) runs a second shortcut machine with the dictation
shortcut's Hold/Toggle, double-tap lock, and double-tap-only settings. It
dictates and pastes with processing, as the dictation shortcut does; Paste
last and meeting paste still come only from the main machine. The shortcut
that starts a capture finishes or cancels it, and the other stands down
until its keys lift while it records. Turning the toggle off or clearing the
binding mid-capture cancels that capture. Off, nothing is matched,
suppressed, or checked for it, and Hex is unchanged. Checks:
`the_second_dictation_shortcut_needs_its_lab_and_its_own_keys` in
[app_settings.rs](../../src/app_settings.rs) and
`the_shortcut_that_starts_a_capture_owns_it` in
[recognition.rs](../../src/recognition.rs) (macOS only). Not yet observed: a
real Mac with an external keyboard, or the recorder row's rendering (macOS
builds only; no golden).

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

Sided modifier chords (`sided-modifier-chords`, issue 710, macOS): while
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

Benchmark details (`benchmark-details`, issue 647, every OS): while on,
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
the FLEURS English test (4.77% WER, 647 utterances, same conditions); every other figure is English WER with dataset and source
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
├── "Published conversion benchmarks; not measured in Hex" (folded) -> conditions
│   (FLEURS test, Q8_0, CUDA, batch 8, each language's hint, no timestamps,
│   transcribe.cpp 021aa17d; file digest, dataset revision, scorer unknown), then
│   25 FLEURS WERs from transcribe.cpp 7045d1c's catalog/parakeet-ultra.json
├── Accuracy 98.2% / Labs › Benchmark details: LibriSpeech test-clean 1.80%
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

Section key alone (`section-key`, issue 1050, Linux): for push-to-talk on
the ISO § key, left of 1 above Tab, without a modifier.

```ts
Settings › General › Shortcut, press § alone      // labs.section-key
├── Toggle off -> "Add a modifier", as before; § with a modifier records as before
├── Toggle on  -> saved as § (hex settings also takes `section`); Settings shows §
│   ├── X11 -> grabs the keycode that types XK_section 0xa7
│   └── Wayland portal -> preferred trigger `section`
├── hex settings set dictation_hotkey §, off -> "section needs Labs › Section key
│   alone: hex labs on section-key"
└── Toggle turned off (row, hex labs off, hex settings), or settings load with
    it off -> a Linux shortcut on § alone goes back to its default (Dictate
    without processing unbound), so the listener never registers it
```

While bound, Hex takes § from every app (an X11 grab or a portal shortcut
holds its key), so typing § needs another key. The window's
`settings::recorded_binding` and `control/settings.rs`'s `parse_hotkey` let a
lone § through on Linux; the gates are `AppSettings::section_key_allowed` in
the window's `settings::validate` and in `control/settings.rs`'s `apply`
(`special_key_refusal`), and `AppSettings::repair_section_key`
(`app_settings/shortcut_repair.rs`) where settings load and where the toggle
turns off. Off, Hex is unchanged: a lone § is refused everywhere, and the
added `section` keysym names are reached only by a saved § shortcut (§ with a
modifier now also resolves on X11 and the portal, where it was refused as an
unsupported key). macOS and Windows don't have it. Checks:
`section_alone_needs_its_labs_toggle` in
[control/settings.rs](../../src/control/settings.rs) and
[rune_ui/settings.rs](../../src/rune_ui/settings.rs),
`the_section_key_has_portal_and_x11_names` in
[linux/input.rs](../../src/linux/input.rs), and
`a_lone_section_key_goes_back_to_its_default_while_its_labs_toggle_is_off` in
[app_settings/shortcut_repair.rs](../../src/app_settings/shortcut_repair.rs).
Not yet observed: a real ISO keyboard on X11 or a Wayland portal granting
`section`; whether Rune reports § as text on every layout; the Labs page
goldens need regenerating on Linux with the new row.

Dictation stats (`history-stats`, issue 790, every OS): while on, the History
pane shows a "Dictation stats" block above the list once anything is retained:
dictations, words, words per minute, how often Output changed the text, the
daily streak, and the top five apps; `hex history stats [--since] [--until]
[--json]` prints the same ([transcript-cli.md](../transcript-cli.md)). It sums
every entry within retention (`History::stats`, `history/stats.rs`), not the
pane's 200-entry search, so it ends with what retention removes; the rollup that
outlives retention, streak notifications, and per-replacement counts are not
built. The gates are `labs::history_stats()`, read when the pane reloads (off,
it reads nothing for stats, and sums again only when the entries or the toggle
change) and where `hex history stats` starts (off, it fails naming `hex labs on
history-stats` before History is read), and the window's own setting for the
block. Off, History is exactly as before. Checks: the tests in
[history/stats.rs](../../src/history/stats.rs) (words, speed, changes, top
apps, streak across a gap, expired entries skipped past the search cap);
`history_stats_parse_sum_retained_entries_and_need_the_labs_toggle` in
[control/transcript_cli_tests.rs](../../src/control/transcript_cli_tests.rs);
`history_stats_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: the block in a real window;
there is no `--snap` scene for it (the fixture has no store), and the Labs
page goldens need regenerating on Linux with the new row.

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
A clip in which no frame rose above the noise floor is left whole, as the
stream does for #841. The job deadline, History's duration, and the silence
discard still use the full clip; process.log adds a `skipped long pauses
before whole-clip transcription` line with `pauses_dropped_ms`. macOS,
streams, meetings, captures, and the local API are unchanged, and off the
clip is passed through untouched. Checks:
`skipping_pauses_cuts_a_whole_clips_long_pauses_to_their_ends`,
`skipping_pauses_leaves_short_pauses_and_quiet_only_clips_whole`, and
`with_skip_pauses_off_the_whole_clip_is_untouched` in
[transcription/engine.rs](../../src/transcription/engine.rs);
`skip_pauses_is_its_own_bit_and_off_by_default` in [labs.rs](../../src/labs.rs).
Not yet observed: a real dictation with the toggle on in Hex (the reporter
measured their own branch on an i5-8259U); the Labs page goldens need
regenerating on Linux with the new row.

Stop when the call ends (`stop-when-call-ends`, issue 835, macOS like
Meetings; its row shows wherever Meetings does, in macOS builds): for an
Always record call that keeps recording after hang-up. The gate,
`labs::stop_when_call_ends()`, is read on each pass of the meeting
controller (`poll_recording` in
[meeting/watcher.rs](../../src/meeting/watcher.rs)), and only for a recording
Hex started on its own. While on, the call app counts as present while it
holds the microphone or any of its Core Audio processes has output running
(`microphone_activity::application_running_output`), since WhatsApp keeps
the microphone while muted and closes both on hang-up. Once both are off for
about 3 s, the "left the call" capsule says "Hex stops recording in 30 s
unless you keep recording."; Keep recording sends
`ControllerCommand::KeepRecording`, which drops the stop for this absence, and
the call app coming back drops it too. After 30 s (`AUTO_STOP_GRACE`,
`AutoStop` in [meeting/detection.rs](../../src/meeting/detection.rs)) the
controller stops the recording as Stop & Transcribe does and logs it. Manual,
offered, and calendar recordings never stop, and off, nothing reads the
output and the capsule and controller are exactly as before. Not built: the
leave-call button as a signal, per-app rules, and holding off while the
network is down. Checks: `stop_when_call_ends_is_its_own_bit_and_off_by_default`
in [labs.rs](../../src/labs.rs) and the auto-stop checks listed in
[meetings.md](meetings.md#record). Not yet observed: a real WhatsApp, Zoom,
or Teams call on a Mac with the toggle on; the row renders only in macOS
builds, so no golden changes.

More call apps (`more-call-apps`, every OS with Meetings; its row shows
wherever Meetings does): for calls in messaging apps and on websites call
detection did not recognise. The gate, `labs::more_call_apps()`, is read by
call detection's each pass ([meeting/watcher.rs](../../src/meeting/watcher.rs)
passes it to `candidate_from_microphone`) and by Settings › Meetings ›
Detected calls (`app_settings::known_meeting_providers`). While on, a
microphone held by Signal (`org.whispersystems.signal-desktop`), Telegram
(`ru.keepcoder.Telegram`, also `com.tdesktop.Telegram` and
`org.telegram.desktop`), WeChat (`com.tencent.xinWeChat`), LINE
(`jp.naver.line.mac`), Webex (`com.webex.meetingmanager`, also
`com.cisco.webexmeetingsapp`), Element (`im.riot.app`), or the Phone app
(`com.apple.mobilephone`) is that app's call ("Signal call", "Webex
meeting", "Phone call", …). Each Electron app's helpers fold to its parent
through [microphone_activity.rs](../../src/microphone_activity.rs), and on
Windows and Linux the same apps' executables (`signal`, `signal-desktop`,
`telegram`, `telegram-desktop`, `wechat`, `weixin`, `line`, `webex`,
`element`, `element-desktop`) map to the same keys. Signal, Telegram, WeChat,
LINE, and Element also hold the microphone for voice notes, so they keep it
about 20 s before they count as calls, as WhatsApp does; Webex and Phone
count after 2 s. A browser on Google Voice (`voice.google.com`), Messenger
(`messenger.com`), Teams (`teams.live.com`), FaceTime
(`facetime.apple.com`), GroupMe (`web.groupme.com`), or any `webex.com`
subdomain is named after the service (a Webex page's provider is
`webex.com`, so one choice covers every subdomain); these names need the
browser's page, which Hex reads on macOS ([meetings.md](meetings.md#detect-and-offer)).
Those choices and the new apps are listed in Detected calls, and Ask /
Always record / Never offer work as for the existing services. Off, nothing changes: those processes are
not meeting apps, the pages stay unknown pages ("Browser call") after 20 s,
and Detected calls lists exactly the services it did before. Checks:
`more_call_apps_are_unknown_until_their_lab_is_on`,
`more_call_apps_names_its_apps_and_websites_with_their_lab_on`, and
`a_telegram_voice_note_is_not_offered_but_a_sustained_call_is` in
[meeting/detection.rs](../../src/meeting/detection.rs);
`more_call_app_bundle_variants_and_executables_canonicalize` in
[microphone_activity.rs](../../src/microphone_activity.rs);
`more_call_apps_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: a real Signal, Telegram,
WeChat, LINE, Webex, Element, or Phone call, or any of the websites. Goldens:
`mac-labs` (light, dark), `windows-labs`, and `linux-labs` show the new row.

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

Model in use first (`picker-active-first`, issue 1019, every OS): while on,
the speech model picker remembers the saved model when it opens, only if it
is installed (`Picker::in_use_first`, set in `picker::open`, cleared in
`close`), so the order never moves while the picker is open, even if another
model is chosen or prepared. Compare puts it in the first column and fills
the rest with the first models that speak the language, without it; a model
being prepared that the three leave out still takes the last column. Every
model moves it to the top of its group, and an import in use puts "Your
model files" above the language's group; with Labs › Sort and filter models
sorting By size or By name the list keeps that exact order, and By
recommendation leads with it. The gate is the ordering itself
(`picker::pinned` and `list_first` read the toggle; `compare::columns` and
`list::groups` take what they return); off, both views are exactly as
before. Checks: `the_model_in_use_is_fixed_while_the_picker_is_open` and
`without_model_in_use_first_nothing_is_pinned` in
[picker.rs](../../src/rune_ui/picker.rs),
`the_model_in_use_takes_the_first_column_only_when_pinned` in
[picker/compare.rs](../../src/rune_ui/picker/compare.rs), and
`the_model_in_use_leads_its_group_only_when_pinned` in
[picker/list.rs](../../src/rune_ui/picker/list.rs). Not yet observed: the
picker in a real window with the toggle on, or an import's group moving
(no automated check builds an import entry); the Labs page goldens need
regenerating on Linux with the new row.

Use model files where they are (`model-in-place`, issue 941, every OS): for
one speech model file shared by several accounts. While on, the speech model
picker's Every model header adds "Use model file where it is…" beside "Add
model file…"; the chosen GGUF is checked, hashed once, and recorded with its
path (`custom_models::add_in_place`), never copied
([own-models.md](own-models.md#use-a-model-file-where-it-is-labs)). The gate
is `model_import::in_place_on`, read when the button draws and again when the
dialog answers, before the worker starts. Off, the picker is exactly as
before. Checks:
`a_file_added_in_place_is_recorded_by_path_copied_nowhere_and_forgetting_leaves_it`
in [transcription/custom_models.rs](../../src/transcription/custom_models.rs);
`every_feature_is_off_by_default_and_round_trips` in
[labs.rs](../../src/labs.rs). Not yet observed: a real window or a second
macOS account; the Labs page goldens need regenerating on Linux with the new
row.

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
dictation again through a different model.

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
├── History card, entry with audio -> "Saved recording" + "Transcribe again" menu
│   ├── Lists every available model installed for the language it would use
│   ├── A model that takes a language hint (not Parakeet v3/Ultra, GigaAM,
│   │   Granite, Bark) -> "<model> · Language: <saved language>" menu (Auto when it
│   │   detects, then its languages) + Transcribe; the language applies to
│   │   that run only, never to settings (#1068)
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
└── Toggle off -> nothing new is saved and the menu hides; existing audio stays
    until it expires or History is cleared
```

No playback yet. Off, History and dictation are exactly as before: no copy
of the clip is made. Checks: `a_kept_recording_is_an_owner_only_wav_that_reads_back`,
`recordings_leave_with_their_entries`,
`the_recordings_byte_cap_drops_the_oldest_audio_but_keeps_the_text`,
`a_recording_another_process_has_not_saved_yet_survives_a_sweep`,
`replacing_the_text_keeps_the_entry_and_its_recording`, and
`retranscribing_a_missing_recording_says_so` in
[history.rs](../../src/history.rs);
`transcribe_again_shows_only_with_its_lab_and_replace_updates_the_entry` and
`transcribe_again_offers_a_language_only_for_models_that_take_one` in
[tests.rs](../../src/rune_ui/tests.rs);
`keep_recordings_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs); the tests in
[history/versions.rs](../../src/history/versions.rs) (newest first, five at
most, switch back, text cap, Clear all). Not yet observed: a real dictation kept and
transcribed again with another model in a window, on any OS.

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

Notch indicator (`notch-indicator`, macOS only): for MacBooks with a
notch, a recording indicator in the Dynamic Island's compact style that
reads as part of the Mac. It is not Dot scope: the notch keeps its height
and widens by `hud_placement::EAR_WIDTH` (34 pt) on each side, with fully
rounded bottom corners, and draws one SF Symbols–style glyph beside the
camera on each side in Apple's dark system colours (`ears` in
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
next dictation. `hud_placement::ears` places it only on a screen whose
`NSScreen` reports a notch, and only with Settings' indicator position at
Top; a flat external display, Bottom, and Hidden keep what they did, and
with the lab off HUD Lab's placement stands. Linux and Windows have no row,
`hex labs on notch-indicator` doesn't know it there, and `hex settings set
labs.notch_indicator` answers `unsupported` (as any `labs.` switch for
another OS now does). Checks: `ears_widen_the_notch_sideways_at_its_own_height`
in [rune_hud/hud_placement.rs](../../src/rune_hud/hud_placement.rs);
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
├── Escape     -> closes it, nothing shot; Escape reaches no other app or Hex shortcut
└── Release    -> shot at the display's own pixels (ScreenCaptureKit), Hex's overlays,
                  the picker, and the pointer left out
                  ├── PNG (pHYs at the display's scale) -> "Hex Screenshot 2026-11-04 at 14.03.22.png"
                  │   in screenshots.folder (the Desktop when unset); a second shot that second gets " (2)"
                  ├── clipboard -> the PNG and the file's URL, one pasteboard item
                  └── thumbnail bottom right for 5 s (held while the pointer is on it), 16 pt in,
                      its corner concentric with the display's (30 pt, a design constant):
                      slides in from past the right edge on the HUD's spring and out the same way
                      (Reduce Motion: a fade); ✕ shows inside its top-left corner on hover;
                      click -> opens it in the editor, drag -> the file, ✕ -> closes
                      a failure -> the error tone; there is no success sound (Hex has no shutter cue)
```

A click on the thumbnail opens the editor
([screenshot_thumbnail/editor.rs](../../src/rune_ui/screenshot_thumbnail/editor.rs)), a panel
centred on the display under the pointer, the capsule's corners and
shadow, growing in on the HUD's spring (Reduce Motion: a fade). The shot
shows at no more than its own points. It stays until Escape or ✕:

```ts
editor
├── Pen, Arrow, Box, Text, Crop -> a drag on the shot draws in Annotate's red;
│                                   Text: a click, then type, Enter or Escape ends it;
│                                   Crop: a drag shades what's left out, a click clears it
├── ⌘Z            -> the text being typed, else the last mark, else the crop
├── Copy (⌘C)     -> the edited PNG on the clipboard
├── Save (⌘S)     -> a new file in screenshots.folder; the first shot is never changed
├── Send to…      -> a list of open OpenCode sessions (typing searches title, folder, id,
│                    as Annotate's; ↑↓ Enter); the PNG goes to the session as an image
├── a drag on the panel's edge or the toolbar's gap -> moves it, kept on the display
└── Escape        -> ends the text, else closes the list, else closes the editor
```

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

The shortcut is taken in Hex's session tap (`screenshots::key`, called
from [suppression.rs](../../src/suppression.rs) before the listener's
own handling), never by the listener, and its press and release stay with
it. With the lab off the tap returns at once and nothing is made: no
picker window, no thumbnail, no work. Screen Recording is Annotate's: a
press without it shows Annotate's permission helper instead of the
picker.

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
`thumbnail_goldens` (rest and hover, light and dark) and the thumbnail's
timer in [screenshot_thumbnail.rs](../../src/rune_ui/screenshot_thumbnail.rs);
`editor_goldens` (marks and the Send to… list, light and dark), the
layout, each tool's drag, text, undo, the list's search, and both exports
(`an_export_draws_the_marks_over_the_shot` renders on a GPU, skipping
without one) in [screenshot_thumbnail/editor/tests.rs](../../src/rune_ui/screenshot_thumbnail/editor/tests.rs),
and the mark model in [screenshot_thumbnail/editor/marks.rs](../../src/rune_ui/screenshot_thumbnail/editor/marks.rs);
`screenshots_is_its_own_bit_and_off_by_default` in [labs.rs](../../src/labs.rs);
its card renders as `--scene mac-labs-screenshots`. Not yet observed: a
real ⌘⇧4 on a Mac (with macOS's shortcut off), a shot of each kind, the
clipboard paste, the thumbnail's drag, the editor on a real display
(its keys, Copy, Save, and a session receiving a sent shot), and whether a session
tap can ever see ⌘⇧4 while macOS's shortcut is on (inferred from how
symbolic hotkeys are dispatched, not tested). The row renders only in
macOS builds, so the settings goldens don't change.

Annotate 15 s replay (`annotate-replay`, macOS 27+ only, group Annotate): keeps
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

Movable meeting capsules (`movable-meeting-capsules`, issue 1093, macOS
like Meetings; its row shows wherever Meetings does, in macOS builds): for
panel capsules that sit over another app's window controls or toolbar. The
gate, `labs::movable_meeting_capsules()`, is read each time a panel capsule
shows (`MeetingCapsules::show_panel` in
[rune_ui/meeting_capsules.rs](../../src/rune_ui/meeting_capsules.rs)). While
on, the panel (offer, reminder, left the call, Meeting transcript ready,
Recording automatically) opens in the saved
`meetings.recording_indicator_corner` and its body drags like the recording
capsule's: it follows the pointer, snaps to the corner nearest its centre
(measured with its own width) on release, and saves that corner, which the
recording capsule then uses too. A press on a button still presses it, and a
cancelled drag (Escape) goes back without saving. Off, the panel opens in
the top right and doesn't drag, exactly as before. Not built: clicks passing
through the panel outside its buttons. Checks:
`a_released_panel_snaps_by_its_own_size`,
`each_corner_places_the_panel_at_its_edges`, and
`only_a_movable_panel_drags` in
[rune_ui/meeting_capsules.rs](../../src/rune_ui/meeting_capsules.rs);
`movable_meeting_capsules_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: a drag on a real Mac; the
row renders only in macOS builds, so no golden changes.

Choose meeting summary model (`meeting-summary-model`, issues 1107 and 1160)
graduated: Meetings › Settings › After a meeting › Summaries picks Off, On
this Mac, or OpenCode, with OpenCode's Model nested under it
([meetings.md](meetings.md)). Its bit (1 << 24) stays unused.

Vocabulary in meeting notes (`meeting-notes-vocabulary`, issue 1154; every
OS with Meetings; its row shows wherever Meetings does): for names and terms
spelled right in a meeting's final title and summary, as live chapters
already do. The gate, `labs::meeting_notes_vocabulary()`, is read at each
final notes generation (`summarize_claimed` in
[meeting/notes.rs](../../src/meeting/notes.rs)), so automatic notes and
Regenerate summary both use the vocabulary as it is then. While on, the
prompt starts with live chapters' line, "Names and terms that may appear:"
and up to 30 terms (`chapters::vocabulary_line`), ahead of the calendar
event and the transcript; its length comes out of the excerpt budget like
the calendar's. Provisional titles while recording are unchanged. Off, or
with an empty vocabulary, the prompt is exactly as before. Not built: a
per-meeting context field, or adding terms to retranscription. Checks:
`vocabulary_terms_lead_the_notes_prompt_and_share_its_budget` in
[meeting/notes.rs](../../src/meeting/notes.rs);
`every_feature_is_off_by_default_and_round_trips` in
[labs.rs](../../src/labs.rs). Not yet observed: real notes written with the
toggle on; the Labs page goldens that show Meetings rows need regenerating
with the new row.

Meeting finished event (`meeting-finished-event`, issue 964; every OS with
Meetings; its row shows wherever Meetings does): for a companion app that
processes a meeting once its transcript is ready, without watching the
meetings folder or polling `hex meetings list`. While on, the local API's
authenticated `GET /meetings/events` is a server-sent event stream that
sends `{"type":"meeting.finished","id":"<id>"}` once per meeting, after
its final transcript and Markdown are written and the manifest says
`complete` (`record` after Stop, or `finish_pending` for a meeting Hex quit
while recording), with a `: keep-alive` comment every 15 s
([streams.md](../streams.md#meeting-finished-events-get-meetingsevents-labs)).
Only the id is sent, never a title, path, transcript, or audio, and nothing
leaves the computer; the client reads the meeting with `hex meetings show
<id>`. One subscriber at a time (a second answers `409 subscriber-busy`), so
it holds at most one of the four HTTP workers. The gates are
`labs::meeting_finished_event()`, read where the route starts (off: `404
labs-off`), on each 250 ms pass of an open stream (off ends it), and where
`meeting::finished_events::publish` is called; with it off, or with nobody
subscribed, nothing is queued or kept. A failed final pass sends nothing. A
client that was offline catches up by subscribing, then listing from its saved
cursor and deduplicating by id (at least once;
[streams.md](../streams.md#catching-up-after-being-offline)). Not
built: the webhook with audio upload, retries, or a command run on finish
that the issue also asked for. Checks:
`a_finished_meeting_reaches_the_one_subscriber_as_its_id` and
`nothing_is_sent_or_kept_while_the_toggle_is_off` in
[meeting/finished_events.rs](../../src/meeting/finished_events.rs);
`a_finished_meeting_is_streamed_as_its_id_to_one_authenticated_subscriber`
in [local_api/meeting_events.rs](../../src/local_api/meeting_events.rs)
(unauthenticated 401, `labs-off`, one subscriber, the event, the toggle
ending the stream); `meeting_finished_event_is_its_own_bit_and_off_by_default`
in [labs.rs](../../src/labs.rs). Not yet observed: a real recorded call
sending the event, on any OS; the Labs page goldens that show Meetings rows
need regenerating with the new row.

Meeting platform icons (`meeting-platform-icons`, issue 1290) graduated:
each recording in the Meetings list always shows its call service's icon, or
a neutral one ([meetings.md](meetings.md)). `hex labs on|off
meeting-platform-icons` still answers with that and exits 0
(`labs::GRADUATED`); the saved `labs.meeting_platform_icons` key is ignored.

Normal features, not Labs: dictation, Annotate (every build; its own pane
and switch), lowering audio while dictating (#210), Dot scope, chord shortcuts, QuickJS transformations, clipboard
restore (Labs › Keep dictation on clipboard skips it on macOS), the
vocabulary list itself (rewriting and search), and Linux's Terminal paste.

## Features

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
  fed and the word counts, never the words. Checks:
  `a_dictation_stream_publishes_its_text_after_each_feed_with_live_text`,
  `without_live_text_a_dictation_stream_publishes_nothing`,
  `each_dictation_stream_has_its_own_text`, and
  `a_failed_feed_clears_the_text_and_stops_publishing` in
  [transcription/engine.rs](../../src/transcription/engine.rs);
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
- Vocabulary biasing: Parakeet transcription biased toward the
  [vocabulary](vocabulary.md) (`transcribe_cpp::RunExtension::ParakeetVocabulary`),
  the request behind issue 452 / PR 457. Off (the default),
  `dictation::vocabulary::transcription_bias` returns nothing, so no run or stream takes
  the list; rewriting and search still use it, and the Vocabulary sheet says
  so.
- Terminal paste (Windows): paste with Ctrl+Shift+V instead of Ctrl+V for
  apps that ignore a synthetic Ctrl+V (issue 477 / PR 478). Its toggle is the
  shared `paste_with_shift`; Linux keeps it as a General › Advanced row.

Checks: `labs_is_one_flat_list_of_toggles_off_by_default`,
`contributed_options_are_labs_toggles_off_by_default`,
`the_browser_labs_toggle_turns_on_and_asks_for_system_audio`,
`keyboard_model_switching_is_a_labs_toggle_wherever_layouts_are_observable`,
`voice_filter_shows_only_while_its_labs_toggle_is_on`, and
`an_installed_driver_keeps_the_voice_filter_on` in
[panes.rs](../../src/rune_ui/tests/panes.rs);
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
