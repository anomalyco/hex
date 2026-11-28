---
name: hex
description: Read Hex's local dictation history and meeting transcripts, or drive Hex dictation, settings, and models from the command line. Use when asked what was said in a meeting, for a transcript or notes, to search past dictations, or to install and manage Hex. Everything is local; Hex has no cloud API.
---

# Hex

Hex is a local voice appliance for macOS, Windows, and Linux: dictation, no-bot
meeting transcription, screen capture, and a read-only MCP server. The `hex`
command reads files on this machine. It never sends audio or text anywhere.

Source: https://github.com/anomalyco/hex (MIT). Docs:
https://anoma.ly/hex/llms-full.txt.

## Check what is installed

```sh
hex --version
hex status --json
```

`hex status --json` names what Hex is doing and the OS's capabilities. If
`hex` is missing on a Mac, the app may still be installed:

```sh
ln -s /Applications/Hex.app/Contents/MacOS/hex ~/.local/bin/hex
```

Linux and Windows installs put it on the PATH. Do not install Hex unless the
person asks; read-only commands need no running app.

## Read meetings

Meetings is a beta on macOS, Windows, and Linux, turned on in Settings › Labs.
The read commands work on all three and can read a mirrored folder with
`--meetings-dir <path>`.

```sh
hex meetings list --since 7d --json          # ids, titles, times, durations
hex meetings show latest --md                # the whole transcript as Markdown
hex meetings show <id> --from 12:30 --to 18:00 --md
hex meetings chapters latest --json          # topics with time spans
hex meetings search 'release schedule'       # ranked passages across meetings
hex meetings search --meeting latest 'plan'
hex meetings show latest --srt > call.srt
```

- `latest` is the newest finished meeting. A meeting still recording reports
  only its transcript so far.
- Transcripts are speech recognition, so words can be misheard. Search is
  stemmed and fuzzy; quote a phrase to require it exactly.
- Speakers are `You` (the microphone) and `Them` (system audio). Remote
  speakers are not separated.
- Notes are a stored title and at most two sentences. Decisions and action items
  are in the transcript, not the notes.
- Every command takes `--json`, for a stable shape. Output is versioned.

## Read dictation history

```sh
hex history --since 3d --json
hex history --since 7d --search 'problems board' --json
hex history --app 'OpenCode' --grep 'migration' --json
hex history stats --since 7d --json
```

History keeps 7 days by default, up to 10,000 entries or 16 MiB. It stores text,
never audio, unless Keep recordings was turned on. Do not clear history without
an explicit request: `hex history clear --yes` is permanent.

## Use MCP instead of shelling out

If the client supports MCP, prefer the server: it exposes typed tools and never
starts Hex.

OpenCode, `opencode.json`:

```json
{ "mcp": { "servers": { "hex": { "type": "local", "command": ["hex", "mcp"] } } } }
```

Claude Code:

```sh
claude mcp add hex -- hex mcp
```

Claude Desktop and Cursor:

```json
{ "mcpServers": { "hex": { "command": "hex", "args": ["mcp"] } } }
```

Tools: `list_meetings`, `search_transcripts`, `get_transcript`,
`get_meeting_notes`, `get_chapters`. Resource:
`hex-meeting://<meeting_id>`. The server is read-only: it cannot record, edit,
delete, or generate anything. Setup for each client:
https://anoma.ly/hex/mcp.

## Drive the app

Only run these when asked; they change the person's machine.

```sh
hex start
hex dictation toggle
hex dictation stop --print                    # answers with the transcript
hex transcribe meeting.m4a                   # a file, no app needed
hex settings list                            # every setting key
hex settings get dictation_hotkey
hex settings set dictation_hotkey menu
hex labs list
hex labs on meetings
```

Settings keys are dotted paths (`dictation_hotkey`, `history_retention`,
`meetings.retention`). Read before writing, and never change a setting the
person did not name. Managed Macs may lock features; a locked setting refuses
the write.

## Transcribe a file

```sh
hex transcribe meeting.m4a
hex transcribe interview.wav > transcript.txt
```

It reads WAV, AIFF, CAF, FLAC, MP3, Ogg, WebM, and MP4/M4A/MOV, including a
video's soundtrack, and works whether or not the app is running. Exit 4 means
no speech was heard; nothing is printed.

## Rules

- Never upload or copy Hex audio, transcripts, or history to another service
  unless the person asks for that exact transfer.
- Never delete meetings, clear history, or change retention on your own.
- Say when a transcript is machine recognition, and quote the meeting id and
  time range with any excerpt.
- A read command never launches Hex. Don't start it just to read.