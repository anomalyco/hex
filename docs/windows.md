# Windows Alpha

The Windows port is a source-build alpha with a native GPUI desktop app,
WASAPI capture, local `transcribe.cpp` transcription, a global push-to-talk
shortcut, and generation-safe automatic paste. Its implemented daily-use
surface now includes live microphone/model/shortcut settings, double-tap lock,
Paste Last Dictation, recording tones, a click-through HUD, retained History,
text replacements, a resident system tray, and per-user Launch at login.

The default shortcut is `Ctrl+Win`: hold both keys while speaking and release
either one to transcribe and paste. Tap the shortcut twice within 300 ms to
lock recording, then press it again to finish; Escape cancels. Paste Last
defaults to `Ctrl+Alt+V`. Settings changes are accepted while listening and are
applied live or through an automatic listener restart as appropriate.

## Requirements

- 64-bit Windows 10 or 11.
- PowerShell 7 or Windows PowerShell 5.1.
- `winget` from Microsoft App Installer.
- Visual Studio C++ Build Tools, CMake, and the stable MSVC Rust toolchain.
- A Windows 10 or 11 SDK containing the DirectX `fxc.exe` shader compiler.

Run the checked-in setup script from the repository root. It keeps an existing
compatible Visual Studio installation and installs only missing prerequisites.

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\setup-windows.ps1
```

The script also discovers the installed Windows SDK, persists `GPUI_FXC_PATH`
for GPUI release builds, installs `rustfmt`, Clippy, and rust-analyzer, fetches
Cargo dependencies, and runs `cargo check --all-targets`.

## Smoke Test

List WASAPI inputs and the compute backends visible to `transcribe.cpp`:

```powershell
cargo run -- devices
cargo run -- model devices
```

Install the default English model, load it, and transcribe a WAV file:

```powershell
cargo run -- model install
cargo run -- model check
cargo run -- model transcribe .\sample.wav
```

For Polish, select the multilingual Parakeet model explicitly:

```powershell
cargo run -- model install --model parakeet_v3 --language pl
cargo run -- model check --model parakeet_v3 --language pl
cargo run -- capture --model parakeet_v3 --language pl --copy
```

Open the desktop application after the model is installed:

```powershell
cargo run -- app
```

The app starts its global listener on launch by default. Changing the
transcription model, microphone, dictation shortcut, double-tap behavior, or
Paste Last setting automatically restarts the listener; you do not need to
stop it first. Text replacements and feedback volume apply without a model
reload. The command-line listener uses the settings saved by the app:

```powershell
cargo run -- listen --model parakeet_v3 --language pl
```

`capture` prewarms the selected model and opens the microphone before prompting.
Press Enter once to start, then again to stop. Captures shorter than 300 ms are
discarded. `--device` accepts a case-insensitive fragment from `devices`.

Runtime data is stored under `%APPDATA%\voice-control`. Observations are in
`logs\live.ndjson`, and native/runtime diagnostics are in `logs\process.log`.
Successful pasted dictations are retained, subject to the selected retention
window and hard size limits, in `history.json`. Captured audio is never retained
by default.

## macOS Parity

The target is product and visual parity with macOS, using native Windows
adapters where the operating-system APIs differ. A direct copy of AppKit,
CoreAudio event taps, Metal, `SMAppService`, Accessibility, or
ScreenCaptureKit code would not run on Windows; the Windows implementation must
preserve the same user contract through Win32, WASAPI, GPUI/DirectX, the
current-user startup registry, UI Automation, and Windows capture APIs.

| Area | Windows status |
| --- | --- |
| Local hold-to-dictate, timestamped boundaries, Escape, paste | Implemented |
| Live model, shortcut, and microphone selection | Implemented |
| Double-tap lock and Paste Last Dictation | Implemented |
| Clipboard preservation and generation-safe restoration | Implemented |
| Recording tones and click-through recording/processing HUD | Implemented; the renderer is native GPUI/DirectX rather than the macOS Metal shader |
| History retention, list, detail, copy, delete, and clear | Implemented; text search still needs the Windows field wiring |
| Text replacements | Implemented in the Windows Modes pane using the shared macOS replacement engine |
| Resident tray and Launch at login | Implemented |
| Full mode profiles, corrections, OpenCode rewrite, and TypeScript transformations | Missing |
| Voice Action and selected-text context | Missing; requires a Windows UI Automation adapter |
| Opt-in voice Commands and application/browser context | Missing |
| Release-microphone-while-idle and bounded in-process stream recovery | Missing |
| Output muting, media pause/resume, and idle-sleep ownership | Missing |
| Packaged onboarding, signed installer, and automatic updates | Missing |
| Developer Meetings, live drafts, and meeting paste | Missing |
| Local transcription API and public TypeScript SDK host lifecycle on Windows | Missing |

The shared visual tokens, navigation, pane scaffold, transcription picker,
History presentation, and text input are already reused. Remaining panes should
be added only with their complete Windows behavior; empty look-alike screens do
not count as parity.

## Current Port Boundary

The Windows listener preserves physical press/release timestamps, keeps audio
capture off the transcription and paste worker, and restores the previous
clipboard only when no newer clipboard change supersedes it. The optimized
MSVC release build is verified with GPUI's DirectX shaders. The next parity
slices are full Modes processing, Windows foreground/selected-text context,
Voice Action, and opt-in Commands, followed by packaging/updating and the
developer-only Meetings surface.
