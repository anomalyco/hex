# Live transcription streams (`/streams`)

`/streams` lets another app on this computer transcribe its own audio live on Hex's
loaded dictation model. The app records the microphone (or anything else) and
pushes 16 kHz mono samples; Hex sends partial text while it listens, one final
segment per window, and the final text when the app finishes.

This is the API of the planned shared engine service (`ROADMAP.md`, "Shared
engine service"). Today the running Hex app serves it; later a launchd service
(`anomaly.hex.engine`) will serve the same routes and the Hex UI will become
one more client. Clients written against this document should not need to
change.

The code is `src/local_api/streams.rs` (routes) and `src/transcription/stream.rs`
(windows, priority, lease).

## Availability

- The running Hex app on macOS, Linux, and Windows, with a GGUF model loaded
  by dictation (on Linux and Windows, once the listener is on).
- `GET /capabilities` reports `"streams": true` when the routes exist.
- Streams never load, download, or switch a model. They use whatever model
  and language dictation holds; if there is none, creating a stream answers
  `503 model-not-loaded`.

## Discovery and auth

Read the owner-only discovery file `local-api.json` in Hex's data folder
(`~/Library/Application Support/hex` on macOS, `$XDG_DATA_HOME/hex` or
`~/.local/share/hex` on Linux, `%APPDATA%\hex` on Windows) (`port`, `token`,
`apiVersion`), then call `http://127.0.0.1:<port>` with:

| Header | On | Value |
| --- | --- | --- |
| `Authorization` | every request | `Bearer <token>` from discovery |
| `X-Hex-Stream-Token` | every `/streams/{id}/…` request | the stream's `ownerToken` |
| `Content-Type` | create / audio | `application/json` / `application/octet-stream` |
| `Content-Length` | every request with a body | required; chunked bodies are refused |

A wrong or missing stream token answers `404 stream-not-found`, the same as an
unknown id, so ids can't be probed.

## Audio format

16 000 Hz, mono, little-endian, declared when the stream is created:

- `f32le`: 32-bit floats in [-1, 1] (values outside are clamped; NaN or
  infinity is refused).
- `s16le`: signed 16-bit integers.

Each audio request carries whole samples only. Push small chunks as you record
them; 100 ms to 1 s per request is typical.

## Endpoints

### `POST /streams`

```json
{ "sampleFormat": "f32le", "sampleRate": 16000, "model": "parakeet_v3", "language": "en" }
```

`model` and `language` are optional; when given they must match what
dictation holds, or the answer is `409 model-mismatch` with the held ones:

```json
{ "code": "model-mismatch", "model": "parakeet_v3", "language": "auto" }
```

`201`:

```json
{
  "id": 3,
  "ownerToken": "hex_9f…",
  "model": "parakeet_v3",
  "language": "auto",
  "sampleRate": 16000,
  "sampleFormat": "f32le",
  "partials": true,
  "leaseMs": 10000
}
```

`partials` is `true` only when the loaded model can stream on its device; a
model that can't sends segments and the final only.

### `POST /streams/{id}/audio`

Body: raw samples in the declared format, at most 256 KiB. `204` when queued.
Renews the lease.

### `GET /streams/{id}/events`

Server-sent events, one JSON object per `data:` line, with `: keep-alive`
comments every 15 s. Each subscriber gets every segment from the start, then
live events, then the end, and the response closes. A newer subscriber
replaces an older one. Events stay readable after the stream ends, until the
next stream is created.

```text
data: {"type":"ready","id":3,"model":"parakeet_v3","language":"auto","sampleFormat":"f32le","partials":true}

data: {"type":"partial","segment":0,"stable":"so the plan for","tentative":" today"}

data: {"type":"segment","segment":0,"startMs":0,"endMs":7420,"text":"So the plan for today is the release."}

data: {"type":"final","text":"So the plan for today is the release. …","segments":[…],"durationMs":31250}
```

| `type` | Meaning |
| --- | --- |
| `ready` | Sent first: the stream's model and format. |
| `partial` | The window being transcribed. `stable` is committed text the model won't revise; `tentative` may still change. Replaces the previous partial for the same `segment`. Only when `partials` is true. |
| `segment` | A finished window's final text (empty windows send none). |
| `final` | Every segment joined; the stream has ended. |
| `error` | `{"code","message"}`; the stream has ended (see codes below). |
| `cancelled` | The stream was cancelled. |

Word timings are not sent.

### `POST /streams/{id}/finish`

Transcribes what's left and answers with the final, as the `final` event:

```json
{ "text": "…", "segments": [{ "segment": 0, "startMs": 0, "endMs": 7420, "text": "…" }], "durationMs": 31250 }
```

No more audio is accepted once finishing. It waits up to 120 s (for example
behind a long dictation) and then answers `504 stream-finish-timeout`; the
stream keeps going, so call `finish` again or read the events. A stream that
already failed answers `409` with its `{"code","message"}`; a cancelled one
`409 stream-cancelled`.

### `POST /streams/{id}/heartbeat`

`204`; renews the lease. `409 stream-ended` once the stream has ended.

### `POST /streams/{id}/cancel`

`204`. Discards everything; the stream ends with `cancelled`.

## Lease

A stream ends with `error` `lease-expired` when it receives neither audio nor
a heartbeat for 10 s (`leaseMs`). A paused recorder should send a heartbeat
every few seconds. The lease doesn't run while finishing.

## Limits

| Limit | Value |
| --- | --- |
| Live streams | 1 (a second create answers `409 stream-busy`; an ended stream doesn't count) |
| Create body | 1 KiB |
| Audio request | 256 KiB, read within 5 s |
| Backlog (received, not yet transcribed) | 120 s of audio, then `429 stream-backlog-full` |
| Window | cut at pauses, at most 25 s |
| Lease | 10 s |
| Finish wait | 120 s |

## Scheduling: dictation always comes first

Hex's dictation shares the engine, and the hotkey must never wait for a
stream. So:

- Audio is cut into windows at pauses (at most 25 s). Each window is fed to
  the engine as a *yielding* live stream, at most 0.4 s ahead of what the
  engine has decoded, so a dictation request queues behind very little.
- Any dictation request (the shortcut's capture stream, a whole-clip
  transcription) closes the yielding stream first, and a yielding stream is
  refused while a dictation stream is open.
- From the shortcut press until dictation's text is done, a stream feeds the
  engine nothing; its audio waits in the backlog and catches up afterwards.
- A window whose live stream dictation closed restarts from the window's start
  once dictation is idle. A window that completes while that can't happen, or
  any window on a model that can't stream, is transcribed offline as one
  foreground request on the loaded model: after any dictation already sent,
  before meeting chunks (the same class as capture-word requests).

Dictation's capture never waits on the engine: its audio is recorded
independently and losslessly. Only its transcription can wait, and only for
work already running, because a decode can't be aborted on Metal: the feed in
flight (at most 0.4 s of audio, a few tens of milliseconds, inside the 300 ms
hold threshold) or, rarely, one offline window (up to 25 s of audio) that
started while dictation was idle. Streams go ahead of meeting chunks because
a stream's app is waiting for text now, while meetings catch up.

## Error codes

Errors are JSON `{"code": "…"}` (some add `message`), except `401`, which
has no body.

| Status | Code | When |
| --- | --- | --- |
| 400 | `invalid-request` | Create body isn't the JSON above. |
| 400 | `unsupported-sample-rate` | `sampleRate` isn't 16000. |
| 400 | `unsupported-sample-format` | Not `f32le` or `s16le`. |
| 400 | `unknown-model` | `model` isn't a Hex model id. |
| 400 | `invalid-audio` | Partial sample or non-finite float. |
| 401 | (no body) | Missing or wrong bearer token. |
| 404 | `stream-not-found` | Unknown id, replaced stream, or wrong stream token. |
| 409 | `model-mismatch` | Requested model/language isn't the held one. |
| 409 | `stream-busy` | Another stream is live. |
| 409 | `stream-ended` | Audio or heartbeat after the end. |
| 409 | `stream-finishing` | Audio after `finish`. |
| 409 | `stream-cancelled` | `finish` on a cancelled stream. |
| 409 | `lease-expired`, `model-changed`, `transcription-failed`, `hex-quitting` | `finish` on a stream that failed that way (same codes as the `error` event). |
| 411 | `length-required` | No `Content-Length`. |
| 413 | `request-too-large`, `audio-too-large` | Over the limits. |
| 415 | `unsupported-content-type` | Wrong `Content-Type`. |
| 429 | `stream-backlog-full` | 120 s not yet transcribed; slow down or wait. |
| 503 | `engine-unavailable` | Hex's dictation engine isn't running. |
| 503 | `model-not-loaded` | Dictation holds no GGUF model (for example Apple Speech). |
| 503 | `shutting-down` | Hex is quitting. |
| 504 | `stream-finish-timeout` | `finish` waited 120 s; the stream continues. |

`model-changed` means the user switched dictation's model during the stream;
start a new one. `hex-quitting` means Hex quit.
