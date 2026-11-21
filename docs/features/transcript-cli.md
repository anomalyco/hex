# Read retained transcripts

```ts
Agent or terminal                                // transcripts.cli; macOS release
├── hex history -> Timestamp, app, recorded mode, final text
│   ├── --since / --until / --app / --grep -> Filter, then limit
│   ├── --search <query> -> Rank filtered entries with meetings' fuzzy BM25
│   │   (raw + final text count as one document)
│   ├── --raw -> Include the stored pre-processing transcript
│   ├── --oldest-first -> Reverse timestamp order
│   ├── --json -> Version 1 response, with truncation flag
│   └── stats [--since/--until] [--json] -> Labs › Dictation stats (labs.history_stats):
│       dictations, words, words per minute, changed by Output, daily streak,
│       top apps over every retained entry; toggle off -> fails naming
│       `hex labs on history-stats` before History is read
└── hex meetings
    ├── list -> Newest first, overlapping time range, live status
    ├── show <id|latest> -> Markdown or versioned JSON with notes and entries
    │   ├── --from / --to <mm:ss|h:mm:ss|seconds> -> Only entries overlapping
    │   │   the range, finished or still recording; inverted or past-the-end
    │   │   ranges fail with an error
    │   └── --chapter N -> Only chapter N's entries (not with --from/--to)
    ├── chapters <id|latest> -> Live chapters (experiment live-chapters):
    │   spans, names, lines, whether corrected; none -> says so; --json
    ├── search <query> -> MCP's stemmed, fuzzy BM25 passages; exact stems first
    └── delete <id|prefix> -> the running Hex trashes the meeting and its files
        (Labs › Delete meetings from the command line; Meetings on); the only
        verb here that is not a read
```

Commands work beside the running app and independently of feature opt-ins,
except `hex history stats` (and `meetings show --srt|--vtt`), which need their
Labs toggle, and `meetings delete`, which needs the running Hex, Meetings on,
and its own Labs toggle.
`control/transcript_cli` dispatches before logging, instance locking, local
API, GUI, permissions, and Hex Microphone. Nothing is created or repaired.
Debug `meeting list/show` shares this path; recording remains debug-only.

History uses one open-file snapshot and the saved retention, so concurrent atomic
replacement cannot expose a partial write and expiry is enforced before disk
cleanup. Off retains existing entries. Missing settings default to a week;
malformed history/settings fail with an error. The same owner-only store now
holds at most 10,000 entries and 16 MiB of raw + final text, without an archive.
Already-evicted entries remain unavailable. UI search still returns at most 200;
CLI limits apply after filtering and allow up to 10,000.

`meetings delete <id>` is the one verb that changes anything: the running Hex
resolves the id with the same exact-id or unique six-character-prefix rule as
`show` (never `latest`), then moves the meeting's exported Markdown, its
tracked notes file, and its recording folder to the Trash exactly as the pane's
Delete… does. It refuses with `feature-off` while Meetings or
Labs › Delete meetings from the command line is off, `not-found` for an unknown
or ambiguous id, and the meeting's own code while it records, transcribes,
writes notes, or waits for its final transcript, and `failed` when the Trash
refuses. Success names the deleted id, and an open Meetings pane lists again on
its own one-second rescan. The other verbs never change the store.

Sources: [control/transcript_cli.rs](../../src/control/transcript_cli.rs),
[control.rs](../../src/control.rs) (`delete_meeting` and its host tests),
[history.rs](../../src/history.rs), [meeting/query.rs](../../src/meeting/query.rs),
and early dispatch in [main.rs](../../src/main.rs).
[Command and JSON contracts](../transcript-cli.md) document flags and schema.

Checks in [control/transcript_cli_tests.rs](../../src/control/transcript_cli_tests.rs) cover
filters/order, ranked `--search` within retention, one document per entry
(`history_search_counts_raw_and_final_text_as_one_document`), retention and malformed settings, JSON snapshots, atomic-replace
reads, parsing, and `show --from/--to` JSON, Markdown,
and errors, including `--to 0` (`meeting_show_ranges_filter_json_entries_and_markdown`),
and `chapters` / `show --chapter` JSON and Markdown on the synthetic
`tests/fixtures/meetings-chapters` meeting
(`meeting_chapters_list_and_select_a_chapter_in_json_and_markdown`), and
`meeting_delete_parses_an_id_and_json_beside_the_read_commands` covers the new
verb's parse. Existing history checks cover bounded
storage, owner-only files, and expiry. Meeting query fixtures cover locks, partial
NDJSON, interrupted drafts, and search parity with MCP. These fixture checks do
not establish a signed installed-app invocation; native install evidence must be
recorded separately.

`scripts/test-transcript-cli.py <release-binary>` exercises the real command
parser against copied fixtures while holding the listener lock. It checks that
the commands create no logs or API discovery file and leave all input bytes and
modification times unchanged, with experimental microphone mode requested.

Observed October 22, 2026, on Pro before and after the Rune integration: all 35
targeted history, meeting-query, and CLI tests passed; formatting and
all-target/all-feature Clippy with `-D warnings` passed. The optimized release
binary passed the occupied-lock/no-write smoke. Claude Opus 5.5 reviewed the
implementation and integration independently and found no blockers.
