# Browser connect demo

A single static page that exercises the contract in
[`docs/connect.md`](../../docs/connect.md): PKCE, the consent dialog, the
one-use upload token, and one WAV upload. It never reads Hex's discovery file
or token.

1. Turn on Labs › Browser access to the local API in Hex and add this page's
   origin to its list (for example `http://localhost:5173`).
2. Serve this folder from that origin (any static server works):

   ```sh
   cd prototypes/browser-connect
   python3 -m http.server 5173
   ```

3. Read Hex's local API port, then paste it into the page:

   ```sh
   python3 -c 'import json,os;print(json.load(open(os.path.expanduser("~/Library/Application Support/hex/local-api.json")))["port"])'
   ```

4. Choose a WAV file and press **Ask Hex for one transcription**. Hex opens its
   window with the Allow once / Deny dialog; Allow answers the page, the page
   mints its one-use token, and uploads the file once.

A Deny (or Escape, or the scrim) cancels: the page gets `403 consent-denied`,
and the origin waits 30 seconds before it may prompt again.
