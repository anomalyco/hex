# Hex Preview for Android

**[Download Hex Preview 0.2.2 (build 2325)](https://github.com/anomalyco/hex/releases/download/android-preview-2325/HexPreview-0.2.2-preview.2325.apk)** · 68 MB · Android 12 or later · 2026-11-23

Hex Preview is the [Hex keyboard](https://anoma.ly/hex) for Android, before it reaches Google Play. It installs as its own app beside Hex from Google Play, and it updates itself: it checks this page's `latest.json` when you open it and at most once a day from the keyboard, shows what changed, and installs a new build only when you tap Update.

## Install

1. Open the download link above on your phone and download the APK.
2. Open it. Android asks whether your browser or Files app may install apps: tap **Settings**, turn on **Allow from this source**, then go back and tap **Install**.
3. Open Hex Preview and follow its setup: the microphone, Hex Preview as your keyboard, and the 190 MB speech model.

The first update asks the same once for Hex Preview itself.

## Check a download

- APK SHA-256: `c7ca0eb00f2095d72484cdfca9ade229ecc6acb4390df47e37db258fe682b674`
- Signing certificate SHA-256: `13:35:68:F6:F0:61:4F:E5:7B:C8:8D:AE:CA:32:AF:A7:2C:6A:B7:40:20:26:29:F7:52:FA:A0:33:BE:35:D9:8E`

Every Hex Preview build is signed with this certificate; Android refuses an update signed with any other. Check with `apksigner verify --print-certs` or `keytool -printcert -jarfile`.

## What changed

- Short dictations are about a quarter faster, and the speech model stays ready between dictations.
- Portuguese dictation. On a Portuguese phone Hex no longer writes your Portuguese in English.
- Setup and settings say what the speech model does instead of naming it.

Earlier builds are on the [releases page](https://github.com/anomalyco/hex/releases?q=android-preview) (tags `android-preview-<build>`). Typing and dictation stay on the phone; the update check asks GitHub for this file and carries nothing about you.
