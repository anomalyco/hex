# Vocabulary

One shared list of names Hex should know (people, projects, products), each
with optional "sounds like" spellings a transcript may show instead. An empty
list (the default) changes nothing. The window edits the list on macOS,
Linux, and Windows; the `hex vocabulary` commands are macOS only for now.

```ts
General › Vocabulary   // vocabulary.settings; after Language
├── Row value -> "None", "1 term", "N terms" -> opens the Vocabulary sheet
├── Selected model transcription doesn't bias (ModelDefinition::uses_vocabulary)
│   -> muted note on the row and under the sheet's description: "Whisper
│   large-v3-turbo doesn't use these terms; rewriting and search still do";
│   Parakeet-based models (Parakeet, Underdog Bark) show none
├── Sheet description -> what the list does, then "Terms with spaces match
│   only when said in full" (a term is matched whole; "Dax Rad" doesn't help
│   "Dax" said alone, so a name also said alone gets its own row)
├── Sheet rows -> Term | Sounds like (comma-separated) | ×; scroll past 8
├── Empty list -> Opens with one blank row focused; opening never animates rows
├── Add term -> New empty row, focused; the list glides to its end and the row
│   rises 8 pt as it fades in (SETTLE, bounce 0; Reduce Motion: fade only);
│   saved once the term has text
├── Edits -> Debounced drafts, like Modes' corrections -> settings.json `vocabulary`
├── × -> Removes that row at any click speed or focus; rows keep an identity,
│   so a late edit, focus change, or second × for a removed row touches nothing
├── Done, Escape, or a press outside -> Saves, drops blank rows, closes
├── Past the limits -> Keep the term (cut to 64 characters) and the spellings
│   that fit; inline danger note    // never drops the term
└── Boost -> no control in the window: Normal (default) unless changed with
    `hex settings set vocabulary_boost off|light|normal|strong`
    (settings.json `vocabulary_boost`)  // vocabulary.boost

Transcription (Parakeet: RNN-T and TDT, Underdog Bark too)  // vocabulary.transcription; gguf_session::with_vocabulary
├── Every run and live stream reads the list and the boost at its start:
│   dictation, meetings, captures, the local API
├── First 64 terms (the engine's cap) and their spellings; empty or Off -> unchanged
├── Whole-clip run on an RNN-T model (Parakeet unified): meetings, captures,
│   the local API, dictation that falls back to the whole clip
│   └── The greedy words stand; where the audio could also spell a term, the
│       decoder follows the term alongside them, and the term replaces them
│       only when its score plus the boost per term piece beats theirs at the
│       word boundary (0.6 × the boost when the words don't sound like it)
├── Live stream or TDT model -> after the greedy decode, spans of up to 4 words
│   that look or sound like a term (or match a "sounds like" spelling) are
│   rescored on the same audio frames from the same decoder state: the term
│   replaces the span only when its score plus the boost per term piece beats
│   the heard words'
├── Left alone: case-only differences, inflections ("Abattoirs"), and addresses
│   and paths ("opencode.ai"); rescoring also skips terms under 5 letters
│   without a spelling, which the RNN-T decoder can still write ("Kip" -> Kit)
└── Whisper, Qwen3, SenseVoice, Cohere, Apple Speech -> no biasing

hex vocabulary                           // vocabulary.cli
├── list [--json]
├── add <term> [--sounds-like a,b] -> New term, or new spellings merged into it
│   └── Already there, nothing new -> changed: false
├── remove <term> -> Any case
└── Running Hex saves and reloads settings; an open window adopts the change

OpenCode rewriting (Modes)               // vocabulary.prompt
└── Prompt gains "Vocabulary (spell these exactly when the speaker means them):
    term (may be transcribed as a, b); …" before the foreground application

Live meeting chapters (experiment live-chapters)  // vocabulary.chapters
└── Chapter passes and the boundary pass start with "Names and terms that may
    appear: a, b, …" (first 30 terms, no spellings), read on each call

Search                                   // vocabulary.search
├── hex meetings search, MCP search_transcripts, hex history --search
├── Query word = a one-word term, fuzzy on
│   ├── Its "sounds like" spellings, or the term as two words -> 0.5 (like a prefix)
│   └── A word that sounds like it (≥2 consonant sounds, ≤ half the letters changed,
│       not a stopword or common word) -> 0.3
└── Aliases never count as exact; exact hits always rank first
```

The list lives in `settings.json` as `vocabulary: [{term, sounds_like?}]`,
written only when non-empty, so older files load unchanged and are not
rewritten. Terms are unique ignoring case; at most 500 terms, 64 characters
each, and 16 spellings per term. Read-only commands (`hex history`,
`hex meetings`, `hex mcp`) read the field straight from the file and never load
or migrate settings.

Short names with few consonant sounds (Rune, Dax) would sound like too many
words, so search finds their mis-transcriptions only through "sounds like"
spellings, and rescoring changes them only where a spelling matches.
Transcription never replaces text blindly: a term goes in only when the model
scores it close to what it heard on the same audio. Both live in Hex's
transcribe.cpp fork (branch `hex/parakeet-vocabulary-tree`:
`decode_rnnt_tree` in `src/arch/parakeet/decoder.cpp`, rescoring in
`vocabulary.cpp`); measurements are in
[vocabulary-boost-results.md](../research/vocabulary-boost-results.md).

Provisional defaults, each changed in one place:

```ts
hex vocabulary listed in --help          // control/cli.rs: add #[command(hide = true)] to hide it
Prompt wording                           // dictation_processor.rs: vocabulary_lines
Chapter prompt wording                   // meeting_chapters.rs: vocabulary_line
Short-name rule                          // meeting_query.rs: SOUND_ALIKE_MIN_CONSONANTS
Suggested terms                          // vocabulary.rs: SUGGESTED
Boost strengths (nats per term piece)    // vocabulary.rs: VocabularyBoost::nats
```

Sources: [vocabulary.rs](../../src/vocabulary.rs),
[rune_ui/vocabulary.rs](../../src/rune_ui/vocabulary.rs),
[meeting_query.rs](../../src/meeting_query.rs) (`vocabulary_aliases`,
`sounds_alike`), [dictation_processor.rs](../../src/dictation_processor.rs)
(`vocabulary_lines`), and [control.rs](../../src/control.rs)
(`edit_vocabulary`).

Checks: `vocabulary::tests` (dedupe and merge, clamping, case-insensitive removal,
comma splitting, reading the saved list without settings, the
first 64 terms and their spellings reaching transcription, boost names);
`vocabulary_round_trips_and_stays_out_of_files_without_one` in
[app_settings.rs](../../src/app_settings.rs);
`a_vocabulary_term_finds_its_mis_transcriptions_below_exact_hits`,
`sounds_like_spellings_and_split_words_alias_a_term`,
`vocabulary_never_pulls_in_common_words`, and
`sound_alikes_need_two_consonant_sounds_and_few_changes` in
[meeting_query_tests.rs](../../src/meeting_query_tests.rs);
`prompt_names_vocabulary_terms_and_their_mistranscriptions`;
`vocabulary_edits_report_whether_they_changed_the_list` and the request round
trip in control.rs; `vocabulary_lists_terms_with_their_spellings` in
[control/cli.rs](../../src/control/cli.rs);
`rows_project_to_cleaned_merged_entries`,
`rows_past_the_limits_keep_their_term_and_what_fits`,
`the_sheet_starts_empty_lists_with_a_row_and_saves_on_close`,
`removing_a_focused_row_leaves_the_others_and_late_edits_touch_nothing`,
`removing_the_last_row_then_editing_it_is_harmless`,
`rapid_removals_from_one_frame_remove_only_their_own_rows`, and
`a_reload_retires_the_old_rows` in
[rune_ui/vocabulary.rs](../../src/rune_ui/vocabulary.rs);
`the_vocabulary_row_opens_its_sheet`,
`add_term_scrolls_the_new_row_into_view`, and
`the_vocabulary_row_says_when_the_model_ignores_the_list` in [rune_ui/tests.rs](../../src/rune_ui/tests.rs);
`only_parakeet_based_models_use_the_vocabulary` in
[transcription_models.rs](../../src/transcription_models.rs). Scenes:
`mac-settings-vocabulary`, `mac-settings-vocabulary-empty`,
`vocabulary-unused` (Whisper, no golden). Passed on macOS on
2026-10-23; the unused-model note's checks have not been observed running. Not yet observed: the sheet edited in a running Hex, a CLI edit
against a running Hex, or a rewrite with a live provider.
