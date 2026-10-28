# Vocabulary (experiment)

One shared list of names Hex should know (people, projects, products), each
with optional "sounds like" spellings a transcript may show instead. It is the
`vocabulary` [experiment](experiments.md): with it off, nothing reads the list,
no Settings card shows it, `hex vocabulary` (listed in `--help` as an
experiment) is refused, and Hex behaves as before. macOS only for now.

```ts
Turn it on                               // vocabulary.gate
├── hex experiments enable vocabulary -> restart Hex
└── Off -> no Settings card, `hex vocabulary` refused (feature-off),
           prompts and search see an empty list   // vocabulary::entries / read_saved

General › Vocabulary › Names and terms   // vocabulary.settings; after Dictation
├── Each row -> Term | Sounds like (comma-separated) | ×
├── + -> New empty row; saved once the term has text
├── Add suggested -> Adds vocabulary::SUGGESTED terms not already listed
│   └── Every suggestion present -> Button hidden     // never added without a click
├── Empty -> "No terms yet."
├── Edits -> Debounced drafts, like Modes' corrections -> settings.json `vocabulary`
└── Past the limits -> Keep the term (cut to 64 characters) and the spellings
    that fit; inline danger note    // never drops the term

hex vocabulary                           // vocabulary.cli; marked as an experiment in --help
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
spellings. Transcription itself is unchanged: no model boosting or blind text
replacement.

Provisional defaults, each changed in one place:

```ts
hex vocabulary listed in --help          // control/cli.rs: add #[command(hide = true)] to hide it
Prompt wording                           // dictation_processor.rs: vocabulary_lines
Chapter prompt wording                   // meeting_chapters.rs: vocabulary_line
Short-name rule                          // meeting_query.rs: SOUND_ALIKE_MIN_CONSONANTS
Suggested terms                          // vocabulary.rs: SUGGESTED
```

Sources: [vocabulary.rs](../../src/vocabulary.rs),
[rune_ui/vocabulary.rs](../../src/rune_ui/vocabulary.rs),
[meeting_query.rs](../../src/meeting_query.rs) (`vocabulary_aliases`,
`sounds_alike`), [dictation_processor.rs](../../src/dictation_processor.rs)
(`vocabulary_lines`), and [control.rs](../../src/control.rs)
(`check_vocabulary`, `edit_vocabulary`).

Checks: `vocabulary::tests` (dedupe and merge, clamping, case-insensitive removal,
suggestions, comma splitting, reading the saved list without settings);
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
`rows_project_to_cleaned_merged_entries` and
`rows_past_the_limits_keep_their_term_and_what_fits` in
[rune_ui/vocabulary.rs](../../src/rune_ui/vocabulary.rs). Scenes:
`mac-settings-vocabulary`, `mac-settings-vocabulary-empty`. Passed on macOS on
2026-10-23. Not yet observed: the card edited in a running Hex, a CLI edit
against a running Hex, or a rewrite with a live provider.
