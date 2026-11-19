# Browser connect demo

A single static page that exercises the contract in
[`docs/connect.md`](../../docs/connect.md): PKCE, the consent dialog, the
one-use audio token and one WAV file, and a microphone recording that runs
while the page checks in. It never reads Hex's discovery file or token.

1. Turn on Labs › Connected apps in Hex, and in its card **Allow websites to
   use Hex**.
2. Serve this folder (any static server works), for example from
   `http://localhost:5173`:

   ```sh
   cd prototypes/browser-connect
   python3 -m http.server 5173
   ```

3. Read Hex's local API port, then paste it into the page:

   ```sh
   python3 -c 'import json,os;print(json.load(open(os.path.expanduser("~/Library/Application Support/hex/local-api.json")))["port"])'
   ```

4. Allow the site first. Press either button once: Hex shows no dialog yet
   and the page gets `403 website-not-allowed`. The page's origin now appears
   in Labs › Connected apps under **Websites that tried to contact Hex**;
   press **Allow** there (or **Block** to refuse it for good). Hex never
   shows a dialog after the fact, so ask again from the page.

5. Choose a WAV file and press **Ask Hex for one transcription**. Hex shows its
   permission window with **Deny**, **Allow Once**, and **Always Allow**, unless this
   page already has permission. Either approval lets the page transcribe the
   selected file. Always Allow saves a proof in this origin's local storage;
   if you clear site data, revoke the permission in Labs › Connected apps
   before pairing again.

6. Or press **Record with Hex**. After the recording dialog, Hex records
   until you press **Stop and get the transcript** (or Stop in Hex). The page
   sends `POST /record/heartbeat` every few seconds; close the tab and Hex
   stops the microphone within 10 s.

A Deny (or Return, or Escape) cancels: the page gets `403 consent-denied`,
and the origin waits 30 seconds before it may prompt again.
