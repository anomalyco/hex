# Hex Preview for Android

**[Download Hex Preview 0.2.2 (build 2467)](https://github.com/anomalyco/hex/releases/download/android-preview-2467/HexPreview-0.2.2-preview.2467.apk)** · 75 MB · Android 12 or later · 2026-11-25

Hex Preview is the [Hex keyboard](https://anoma.ly/hex) for Android, before it reaches Google Play. It installs as its own app beside Hex from Google Play, and it finds its own updates: it checks this page's `latest.json` when you open it and at most once a day from the keyboard, shows what changed, and **Download update** opens the new build in your browser.

## Install

1. Open the download link above on your phone and download the APK.
2. Open it. Android asks whether your browser or Files app may install apps: tap **Settings**, turn on **Allow from this source**, then go back, tap the download again and tap **Install**.
3. Open Hex Preview and follow its setup: the microphone, Hex Preview as your keyboard, and the 190 MB speech model.

Updates install the same way: open the APK that Download update downloads and tap **Update** when Android asks.

## Check a download

- APK SHA-256: `43ab25b8f354f2ac4954b4fbb71adf824002c18f75fc9e83457633ef2beca039`
- Signing certificate SHA-256: `13:35:68:F6:F0:61:4F:E5:7B:C8:8D:AE:CA:32:AF:A7:2C:6A:B7:40:20:26:29:F7:52:FA:A0:33:BE:35:D9:8E`

Every Hex Preview build is signed with this certificate; Android refuses an update signed with any other. Check with `apksigner verify --print-certs` or `keytool -printcert -jarfile`.

## What changed

- A new first run: one page, one button for the next step, and Hex says which language it will dictate in.
- The 190 MB speech model waits for Wi-Fi, unless you tap Use mobile data.
- Keys sized like Gboard's, a number row that fits, and Settings › Keyboard › Height.
- Faster English dictation writes numbers as digits.
- A swipe in Nederlands, Polski or Русский now says swipe typing isn't available there yet.
- Translate what I say tells you which languages it works between, and when it needs Español.
- Word replacements can be shared as a Hex words file, and you review a file before adding it.
- New in Labs, all off until you turn them on: Automatic listens for the language, Dictate from Quick Settings, Key colors, Hex key on the left and Adaptive touch.

Earlier builds are on the [releases page](https://github.com/anomalyco/hex/releases?q=android-preview) (tags `android-preview-<build>`). Typing and dictation stay on the phone; the update check asks GitHub for this file and carries nothing about you.
