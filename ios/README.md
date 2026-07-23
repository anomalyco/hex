# HEX For iOS

**Status:** Native prototype, not the distributed macOS product. The containing
app records a temporary WAV, transcribes on-device with Apple Speech or
FluidAudio Parakeet V2, and removes the file after handled inference,
cancellation, or failure. A crash can leave a file in the app's temporary
directory until iOS removes it. The keyboard extension requests jobs and
inserts completed text; it does not own the microphone or speech model.

## Build On A Physical iPhone

Useful microphone and inference testing requires a physical iPhone. The
simulator verifies compilation and UI only.

Prerequisites:

- Xcode 26 or newer.
- [XcodeGen](https://github.com/yonaskolb/XcodeGen).
- An Apple development team that can sign an app and keyboard extension.

Generate the project:

```sh
cd ios
xcodegen generate
open HEXMobile.xcodeproj
```

Before running on a device, replace the checked-in development team and bundle
identifiers in `project.yml`. Create an App Group in your Apple developer account
and use the same identifier in both app and keyboard entitlements.

On iOS 26, HEX prefers Apple's on-device `SpeechTranscriber`. iOS installs and
maintains its language asset. Earlier or unsupported devices use the FluidAudio
Parakeet V2 fallback. The progress bar covers asset download and model preheat.

## Test The Keyboard

1. Launch HEX once and grant Microphone and Speech Recognition access.
2. Add **HEX Keyboard** under **Settings > General > Keyboard > Keyboards**.
3. Enable **Allow Full Access**. iOS requires this for the keyboard extension to
   use the private App Group shared with its containing app. The keyboard
   extension does not use Full Access for network, microphone, or model access.
4. Open a text field, switch to HEX Keyboard, and tap **Open HEX**.
5. HEX prepares a 15-minute keyboard session and may immediately start the
   pending recording. Return to the previous app with the bottom-edge gesture.
6. If the keyboard says **Recording**, tap **Stop and insert**. If it says
   **Ready**, tap **Record** first.

The orange microphone indicator remains visible while the containing app owns
an active keyboard session.

## App Review Notes

HEX requires no account or sign-in. To review keyboard dictation, follow the
steps above. Full Access is required only for the App Group bridge between the
keyboard and containing app. Speech inference stays on-device, the keyboard
extension does not access the microphone or network, and HEX removes temporary
recording audio after each handled job.
