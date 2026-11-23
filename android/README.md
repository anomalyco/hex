# Hex for Android

Hold the Hex key, speak and release to insert text in the current app. A tap starts hands-free dictation; ✓ finishes and ✕ discards. The Hex key sits at the strip’s trailing end, where Gboard has its mic; while dictating it becomes ✓ and the live dot waveform grows out of it, above the keys. While Hex transcribes, a spinner takes the checkmark’s place and ✕ still cancels. The keys are Gboard’s size and place on a Pixel (measured on an emulator), with ?123, comma, space, period and enter in the bottom row; the system’s own switcher in the navigation bar, or Hex menu › Next keyboard, changes keyboards.

## Speech model and privacy

**[Whisper small Q5_1](https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin) is the default; multilingual tiny Q5_1 is bundled and works from first launch in airplane mode.** The [Android benchmark](benchmark/README.md) compares tiny, base, small, Parakeet and Android’s strictly on-device provider on the same 40 clips. Hex’s small configuration reduced WER from 17.54% to 5.26% and used 344 MiB peak native RSS on short clips. Norwegian remains the weakest language.

Welcome explains that the internet is used only to fetch the 190 MB speech model and, when you tap Send, to send feedback. Get started begins a fixed HTTPS download; setup shows progress and lets you continue with tiny. The partial file is kept: when Android closes the connection because Hex went to the background, or Hex is closed or the phone restarts, the download continues with an HTTP range request the next time a Hex screen is open. A real failure (no connection, server error, full storage) says so and offers Retry, which also continues. [ModelStore](app/src/main/java/ly/anoma/hex/speech/ModelStore.kt) verifies the exact byte count and SHA-256 before atomically installing it in private, non-backed-up storage. The keyboard never makes a network request. Missing or unusable small weights fall back to tiny. Voice, words and translations stay on the device; Hex has no telemetry, transcript history or network speech service. The app’s only other request is [Send feedback](#send-feedback), made when you tap Send.

## Keyboard and styles

**Automatic** follows a keyboard layout chosen in Keyboard › Keyboard layout, else the first supported Android system language, else English. Hold the language-labelled space bar to switch immediately between Automatic, English, Español, Deutsch, Norsk (Bokmål), Français, Italiano, Nederlands, Português, Polski and Русский. Português is one entry for Brazil and Portugal (Whisper has one `pt`). It, Nederlands, Polski and Русский keep the English layout (Polish letters are held on their base keys; there are no Cyrillic keys yet) and have no cleanup or list rules; Português has spoken punctuation, the other three don’t. Slide on the Hex key or tap **Hex menu** to change the spoken language or style:

- **Polished** (default): trims standalone hesitation sounds, fixes spacing/capitals and closes sentences.
- **Casual:** lowercase texting, without the period at the end of a single statement.
- **Notes:** “bullet point,” “next bullet,” “new line” and “new paragraph” become list items and breaks.
- **Verbatim:** keeps the recognizer’s words and punctuation; no cleanup is applied.

**Clean up what I say** is off by default. The [deterministic rules](app/src/main/java/ly/anoma/hex/speech/SpokenCleanup.kt) remove hesitations and resolve explicit self-corrections against nearby words of the same kind. Spanish, German and Norwegian use the iOS word lists and pause boundaries; English uses explicit markers, not iOS’s learned tagger or an LLM. Ambiguous repairs stay as spoken. Ordinary negation and language-specific words such as German “eh” and Norwegian “er” stay intact.

**Translate what I say** is also off by default. With Español and translation enabled, **Next dictation** offers **Paste in English** or **Paste as spoken**. The choice is captured when recording begins; after a one-off spoken-language insertion, the next dictation returns to English. Translation is multilingual Whisper’s native speech-to-English task, not a second text model or online SDK. The tested Spanish WAV inserted both “El café todavía está caliente.” and “The coffee is still hot.” offline.

## Typing assistance

All four layouts have a bundled, memory-mapped **HXTM v3** word and word-pair model. English and Spanish keep the original iOS Google Books files. German uses Google Books; Bokmål uses the National Library of Norway’s book counts filtered by Norsk ordbank. Start typing to see ranked suggestions; tap one to insert it. A space brings up next-word predictions. Passwords, addresses and editors that disable suggestions use literal typing.

**Keyboard › Keyboard layout** follows dictation by default; an explicit layout can differ from the space bar’s spoken language. Spanish has a dedicated ñ and ¿/¡ on the second symbols page. German is QWERTZ with ü, ö and ä; hold s for ß. Bokmål has å, ø and æ.

**Keyboard › Suggest swear words** is off by default on every layout. Slurs on the
[curated list](app/src/main/assets/typing/never-suggest.tsv) are never offered, even when
swear words are enabled. The filter applies to completions, next words, autocorrect and
both halves of a missed-space correction. Typing a listed word in full keeps your exact
spelling; its literal chip accepts what you typed. The filter never changes the HXTM models.

On space, the Kotlin decoder compares each tap with the measured key centres and combines those distances with the previous word’s probability. It can recover a missing space. A known spelling requires a higher confidence margin than a nonword. Immediately press **Delete** to restore exactly what you typed, without the space. Undo the same replacement twice and Hex keeps that spelling in later sessions. Only rejection counts keyed by hashes are saved locally; there is no typing journal.

**Keyboard settings and Hex menu → Typing feedback** control key pop-ups and key haptics independently. Both default on; Hex follows system keyboard vibration. Hold a letter, slide to an accent, and release. Turning off pop-ups leaves the deliberate accent picker available.

[English/Spanish typing benchmark, model sources and licences](benchmark/typing/README.md)

## Settings

After setup, Home shows **Ready** or **Finish setup**, a **Dictate** bar and four destinations, following the iOS navigation in #2054. Tap Dictate, speak, then Finish dictation: Hex displays the local result on Home. **Dictation** controls style, language, translation, cleanup and the offline speech model. **Keyboard** has layout, autocorrect, key pop-ups, haptics and a practice field. **Privacy** explains storage and links the model credits. **Labs** has an **Output** group with **Signature** (off): each keyboard dictation ends with “Sent from Hex” on its own line. The line is editable, one line of up to 200 characters, and an empty line adds nothing. The next dictation starts on a new line below the signature, and a signed dictation never continues the signature line. Home’s preview and Copy stay unsigned, and Hex’s own fields are never signed. Labs also has a **Keyboard** group with **Symbols on letter keys** (off): hold a letter from QWERTY’s home or bottom row, such as **m**, to type **/** first, with the letter’s accents after it. Symbols follow the letter, so AZERTY **q**/**w** and QWERTZ **y** get none, row 1 (including AZERTY **a**/**z**) keeps its digits, and off, the keyboard is unchanged. **Keyboard › Spatial autocorrect** controls tap-based correction; it starts on to preserve existing Android behavior. Turning it off keeps dictionary completions and the fixed English typo/contraction rules; turning off Keyboard › Autocorrect disables all automatic replacements.

**Privacy › Privacy policy** opens the [Android policy text](play/policy/privacy-policy.txt) even in airplane mode.

## Send feedback

Home’s **Send feedback** opens a form: Problem, Idea, Question or Other (Other sends no category), an optional subject, the message, up to five pictures and an optional email. Nothing is sent until Send. Then [the app](app/src/main/java/ly/anoma/hex/feedback/) makes one multipart `POST https://api.blhp.app/v1/feedback` to Bellhop, Anomaly’s feedback inbox, with `HttpURLConnection` and a hand-built body, following [Bellhop’s checklist](https://blhp.app/docs/feedback.md): upload progress; an `Idempotency-Key` made at the tap and reused for every retry; network failures and 5xx retried with the same body after 1, 2 and 4 s; `409 request_pending` and `429` retried after `Retry-After` (up to a minute), no other 4xx; limits and the email’s shape checked before sending. `401` and `403 feedback_disabled` show feedback as unavailable and log a warning (debug builds; R8 strips every `android.util.Log` call from release and preview).

The first picture is Hex’s own window, drawn with `PixelCopy` just before the form opens, never the screen, the keyboard or another app; Home’s dictation result is painted over first, so no transcript leaves the phone. More come from Android’s photo picker (no permission). Every picture is re-encoded as JPEG at 80% with the longest side at most 2048 px. With an email, the request carries it and a random install ID kept in the app’s preferences (made at the first such send); without one it is anonymous: no email, install ID or user. Metadata is the screen, the channel (`play` or `preview`), the speech model and the dictation language. Dictations, transcripts, recordings, vocabulary and corrections are never sent, and the keyboard has none of this.

The report key is the Gradle property `hexFeedbackKey` (in `gradle.properties`; publishable, it can only send feedback to Hex), surfaced as `BuildConfig.FEEDBACK_KEY`. A build without it has no Send feedback. Debug builds log each request without the key (`adb logcat -s HexFeedback`), and `am start -n ly.anoma.hex/.MainActivity --ez feedback_drop_first_response true` loses the first answer to show the retry returns the same issue.

## Phase 4 verification

On identical held-out synthetic taps and **unchanged English and Spanish iOS models**, Android’s English word error rate fell from 10.53% to **7.40%**, against **8.39%** for iOS R1 without offsets. Spanish was **10.33%** against **10.43%** for iOS (phase 3 Android: 10.06%). The [step-by-step benchmark](benchmark/typing/README.md#where-the-298-english-errors-came-from) explains the original 298 extra English errors; [the replay table](benchmark/typing/phase4.tsv) gives raw counts, exact sentences and timing.

The Studio verified Spanish, German and Bokmål suggestions and space-triggered corrections in a separate Android app, including **¿¡**, **ß** and **æøå**. The German and Bokmål assets were also copied, byte-for-byte, into the iOS typing-model folder in local commit `230a437e`. The production Swift reader checked all 95,000 words per language and every stored pair score; no iOS source or existing English/Spanish model changed.

With airplane mode on and Wi-Fi off, fixed audio went through the emulator microphone and Whisper small into that editor. Spanish **“Nos vemos mañana en la estación”** and German **“Wir treffen uns morgen am Bahnhof”** matched. Bokmål **“Tusen takk for hjelpen”** also matched; on corpus clip 40 Hex inserted **“Biblioteket stenger tydelig på lørdag”** instead of **“Biblioteket stenger tidlig på lørdag.”** The same Spanish clip also produced a transcript from Home’s Dictate bar.

Five unit tests passed in each of the debug and release variants. Home, the four settings pages and all four detail pages were captured in light and dark. Deselecting Hex as the current keyboard changed **Ready** to **Finish setup · 1 step**; selecting it again through setup restored **Ready**. The local APK, full-size screenshots, replay results and Studio log are under `build/phase4-proof/`.

## How it works

- **App:** Kotlin and Jetpack Compose. Welcome → microphone permission and Android’s keyboard settings/picker → a real practice field. Setup reads live system state; it remembers the page, not the words.
- **Keyboard:** [HexKeyboard](app/src/main/java/ly/anoma/hex/keyboard/HexKeyboard.kt) is an `InputMethodService` with native key views. Letters, shift/caps lock, symbols, delete, editor actions and next keyboard use Android input APIs. It commits through the active `InputConnection`; changing editors or hiding the keyboard discards pending results. Password fields allow typing only. Typing lives in [KeyboardEngine](app/src/main/java/ly/anoma/hex/keyboard/KeyboardEngine.kt), which reaches the editor only through `KeyboardTextTarget`; the views are `KeyboardView` (strip, keys or Hex menu), `StripView` and `KeyPlaneView`; each layout's letters are data in `KeyboardLetters`.
- **Audio:** [MicCapture](app/src/main/java/ly/anoma/hex/speech/MicCapture.kt) records up to 60 s of mono 16 kHz PCM in RAM and releases the microphone before CPU inference. The [recognizer interface](app/src/main/java/ly/anoma/hex/speech/Whisper.kt) operates on that capture and its language/task. Hex requests no audio focus and does not change playback routing.
- **Speed:** the model loads when the Hex key goes down, while the person speaks, and stays loaded between dictations; it is freed 2 minutes after the last one or when Android runs short of memory, and nothing runs while it waits. The encoder's window is sized to the speech for dictations up to 8 s (speech plus 3.84 s, at most the earlier 512 frames); longer ones, and the bundled tiny model, keep Whisper's full 30 s window, since every shorter one tried cost accuracy. Inference uses one thread per fast core (at least half the strongest core's `cpu_capacity`), at most four. On the 40-clip benchmark this cut the emulator's median from 0.76 s to 0.56 s with the same word errors (5.26%).
- **Type:** unmodified [InterVariable.ttf](app/src/main/res/font/inter_variable.ttf) from [Rasmus Andersson and the Inter Project, v4.1](https://github.com/rsms/inter/releases/tag/v4.1), licensed under [SIL OFL 1.1](app/src/main/assets/licenses/inter-ofl.txt). One [Compose font family and native TextView helper](app/src/main/java/ly/anoma/hex/HexType.kt) cover Hex’s text, including keys, menus, strip labels and tabular digits (`tnum`). Android supplies missing glyphs; permission dialogs and the system keyboard picker use the OS font. Gradle verifies the bundled TTF’s SHA-256.
- **Licences:** OpenAI Whisper weights and whisper.cpp/ggml are MIT. Kotlin/AndroidX are Apache 2.0; libc++ is Apache 2.0 with LLVM exceptions. [Bundled credits and licence texts](app/src/main/assets/licenses/). No ML Kit, Parakeet or third-party language packs are shipped.

## Build and run

Use the Studio’s JDK 21, SDK platform 36, NDK 27.2.12479018 and CMake 3.22.1. The Play packaging checks use Build Tools 36.0.0 and bundletool 1.18.3. From the synced `android/` directory, with an ARM64 emulator or phone running:

```sh
export JAVA_HOME="/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
export ANDROID_HOME="$HOME/Library/Android/sdk"
export PATH="$ANDROID_HOME/platform-tools:$JAVA_HOME/bin:$PATH"
./gradlew assembleDebug
./gradlew test
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n ly.anoma.hex/.MainActivity
```

On the headless Studio, start an ARM64 AVD with `-audio none -grpc 8566 -grpc-use-token`. [inject.py](benchmark/inject.py) uses the matching emulator discovery record and streams paced mono PCM16 into the guest microphone. Generate the bindings from the SDK emulator’s `lib/emulator_controller.proto` with `grpcio-tools`, then put them on `PYTHONPATH`. No recording or transcript is injected directly into the application.

For the upload bundle, run `./gradlew :app:bundleRelease`. Release packaging requires a private properties file at `~/.config/hex-android/keystore.properties` or the path in `HEX_ANDROID_KEYSTORE_PROPERTIES`. It must contain `storeFile` (relative to that file, or absolute), `keyAlias`, `storePassword` and `keyPassword`. Release uses R8 and resource shrinking and never falls back to the debug key. Gradle writes `app/build/outputs/bundle/release/app-release.aab`.

## Hex Preview (outside Google Play)

Hex Preview is the `preview` build type: the release build as an APK, application id `ly.anoma.hex.preview`, named Hex Preview in the launcher and in Android's keyboard list, so it installs beside Hex from Google Play with its own settings, words and speech model. It is signed with its own key (never the Play upload key), read from `~/.config/hex-android-preview/keystore.properties` or `HEX_ANDROID_PREVIEW_KEYSTORE_PROPERTIES`; every build must be, because Android refuses an update signed with any other. The certificate's SHA-256 is `13:35:68:F6:F0:61:4F:E5:7B:C8:8D:AE:CA:32:AF:A7:2C:6A:B7:40:20:26:29:F7:52:FA:A0:33:BE:35:D9:8E`.

Its updater lives only in `app/src/preview` (code, permissions, strings); debug and release build `app/src/play`'s empty `Updates`, which is inlined away, because Google Play forbids an app updating itself. It reads `latest.json` on the `android-preview` branch when the app opens and at most once a day when the keyboard opens (never in a password field; Settings › Check for updates automatically). An update downloads only after Update is tapped, and goes to `PackageInstaller` only once its size, SHA-256 and signing certificate match.

[`scripts/preview-release.sh`](scripts/preview-release.sh) publishes a build: the build number is the count of commits up to the built commit of `dev`, the version name `<versionName>-preview.<build>`, the tag `android-preview-<build>` (a pre-release, never moved), and the channel branch `android-preview` holds only `latest.json` and the README people land on.

**Scope:** Android 12+ (min API 31), target 36, ARM64, six typing models and ten dictation languages. See the [Play release evidence, listing drafts, phone screenshots and real-device checklist](play/README.md). The Studio also exercised offline fallback, all four styles, per-dictation translation, both appearances and insertion into a separate app with music playing in earlier phases.
