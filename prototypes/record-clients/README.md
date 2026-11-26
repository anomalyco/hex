# Recording clients

Example clients, not part of Hex: copy them into your own app. They show
Hex's revocable recording grant, as specified in
[`docs/connect.md`](../../docs/connect.md#recording-by-a-paired-client). They
use Hex's owner-only native channel, so Hex identifies the process itself and
no app name, `Origin`, or discovery token is trusted or read:

| OS | Channel | Hex names the process by |
| --- | --- | --- |
| macOS | `~/Library/Application Support/hex/record.sock` | its code signature |
| Linux | `~/.local/share/hex/record.sock` (`$XDG_DATA_HOME/hex`) | its Flatpak or Snap app id, else its executable |
| Windows | the named pipe that `%APPDATA%\hex\record.pipe` names | its Authenticode publisher and file name, else its path |

Both clients find the channel the same way (`HEX_APPLICATION_SUPPORT_DIR`
moves Hex's folder, `HEX_RECORD_SOCKET` names the socket or pipe directly)
and make the same calls on every OS.

- [`electron-main.mjs`](electron-main.mjs) — an Electron **main process**
  (Node 18+). Run it with `electron electron-main.mjs` or
  `node electron-main.mjs`; it pairs once, records twice while checking in,
  prints each transcript, and revokes the grant.
- [`native-rust.rs`](native-rust.rs) — a standalone Rust example, also usable
  from a Rune host. Copy it into a binary with `sha2`, `serde_json` and
  `getrandom`, then run it; it does the same pair → record twice → revoke
  sequence.

Both record once for **Allow Once**, or twice then revoke for **Always Allow**. **Deny**, Return,
Escape, or letting the dialog time out fails the pairing without recording.
Pairing starts nothing; each `POST /record/start` under the grant opens a
fresh session that runs for as long as the client checks in
(`POST /record/heartbeat` every `leaseMs / 3`; the demos record five seconds,
`HEX_RECORD_MS` for the Electron one). Stop checking in (quit, crash) and Hex
stops the microphone within one lease (10 s). Hex's window shows the live card
with **Stop recording** and **Cancel**. **Revoke** in Hex's window
(Labs › Connected apps, under Connected) or `POST /record/revoke` ends the grant, cancelling a
live session and making further starts fail.

Grant tokens expire when Hex quits. Always Allow persists until revoked; the
same program can pair on the next launch without another dialog.

An unpackaged Electron app, or a script run by `node` or `python3`, is named
by its interpreter (`…/electron/dist/electron`, `/usr/bin/node`), because that
is the program the OS reports: allowing it allows every script it runs. Use
Allow Once for shared hosts, and a packaged (on macOS and Windows, signed) app
for a grant of its own.

Both need Labs › Connected apps on in Hex (native apps need nothing else).
Web pages use the loopback HTTP API instead (`POST /record/pair` from an
`Origin` allowed in Labs › Connected apps, with Allow websites to use Hex
on); they cannot open the native channel.
