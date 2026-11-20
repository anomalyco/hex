# Hex for Android

Hold the Hex key, speak and release to insert text in the current app. A tap starts hands-free dictation; ✓ finishes and ✕ discards. The live dot waveform stays above the keys. While Hex transcribes, a spinner takes the checkmark’s place and ✕ still cancels.

## Speech model and privacy

**[Whisper small Q5_1](https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin) is the default; multilingual tiny Q5_1 is bundled and works from first launch in airplane mode.** The [Android benchmark](benchmark/README.md) compares tiny, base, small, Parakeet and Android’s strictly on-device provider on the same 40 clips. Hex’s small configuration reduced WER from 17.54% to 5.26% and used 344 MiB peak native RSS on short clips. Norwegian remains the weakest language.

Welcome explains that the internet is used only to fetch the 190 MB speech model. Get started begins a fixed HTTPS download; setup shows progress and lets you continue with tiny or retry. [ModelStore](app/src/main/java/ly/anoma/hex/speech/ModelStore.kt) verifies the exact byte count and SHA-256 before atomically installing it in private, non-backed-up storage. The keyboard never makes a network request. Missing or unusable small weights fall back to tiny. Voice, words and translations stay on the device; Hex has no telemetry, transcript history or network speech service.

## Keyboard and styles

**Automatic** uses the first supported Android system language, or English if none is supported. Hold the language-labelled space bar to switch immediately between Automatic, English, Español, Deutsch and Norsk (Bokmål). Swipe up on the Hex key or tap **Hex menu** to change the spoken language or style:

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

After setup, Home shows **Ready** or **Finish setup**, a **Dictate** bar and four destinations, following the iOS navigation in #2054. Tap Dictate, speak, then Finish dictation: Hex displays the local result on Home. **Dictation** controls style, language, translation, cleanup and the offline speech model. **Keyboard** has layout, autocorrect, key pop-ups, haptics and a practice field. **Privacy** explains storage and links the model credits. **Experiments › Spatial autocorrect** controls tap-based correction; it starts on to preserve existing Android behavior. Turning it off keeps dictionary completions and the fixed English typo/contraction rules; turning off Keyboard › Autocorrect disables all automatic replacements.

**Privacy › Privacy policy** opens the [Android policy text](play/policy/privacy-policy.txt) even in airplane mode.

## Phase 4 verification

On identical held-out synthetic taps and **unchanged English and Spanish iOS models**, Android’s English word error rate fell from 10.53% to **7.40%**, against **8.39%** for iOS R1 without offsets. Spanish was **10.33%** against **10.43%** for iOS (phase 3 Android: 10.06%). The [step-by-step benchmark](benchmark/typing/README.md#where-the-298-english-errors-came-from) explains the original 298 extra English errors; [the replay table](benchmark/typing/phase4.tsv) gives raw counts, exact sentences and timing.

The Studio verified Spanish, German and Bokmål suggestions and space-triggered corrections in a separate Android app, including **¿¡**, **ß** and **æøå**. The German and Bokmål assets were also copied, byte-for-byte, into the iOS typing-model folder in local commit `230a437e`. The production Swift reader checked all 95,000 words per language and every stored pair score; no iOS source or existing English/Spanish model changed.

With airplane mode on and Wi-Fi off, fixed audio went through the emulator microphone and Whisper small into that editor. Spanish **“Nos vemos mañana en la estación”** and German **“Wir treffen uns morgen am Bahnhof”** matched. Bokmål **“Tusen takk for hjelpen”** also matched; on corpus clip 40 Hex inserted **“Biblioteket stenger tydelig på lørdag”** instead of **“Biblioteket stenger tidlig på lørdag.”** The same Spanish clip also produced a transcript from Home’s Dictate bar.

Five unit tests passed in each of the debug and release variants. Home, the four settings pages and all four detail pages were captured in light and dark. Deselecting Hex as the current keyboard changed **Ready** to **Finish setup · 1 step**; selecting it again through setup restored **Ready**. The local APK, full-size screenshots, replay results and Studio log are under `build/phase4-proof/`.

## How it works

- **App:** Kotlin and Jetpack Compose. Welcome → microphone permission and Android’s keyboard settings/picker → a real practice field. Setup reads live system state; it remembers the page, not the words.
- **Keyboard:** [HexKeyboard](app/src/main/java/ly/anoma/hex/keyboard/HexKeyboard.kt) is an `InputMethodService` with native key views. Letters, shift/caps lock, symbols, delete, editor actions and next keyboard use Android input APIs. It commits through the active `InputConnection`; changing editors or hiding the keyboard discards pending results. Password fields allow typing only.
- **Audio:** [MicCapture](app/src/main/java/ly/anoma/hex/speech/MicCapture.kt) records up to 60 s of mono 16 kHz PCM in RAM and releases the microphone before CPU inference. The [recognizer interface](app/src/main/java/ly/anoma/hex/speech/Whisper.kt) operates on that capture and its language/task. Hex requests no audio focus and does not change playback routing.
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

**Scope:** Android 12+ (min API 31), target 36, ARM64, four typing models and four dictation languages. See the [Play release evidence, listing drafts, phone screenshots and real-device checklist](play/README.md). The Studio also exercised offline fallback, all four styles, per-dictation translation, both appearances and insertion into a separate app with music playing in earlier phases.
