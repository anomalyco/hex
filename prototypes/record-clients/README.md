# Recording clients

Minimal clients for Hex's revocable recording grant, as specified in
[`docs/connect.md`](../../docs/connect.md#recording-by-a-paired-client). They
use Hex's owner-only Unix socket
(`~/Library/Application Support/hex/record.sock`), so Hex identifies the
process by its code signature — no app name, `Origin`, or discovery token is
trusted or read.

- [`electron-main.mjs`](electron-main.mjs) — an Electron **main process**
  (Node 18+). Run it with `node electron-main.mjs`; it pairs once, records
  twice while checking in, prints each transcript, and revokes the grant.
- [`native-rust.rs`](native-rust.rs) — a standalone Rust example, also usable
  from a Rune host. Copy it into a binary with `sha2` and `serde_json`, then
  run it; it uses `HEX_RECORD_SOCKET` or the default path and does the same
  pair → record twice → revoke sequence.

Both record once for **Allow Once**, or twice then revoke for **Always Allow**. **Deny**, Return,
Escape, or letting the dialog time out fails the pairing without recording.
Pairing starts nothing; each `POST /record/start` under the grant opens a
fresh session that runs for as long as the client checks in
(`POST /record/heartbeat` every `leaseMs / 3`; the demos record five seconds,
`HEX_RECORD_MS` for the Electron one). Stop checking in (quit, crash) and Hex
stops the microphone within one lease (10 s). Hex's window shows the live card
with **Stop recording** and **Cancel**. **Revoke** in Hex's window
(Labs › Browser access) or `POST /record/revoke` ends the grant, cancelling a
live session and making further starts fail.

Grant tokens expire when Hex quits. Always Allow persists until revoked; the
same signed binary can pair on the next launch without another dialog.

Web pages use the loopback HTTP API instead (`POST /record/pair` with an
`Origin` listed in Labs › Browser access to the local API); they cannot open
the Unix socket.
