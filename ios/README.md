# Hex for iPhone

**Status:** Internal TestFlight app (`HEXMobile`), custom keyboard extension
(`HEXKeyboard`), and Live Activity + Control Center widget extension
(`HEXWidgets`). The containing app records a generation-scoped temporary WAV,
transcribes on-device with Apple Speech (`SpeechTranscriber` on iOS 26+) or
FluidAudio Parakeet V2 (iOS 18+), and removes each capture's file after
inference, cancellation, or failure. The keyboard extension sends commands and
inserts completed text over the shared App Group (`group.com.kitlangton.hex.mobile`);
it never accesses the microphone, speech model, or network.

## Capabilities

- **One-tap keyboard dictation (`HEXKeyboard`):**
  - **Setup:** The keyboard needs Full Access to reach Hex through the App
    Group. Without it, the keyboard shows the Settings path and an **Open Hex
    setup** button (`hex-dictation://keyboard/setup`). The app's setup card
    shows live status for the keyboard, Full Access, the microphone, and the
    speech model, and opens Hex's page in Settings. Full Access shows as on
    once the keyboard has run with it (`keyboard.fullAccessSeenAt`).
  - **Opening Hex:** SwiftUI `Link`, `openURL`, and `extensionContext.open`
    do nothing in a keyboard extension, so the keyboard opens Hex through
    the `UIApplication` at the end of its responder chain
    (`ContainingAppOpener`).
  - **Cold start:** Tapping **Start Hex** opens the containing app
    (`hex-dictation://keyboard/record?...`) with a 10-second
    handoff window, starts recording, and opens the selected host app's URL
    scheme (`messages://`, `slack://`, `claude://`, `chatgpt://`,
    `mobilenotes://`, `discord://`, `mail://`, `x-web-search://`, etc.) using
    public `UIApplication.shared.open(_:)`. Because iOS keyboard extensions
    cannot inspect the host app's bundle ID without private APIs, you pick your
    usual return app once on the `↩` chip (or jump bar), and apps without a
    registered URL scheme fall back cleanly within ~1s to the bottom-edge swipe
    bar and quick-jump chips.
  - **Warm session (`Off`, `5m`, `15m`, `1h`):** Keeps an `AVAudioEngine` input
    session alive for the chosen duration (default `15m`) so subsequent taps on
    **Dictate** record directly inside the keyboard with zero app switches. The
    iOS orange microphone indicator stays visible while warm, and you can end
    the warm session anytime from the app header.
  - **Styles:** `Polished` (sentence case + punctuation + filler trim),
    `Casual` (lowercase chat without trailing period), `Notes` (spoken
    `"bullet"` / `"next line"` markers), and `Verbatim`, with cursor-aware
    insertion spacing and repeating backspace.
- **Resilient capture & expiry contract (`DictationController`):**
  - Monotonic `jobGeneration`, retained `transcriptionTask`, and unique
    per-capture WAV filenames ensure an expiring warm session never drops an
    already-accepted transcription (deliverable for 30s), while a stale
    winding-down inference can never overwrite a newer capture or delete its
    audio file.
  - Phone/FaceTime/Siri audio interruptions finalize captures `>= 300 ms` so
    spoken words are not lost, cancel shorter taps cleanly out of `Listening`,
    and resume warm keepalive when iOS signals `.shouldResume`. Bluetooth HFP
    input is opt-in (**Use Bluetooth headphones**, default off) so AirPods stay
    in full-fidelity output by default.
- **Action Button, Control Center, Back Tap & Live Activity:**
  - `DictateWithHexIntent` (`AudioRecordingIntent` + `LiveActivityIntent`) and
    `HexDictationControl` (`ControlWidget`) start recording on first press with
    a Dynamic Island / Lock Screen Live Activity (`Stop` and `Cancel` buttons)
    and transcribe + copy to the clipboard on second press.

## Build & Test

Prerequisites: Xcode 26+ and [XcodeGen](https://github.com/yonaskolb/XcodeGen) 2.46.0+.

```sh
cd ios
xcodegen generate
xcodebuild test -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=<SIM_UDID>"
```

## Simulator vs. Physical iPhone Proof Limits

- **Verified in Simulator (`HEXMobileTests` + `--snap-state` scenes):** Compilation
  across `HEXMobile`, `HEXKeyboard`, and `HEXWidgets`; App Group command/snapshot
  framing; the `DictationController` generation/expiry race and 30s post-close
  transcript delivery; 10s handoff window + URL-scheme auto-return fallback;
  interruption handling; deterministic style formatting; and UI layouts.
- **Requires a physical iPhone:** Real `AVAudioSession` background keepalive
  survival across lock/app switches, live orange indicator behavior, on-device
  `SpeechTranscriber` / Neural Engine Parakeet latency, URL-scheme return into
  installed third-party apps (Messages, Slack, Claude, ChatGPT), Dynamic Island
  Live Activity interaction, and hardware Action Button invocation.
