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
| History retention, list, detail, search, copy, delete, and clear | Implemented |
| Text replacements | Implemented in the Windows Modes pane using the shared macOS replacement engine |
| Resident tray and Launch at login | Implemented |
| Application modes with per-mode corrections | Implemented; modes match by application name — web-domain rules, OpenCode rewrite, and TypeScript transformations remain |
| Voice Action with selected-text capture and in-app OpenCode model selection | Implemented; selection is read through a clipboard round trip rather than UI Automation |
| Recognition hints for Whisper-family models | Implemented |
| Release microphone while idle | Implemented; mutually exclusive with audio pre-roll, as documented in Settings |
| While-dictating audio control (mute other apps or pause media) | Implemented via WASAPI session volumes and GSMTC |
| Signed automatic self-update for managed installs | Implemented; the same ed25519 feed contract as Linux, activating into a user-local versions directory with a restart button — source builds keep the GitHub release link |
| Web-domain mode rules and browser context via UI Automation | Implemented; the page URL comes from the browser's UIA document element, bounded so a hung provider degrades to application-only context |
| Opt-in voice Commands | Missing; blocked on a Windows streaming command model |
| Managed installer with Start Menu integration and clean uninstall | Implemented as `scripts/install-windows.ps1`; it creates the self-updating versions layout, and updates retarget the managed shortcut and Launch-at-login entry |
| First-run onboarding | Implemented; a fresh install picks a dictation language, downloads its recommended model with live progress, and starts listening only after the dialog closes — shared with the Linux shell |
| Activity pane with the live session and recent transcripts | Implemented; one shared read-only pane on both port shells |
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
slices are OpenCode mode rewriting with TypeScript transformations, the local
transcription API host, and the developer-only Meetings surface; opt-in Commands
wait on a Windows streaming command model. Shared first-run onboarding already
ships. Windows releases are prepared and published with
[`scripts/release-windows.sh`](../scripts/release-windows.sh), which signs
the update feed with the same release key as Linux and publishes
[`scripts/install-windows.ps1`](../scripts/install-windows.ps1); users
install with:

```powershell
powershell -ExecutionPolicy Bypass -Command "iwr https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/install-windows.ps1 -OutFile install-windows.ps1; ./install-windows.ps1"
```
