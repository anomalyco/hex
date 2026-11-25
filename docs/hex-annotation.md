# Hex Annotation v1

A capture is a directory package, not a ZIP and not an entry in a database. The writer allocates a fresh `<unix-ms>-<128 random bits>.hexannotation` under `<Hex application support>/annotations` (`%LOCALAPPDATA%\hex\annotations` on Windows so screen recordings stay machine-local out of Roaming Profiles), or the explicit `capture.folder`. It never renames an existing capture. On Unix the package is `0700` and its completed files are `0600`; Windows uses inherited directory ACLs. Choose a private location, particularly on Windows: a shared volume may have different ACLs.

```text
chart-note.hexannotation/
  annotation.json        published last, by a same-directory atomic rename
  summary.md             human-readable handoff, not the source of truth
  frames/                preview keyframes; frames-full/ or frames-clean/ when available
  contact-sheet.png
  recording.mp4          optional; retention can prune this and audio.wav
  audio.wav
  capture.json           recorder metadata, not the annotation manifest
```

`annotation.json` is `{"formatVersion":1,"annotation":{…}}`. The annotation payload is the full capture model, not a list projection: its `id`, `createdAtMs`, target and duration; transcript segments and word times; normalized vector `strokes`, mark geometry and keyframe associations; events and cursor; contact sheet and relative media paths (`video`, `audio`, `autocrop`, optional `cleanFrames` for un-inked keyframe plates); audio stop, video offset, and optional destination. Unknown additive fields on both the outer `Package` envelope and `annotation` round-trip intact through `publish` updates (`record_destination`, `transcribe_again`), leaving room for v1.x non-destructive ink edit layers without a `formatVersion: 2` break. [A synthetic v1 payload](../tests/fixtures/annotations/marked-v1.json) illustrates these fields. Times are milliseconds on the capture clock; subtract `videoOffsetMs` to seek in the movie. Readers must not invent a new completion time from file modification time, import time or a send. The folder name is a locator; moving a package keeps its manifest identity.

## Opening old files doesn't claim them

`hex annotate show <path>` and `hex annotate inspect <path>` read this package or an existing `.scrawl/scrawl.json` in place. Captures, Unified History, Paste Last and `hex annotate list` search the write root and, with no custom root, the old default folder if present. Redirecting Hex's data root or running a test build disables that implicit legacy search. Both formats can coexist in a custom folder. A duplicate ID is listed once (prefer Hex format, then a deterministic path); a full path still selects the particular copy. Missing, oversized (>64 MiB), invalid and future manifests are not completed annotations. Missing movies leave the words, marks and remaining stills; showing or editing a picture does not reconstruct a movie. Discovery runs no helper, renderer, delivery or migration. There is no bulk import. The standalone Scrawl app does not read `.hexannotation`.

Hex's router writes its own decisions, log, undo state, retry receipts and prompt outbox under `<support>/annotation-router`. It does not drain the old router or retry old `.scrawl/.deliver.json` files. The capture retention worker and delayed speech re-transcription consider only published Hex v1 packages, use separate staging, and ignore partial, future and legacy packages. Legacy files are read-only to the CLI send command: recording a new destination would rewrite the original. A user's explicit Trash action in Captures is separate from discovery.

An in-package delivery marker is not a queue. On a failed send Hex writes a deterministic private receipt outside the package under `<support>/annotation-router/retries`, naming the original path, ID, attempt time and requested session; explicit redirects replace it in place and undos remove it immediately. The retry worker reads those receipts, never discovering work by scanning a shared captures folder. A package copied to another path does not inherit a retry. Missing or inconsistent state cannot authorize delivery.

## History, Paste Last, Helper, and Packaging Boundaries

- **History & Paste Last:** `capture_host/paste_source.rs` issues `.hex-annotate-v1` receipts bound with SHA-256 to the canonical package path, directory `(dev, ino)`, random generation, `ready_at`, and manifest `(id, createdAtMs)`. Legitimate in-place updates (`record_destination`, `transcribe_again`) stay valid, while copying or moving a package invalidates the receipt. Pre-cutover two-field receipts on `.scrawl` packages remain accepted in their original location when `<stem> == id` and any `summary.md` locator matches. Unified History merges display rows across `annotation_roots()` by `manifest_id` without promoting unmarked or copied packages into pasteable receipts.
- **Helper:** Hex builds and signs the in-repo Swift `scrawl-helper` alongside its executable. Playback probing/filmstrips, movie export and iPhone mirroring use it; these are not helper-free. Linux has FFmpeg/portal dependencies; Windows has its native media adapter.
- **Shared state:** the read-only one-time settings import, original custom folder, old decisions/outbox, pending deliveries and unfinished media stay untouched. A reply from the old ingest port and a running `dev.kitlangton.scrawl` still gate Hex retries; the app-running check also gates the shortcut and retention. Hex does not launch the legacy recovery worker.
- **Packaging:** macOS declares `anomaly.hex.annotation` as `com.apple.package`, not as a handler for `.scrawl`. No Finder double-click / Windows Explorer / Linux file-manager open association is claimed until a document-open handler exists.
- **Handoff:** the same manifest reader feeds clipboard, Paste Last, editor sound/trim, and inline-image send. A slim ZIP is a separate light attachment, not a migration; it excludes video, audio, full/clean frames, retries and the paste receipt.

`bash scripts/test-annotation-files.sh` runs named fixture tests with a temporary `HOME`, isolated application data and nonexistent helper paths. On macOS add `--editor-media` to run the production filmstrip decoder with Hex's own built helper: it reads the checked-in RGBW movie from both package types, checks spoken-word offsets, exports the untrimmed clip, then removes only the fixture video and checks that the manifest's marks survive.
