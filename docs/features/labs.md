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
below is `Everyone` today. There is no master switch. **Labs** is always the last item in the
sidebar and lists every Labs feature as one flat row (name, one-line
description, toggle), each **off by default**, plus Send feedback and Run
setup again.

```ts
Feature             Pane  Toggle (settings.json, hex settings)   On: what it does
├── Meetings (beta) yes   meetings.enabled             (macOS)   Meetings joins the sidebar: call detection, recording, transcripts
├── Voice Filter    yes   experimental.hex_microphone  (macOS, Linux) Voice Filter joins the sidebar; feeds the Hex Microphone (macOS driver, Linux PipeWire source)
├── Playground      yes   labs.playground                        a Playground sidebar item and ⇧⌘D open the story browser
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
├── Spoken formatting no labs.spoken_formatting      spoken "new line"/"line break", "new paragraph", "open/close quote" become line breaks and quotation marks (#630)
├── Spoken symbols  no    labs.spoken_symbols          spoken "hashtag", "underscore", "comma", … become symbols joined to their words: "#release", "my_variable", "done, thanks" (#678)
├── Remove filler words no labs.filler_words          um, umm, uh, uhh, erm, er, ah leave dictations, with no model (#631)
├── Subtitle export no    labs.subtitle_export         (macOS)   hex meetings show --srt|--vtt writes a meeting as timed subtitles (#633)
├── Second dictation shortcut no labs.second_dictation_shortcut + second_dictation_hotkey (macOS)
│                                        another shortcut dictates like the main one (#627); General shows its recorder while on
├── Smart spacing   no    labs.smart_spacing           (macOS)   a dictation pasted right after a word gets one leading space (#632)
├── Sided modifier chords no labs.sided_modifier_chords (macOS) the recorder keeps each modifier's side in a modifier-only chord: R⌘ R⌥ (#710)
├── Tap or hold      no    labs.tap_or_hold             (macOS)   in Toggle mode, a press held past 300 ms dictates until it lifts; a short tap still locks (#711)
├── Sort and filter models no labs.picker_sort_filter   the Local transcription picker sorts by recommendation, size, or name and can show only models for the saved language (#645)
├── Benchmark details no  labs.benchmark_details        picker cards show raw WER/CER (lower is better), language, dataset, source, and conditions; incomparable "Most accurate" badges drop (#647)
├── Use your own speech model no labs.custom_models     the picker's "Add model file…" and `hex model import` copy in a GGUF checked from its header (#648, #743)
├── Parakeet Ultra  no    labs.parakeet_ultra          the picker offers Moondream's Parakeet Ultra (pinned Q8_0, CC BY 4.0) with a details view; stays on while it dictates (#648)
├── Special keys alone no labs.standalone_special_keys (Linux) Menu, Pause, Scroll Lock may be a shortcut's key, alone or with modifiers (#767)
└── Terminal paste  no    paste_with_shift             (Windows) paste with Ctrl+Shift+V (#477/#478; Linux keeps it in General)

Settings › Labs                          // labs.page
├── "Try unfinished features. They may change or break." + Send feedback -> https://github.com/anomalyco/hex/issues/new
├── Opened by a link (macOS) -> hex://labs opens this page; hex://labs/<feature> also scrolls to the
│   feature's row (centred) and pulses it for ~1.4 s (layer_02 fill, then back); it never turns the
│   feature on. An unknown or absent name -> the page opens and process.log notes it. hex://settings/<pane>
│   (general, output, history, meetings, voice-filter) opens a main pane; one Hex doesn't show -> General.
│   Links over 2048 bytes are ignored; a test build's scheme is hex-test://
├── One row per feature this OS has -> press flips it, saved at once
│   ├── A pane feature on -> its pane is a normal top-level sidebar item; off -> hidden entirely
│   └── Voice Filter while its driver is installed -> on, disabled, "Uninstall the driver in Voice Filter to turn it off."
└── Run setup again -> replays the new onboarding (below)

hex labs                                 // labs.cli; edits settings.json, no running Hex
├── (list) -> every feature this build has: name, on/off, audience, description, its hex settings key
├── link <feature> -> macOS: prints hex://labs/<feature> (hex-test:// in a test build); no Copy link on the rows, 20 of them would only add noise
├── on <feature> | off <feature> -> a running Hex saves and applies it (hex settings set); otherwise settings.json
├── off voice-filter with the driver installed -> refused: uninstall the driver
└── hex experiments … -> points at hex labs

hex settings set labs.live_text true     // macOS: through the running Hex
```

Checks: `rune_ui::deep_link` tests (parser, every feature's link reads back);
`a_labs_link_*` in tests/panes.rs and the `mac-labs-link` golden (Linux).

Settings saved before this model may carry `labs.enabled`; it is read and
ignored, and every feature keeps its own toggle. `hex status` lists the
features on (`Labs` line on macOS, `labs` array in the macOS and Linux JSON,
`labs\t…` on Windows).

Every toggle applies at once, from the window or `hex labs on|off` (issue
568, from @maharshi365): Live text is asked as each dictation starts and its
plate is always installed (prewarmed only when Live text is on at launch);
the window always attaches the playground with Rune's own ⇧⌘D off, and its
sidebar item, ⇧⌘D, the macOS View menu, and story links open it only while
the toggle is on. Live chapters are read when a recording starts. Nothing
starts at launch for a feature that is off. Limit: the macOS window's fixed
size is chosen when it is created, so it becomes resizable for the
playground only after Hex restarts.

Gate a Labs feature where its work starts (the worker, thread, or capture),
not only where its UI appears: `labs::live_text()`, `labs::live_chapters()`,
`labs::playground()`, `labs::vocabulary_biasing()`,
`app_settings::meetings_enabled()`, or `app_settings::hex_microphone_enabled()`.
Add a new feature to `labs::Feature` (name, settings key, description, audience), give
it a row in `rune_ui/labs.rs` and a `hex settings` key, and default it off.
When a feature graduates, remove it from Labs.

Mouse shortcuts off: the recorder ignores mouse buttons, `hex settings`
refuses one, and a saved mouse-button shortcut goes back to its default when
settings load (macOS), with a one-time note in General
(`mouse_shortcut_notice`). Check:
`mouse_shortcuts_go_back_to_their_default_while_their_labs_toggle_is_off` in
[app_settings.rs](../../src/app_settings.rs).

Unload model when idle (`unload-idle`, issue 442 from @theoneand33): while
on, its row shows "Unload after" (5, 15, 30, or 60 minutes; default 15).
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
replacements). Whole phrases, ignoring case, are replaced: "new line" and
"line break" by one line break, "new paragraph" by two, "open quote" and
"close quote" by `"`; the commas, periods, and spaces the model puts around a
command go with it ("Hello. New line. Next" becomes "Hello.\nNext"). Words
such as "newline" or "new liner" stay as they are. Only English phrases are
recognized. Dictate without processing, meetings, and live text are
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
Whole English words, ignoring case, become symbols joined to their
neighbours: "hashtag" and "hash sign" become `#` and "at sign" `@`, joined to
the next word ("hashtag release" → "#release"); "underscore" `_`, "hyphen"
`-`, and "slash" `/` join both words ("my underscore variable" →
"my_variable"); "comma" `,`, "colon" `:`, and "semicolon" `;` join the
previous word and leave one space before the next ("done comma thanks" →
"done, thanks"). The commas, periods, and spaces the model puts around a
spoken symbol go with it. Words such as "commas", "hashtags", "slashed", or
"colonel" stay as they are, and the word list is fixed (no per-user symbol
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
Chromium 155/156 typing path use the adjusted text. Lowercasing the first
letter is not done. Off, pasting is exactly as before. Checks:
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

Tap or hold (`tap-or-hold`, issue 711, macOS): one dictation shortcut
works as both tap-to-toggle and push-to-talk. It acts only in Toggle mode
(`shortcut_mode: toggle`). The gate is the macOS shortcut machine
(`DictationHotkey` in [suppression.rs](../../src/suppression.rs)), which reads
`labs::tap_or_hold()` at each release of a Toggle press. With it on, a press
released at least `MINIMUM_HOLD_DURATION` (300 ms, the hold threshold) after
it started finishes the capture on release, as Hold does, with the usual
200 ms tail. A shorter press locks the capture as plain Toggle does, and the
next press finishes it. Chord discards and the press that finishes a lock are
unchanged. It applies to every `DictationHotkey` in Toggle mode.
Off, or in Hold mode, nothing changes. Linux and Windows don't have it.
Checks: `tap_or_hold_locks_a_short_tap_and_finishes_a_long_hold_on_release`
and `tap_or_hold_still_discards_a_chord_and_off_keeps_plain_toggle` in
[suppression.rs](../../src/suppression.rs) (macOS only);
`tap_or_hold_is_its_own_bit_and_off_by_default` in
[labs.rs](../../src/labs.rs). Not yet observed: a real Mac with the toggle
on. The row renders only in macOS builds, so no golden changes.

Sort and filter models (`picker-sort-filter`, issue 645, every OS): while
on, the Local transcription picker shows one row between its header and the
cards: a sort (By recommendation, the default and the catalog's order; By
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
each Local transcription picker card that has a benchmark record
(`transcription::models::benchmark`; Apple Speech has none and keeps
"Accuracy: System") shows "Error rate" as the raw figure with its direction,
such as "7.57% CER · lower is better", in place of the transformed accuracy
percentage, and one wrapping line under the metrics: the measured language,
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
7045d1c); every other figure is English WER with dataset and source
unrecorded. A "Most accurate" badge stays only when its record is comparable
(`BenchmarkRecord::comparable`: same metric, language, and a known, equal
dataset) with every other recommended model's for the saved language, so
today it drops everywhere. The gate is the picker's card (`benchmark_details_on`);
the feature does no other work. Off, the picker is exactly as before.
Checks: `benchmark_details_show_raw_rates_and_mark_unknowns`,
`most_accurate_needs_comparable_figures_only_with_benchmark_details`, and
`benchmark_details_are_off_by_default` in
[picker.rs](../../src/rune_ui/picker.rs);
`only_figures_on_the_same_metric_language_and_dataset_compare` in
[models.rs](../../src/transcription/models.rs). Not yet observed: the cards
rendered with the toggle on; there is no `--snap` scene for it, and the Labs
page goldens need regenerating on Linux with the new row.

Use your own speech model (`custom-models`, issues 648 and 743, every OS):

```ts
Picker header -> "Add model file…" -> native dialog (GGUF, All files)   // labs.custom-models
├── Header check, before anything loads (transcription::gguf_header): GGUF v2/v3;
│   general.architecture parakeet | whisper | qwen3_asr | cohere_asr | sensevoice;
│   stt.variant; general.languages with a code Hex knows; only F32, F16, and
│   Q8_0 tensors (the types every build runs on Metal and Vulkan); the tensor
│   data fits the file. Anything else -> "<file> can't be imported: <reason>"
├── Copy -> models/custom/<16 hex>.gguf while hashing; the copy is checked again;
│   receipt <16 hex>.gguf.verified.json (size, SHA-256, mtime); manifest
│   <16 hex>.json (name, general.name, family, variant, languages, detection,
│   file name, licence, repo URL). The original is never touched; a failed or
│   cancelled import removes only its partial copy; the same file again keeps
│   the first import
├── ID custom-<first 16 hex of the SHA-256> (TranscriptionModelId::Custom)
├── Card -> name from general.name (Rename edits it), languages from the file,
│   offline only (no streaming), Parakeet without language prompt or Auto,
│   no benchmark; Use prepares it like a catalog model (verifies Hex's copy
│   against its receipt instead of downloading)
├── Remove -> Hex's copy, receipt, and manifest only; not the active model
├── Changed -> "Check again" rehashes the copy: the recorded SHA-256 again ->
│   new receipt, ready; else it stays changed. Missing or changed -> "Import
│   again…": the same file replaces the copy and keeps the import's name
└── Saved import missing, changed (size or mtime), or toggle off -> the choice
    stays saved; the listener runs a downloaded fallback (the system language's
    recommendation if downloaded, else the first downloaded catalog model;
    custom_models::runnable); General's model row says so; the card warns
```

The local digest identifies the file Hex copied; it is not a publisher
checksum and Hex vouches for nothing in it. The selection, recent models,
and keyboard-language mappings keep an import while it can't run
(`models::validate_saved`), and a build that doesn't know `custom-…` keeps its
default for that field without rewriting `settings.json` (#744). `hex model
import <path> [--name N]`, `hex model rename <id> <name>`, `hex model repair
<id>`, and `hex model remove <id>` (also `hex models …`) run in the CLI process; a running Hex lists
the change at once (the registry rereads `models/custom/` when its folder
changes). `hex model list`, the local API's `/models`, and the Linux and
Windows tray list ready imports; the macOS menu bar's model list is built at
launch from the catalog and does not list them yet.

Observed on apollo (Linux, Radeon 8060S over Vulkan, 2026-11-01): Parakeet
primeLine Q8_0 (`handy-computer/parakeet-primeline-gguf`, not in the catalog)
imported as `custom-dbd1ec2985aea64d`, verified, loaded, and transcribed the
FLEURS German and English samples through `hex model transcribe` (the Linux
dictation loader); a junk file and the toggle off were refused; touching or
moving the copy made it "changed" or "missing" with the saved selection kept;
rename and remove worked and the original stayed. Not observed: the picker
flow in a real window, macOS (Metal) and Windows at all.
Checks: `transcription::gguf_header::tests`,
`transcription::custom_models::tests`, and the picker golden
`rune-picker-custom-models-light`.

Parakeet Ultra (`parakeet-ultra`, issue 648, every OS): a pinned catalog
entry, `parakeet_ultra`, listed and runnable only while the toggle is on
(`ModelDefinition::available` reads `labs::parakeet_ultra`).

```ts
Picker card -> "Parakeet Ultra" + Labs tag; 25 languages, offline, Token     // labs.parakeet-ultra
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
├── Other OSes -> no toggle; a bare one still needs a modifier, as before
├── Window recorder -> refuses them while off ("Add a modifier"); Rune reports
│   none of these keys yet, so the recorder can't record them either way
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
- New onboarding (no toggle; it runs only when invoked): the onboarding
  story, replayed on every OS
  (`rune_ui/onboarding_kit/live.rs`): the real model download, Microphone and
  Accessibility rows that follow macOS (ready at once on Linux and Windows,
  which ask for no permissions), and the practice step as a mini game
  (`onboarding_kit/game.rs`): one lit message card at a time, answered by a
  real dictation (live words fly in while the shortcut is held, the paste
  lands in an unseen focused field), struck by lightning and burnt away,
  then a score card with Keep playing and Continue; Done always skips on.
  The window's tour stays
  the state machine underneath. Labs' **Run setup again** and
  `hex onboarding reset` (macOS control socket, the Linux service socket,
  and a named event on Windows) forget that onboarding finished
  (`desktop::onboarding::reset_at`) and replay the first run without touching
  settings, history, models, or permissions; ready steps pass at once. A new
  install still gets the tour as its first run. Checks:
  `labs_run_setup_again_opens_the_first_run` and
  `run_setup_again_opens_the_first_run_on_linux_and_windows` in
  [panes.rs](../../src/rune_ui/tests/panes.rs). Every OS reads the practice
  dictation from the listener's event log (`rune_ui/practice.rs`). The
  game's checks are the `onboarding_kit::game` tests; not yet observed: a
  real dictation playing it end to end.
- Live text: a streaming dictation shows what you have said so far while
  you dictate, as one click-through line just past the recording HUD (every
  OS; see [Streaming Finish](dictation.md#streaming-finish)). Its toggle
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
  ├── Output, Modes … HUD Lab, Activity, Transcription model, Shortcut explainer, Onboarding tour
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
