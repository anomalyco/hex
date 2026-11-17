# Browser connect: one transcription with consent

A web page served from an origin listed in Labs › Browser access to the local
API can ask Hex's window for **one transcription upload**. The page never
receives the local API's bearer token: Hex asks the person in its own window,
and a code plus PKCE produces a one-use upload token scoped to the page's
verified origin.

This is a prototype contract. It exists to prove the consent boundary, not to
be a general browser API. `api-browser-access` (the Lab) stays off by default.

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
- Labs › Browser access to the local API on, and the page's origin in its
  list. That list already gates CORS (`src/local_api/cors.rs`); connect adds
  no second list.
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
