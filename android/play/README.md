# Hex keyboard on Google Play

**Before submission:** the [public privacy policy](https://dev.anoma.ly/hex/privacy) currently returns HTTP 200 but explicitly covers only iPhone. Add the [Android policy text](policy/privacy-policy.txt) at that URL alongside the iPhone policy. The signed app already shows the Android text offline in Privacy → Privacy policy.

## Listing files

Default language: English (United States). Store category: **Tools**. No account or purchase is needed; a reviewer can use the welcome screen’s practice field. Dictation requires microphone permission and the keyboard must be selected to use it in another app.

| Field | Draft |
| --- | --- |
| App name · 12/30 characters | [Title](en-US/title.txt) |
| Short description · 69/80 | [Short description](en-US/short-description.txt) |
| Full description · 841/4,000 | [Full description](en-US/full-description.txt) |
| Declarations | [Data safety](data-safety.md) · [IARC answers](content-rating.md) |
| Store art | [512 × 512 icon](artwork/icon.png) · [1024 × 500 feature graphic](artwork/feature-graphic.png), from the shipped mark and Inter |

**Phone screenshots** — 1080 × 1920, portrait 9:16, RGB PNG. Captured from the final signed build, with synthetic input:

1. [Typing](screenshots/01-keyboard.png) — Hex suggests “library” while a sentence is being typed.
2. [Dictation](screenshots/02-dictation.png) — the listening strip shows activity, finish and cancel.
3. [Settings](screenshots/03-settings.png) — the ready state, Dictate and four settings pages.
4. [Welcome](screenshots/04-welcome.png) — how dictation works and why there is a model download.

## Release candidate

`ly.anoma.hex` · 0.1.0 (version code 1) · Android 12+ · arm64-v8a. These are the existing package ID and version; this is the first local upload candidate. No store registration or rating is assumed.

| Size | Bytes |
| --- | ---: |
| Upload-signed AAB | 41,192,298 |
| Estimated initial device download, bundletool 1.18.3 | 38,337,704 |
| Separate one-time Whisper small download | 190,085,487 |

Bundletool reported the same minimum and maximum for the actual API 36 arm64-v8a, en-US, 440 dpi device spec and for the API 31 emulator. The model downloaded by Get started is not part of the initial store estimate. A smaller multilingual model is included in the app. AAB SHA-256: `904d5e2b045e003e5be9beacb71839b174101c247677624530a448d9d6a5d171`.

**Policy checks on the signed bundle:**

- Targets API 36, the [new phone-app requirement](https://developer.android.com/google/play/requirements/target-sdk) effective August 31, 2026. R8 and the resource shrinker ran. The release DEX has no Android log calls; the native Hex diagnostic, coroutine debug-probe resource and benchmark receiver are absent.
- The merged manifest requests only `RECORD_AUDIO` (user-started dictation) and `INTERNET` (the one model download). `BIND_INPUT_METHOD` protects the bound IME; it is not a requested permission. The system resolves its settings to `ly.anoma.hex.MainActivity`. No foreground service is declared or started.
- The external editor’s input connection received **zero context reads** during typing, deletion and a dictation attempt in text, visible, web and numeric password fields. Two literal writes and one deletion reached each field; none offered a suggestion or started the microphone. The same trace detected ten context reads and three suggestions in an ordinary field.
- All seven ELF load segments in `libhexspeech.so` and `libandroidx.graphics.path.so` are 16 KB aligned. BundleConfig requests 16 KB packaging, both stored split-APK entries are on 16 KB boundaries, and the arm64 split passed `zipalign -c -P 16 4`. The release ran dictation on a guest with 16,384-byte pages.

## Device matrix

Both Studio emulators started with no installed model. Each downloaded the pinned Whisper small file from setup, verified its hash, then completed the rest in airplane mode with Wi-Fi disabled. In the practice field, tapping “library” completed “Meet me at the library”. In a separate Android Settings editor, the release inserted **“Meet me at the library tomorrow.”** from the emulator microphone.

| Emulator | Guest page | First-run ready¹ | Typing PSS | Peak dictation PSS / RSS² | Home PSS after |
| --- | ---: | ---: | ---: | ---: | ---: |
| Android 12 / API 31, 2.5 GiB, 2 vCPUs | 4 KB | 18.6 s | 68.7 MiB | 376.7 / 492.5 MiB | 80.5 MiB |
| Android 16 / API 36, 4 GiB, 4 vCPUs | 16 KB | 19.8 s | 63.3 MiB | 367.3 / 496.5 MiB | 68.0 MiB |

¹ From Get started until the verified model was observed ready, including the permission interaction.

² Sampled from Android’s `dumpsys meminfo` on one short dictation (43 and 35 samples); PSS apportions shared pages, RSS counts them in full. The app process survived both runs. These are emulator observations, not handset performance targets.

## Real-device checklist

The [upload-key-signed APK set](../build/phase5-proof/Hex-0.1.0-device.apks) can be installed on a connected arm64 Android 12+ handset with `bundletool install-apks --apks=<file>` without using Play.

1. **Microphones and audio routes:** dictate with the handset mic and with Bluetooth earbuds while music is playing. Check input quality, playback continuity and that cancel or loss of focus releases the mic.
2. **Low-memory hardware:** on a 2–3 GB phone, time a cold dictation and several long ones. Check insertion latency, heat, battery use and whether Android evicts the IME or the app you’re typing in.
3. **Physical typing:** try rapid taps, accents, corrections and undo with one hand. Check haptics, edge keys and the language switch on an OEM keyboard picker and in a password-manager field.
4. **Mobile network handoff:** interrupt the model download between cellular and Wi-Fi, then retry. Confirm the smaller model remains usable and the completed model works offline.

The [Studio verification and cleanup log](../build/phase5-proof/studio-verification.txt) and [checksums](../build/phase5-proof/SHA256SUMS) accompany the local build.
