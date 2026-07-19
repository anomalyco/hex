# HEX for iOS

This is the native iOS transcription prototype. It records a temporary local
WAV, transcribes it on-device with Apple Speech or the FluidAudio Parakeet V2
fallback, and deletes the audio after inference.

Generate and open the Xcode project with:

```sh
cd ios
xcodegen generate
open HEXMobile.xcodeproj
```

On iOS 26, HEX prefers Apple's on-device `SpeechTranscriber`; its language asset
is installed and maintained by iOS. HEX selects the first supported locale from
the user's ordered system languages, then falls back to the current locale and
English. Earlier or unsupported devices retain the
FluidAudio Parakeet V2 fallback. The progress bar covers asset download and
model preheating.

To use dictation in another app:

1. Add **HEX Keyboard** under **Settings > General > Keyboard > Keyboards**.
2. Turn on **Allow Full Access** for HEX. Apple requires this before a keyboard
   can access its containing app's private App Group; HEX does not use that
   permission for network access.
3. In any text field, choose the HEX keyboard and tap **Open HEX**. HEX prepares
   local speech and starts the 15-minute session automatically.
4. Swipe right along the bottom edge to return to the previous app, then tap
   **Record** in the HEX keyboard.
The containing app owns microphone capture and speech inference. The keyboard
extension only sends bounded record/stop jobs and inserts completed text through
`UITextDocumentProxy`. The orange microphone indicator remains visible while a
keyboard session is active. Microphone capture and useful inference require a
physical iPhone; the simulator build verifies compilation and UI only.

## TestFlight review notes

HEX requires no account or sign-in. To test keyboard dictation:

1. Launch HEX once and allow Microphone and Speech Recognition access.
2. Add **HEX Keyboard** under **Settings > General > Keyboard > Keyboards**.
3. Enable **Allow Full Access**. iOS requires this before the keyboard can use
   the private App Group shared with its containing app. The keyboard extension
   does not access the network, microphone, or speech models.
4. Open any text field, switch to HEX Keyboard, and tap **Open HEX**.
5. Wait for HEX to report that it is ready, swipe right along the bottom edge
   to return, then tap **Record** in the keyboard.
6. Tap **Stop and insert**. HEX transcribes on-device and inserts the result in
   the active text field.
