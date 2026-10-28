# Granola Import (research, not implemented)

Goal: bring older Granola meetings into Hex's meeting format so `hex mcp`
can search them beside Hex recordings. Researched from Granola's public docs on
2026-10-15; nothing was logged into, called, or decrypted.

## Official Paths

```ts
Granola desktop
├── CSV export                    // Settings > Profile > Generate CSV
│   ├── Emailed link, valid 24 h, opened while signed in; 1 export / 24 h
│   ├── Title, summary, transcript, basic details; full history, including
│   │   notes older than 30 days on Basic (free)
│   ├── Only notes you own that have a summary; no deleted notes
│   └── On by default for Basic and Business; Enterprise admins may disable
├── Personal API                  // https://public-api.granola.ai/v1
│   ├── Bearer grn_ key from Settings > Connectors > API keys
│   ├── Business plan members (Enterprise: admin-gated scopes)
│   ├── GET /v1/notes (created_after, cursor), GET /v1/notes/{not_…}
│   └── GET /v1/notes/{id}/transcript: items with speaker.source
│       microphone|speaker, attribution, optional name, start_time/end_time
└── Granola MCP                   // https://mcp.granola.ai/mcp, OAuth
    ├── Basic: personal notes from the last 30 days; transcript tool paid only
    └── Business: full history, get_meeting_transcript
```

Sources: [Exporting historical notes](https://docs.granola.ai/help-center/sharing/exporting-notes),
[Granola API](https://docs.granola.ai/introduction),
[Get Transcript](https://docs.granola.ai/api-reference/get-transcript),
[Granola MCP](https://docs.granola.ai/help-center/sharing/integrations/mcp).
Granola also documents transcript auto-deletion; check that setting before
assuming old transcripts still exist.

The local cache on the Pro (`cache-v6.json.enc`, `granola.db`) is encrypted.
Do not reverse-engineer it; third-party scripts that do are unofficial.

## Mapping To Hex

Granola's macOS transcript sources map directly: `microphone` -> `You`
(`MeetingSource::Microphone`), `speaker` -> `Them` (`System`). Absolute
`start_time`/`end_time` become offsets from the first item. A note becomes a
folder `meetings/granola-<not_id>/` with a `manifest.json` (status `complete`,
`created_at_ms` from the meeting start, `app: "Granola import"`, title), a
`transcript.ndjson`, and a `transcript.md` so the folder reads as published.
Granola summaries are not Hex notes; keep them in a separate field or a
Markdown file rather than impersonating Apple Intelligence `notes`.

## Recommendation

1. Kit runs the CSV export once (free on his plan, full history, no API key).
2. A one-shot importer (`hex meeting import-granola <csv>`, debug first) writes
   folders atomically, skips ids that already exist, and never overwrites.
   CSV transcripts may lack timestamps or speaker sources; check one real row
   before designing the parser, and fall back to a single `Them` stream with
   zero offsets if they do.
3. Use the Personal API only if the CSV lacks speaker sources or times and Kit
   is on Business; it needs a stored `grn_` key, so ask Kit first.

Open questions: Kit's Granola plan, whether the CSV transcript column keeps
microphone/speaker attribution and timing, and whether auto-deletion removed
older transcripts.
