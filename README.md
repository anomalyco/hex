# Hex Preview for Android

**[Download Hex Preview 0.2.2 (build 2448)](https://github.com/anomalyco/hex/releases/download/android-preview-2448/HexPreview-0.2.2-preview.2448.apk)** · 75 MB · Android 12 or later · 2026-11-25

Hex Preview is the [Hex keyboard](https://anoma.ly/hex) for Android, before it reaches Google Play. It installs as its own app beside Hex from Google Play, and it finds its own updates: it checks this page's `latest.json` when you open it and at most once a day from the keyboard, shows what changed, and **Download update** opens the new build in your browser.

## Install

1. Open the download link above on your phone and download the APK.
2. Open it. Android asks whether your browser or Files app may install apps: tap **Settings**, turn on **Allow from this source**, then go back, tap the download again and tap **Install**.
3. Open Hex Preview and follow its setup: the microphone, Hex Preview as your keyboard, and the 190 MB speech model.

Updates install the same way: open the APK that Download update downloads and tap **Update** when Android asks.

## Check a download

- APK SHA-256: `749bd4a9c86d1921b312ffd4937a98016eede616ab4bc88e9fafe6bc88fd23a1`
- Signing certificate SHA-256: `13:35:68:F6:F0:61:4F:E5:7B:C8:8D:AE:CA:32:AF:A7:2C:6A:B7:40:20:26:29:F7:52:FA:A0:33:BE:35:D9:8E`

Every Hex Preview build is signed with this certificate; Android refuses an update signed with any other. Check with `apksigner verify --print-certs` or `keytool -printcert -jarfile`.

## What changed

- Short dictations no longer repeat themselves when you use custom vocabulary or Learn from my fixes.
- Dictation keeps its threads ready between steps, on your phone's fast cores, so it finishes sooner on phones that also have slower cores.
- New in Labs › Dictation: Learn from my fixes, off by default. Fix a word right after dictating and, after the second time, Hex writes it your way. The pairs stay on this phone, and Delete what Hex learned removes them.
- With Autocorrect off, the suggestion strip still offers the nearest words to tap; space never changes your word.
- Labs › Diagnostics now says which cores dictation runs on, and the speed test's rows keep the same names from build to build.
- The low-memory note beside Undo is no longer cut off.
- Português now has its own keyboard: autocorrect, suggestions, next words and swipe typing, with accents on long press (á ã ç é ê ô…). It's tuned for Brazilian Portuguese first; Portugal's own spellings, like "facto" or "telemóvel", are kept as you type them.

Earlier builds are on the [releases page](https://github.com/anomalyco/hex/releases?q=android-preview) (tags `android-preview-<build>`). Typing and dictation stay on the phone; the update check asks GitHub for this file and carries nothing about you.
