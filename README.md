# Hex Preview for Android

**[Download Hex Preview 0.2.2 (build 2397)](https://github.com/anomalyco/hex/releases/download/android-preview-2397/HexPreview-0.2.2-preview.2397.apk)** · 68 MB · Android 12 or later · 2026-11-24

Hex Preview is the [Hex keyboard](https://anoma.ly/hex) for Android, before it reaches Google Play. It installs as its own app beside Hex from Google Play, and it finds its own updates: it checks this page's `latest.json` when you open it and at most once a day from the keyboard, shows what changed, and **Download update** opens the new build in your browser.

## Install

1. Open the download link above on your phone and download the APK.
2. Open it. Android asks whether your browser or Files app may install apps: tap **Settings**, turn on **Allow from this source**, then go back, tap the download again and tap **Install**.
3. Open Hex Preview and follow its setup: the microphone, Hex Preview as your keyboard, and the 190 MB speech model.

Updates install the same way: open the APK that Download update downloads and tap **Update** when Android asks.

## Check a download

- APK SHA-256: `06cf36e3a0da967809796443cfca35d74b93ec1f5964f4d3f8aae173577f2a32`
- Signing certificate SHA-256: `13:35:68:F6:F0:61:4F:E5:7B:C8:8D:AE:CA:32:AF:A7:2C:6A:B7:40:20:26:29:F7:52:FA:A0:33:BE:35:D9:8E`

Every Hex Preview build is signed with this certificate; Android refuses an update signed with any other. Check with `apksigner verify --print-certs` or `keytool -printcert -jarfile`.

## What changed

- Key haptics are fixed on Samsung phones where the keys stayed silent: Hex's own Haptics switch now decides, and a key press plays a firmer click.
- The screen stays on while you dictate, so a short screen timeout no longer cuts a dictation off.
- New in Hex Preview › Labs: Diagnostics, a report with a speed test and test taps that you can copy and send us. Nothing is sent by itself.
- If your keys are still silent or dictation is slow, the Diagnostics report shows us why.
- Updating: if Android asks you to allow your browser to install apps, allow it, go back, and tap the download again.

Earlier builds are on the [releases page](https://github.com/anomalyco/hex/releases?q=android-preview) (tags `android-preview-<build>`). Typing and dictation stay on the phone; the update check asks GitHub for this file and carries nothing about you.
