# Recording clients

Minimal clients for Hex's one-recording consent, as specified in
[`docs/connect.md`](../../docs/connect.md#one-microphone-recording). They use
Hex's owner-only Unix socket (`~/Library/Application Support/hex/record.sock`),
so Hex identifies the process by its code signature — no app name, `Origin`,
or discovery token is trusted or read.

- [`electron-main.mjs`](electron-main.mjs) — an Electron **main process**
  (Node 18+). Run it with `node electron-main.mjs`; it asks Hex, starts, and
  finishes one recording, printing the transcript.
- [`native-rust.rs`](native-rust.rs) — a standalone Rust example, also usable
  from a Rune host. Copy it into a binary with `sha2` and `serde_json`, then
  run it; it uses `HEX_RECORD_SOCKET` or the default path.

Both wait for the person to press **Allow once** in Hex. Denying, Escape, the
scrim, or letting the dialog time out fails the call without recording. The
recording stops itself at the lease, and Hex's window shows the live card with
**Stop recording** and **Cancel**.

Web pages use the loopback HTTP API instead (`POST /record/authorize` with an
`Origin` listed in Labs › Browser access to the local API); they cannot open
the Unix socket.
