# Local consent: one upload or one recording

Two narrow asks share one handshake: **one transcription upload** from a web
page, and **one microphone recording** from a web page, an Electron main
process, or any native/Rune process. Neither caller ever receives Hex's local
API bearer token. Hex asks the person in its own window, and a code plus PKCE
produces a one-use token scoped to the caller's verified identity:

- a web page is bound to the `Origin` its browser sent, on the loopback HTTP
  API (a Labs › Browser access origin);
- a native process is bound to the audit token of the Unix-socket peer, and
  its identity is the code-signing identifier and team `Security.framework`
  reports for that token. A name it claims is never the identity.

This is a prototype contract. It exists to prove the consent boundary, not to
be a general local API. `api-browser-access` (the Lab) stays off by default.

The recording section is [below](#one-microphone-recording); the existing
upload flow keeps its own paths and scope.

```ts
A page's fetch -> POST /connect/authorize            // no bearer token
├── Origin not listed, or missing -> 403 origin-not-allowed (no dialog)
├── No JSON body, or not S256 -> 400 invalid-request
├── A dialog is already open -> 409 consent-busy
├── Listed origin -> Hex's window: "Allow one transcription?" [Deny] [Allow once]
│   ├── Deny, Escape, scrim, or 60 s with no answer -> 403, no code
│   │   └── Deny also starts a 30 s cooldown for that origin (429 consent-cooldown)
│   └── Allow once -> 200 { code, expiresInMs, origin, scope, state }
├── POST /connect/token { code, codeVerifier }
│   ├── Wrong origin, wrong verifier, spent, or expired -> 403 invalid-grant
│   └── Correct -> 200 { uploadToken, expiresInMs, origin, scope }
└── POST /transcriptions with X-Hex-Connect-Token
    ├── Any other route or method -> 401 (the token reaches nothing else)
    ├── Another origin, spent, or expired -> 401
    ├── process= or timestamps= -> 403 connect-scope (the token is spent)
    └── Plain WAV upload -> the existing transcript response (one use)
```

## Requirements

- The running Hex app on macOS (`hex app`). `hex service` has no window, so
  the routes answer `403 connect-unavailable`. Linux and Windows serve the
  routes but wire no dialog; the same `403` is what a page sees.
- Labs › Browser access to the local API on. Its switch gates both asks; only
  a web caller also needs its origin in the list. That list already gates CORS
  (`src/local_api/cors.rs`); connect and recording add no second list, and a
  native caller never needs an origin.
- A preflight from the origin is answered as before (`OPTIONS` 204). The
  connect POSTs are JSON, so a browser sends a preflight: a page cannot
  trigger a dialog with a cross-site form or `no-cors` fetch.

## `POST /connect/authorize`

Request (JSON, at most 8 KiB):

```json
{
  "codeChallenge": "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
  "codeChallengeMethod": "S256",
  "clientName": "Field Notes",
  "state": "opaque-app-value"
}
```

`codeChallenge` is the base64url (no padding) SHA-256 of the page's
`codeVerifier`, per RFC 7636. `codeChallengeMethod` may be omitted, but only
`S256` is accepted. `clientName` is optional; Hex shows it as "The page calls itself: …"
only when it is at most 40 plain characters (no controls, no bidi overrides). `state` is echoed back unchanged (at most the body limit).

The response is sent only after the person answers:

```json
{
  "code": "hexc_…",
  "expiresInMs": 120000,
  "origin": "https://app.example",
  "scope": "transcription:upload",
  "state": "opaque-app-value"
}
```

The origin named in the dialog is the request's own `Origin` header, checked
to be a canonical, listed entry. The grant is bound to that exact origin.

`GET /capabilities` adds `"connect": true` when a window can ask.

## `POST /connect/token`

```json
{ "code": "hexc_…", "codeVerifier": "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk" }
```

The verifier is 43–128 unreserved characters. A code is spent by its first
exchange, right or wrong, so a page gets one guess against one consent.

```json
{
  "uploadToken": "hexu_…",
  "expiresInMs": 120000,
  "origin": "https://app.example",
  "scope": "transcription:upload"
}
```

## `POST /transcriptions`

The existing route, with the upload token instead of `Authorization`:

```http
POST /transcriptions HTTP/1.1
Origin: https://app.example
X-Hex-Connect-Token: hexu_…
Content-Type: audio/wav
Content-Length: …
```

The token is spent by the attempt, whatever the upload turns out to be. It
is accepted only here, only from the origin it was minted for, and only for a
plain upload: `?process=global`, `?timestamps=…`, and every other query
option answer `403 connect-scope`, and the token is gone, so a page must ask
again. `?model=` and `?language=` work as they do for a bearer-token upload.

The response is the route's normal JSON (`transcript`, `durationMs`).

## What the grant cannot do

- No microphone, recording, dictation, `/dictations`, `/streams`, meeting,
  History, output processing, clipboard, paste, Accessibility, or desktop
  control. The upload token is checked before routing and never becomes the
  API's bearer token.
- The page never sees `local-api.json`, its `token`, or another origin's
  grant. Codes and tokens are 32 random bytes from `getrandom`, stored only in
  this process.
- There is no auto-allow, no remembered origin, and no background prompting.
  One dialog at a time; a Deny starts a cooldown; an unanswered dialog ends
  after 60 s.
- The browser offers no server-side proof that its request followed a click
  (`navigator.userActivation` never leaves the page), so the native dialog is
  the gate it can enforce: nothing is allowed without a press, Escape and the
  scrim deny, and a denied page waits 30 s before it may ask again. A page
  cannot reach the POST without a CORS preflight for `application/json`.

## Limits

| Limit | Value |
| --- | --- |
| Authorize/token body | 8 KiB, read within 5 s |
| Code lifetime | 120 s, one use |
| Upload token lifetime | 120 s, one use |
| Dialog wait | 60 s, then `403 consent-timeout` |
| Deny cooldown | 30 s per origin (`429 consent-cooldown`) |
| Concurrent dialogs | 1 (`409 consent-busy`) |
| Upload size | The route's existing 64 MiB |

An HTTP worker blocks while the dialog is open (up to a minute); the local API
has four, so a page cannot starve the app by asking repeatedly.

## What is not proven

The dialog is rendered by Hex's own offscreen snapshot tooling
(`hex app -- --snap … --scene browser-connect`); a real browser page, a real
click, and a real model transcription from a page were not exercised. Copy is
English only. The origin check trusts the browser's `Origin` header, which is
the same trust the CORS gate already places in it; a malicious local process
that can reach loopback without a browser can open the dialog only by claiming
a listed origin, and the person still has to allow it.

## One microphone recording

A client asks Hex to record **once** with its normal dictation microphone, then
receives only the transcript. Raw audio, levels, partials, History, paste,
microphone selection, and desktop control are not exposed.

```ts
Native client (record.sock)        Web page (loopback + Origin)
        │                                   │
        └──────── POST /record/authorize ───┘     // PKCE S256, no bearer
             ├── native peer identity unreadable -> 403 identity-unverified
             ├── native/Web not the one already prompting -> 409 consent-busy
             ├── Hex window: "Allow one microphone recording?"
             │     recipient, team note, and the lease
             │     [Deny] [Allow once]
             │   ├── Deny / Escape / scrim / 60 s -> 403, no code; Deny cools down 30 s
             │   └── Allow once -> 200 { code, expiresInMs, scope: "record:one" }
             ├── POST /record/token { code, codeVerifier }
             │   └── correct, same verified channel -> 200 { id, sessionToken,
             │       expiresInMs (start window), maxRecordingMs }
             ├── POST /record/start    X-Hex-Record-Token -> 201 recording
             ├── GET  /record/status   -> waiting | recording | stopped | finished | cancelled | expired
             ├── POST /record/finish   -> 200 { transcript, durationMs }   (once)
             └── POST /record/cancel   -> 204, discards
```

While the session is live, Hex's window shows a non-modal card naming the
recipient, the time left on the lease, **Stop recording** (keeps the
transcript for the client's `finish`) and **Cancel** (discards it). The client
can stop or cancel through its token too; either side ends the one recording.

### Native identity

`record.sock` in Hex's data folder is mode `0600` and serves `/record/*` only.
On each connection Hex reads `LOCAL_PEERTOKEN` and asks
`SecCodeCopyGuestWithAttributes`/`SecCodeCopySigningInformation` for the
signing identifier and team. The consent dialog shows that; if the identity
cannot be read, the request is refused (`403 identity-unverified`) rather than
falling back to an app name or path. Grants and session tokens are bound to the
peer's audit token, so another process cannot spend them.

Native clients need no `Origin`, no bearer token, and no discovery file. The
loopback API is not their channel, and the Unix socket has no route for it.

### Lease

| Limit | Value |
| --- | --- |
| Code lifetime | 120 s, one use |
| Start window after the token | 60 s, then `409 session-expired` |
| One recording | 120 s maximum; it stops itself |
| After the stop | 30 s for `finish`, then discarded (`409 session-expired`) |
| Concurrent recordings | 1 (`409 session-busy` on the next token) |
| Body | 8 KiB; session actions take no body |

`maxRecordingMs` and `remainingMs` are reported; the window's card counts down
the same lease. A recording stopped by the lease is still finishable inside
the grace window.

### Errors

| Status | Code | When |
| --- | --- | --- |
| 400 | `invalid-request` | Not the JSON above, not S256, a body on a session action |
| 403 | `identity-unverified` | The native peer's signature could not be read |
| 403 | `origin-not-allowed` | A browser request's Origin is missing or unlisted |
| 403 | `consent-denied`, `consent-timeout`, `record-unavailable` | Deny, no answer, no window/gate |
| 403 | `invalid-grant` | Wrong channel, verifier, spent, or expired code |
| 404 | `record-not-found` | Unknown token or one bound to another channel |
| 409 | `session-busy`, `session-expired`, `session-finished`, `session-cancelled`, `not-started`, `already-started`, `finish-in-progress` | Session state |
| 429 | `consent-cooldown` | The recipient was denied less than 30 s ago |
| 503 | `capture-unavailable` | No capture loop could start the recording |

### What is not exposed

- No raw audio, no `/record/audio`; a `X-Hex-Record-Token` reaches only
  `start`, `status`, `finish`, and `cancel`.
- No `/dictations`, `/streams`, `/transcriptions`, `/control`, `/models`,
  History, clipboard, Accessibility, or settings. The record token is checked
  before the bearer token and never becomes it.
- Session tokens live only in Hex's process; they are 32 random bytes, one use
  for `finish`, and bound to the Origin or audit token that consented.

### Clients

[`prototypes/record-clients/electron-main.mjs`](../prototypes/record-clients/electron-main.mjs)
is an Electron main-process example,
[`native-rust.rs`](../prototypes/record-clients/native-rust.rs) a Rust example
that also runs inside a Rune host, and
[`browser-connect/index.html`](../prototypes/browser-connect/index.html) drives
the browser upload flow (the recording routes are the same exchange with
`/record/*`).

### What is tested, and what is not

`local_api` tests drive the whole exchange over loopback HTTP with a recording
backend that never opens a microphone: allow and deny, one-use codes and
sessions, overflow and expiry, lease stop and grace, the window's Stop and
Cancel, and the token's inability to reach any other route or origin. On
macOS, one test connects to the real `record.sock`, reads the peer's audit
token, and records through the fake backend; it skips (with a printed note)
when the test binary has no signing identity. The dialog and the live card are
rendered by `hex app -- --snap` scenes (`record-consent`, `record-active`);
nothing was recorded from a real microphone, no OS grant was approved, and no
real browser page or Electron process was run.
