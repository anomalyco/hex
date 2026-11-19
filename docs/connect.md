# Local consent: one upload, or a recording grant

Two narrow asks share one handshake: **one transcription upload** from a web
page, and a **revocable grant to start recordings** for a web page, an
Electron main process, or any native/Rune process. Neither caller ever
receives Hex's local API bearer token. Hex asks the person in its own window,
and a code plus PKCE produces a token scoped to the caller's verified identity:

- a web page is bound to the `Origin` its browser sent, on the loopback HTTP
  API (a website the person allowed in Labs › Connected apps);
- a native process is bound to the one peer process on Hex's owner-only
  native channel, and its identity is what the OS reports for it: on macOS
  the code-signing identifier and team, on Linux its sandbox app id or
  executable, on Windows its image's Authenticode publisher or path
  ([Native identity](#native-identity)). A name it claims is never the
  identity.

This is a narrow consent contract, not a general local API.
`connected-apps` (the Lab, Labs › Connected apps)
stays off by default, and so does its Allow websites to use Hex. Labs ›
Browser access to the local API is a separate developer feature (CORS for
the full bearer-token API from the origins it lists) and plays no part in
any of this.

**Any website can now ask to be let in.** With the lab and its websites
switch on, Hex answers these routes' CORS for every `http(s)` origin, so any
page the person visits can send a consent request. An unknown page never
gets a dialog: it is refused and listed for the person to allow or block,
and only an allowed page can open the window, where it still needs Allow
Once or Always Allow. One dialog at a time, a 30 s cooldown per denied or
unanswered recipient (across both asks), and the window's allow guard
still apply.

The recording section is [below](#recording-by-a-paired-client); the existing
upload flow keeps its own paths and scope.

```ts
A page's fetch -> POST /connect/authorize            // no bearer token
├── Lab or Allow websites to use Hex off, Origin missing or not http(s)
│     -> 403 origin-not-allowed (no dialog, nothing listed, no CORS)
├── Blocked in the lab -> 403 website-blocked (no dialog, never listed)
├── Not allowed yet -> 403 website-not-allowed (no dialog); listed once under
│     Asking to connect in Hex › Connected apps (origin and time), where the
│     person chooses Allow (it may ask on its next request) or Block
├── No JSON body, or not S256 -> 400 invalid-request
├── A dialog is already open -> 409 consent-busy
├── Allowed origin -> Hex's permission window: "Allow this page to transcribe audio?" [Deny] [Allow Once] [Always Allow]
│   ├── Deny, Return, Escape, or 60 s with no answer -> 403, no code
│   │   └── Deny or no answer starts a 30 s cooldown for that origin, across both asks (429 consent-cooldown)
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
  has no window, so the routes answer `403 connect-unavailable`; it opens no
  native channel either.
- Labs › Connected apps on. Its switch gates both asks, for apps and
  websites alike; off, every consent route answers as it did before the lab
  (`origin-not-allowed` for a page, `record-unavailable` for a native app,
  `401` for a grant or session token), a pending dialog is withdrawn, a live
  consented recording is cancelled, the native socket or pipe is closed
  (within 0.2 s, ending its connections; Hex does no identity work while it
  is off), and saved grants, the website lists, and the websites switch are
  kept but unused until it is on again. The native channel serves at most 8
  clients at once; one more is answered `503 native-busy`.
- A desktop that can show Hex's permission window. A Wayland compositor
  without layer-shell (GNOME) can't: there Settings › Labs keeps the
  Connected apps card, disabled while off, with a note saying so, `hex labs`
  prints the same note in its list and after `hex labs on connected-apps`,
  and every ask answers unavailable within a second instead of waiting
  unseen.
- For a web page, also the lab's **Allow websites to use Hex** and the page's
  exact origin under **Allowed websites** (added with Allow on its entry in
  **Asking to connect**). Being allowed to ask grants nothing. Both lists
  are in the Connected apps pane, Hex's sidebar item while the lab is on.
  The switch off works like the lab off for websites only: no website asks,
  is listed, gets CORS, or uses a grant it has.
- CORS (`src/local_api/access.rs`): with both switches on, `OPTIONS` and the
  responses of `/connect/*`, `/record/*`, and a `POST /transcriptions` that
  carries `X-Hex-Connect-Token` and no `Authorization` name the request's own
  `Origin` (never `*`, never credentials). The preflight allows only
  `Content-Type`, `X-Hex-Connect-Token`, `X-Hex-Record-Grant`, and
  `X-Hex-Record-Token`, so a page can't send the bearer token through it. No
  other route (`/control`, `/dictations`, `/streams`, `/health`, …) answers
  CORS for Connected apps; Labs › Browser access keeps its own list for
  those. The connect POSTs are JSON, so a browser sends a preflight: a page
  cannot trigger a dialog with a cross-site form or `no-cors` fetch.

## `POST /connect/authorize`

Request (JSON, at most 96 KiB):

```json
{
  "codeChallenge": "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
  "codeChallengeMethod": "S256",
  "clientName": "Field Notes",
  "icon": "data:image/png;base64,iVBORw0KGgo…",
  "state": "opaque-app-value"
}
```

`codeChallenge` is the base64url (no padding) SHA-256 of the page's
`codeVerifier`, per RFC 7636. `codeChallengeMethod` may be omitted, but only
`S256` is accepted. `clientName` is optional; Hex shows it as "The page calls itself: …"
only when it is at most 40 plain characters (no controls, no bidi overrides). `state` is echoed back unchanged (at most the body limit).

`icon` is optional: the page's favicon as a `data:` URL of a PNG
(`image/png`) or ICO (`image/x-icon`, `image/vnd.microsoft.icon`), base64,
at most 64 KiB decoded and 1024 px a side. Hex decodes it locally, on the
request's worker, scales it to at most 128 px, and shows it on the identity
card's tile. Anything else (a URL, another type, a broken or oversized
image) is ignored and the card shows its globe; Hex never fetches an icon.
Like a browser tab's favicon it is only the page's claim: the card still
names the page by its address. The same field works on `POST /record/pair`.

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
to be a canonical, allowed entry; the dialog says it is the address reported
by the browser. The grant is bound to that exact origin.

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
  One dialog at a time; a Deny, or a dialog left unanswered for 60 s, starts
  a cooldown for that recipient in both asks.
- The browser offers no server-side proof that its request followed a click
  (`navigator.userActivation` never leaves the page), so the native dialog is
  the gate it can enforce: nothing is allowed without a press (and none in the
  window's first second), Return and Escape deny, and a denied page waits 30 s before it may ask again. A page
  cannot reach the POST without a CORS preflight for `application/json`.

## Limits

| Limit | Value |
| --- | --- |
| Authorize/token body | 96 KiB (for the favicon), read within 5 s |
| Code lifetime | 120 s, one use |
| Upload token lifetime | 120 s, one use |
| Dialog wait | 60 s, then `403 consent-timeout` |
| Deny cooldown | 30 s per recipient after a Deny or no answer, across transcription and recording (`429 consent-cooldown`) |
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

The identity card names who is asking, by what Hex verified:

```ts
native app -> its icon, the name Hex derived from verified data (macOS bundle,
              Linux desktop entry, Windows version info; see Native identity),
              else a program's file name, else its identifier; the identifier
              (bundle id, app id, or full program path) under it unless it is
              already the name; then how it was verified, never repeating it:
              macOS signature   -> "Code signature verified · team K7Z4Q2M8A1" (or "· no team"),
                                   only for `anchor apple generic` (Developer ID, App Store, Apple)
              Windows signature -> "Signed by {publisher}"
              Flatpak / Snap    -> "Flatpak app" / "Snap app"
              only a path       -> "Not signed" (warning icon)
              no icon           -> the app glyph
website    -> its favicon on the tile (if it sent a valid one, else the globe),
              its address, "Address reported by your browser", and the name it
              gave itself as "Calls itself “…” · not verified"
```

On macOS the name and icon come from the bundle at the path
`Security.framework` reports for the peer's verified static code
(`src/local_api/mac_app.rs`): the code is resolved again from the peer's
audit token and used only when its code-directory hash matches the identity
the grant is bound to. The lookup runs on the request's worker, is skipped
after 0.8 s, and is cached by code-directory hash. Nothing the client sends
names it. The footer says Always Allow lasts until it is revoked in Hex ›
Connected apps.

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
(X11, lavapipe) with `curl` asks from an allowed origin and `xdotool` clicks:
both asks showed centred with the main window closed, Allow Once during the
guard did nothing, Escape did not reach the window, Deny answered
`consent-denied` and hid it, and Allow Once after the guard issued a code.
A later run with Labs › Connected apps on (websites included, one allowed
origin in `local-consent.json`) answered an unknown page's pairing with
`website-not-allowed` and CORS, wrote it to the file's tried list, and showed
the allowed page's dialog with "Address reported by your browser" and the
Connected apps footer.
On macOS the native window (its placement, level, key focus, and focus
returning to the app in front) was not shown on a real screen in a test;
Windows was only compiled, and Wayland, GNOME's fallback, a real browser
page, and a real model transcription from a page were not exercised. Copy is
English only. The origin check trusts the browser's `Origin` header; a
malicious local process can claim an allowed origin to *ask*, or fill
Asking to connect with made-up origins (20 at most), but
not silently reuse a saved permission.
On the first Always Allow response, a web client receives `permissionProof`.
Store it in origin-scoped storage and send it in later authorize/pair JSON for
that capability. Hex saves only its SHA-256 digest. A missing or incorrect proof
returns `403 permission-proof-required`; revoke in Hex › Connected apps and pair again if the
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
Native client (socket or pipe)     Web page (loopback + Origin)
        │                                   │
        └──────── POST /record/pair ────────┘     // PKCE S256, no bearer
             ├── native peer identity unreadable -> 403 identity-unverified
             ├── already paired -> 409 already-paired (no dialog)
             ├── another dialog is open -> 409 consent-busy
             ├── Hex's permission window: "Allow microphone recording?"
             │     recipient, team note, and that pairing starts nothing
             │     [Deny] [Allow Once] [Always Allow]
             │   ├── Deny / Return / Escape / 60 s -> 403, no code
             │   │     Deny or no answer cools that recipient down for 30 s, both asks
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

A grant is withdrawn in Hex's window (the Connected apps pane lists every
connected app and website with **Revoke**) or by the client's own `POST /record/revoke`. Revoking
cancels any live session, drops pairing codes issued before the revoke, and
makes further starts fail. The client may pair again, which asks the person
again. Pairing while a grant already exists is `409 already-paired`; it does
not open a second dialog.

### Native identity

Native clients connect to Hex's own channel, which only this user can open and
which serves `/record/*` only. Hex asks the OS who the peer is on every
connection; if it cannot tell, the request is refused (`403
identity-unverified`) rather than falling back to an app name or path the
client sent. A grant and its sessions are bound to that one peer process, so
another process cannot spend them even if it learns the token.

| | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Channel | `record.sock` in Hex's data folder, mode `0600` | `record.sock` in Hex's private data folder (`0700`, socket `0600`) | `\\.\pipe\anomaly.hex.record.<random>`, named in `record.pipe` in Hex's data folder; DACL for Hex's user only, `PIPE_REJECT_REMOTE_CLIENTS` |
| Peer | `LOCAL_PEERTOKEN` audit token; `getpeereid` user is Hex's | `SO_PEERCRED`; same uid as Hex; pinned by `SO_PEERPIDFD` (Linux 6.5+) and its `/proc/<pid>/stat` start time before and after | `GetNamedPipeClientProcessId`, opened at once; started before the connection; token user is Hex's |
| Identity | `SecCodeCopySigningInformation`: signing identifier and team, when the signature satisfies `anchor apple generic`; an ad-hoc or otherwise unanchored signature is named by its path, like an unsigned program elsewhere | Flatpak `[Application] name` in `/proc/<pid>/root/.flatpak-info`; else an enforced Snap AppArmor label `snap.<snap>.<app>`; else `/proc/<pid>/exe` (absolute, not deleted) | `WinVerifyTrust` on the image, no revocation check and cache-only URL retrieval (Hex never reaches the network): the signer's subject CN; else the image path |
| Grant key | identifier (or, unanchored, the path), team and code-directory hash | the sandbox app id, else the executable path | publisher and image file name (an update keeps it), else the full path |
| Name and icon | the verified bundle (above) | the desktop entry the identity points to (`<app id>.desktop`, `<snap>_<app>.desktop`, or the entry whose `TryExec`/`Exec` program is that executable): its `Name`, and its `Icon` from the hicolor theme or pixmaps | the image's `FileDescription` (else `ProductName`) and first icon |

The name and icon are display only, looked up off the request thread, cached,
and waited for at most 400 ms; they never decide a grant. On Linux and for an
unsigned Windows program they are only as trustworthy as whoever wrote the
desktop entry or the file's version info, so the dialog's identity is the app
id or path.

The native identity is the program the OS reports, not a document, script, or
renderer. Allowing a shared interpreter such as Node, Python, or an
unpackaged Electron runtime therefore allows that interpreter, with every
script it runs, not just one script. Use Allow Once for shared hosts.

Same-user isolation bounds all of this. A program running as the same user
can usually reach another's process (debugging it, or injecting into it on
Windows); outside a sandbox the identity says which program connected, not
that no other code runs inside it. On Linux kernels before 6.5 (no
`SO_PEERPIDFD`), a peer that exits right after connecting leaves a short
window in which a new process could reuse its pid unseen. Flatpak and Snap
apps reach the socket only when their sandbox lets them see Hex's data
folder.

Native clients need no `Origin`, no bearer token, and no discovery file. The
loopback API is not their channel, and the native channel has no route for it.

### Lease

| Limit | Value |
| --- | --- |
| Pairing code lifetime | 120 s, one use |
| Grant token | Process-local; one session for Allow Once, reusable for Always Allow |
| One recording | No length limit while the client checks in |
| Check-in lease | 10 s (`leaseMs`); the microphone stops when it lapses |
| After the stop | 30 s for `finish`, then discarded (`409 session-expired`) |
| Concurrent recordings | 1 (`409 session-busy` on the next start) |
| Body | 96 KiB (a page's favicon in `pair`); start/status/heartbeat/finish/cancel/revoke take no body |

The lease is the one Hex's `/dictations` and `/streams` use. Only
`POST /record/heartbeat` with the session's own token, on the channel that
started it (the same `Origin` or native peer process), renews it; `status` does not.
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
matches require the grant key above and how the OS verified it: on macOS the
validated signing identifier, team and code-directory hash, so a changed binary
must ask again. Allowed web origins match independently of the
page's claimed name. Transcription and microphone recording are separate choices.

The Connected apps pane (Hex's sidebar item while Labs › Connected apps is
on) lists saved permissions under **Connected** even when the client is
offline: each with its icon and name, who it is, what it may do (Microphone
recording or Transcription, always allowed or allowed once), and **Revoke**.
So it can, each saved permission also keeps the recipient's display name (a
native app's, from its verified bundle) and icon as a small PNG, refreshed
whenever it asks again; matching never reads either, and an icon that no
longer reads is dropped without losing the permission.

The same owner-only file keeps the lab's website lists, which the pane shows
under **Websites** while **Allow websites to use Hex** is on (off, they are
hidden and the switch says websites can't ask): **Asking to connect**, with
its count (origin and the most recent attempt time only, one entry per
origin, the newest 20, rewritten on disk at most once a minute per origin;
**Allow** or **Block**), then **Allowed websites** (may ask; **Remove**) and
**Blocked websites** (answer `website-blocked`, never listed again;
**Unblock**), both folded behind a disclosure with their counts. An allowed
or blocked website keeps the favicon it last sent with an ask; a website
asking to connect has none, since Hex refuses its request before reading it. An origin is in one list at most. Removing or blocking a website
ends its pending dialog and live recording at once; Hex never shows a dialog
after the fact for a website the person just allowed.
Revoke deletes just that capability from disk. It invalidates its outstanding
codes and tokens; for recording it also cancels an active session. A failed
save never grants access, and a failed revoke does not disappear from the list.

### Errors

| Status | Code | When |
| --- | --- | --- |
| 400 | `invalid-request` | Not the JSON above, not S256, a body on a token action |
| 401 | `invalid-grant` | Unknown, revoked, or other-channel grant; a session token used as a grant |
| 403 | `identity-unverified` | The OS could not say who the native peer is (another user, a pid that changed, no signature on macOS) |
| 403 | `origin-not-allowed` | Labs › Connected apps or its websites switch is off, or the Origin is missing or not an http(s) origin |
| 403 | `website-not-allowed` | A website the person has not allowed (listed under Asking to connect) |
| 403 | `website-blocked` | A website the person blocked |
| 403 | `consent-denied`, `consent-timeout`, `record-unavailable` | Deny, no answer, no window/gate |
| 403 | `invalid-grant` | Wrong, spent, or expired pairing code |
| 404 | `record-not-found` | Unknown session token or one bound to another channel |
| 409 | `stopped` | A check-in after the recording stopped (lease, Stop in Hex, or the client's `finish`) |
| 409 | `already-paired` | That recipient already has a grant |
| 409 | `session-busy`, `session-expired`, `session-finished`, `session-cancelled`, `not-started`, `finish-in-progress` | Session state |
| 429 | `consent-cooldown` | The recipient was denied, or left unanswered, less than 30 s ago, in either ask |
| 503 | `capture-unavailable` | No capture loop could start the recording |
| 503 | `native-busy` | The native channel already serves 8 clients |

### What is not exposed

- No raw audio, no `/record/audio`; a grant token reaches only `start` and
  `revoke`, and a session token only `status`, `heartbeat`, `finish`, and
  `cancel` for its own session.
- No `/dictations`, `/streams`, `/transcriptions`, `/control`, `/models`,
  History, clipboard, Accessibility, or settings. Both record headers are
  checked before the bearer token and never become it.
- Tokens live only in Hex's process; they are 32 random bytes, the `finish`
  transcript is handed over once, and each is bound to the Origin or native
  peer process that paired.

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
125 s, past the old two-minute cap. On macOS and Linux, one test connects to the real `record.sock`, reads the
peer's identity, pairs, records twice through the fake backend, and revokes;
on macOS it skips (with a printed note) when the test binary has no signing
identity. `local_api::native::linux` tests parse `.flatpak-info` and AppArmor
labels, refuse deleted executables, another user's peer, and a process whose
start time changed or that went while being read, name a socket pair's peer
by its executable, and match desktop entries by `Exec`/`TryExec`, PATH, and
desktop file id precedence.
Labs › Connected apps is covered by `local_api::tests::connected_apps` (each
switch off on every consent route, CORS on only its own routes and never with
`Authorization`, an unknown website refused and listed once, Allow and Block,
an allowed website still needing an answer, Browser access having no effect,
the revoke list covering apps and websites, and a pending ask and live
recording ending when the lab goes off) and the website lists' bounds and
reload by `local_api::permissions` tests. The live card and the Labs ›
Connected apps section are rendered by `hex app -- --snap` scenes
(`record-active`, `mac-labs-connected-apps`), and
the permission window by the ignored snapshot test above; nothing was recorded from a real microphone, no
OS grant was approved, and no real browser page was run.

On Linux the running app was driven under Xvfb with a PulseAudio null-sink
microphone playing the JFK sample: the Rust example and an Electron 38 main
process (`electron electron-main.mjs`) each opened the permission window
naming their executable (`…/record-once`, `…/electron/dist/electron`), Deny
answered `consent-denied`, Allow Once recorded and transcribed once, Always
Allow saved an `executable` scope in `local-consent.json` and, after a Hex
restart, paired again without a dialog, recorded twice, and revoked; Hex
showed the Rust example with the name and SVG icon of its desktop entry.
Windows was built and linted only
(`cargo xwin clippy`); its pipe, signature check, and clients were not run.
