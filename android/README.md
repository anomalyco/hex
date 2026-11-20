# Hex for Android

Hold the Hex key, speak and release. The keyboard inserts the result in the app you’re already using. A tap starts hands-free dictation; tap again to finish, or choose Cancel.

## How it works

- **App:** Kotlin and Jetpack Compose. Welcome → microphone permission and Android’s keyboard settings/picker → a real practice field. Setup reads the current system state and remembers the page, not what you typed.
- **Keyboard:** [HexKeyboard](app/src/main/java/ly/anoma/hex/keyboard/HexKeyboard.kt) is an `InputMethodService` with native key views. Letters, shift/caps lock, symbols, delete, editor actions and next keyboard work through Android’s input APIs. Dictation commits through the active `InputConnection`. Changing editors or hiding the keyboard discards pending results. Password fields allow typing only.
- **Audio:** [MicCapture](app/src/main/java/ly/anoma/hex/speech/MicCapture.kt) opens a 16 kHz mono `AudioRecord` inside the IME. It releases the mic before CPU inference and never requests audio focus or changes the playback route. Audio stays in memory; the APK has no Internet permission.

## Recognizer choice

**Bundled [Whisper tiny](https://huggingface.co/ggerganov/whisper.cpp/tree/5359861c739e955e79d9a303bcbc70fb988958b1), Q5_1, with [whisper.cpp 1.9.5](https://github.com/ggml-org/whisper.cpp/tree/v1.9.5).** The 32.2 MB model works on the first launch without a download or an OEM speech service. Gradle and CMake verify pinned SHA-256 digests at build time. OpenAI [licenses its code and model weights under MIT](https://github.com/openai/whisper#license); whisper.cpp/ggml are also MIT. The APK includes their [notices](app/src/main/assets/licenses/), Apache 2.0 for Kotlin/AndroidX and the LLVM runtime terms.

**Alternative:** Android’s [on-device recognizer](https://developer.android.com/reference/android/speech/SpeechRecognizer) needs no bundled weights or paid API. The API 35 Google image had English installed and recognized “The coffee is still warm” offline without taking music focus. Availability, installed languages and focus behavior depend on the provider. Revisit as an optional engine after testing real phones; use `isOnDeviceRecognitionAvailable` and `createOnDeviceSpeechRecognizer`. [`EXTRA_PREFER_OFFLINE`](https://developer.android.com/reference/android/speech/RecognizerIntent#EXTRA_PREFER_OFFLINE) alone is not a privacy guarantee.

## Build and run

Use the Studio’s JDK 21, SDK platform/build-tools 35, NDK 27.2.12479018 and CMake 3.22.1. From the synced `android/` directory, with an ARM64 emulator or phone running:

```sh
export JAVA_HOME="/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
export ANDROID_HOME="$HOME/Library/Android/sdk"
export PATH="$ANDROID_HOME/platform-tools:$JAVA_HOME/bin:$PATH"
./gradlew assembleDebug
./gradlew test
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n ly.anoma.hex/.MainActivity
```

On the headless Studio, start an ARM64 AVD with `-audio none -grpc 8564 -grpc-use-token`. While Hex is listening, send a mono PCM16 WAV through the emulator’s authenticated [injectAudio API](https://android.googlesource.com/platform/external/qemu/+/refs/heads/emu-master-dev/android/android-grpc/python/samples/src/audio/inject_audio.py) in real time. The null host backend supplies a clock even without a physical input device. This exercises the real guest microphone path.

## Phase 1 limits

Android 12+ (API 31) gives a consistent microphone privacy indicator/toggle and supports the strict platform recognizer for later comparison. This build targets API 35 and ARM64; recognition and layout are English, with a 60-second cap per utterance. The Studio emulator verified offline insertion into a separate app while its music kept playing. Real-phone accuracy, latency and Bluetooth routing are unmeasured. Styles, translation, typing assistance, additional layouts, settings and experiments are later phases.
