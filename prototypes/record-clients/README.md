# Recording clients

Minimal clients for Hex's revocable recording grant, as specified in
[`docs/connect.md`](../../docs/connect.md#recording-by-a-paired-client). They
use Hex's owner-only Unix socket
(`~/Library/Application Support/hex/record.sock`), so Hex identifies the
process by its code signature — no app name, `Origin`, or discovery token is
trusted or read.

- [`electron-main.mjs`](electron-main.mjs) — an Electron **main process**
  (Node 18+). Run it with `node electron-main.mjs`; it pairs once, records
  twice, prints each transcript, and revokes the grant.
- [`native-rust.rs`](native-rust.rs) — a standalone Rust example, also usable
  from a Rune host. Copy it into a binary with `sha2` and `serde_json`, then
  run it; it uses `HEX_RECORD_SOCKET` or the default path and does the same
  pair → record twice → revoke sequence.

Both wait for the person to press **Allow** in Hex. **Not now**, Escape, the
scrim, or letting the dialog time out fails the pairing without recording.
Pairing starts nothing; each `POST /record/start` under the grant opens a
fresh session that stops itself at the lease, and Hex's window shows the live
card with **Stop recording** and **Cancel**. **Revoke** in Hex's window
(Labs › Browser access) or `POST /record/revoke` ends the grant, cancelling a
live session and making further starts fail.

Grants are kept in Hex's process, not on disk: quitting Hex revokes every
pairing, and the client must pair again next launch. That is deliberate until
there is a secure store for a client's identity.

Web pages use the loopback HTTP API instead (`POST /record/pair` with an
`Origin` listed in Labs › Browser access to the local API); they cannot open
the Unix socket.
