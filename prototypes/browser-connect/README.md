# Browser connect demo

A single static page that exercises the contract in
[`docs/connect.md`](../../docs/connect.md): PKCE, the consent dialog, the
one-use audio token and one WAV file, and a microphone recording that runs
while the page checks in. It never reads Hex's discovery file or token.

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
   window with **Deny**, **Allow Once**, and **Always Allow**, unless this
   page already has permission. Either approval lets the page transcribe the
   selected file. Always Allow saves a proof in this origin's local storage;
   if you clear site data, revoke the permission in Labs before pairing again.

5. Or press **Record with Hex**. After the recording dialog, Hex records
   until you press **Stop and get the transcript** (or Stop in Hex). The page
   sends `POST /record/heartbeat` every few seconds; close the tab and Hex
   stops the microphone within 10 s.

A Deny (or Escape, or the scrim) cancels: the page gets `403 consent-denied`,
and the origin waits 30 seconds before it may prompt again.
