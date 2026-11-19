# Local consent: one upload, or a recording grant

Two narrow asks share one handshake: **one transcription upload** from a web
page, and a **revocable grant to start recordings** for a web page, an
Electron main process, or any native/Rune process. Neither caller ever
receives Hex's local API bearer token. Hex asks the person in its own window,
and a code plus PKCE produces a token scoped to the caller's verified identity:

- a web page is bound to the `Origin` its browser sent, on the loopback HTTP
  API (a Labs › Browser access origin);
- a native process is bound to the audit token of the Unix-socket peer, and
  its identity is the code-signing identifier and team `Security.framework`
  reports for that token. A name it claims is never the identity.

This is a prototype contract. It exists to prove the consent boundary, not to
be a general local API. `api-browser-access` (the Lab) stays off by default.

The recording section is [below](#recording-by-a-paired-client); the existing
upload flow keeps its own paths and scope.

```ts
A page's fetch -> POST /connect/authorize            // no bearer token
├── Origin not listed, or missing -> 403 origin-not-allowed (no dialog)
├── No JSON body, or not S256 -> 400 invalid-request
├── A dialog is already open -> 409 consent-busy
├── Listed origin -> Hex's permission window: "Allow this page to transcribe audio?" [Deny] [Allow Once] [Always Allow]
│   ├── Deny, Return, Escape, or 60 s with no answer -> 403, no code
│   │   └── Deny also starts a 30 s cooldown for that origin (429 consent-cooldown)
│   └── Allow Once or Always Allow -> 200 { code, expiresInMs, origin, scope, state }
├── POST /connect/token { code, codeVerifier }
│   ├── Wrong origin, wrong verifier, spent, or expired -> 403 invalid-grant
│   └── Correct -> 200 { audioToken, expiresInMs, origin, scope }
└── POST /transcriptions with X-Hex-Connect-Token
    ├── Any other route or method -> 401 (the token reaches nothing else)
    ├── Another origin, spent, or expired -> 401
    ├── process= or timestamps= -> 403 connect-scope (the token is spent)
    └── Plain WAV upload -> the existing transcript response (one use)
```

## Requirements

- The running Hex app (`hex app`) on macOS, Linux, or Windows. `hex service`
  has no window, so the routes answer `403 connect-unavailable`. Native
  clients pair only on macOS: Linux and Windows have no socket that can tell
  Hex a peer's verified identity, so there a web page is the only caller.
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

The response requires a fresh Allow Once / Always Allow answer or an existing Always Allow permission for transcription:

```json
{
  "code": "hexc_…",
  "expiresInMs": 120000,
  "origin": "https://app.example",
  "scope": "transcription:transcribe",
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
  "audioToken": "hexa_…",
  "expiresInMs": 120000,
  "origin": "https://app.example",
  "scope": "transcription:transcribe"
}
```

## `POST /transcriptions`

The existing route, with the upload token instead of `Authorization`:

```http
POST /transcriptions HTTP/1.1
Origin: https://app.example
X-Hex-Connect-Token: hexa_…
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
- Only Always Allow remembers the origin and capability. Each token still authorizes just one file.
  One dialog at a time; a Deny starts a cooldown; an unanswered dialog ends
  after 60 s.
- The browser offers no server-side proof that its request followed a click
  (`navigator.userActivation` never leaves the page), so the native dialog is
  the gate it can enforce: nothing is allowed without a press (and none in the
  window's first second), Return and Escape deny, and a denied page waits 30 s before it may ask again. A page
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

### The permission window

Both asks show in Hex's permission window: a floating window of its own, not
a dialog inside Hex's main window, on every OS. It appears whether the main
window is open, closed, or Hex is only in the menu bar or tray, and never
opens or raises the main window. It sits centred on the screen under the
pointer, above other apps' windows, and holds no GPU surface while hidden.
On macOS it takes the keyboard while shown without activating Hex (so the
app in front keeps focus after). On Linux and Windows it is the same Rune
overlay spanning that screen, with only the dialog taking the pointer
(layer-shell on Wayland, override-redirect on X11, a topmost no-activate
window on Windows).

```ts
ask starts          -> the window shows, Deny focused; Allow Once and Always Allow disabled for 1 s
Return / Escape     -> Deny (also during that second)
Deny                -> always answers
allow during the 1 s -> nothing
an answer, a 60 s timeout, or the ask otherwise ending -> the window hides at once
```

There is no scrim and nothing outside the dialog to click. One ask at a time,
as before.

Where the window can't take keys or can't open, the platform decides:

```ts
macOS, Wayland with layer-shell -> as above
X11, Windows                    -> buttons work; Return and Escape never reach it
                                   (Rune's overlays there take no keyboard), so
                                   they go to the app in front and grant nothing
Wayland without layer-shell     -> no overlay that takes keys (Rune refuses XWayland
(GNOME)                            for one): the ask answers 403 *-unavailable
                                   within a second instead of waiting unseen
the window's app failed (Linux) -> the same, at once
```

## What is not proven

The permission window's view is rendered offscreen by an ignored test
(`HEX_CONSENT_SNAPS=<folder> cargo test --bin hex -- --ignored
rune_ui::consent_window::tests::snapshots`), and its state, guard, keys, and
answers are tested headless. On Linux the running app was driven under Xvfb
(X11, lavapipe) with `curl` asks from a listed origin and `xdotool` clicks:
both asks showed centred with the main window closed, Allow Once during the
guard did nothing, Escape did not reach the window, Deny answered
`consent-denied` and hid it, and Allow Once after the guard issued a code.
On macOS the native window (its placement, level, key focus, and focus
returning to the app in front) was not shown on a real screen in a test;
Windows was only compiled, and Wayland, GNOME's fallback, a real browser
page, and a real model transcription from a page were not exercised. Copy is
English only. The origin check trusts the browser's `Origin` header, which is
the same trust the CORS gate already places in it; a malicious local process
can claim a listed origin to *ask*, but not silently reuse its saved permission.
On the first Always Allow response, a web client receives `permissionProof`.
Store it in origin-scoped storage and send it in later authorize/pair JSON for
that capability. Hex saves only its SHA-256 digest. A missing or incorrect proof
returns `403 permission-proof-required`; revoke in Labs and pair again if the
browser lost its copy. Native callers use socket peer identity instead.

## Recording by a paired client

A client **pairs with Hex once**. The person sees the caller's verified
identity and allows it to *start recordings*; Hex keeps a revocable grant.
Pairing itself starts nothing: every `POST /record/start` opens a separate
session that records for as long as the client keeps checking in
(`POST /record/heartbeat`), stops itself when the check-ins stop, and ends
with one transcript (`finish`) or nothing (`cancel`). Allow Once gives one session. Always Allow permits repeated sessions until revoked.

Raw audio, levels, partials, History, paste, microphone selection, and desktop
control are not exposed.

```ts
Native client (record.sock)        Web page (loopback + Origin)
        │                                   │
        └──────── POST /record/pair ────────┘     // PKCE S256, no bearer
             ├── native peer identity unreadable -> 403 identity-unverified
             ├── already paired -> 409 already-paired (no dialog)
             ├── another dialog is open -> 409 consent-busy
             ├── Hex's permission window: "Allow microphone recording?"
             │     recipient, team note, and that pairing starts nothing
             │     [Deny] [Allow Once] [Always Allow]
             │   ├── Deny / Return / Escape / 60 s -> 403, no code
             │   │     Deny also cools that recipient down for 30 s
             │   └── Allow -> 200 { code, expiresInMs, scope: "record:pair", state }
             ├── POST /record/token { code, codeVerifier }
             │   └── correct, same verified channel -> 200 { grantId, grantToken,
             │       scope: "record:any", duration: "once" | "always" }            // no recording yet
             ├── POST /record/start    X-Hex-Record-Grant -> 201 { id, sessionToken,
             │       state: "recording", leaseMs }
             ├── POST /record/heartbeat X-Hex-Record-Token          // every few seconds
             │   ├── recording, inside the lease -> 204, another leaseMs
             │   ├── lease already lapsed -> 409 stopped (never restarts it)
             │   └── stopped / finished / cancelled / expired -> 409 with that state
             ├── GET  /record/status   X-Hex-Record-Token
             │       -> { state, elapsedMs, remainingMs, leaseMs }
             │       state: recording | stopped | finished | cancelled | expired
             ├── POST /record/finish   -> 200 { transcript, durationMs }   (once)
             ├── POST /record/cancel   -> 204, discards
             └── POST /record/revoke   X-Hex-Record-Grant -> 204, ends the grant
```

With Always Allow, repeat `start` … `finish` for each distinct recording; each start mints a new
session token. While a session is live, Hex's window shows a non-modal card
naming the recipient, how long it has recorded, **Stop recording** (keeps
the transcript for the client's `finish`) and **Cancel** (discards it). The
client can stop or cancel through its session token too.

A grant is withdrawn in Hex's window (Labs › Browser access lists each paired
client with **Revoke**) or by the client's own `POST /record/revoke`. Revoking
cancels any live session, drops pairing codes issued before the revoke, and
makes further starts fail. The client may pair again, which asks the person
again. Pairing while a grant already exists is `409 already-paired`; it does
not open a second dialog.

### Native identity

`record.sock` in Hex's data folder is mode `0600` and serves `/record/*` only.
On each connection Hex reads `LOCAL_PEERTOKEN` and asks
`SecCodeCopyGuestWithAttributes`/`SecCodeCopySigningInformation` for the
signing identifier and team. The consent dialog shows that; if the identity
cannot be read, the request is refused (`403 identity-unverified`) rather than
falling back to an app name or path. A grant and its sessions are bound to the
peer's audit token, so another process cannot spend them even if it learns the
token.

The native identity is the signed executable, not a document, script, or renderer. Allowing a shared interpreter such as Node therefore allows that interpreter, not just one script. Use Allow Once for shared hosts.

Native clients need no `Origin`, no bearer token, and no discovery file. The
loopback API is not their channel, and the Unix socket has no route for it.

### Lease

| Limit | Value |
| --- | --- |
| Pairing code lifetime | 120 s, one use |
| Grant token | Process-local; one session for Allow Once, reusable for Always Allow |
| One recording | No length limit while the client checks in |
| Check-in lease | 10 s (`leaseMs`); the microphone stops when it lapses |
| After the stop | 30 s for `finish`, then discarded (`409 session-expired`) |
| Concurrent recordings | 1 (`409 session-busy` on the next start) |
| Body | 8 KiB; start/status/heartbeat/finish/cancel/revoke take no body |

The lease is the one Hex's `/dictations` and `/streams` use. Only
`POST /record/heartbeat` with the session's own token, on the channel that
started it (the same `Origin` or audit token), renews it; `status` does not.
Send one every few seconds (`leaseMs / 3` in the examples). When a client
quits, crashes, or hangs, its check-ins stop and Hex stops the microphone
within one lease; the transcript is then finishable for the 30 s grace like
any other stop. A check-in after the lease lapsed is refused and starts
nothing again. While the session records, Hex keeps its own capture lease
alive, so the client's check-ins alone decide how long it runs; Stop, Cancel,
and Revoke in Hex end it at any time. `status` reports `elapsedMs`, and
`remainingMs` until the lease lapses (recording) or the grace ends (stopped).

A browser may slow a hidden tab's timers (Chrome checks in at most once a
minute after five hidden minutes), which lets the lease lapse; a page that
must keep recording in the background should do so from a visible tab or a
native client.

### Revocation and persistence

Codes, grant tokens and sessions are never persisted. Only **Always Allow** saves a
recipient and capability in `local-consent.json` (mode 0600). After restarting
Hex the client pairs afresh, without a new dialog for that permission. Native
matches require the validated signing identifier, team and code-directory hash;
a changed binary must ask again. Listed web origins match independently of the
page's claimed name. Transcription and microphone recording are separate choices.

Labs › Browser access lists saved permissions even when the client is offline.
Revoke deletes just that capability from disk. It invalidates its outstanding
codes and tokens; for recording it also cancels an active session. A failed
save never grants access, and a failed revoke does not disappear from the list.

### Errors

| Status | Code | When |
| --- | --- | --- |
| 400 | `invalid-request` | Not the JSON above, not S256, a body on a token action |
| 401 | `invalid-grant` | Unknown, revoked, or other-channel grant; a session token used as a grant |
| 403 | `identity-unverified` | The native peer's signature could not be read |
| 403 | `origin-not-allowed` | A browser request's Origin is missing or unlisted |
| 403 | `consent-denied`, `consent-timeout`, `record-unavailable` | Deny, no answer, no window/gate |
| 403 | `invalid-grant` | Wrong, spent, or expired pairing code |
| 404 | `record-not-found` | Unknown session token or one bound to another channel |
| 409 | `stopped` | A check-in after the recording stopped (lease, Stop in Hex, or the client's `finish`) |
| 409 | `already-paired` | That recipient already has a grant |
| 409 | `session-busy`, `session-expired`, `session-finished`, `session-cancelled`, `not-started`, `finish-in-progress` | Session state |
| 429 | `consent-cooldown` | The recipient was denied less than 30 s ago |
| 503 | `capture-unavailable` | No capture loop could start the recording |

### What is not exposed

- No raw audio, no `/record/audio`; a grant token reaches only `start` and
  `revoke`, and a session token only `status`, `heartbeat`, `finish`, and
  `cancel` for its own session.
- No `/dictations`, `/streams`, `/transcriptions`, `/control`, `/models`,
  History, clipboard, Accessibility, or settings. Both record headers are
  checked before the bearer token and never become it.
- Tokens live only in Hex's process; they are 32 random bytes, the `finish`
  transcript is handed over once, and each is bound to the Origin or audit
  token that paired.

### Clients

[`prototypes/record-clients/electron-main.mjs`](../prototypes/record-clients/electron-main.mjs)
is an Electron main-process example,
[`native-rust.rs`](../prototypes/record-clients/native-rust.rs) a Rust example
that also runs inside a Rune host, and
[`browser-connect/index.html`](../prototypes/browser-connect/index.html) drives
the browser transcription flow and a recording that checks in until the page
stops it.

### What is tested, and what is not

`local_api` tests drive the whole exchange over loopback HTTP with a recording
backend that never opens a microphone: allow and deny, one prompt for repeated
recordings, distinct sessions, code use and expiry, the window's Stop and
Cancel, revoke from the client (and that a revoked grant opens nothing), and
neither token's ability to reach any other route or origin. The check-in
lease is tested with shortened leases: a recording that checks in runs for 25
leases and still finishes; one that never checks in stops and stays
finishable for the grace; a late check-in is refused and restarts nothing;
check-ins from another origin, with the grant token, a guessed token, or a
body leave the lease to lapse; Stop, Cancel, and both revokes end a
checking-in recording and refuse its next check-in; and a recording the
capture loop lost is cancelled. One ignored test runs the real 10 s lease for
125 s, past the old two-minute cap. On macOS, one test connects to the real `record.sock`, reads the
peer's audit token, pairs, records twice through the fake backend, and revokes;
it skips (with a printed note) when the test binary has no signing identity.
The live card and the Labs › Browser access revoke rows are rendered by
`hex app -- --snap` scenes (`record-active`, `mac-labs-browser-access`), and
the permission window by the ignored snapshot test above; nothing was recorded from a real microphone, no
OS grant was approved, and no real browser page or Electron process was run.
