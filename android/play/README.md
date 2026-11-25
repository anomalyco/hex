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

`ly.anoma.hex` · 0.2.0 (version code 3) · Android 12+ · arm64-v8a. The first open-testing release, built from `dev` at `bbebfa75` plus the version bump (#2286). 0.1.1 (version code 2) is on internal testing.

| Size | 0.2.0 bytes | 0.1.0 bytes |
| --- | ---: | ---: |
| Upload-signed AAB | 47,950,821 | 41,192,298 |
| Estimated initial device download, bundletool 1.18.3 | 44,440,314 | 38,337,704 |
| Separate one-time Whisper small download | 190,085,487 | 190,085,487 |

Bundletool reported the same minimum and maximum for an API 36 arm64-v8a, en-US, 440 dpi device spec and for an API 31, 420 dpi one. The model downloaded by Get started is not part of the initial store estimate. A smaller multilingual model is included in the app. AAB SHA-256: `465d077ad7dde0a2cf625de9f1219fea478176377792d0069d66c150849184c1`.

**Policy checks on the signed bundle:**

- Targets API 36, the [new phone-app requirement](https://developer.android.com/google/play/requirements/target-sdk) effective August 31, 2026. R8 and the resource shrinker ran, and the bundle is signed with the upload key. The release DEX has no reference to `android.util.Log`; typing and dictating a sentence on both emulators left none of its words in logcat.
- The merged manifest requests `RECORD_AUDIO` (user-started dictation), `INTERNET` (the model downloads and feedback you send), `VIBRATE` (key haptics) and `ACCESS_NETWORK_STATE` (whether the network is metered, so the 190 MB model waits for Wi-Fi unless the person chooses mobile data). Only `RECORD_AUDIO` asks the person; the others are granted at install. `BIND_INPUT_METHOD` protects the bound IME; it is not a requested permission.
- The external editor’s input connection received **zero context reads** while typing, deleting and pressing the Hex key in text, visible, web and numeric password fields on API 36 and API 31. Only literal writes reached each field, the strip said “Dictation off in password fields” and the microphone stayed off. The same trace counted twelve context reads, and the microphone started, in an ordinary field.
- All seven ELF load segments in `libhexspeech.so` and `libandroidx.graphics.path.so` are 16 KB aligned. BundleConfig requests 16 KB page alignment, and the arm64 and master splits passed `zipalign -c -P 16 4`. The release ran dictation on a guest with 16,384-byte pages.

**Emulator checks of this bundle** (installed from the AAB with bundletool, no phone): on API 36 and API 31 the first run downloaded and verified the model, then in airplane mode “teh” became “the” in another app, an emoji came from the emoji panel, the number pad typed, the phone pad opened, French turned “jai” into “J’ai”, all six typing models offered suggestions, words and shortcuts from Android’s personal dictionary were used, TalkBack’s explore-by-touch typed letters (API 31’s image has no TalkBack, so it was copied from the API 36 image), the licences and privacy pages opened, and one sentence was dictated through the emulator microphone. No crash or ANR. On API 36, installing 0.2.0 over a signed 0.1.1 kept the speech model, the style, the dictation language, the keyboard settings and the selected keyboard.

**Known issue:** on Android 16 (API 36) the model download stops 5 to 6 seconds after Hex goes to the background, for example while you turn on the keyboard in Settings, and the partial file is deleted; a trip of 1 to 3 seconds survives. Setup then offers Retry. It does not happen on API 31.

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
