# Read transcripts from the command line

The macOS release binary can read retained dictations and recorded meetings
while Hex runs. Use `/Applications/Hex.app/Contents/MacOS/hex` if `hex` is not
on your PATH.

```sh
hex history --since 3d --app 'OpenCode Beta' --grep problems --json
hex history --since 2026-10-20 --until 2026-10-21 --raw --oldest-first --limit 500
hex history --since 7d --search 'problems board' --json
hex history stats --since 7d --json   # Labs › Dictation stats
hex meetings list --since 7d --json
hex meetings show latest --md
hex meetings show 1790000500000-1002 --json
hex meetings show 1790000500000-1002 --from 113:03 --to 120:43 --md
hex meetings chapters latest --json
hex meetings show 1790003000000-1005 --chapter 2
hex meetings show latest --srt > call.srt   # Labs › Subtitle export
hex meetings search 'release schedule'
```

## History filters apply before the limit

`history` returns successful retained results, including Send and Voice Action.
Each result has a timestamp, application, recorded processing mode if present,
and final text. `--raw` adds the stored pre-processing transcript. Older entries
may have no app or mode; Hex does not infer missing metadata.

- `--since` is inclusive and `--until` is exclusive. A date alone means local
  midnight; `--until 2026-10-20` includes all of October 20.
- Times accept `3d`, `36h`, `90m`, `2w`, `today`, `yesterday`, `now`, dates,
  local date-times, RFC 3339, or Unix milliseconds (10-digit values are seconds).
  Relative times count backwards from the query time.
- `--app` matches the entire application name, ignoring case.
- `--grep` matches a case-insensitive literal substring in raw or final text,
  including raw text when `--raw` is absent. It is not a regular expression.
- `--search QUERY` ranks the remaining entries with the meeting search's
  stemmed, fuzzy BM25 over final and raw text, so `bord` finds `board` and
  `scheduling` finds `schedule`. Quoted phrases are required, exact (stemmed)
  matches. Unmatched entries are dropped. It combines with the other filters and
  cannot be used with `--oldest-first`. Statistics cover only the filtered,
  unexpired entries, so scores compare within one response, not across queries.
- Results sort by timestamp and then ID, newest first; with `--search`, by score
  and then newest first. `--oldest-first` reverses the time order. `--limit`
  defaults to 50 and accepts 1–10,000.
- The saved retention window filters expired entries even before the app's next
  cleanup. Missing settings use seven days. Invalid settings fail rather than
  silently widening the retention window. Off preserves existing entries and
  prevents new ones, matching the History pane.

## History stats sum up what is retained

`history stats` (Labs › Dictation stats, `hex labs on history-stats`, off by
default) prints a summary of the retained entries: dictations, words (the
inserted text split on whitespace), words per minute (words over recorded
audio minutes, counting only entries with audio; unknown without any), how
many entries Output changed (inserted text differs from the transcript, ignoring
surrounding whitespace), the daily streak (consecutive local days with an entry,
ending today or, with none yet today, yesterday), active days, and the five
most used applications. With the toggle off it fails, naming the command that
turns it on, before History is read. `--since` and `--until` narrow it with the
same time syntax; the retention window always applies. It counts only what
History still holds: nothing is kept after retention removes an entry, and
which replacements fired is not tracked.

## Meetings use the same queries as MCP

`meetings list` defaults to 20 entries (maximum 200). It accepts `--since` and
`--until`, using the same time syntax as history. Meetings qualify when their
duration overlaps the range; they sort newest first. Active recordings are
included and labeled. Unreadable folders are reported, not repaired.

`meetings show` accepts an exact ID, an unambiguous prefix of at least six
characters, or `latest` (the newest readable meeting, including one in progress).
The default and `--md` return full Markdown with metadata, stored notes, and
speaker turns. `--json` returns transcript entries with relative millisecond
offsets. No notes or transcripts are generated.

`--from` and `--to` limit `show` to part of the meeting, with either output.
They take offsets into the meeting as `mm:ss` (minutes may exceed 59, as in
`113:03`), `h:mm:ss`, or seconds (`90`, `1.14`, `90s`, `1.5m`), rounded to the
millisecond; either may be omitted. Minutes and hours are whole numbers,
seconds stay below 60, and so do `h:mm:ss` minutes; negative, infinite, or
overflowing offsets are rejected. Every entry overlapping the range is included
whole, so the first and last entries may start before `--from` or end after
`--to`. Markdown adds a `- Range:` line; JSON adds `range`. A `--from` at or
after `--to` (an omitted `--from` is 0, so `--to 0` fails), or at or after the
meeting's end, is an error. A recording in progress runs until now, so a range
past its rolling transcript so far is empty rather than an error, and one past
now fails with "so far". The MCP `get_transcript` tool takes the same range as
`start`/`end` or, as separate schema fields, `from`/`to`; giving both names for
one bound is an error.

`meetings chapters` lists the chapters a meeting recorded with the
`live-chapters` experiment: number, `mm:ss–mm:ss` span, model-written name, and
one line, and whether the boundaries were corrected after Stop. It takes the
same IDs as `show`. A meeting without chapters says so (text) or returns an empty
`chapters` list (JSON); nothing is generated. `show --chapter N` returns only
chapter N's entries (counting from 1), with a `## <name>` section in Markdown
and `chapter` in JSON; it cannot be combined with `--from`/`--to`, and a number
outside the list is an error. Plain `show` Markdown adds a `## Chapters` list
when there are any.

`show --srt` and `show --vtt` (Labs › Subtitle export, `hex labs on
subtitle-export`, off by default) write timed subtitles to stdout instead,
for example `hex meetings show latest --vtt > call.vtt`. With the toggle off
they fail, naming the command that turns it on, before any meeting is read.
They take the same IDs, `--from`/`--to`, and `--chapter` as the other outputs
and conflict with `--md`, `--json`, and each other. Each transcript entry (a
speaker turn) becomes one cue timed by its offsets from the meeting's start,
not by word timings: SRT numbers its cues, uses `HH:MM:SS,mmm` times, and
prefixes the text `You:` or `Them:`; VTT starts with `WEBVTT`, uses
`HH:MM:SS.mmm` times and `<v You>`/`<v Them>` voice tags, and escapes `&`, `<`,
and `>`. An end before its start is clamped to the start, entries with no text
are skipped, and blank lines and `-->` are removed from the text so a cue
can't break.

`meetings search QUERY` uses the MCP query's stemmed, fuzzy BM25 ranking over
transcripts and stored notes. Quoted phrases constrain the search. `--since`,
`--until`, and `--limit` (default 10, maximum 50) are supported; output is text.
Search for a timestamp, then read around it with `show --from/--to`.

Exact stems clearly outrank fuzzy matches. Passages matching every unquoted
term exactly come first, whatever their scores. Within each tier a prefix
counts half as much as an exact stem, one or two edits 0.35, and a Soundex
sound-alike 0.2, also for the share of terms a passage covers. Spelling
fallbacks (edits and sound-alikes) are tried only for a term found exactly in
fewer than three passages, and never reach a word in more than 0.5% of the
passages searched (and at least five), except that a term with no exact match
still reaches a common word one edit away (never two). So `blob` does not pull
in `blog`, `Bob`, or `believe` once three passages say `blob`, and `media` does
not pull in a word as common as `made`. When several words in a passage could
stand for one term, the exact stem wins, then the higher score, the more
trusted match, and the alphabetically first word, so results never vary between
runs. `hex history --search` uses the same ranking, with an entry's raw and
final text counted as one document (so a word in both counts once) and its
better text as the hit.

With the `vocabulary` experiment on, both searches (and MCP
`search_transcripts`) also read the saved [vocabulary](features/vocabulary.md)
from `settings.json` without loading or migrating it. A query word that is a
one-word vocabulary term also finds its "sounds like" spellings and the term
split into two adjacent words (0.5, like a prefix), and words that sound like
it (0.3) unless they are common. Neither ever counts as an exact match.

Debug builds retain `meeting record`, `watch`, `probe`, `transcribe`, and other
developer commands. Their `meeting list/show` commands use the read-only query
path too. The plural `meetings` commands are available in release builds.

## JSON version 1

Every JSON response has integer `version: 1`. Consumers should ignore unknown
fields; incompatible field changes require a new version. All `*_ms` fields are
integer milliseconds. Absolute timestamps use Unix time. Nullable fields are
present as `null`, except `raw_text`, which is omitted without `--raw`,
`score` and `snippet`, which are omitted without `--search`, `range`, which
is omitted without `--from` or `--to`, `chapters`, which is omitted when a
meeting has none, and `chapter`, which is omitted without `--chapter`.
JSON goes to stdout; errors go to stderr with a nonzero exit status. Empty
results are successful responses. `truncated` reports the query's output limit,
not data already expired or evicted from the store.

```ts
type History = {
  version: 1
  retention: "off" | "day" | "week" | "month" | "forever"
  truncated: boolean
  entries: {
    id: number
    timestamp_ms: number
    application: string | null
    mode: string | null
    kind: "dictation" | "send" | "voice_action" | "printed" | "raw"
    final_text: string
    raw_text?: string // present only with --raw
    score?: number // present only with --search; higher is more relevant
    snippet?: string // present only with --search; matches wrapped in **
  }[]
}

type HistoryStats = { // hex history stats --json
  version: 1
  retention: "off" | "day" | "week" | "month" | "forever"
  dictations: number
  words: number
  audio_ms: number
  words_per_minute: number | null // null without recorded audio
  changed_by_output: number
  streak_days: number
  active_days: number
  top_apps: { application: string; dictations: number }[] // at most 5, most first
}

type Meeting = {
  id: string
  title: string
  timestamp_ms: number
  duration_ms: number
  application: string | null
  status: "starting" | "recording" | "transcribing" | "complete" | "interrupted" | "failed"
  in_progress: boolean
  transcript_source: "final" | "rolling" | "missing" // older builds also reported "draft"
  warnings: string[]
}
type MeetingList = {
  version: 1
  meetings: Meeting[]
  skipped: { id: string; reason: string }[]
  truncated: boolean
}
type MeetingShow = {
  version: 1
  meeting: Meeting
  notes: { title: string; summary: string; generated_at_ms: number } | null
  range?: { from_ms: number; to_ms: number | null } // only with --from or --to
  chapters?: Chapter[] // only when the meeting has chapters
  chapter?: Chapter // only with --chapter
  entries: { // with a range or chapter, only the entries in it
    source: "microphone" | "system" // You / Them; remote speakers aren't separated
    start_ms: number // relative to the meeting start
    end_ms: number
    text: string
  }[]
}
type Chapter = {
  number: number // counting from 1
  title: string // model-written
  summary: string // one line; may be empty
  start_ms: number // relative to the meeting start
  end_ms: number // the next chapter's start, or the meeting's end (now while recording)
}
type MeetingChapters = {
  version: 1
  meeting: Meeting
  chapters: Chapter[] // empty when none are stored
  refined: boolean // boundaries corrected over the whole transcript after Stop
}
```

## Reads never start or change the app

These commands dispatch before logging, instance locking,
local API startup, permissions, GUI, and Hex Microphone. They open no microphone,
load no model, make no network request, and create no files or directories.
History reads one file descriptor, so an atomic replacement yields a complete
old or new snapshot. Malformed history produces an error without recovery writes.

Data comes from `~/Library/Application Support/hex`. `HEX_APPLICATION_SUPPORT_DIR` overrides the
root for fixtures or a mirrored copy. These CLI commands currently target macOS;
Linux and Windows retain their existing command surfaces.

## Retention stays bounded in one store

The history cap is now 10,000 entries or 16 MiB of raw plus final text, whichever
is reached first, up from 2,000 / 4 MiB. Per-entry limits, owner-only files,
atomic replacement, expiry, explicit deletion, and Clear all remain shared with
the History pane. Older entries already evicted cannot be recovered by this change.

Keeping one store avoids a second set of expiry and deletion rules. A monthly
archive would need atomic rollover, ID deduplication, expiry while idle, and
deletion across every file. The larger cap is a bounded increment using the
existing store; it still rewrites the JSON file on each successful result. The
cap is not a guarantee to retain every result within a chosen window, and
Forever is also hard-capped. No separate archive is created.
