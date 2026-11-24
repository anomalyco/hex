# Hex Preview for Android

**[Download Hex Preview 0.2.2 (build 2348)](https://github.com/anomalyco/hex/releases/download/android-preview-2348/HexPreview-0.2.2-preview.2348.apk)** · 67 MB · Android 12 or later · 2026-11-24

Hex Preview is the [Hex keyboard](https://anoma.ly/hex) for Android, before it reaches Google Play. It installs as its own app beside Hex from Google Play, and it finds its own updates: it checks this page's `latest.json` when you open it and at most once a day from the keyboard, shows what changed, and **Download update** opens the new build in your browser.

## Install

1. Open the download link above on your phone and download the APK.
2. Open it. Android asks whether your browser or Files app may install apps: tap **Settings**, turn on **Allow from this source**, then go back and tap **Install**.
3. Open Hex Preview and follow its setup: the microphone, Hex Preview as your keyboard, and the 190 MB speech model.

Updates install the same way: open the APK that Download update downloads and tap **Update** when Android asks.

## Check a download

- APK SHA-256: `dd0024c771b04a167d3bbf385d19aaa178f9c83a58ef5cc50eaf0e738102ee77`
- Signing certificate SHA-256: `13:35:68:F6:F0:61:4F:E5:7B:C8:8D:AE:CA:32:AF:A7:2C:6A:B7:40:20:26:29:F7:52:FA:A0:33:BE:35:D9:8E`

Every Hex Preview build is signed with this certificate; Android refuses an update signed with any other. Check with `apksigner verify --print-certs` or `keytool -printcert -jarfile`.

## What changed

- Swipe typing: slide across a word's letters to type it. On by default, in Hex Preview › Keyboard.
- Number row: switch it on in Hex Preview › Labs.
- Dictation finishes sooner when you pause or stop talking, and ✕ stops it faster.
- Hex Preview no longer installs its own updates, which is what Google Play Protect was flagging. From now on, Download update opens the download in your browser.

Earlier builds are on the [releases page](https://github.com/anomalyco/hex/releases?q=android-preview) (tags `android-preview-<build>`). Typing and dictation stay on the phone; the update check asks GitHub for this file and carries nothing about you.
