# Windows Beta

The Windows port is a beta with a native GPUI desktop app, WASAPI capture,
local `transcribe.cpp` transcription, a global shortcut, and generation-safe
automatic paste. Its daily-use surface includes live microphone, model, and
shortcut settings, Hold or Toggle shortcut modes, double-tap lock, Paste Last
Dictation, recording tones, a click-through HUD, retained History, text
replacements, a resident system tray, and per-user Launch at login. Global and
contextual Modes can rewrite completed dictation through the same
deadline-bounded OpenCode processing policy as macOS, then run ordered built-in
or user-defined TypeScript transformations.

The default shortcut is `Ctrl+Space`. In **Hold** mode, hold both keys while
speaking and release Space to transcribe and paste. In **Toggle** mode, press
`Ctrl+Space` to start and again to finish. Only Escape cancels; holding a
modifier on its own does nothing. HEX consumes the chord while it matches, so
applications and input methods no longer see `Ctrl+Space`; rebind it in Settings
if you rely on that chord elsewhere. In Hold mode, tap the shortcut twice within
300 ms to lock recording, then press it again to finish. Paste Last defaults to
`Ctrl+Alt+V`. Settings changes apply live or through an automatic listener
restart.

The Settings card reads **Enabled** when the shortcut works system-wide in every
app and **Disabled** when it does nothing.

## Install

Download and run the installer for your PC:

- x64: [`HEX-Setup-2.3.1-x64.exe`](https://downloads.hex.kitlangton.com/releases/HEX-Setup-2.3.1-x64.exe)
- ARM64: [`HEX-Setup-2.3.1-arm64.exe`](https://downloads.hex.kitlangton.com/releases/HEX-Setup-2.3.1-arm64.exe)

[hex.kitlangton.com](https://hex.kitlangton.com/) picks the right one. The
installer needs no administrator rights: it installs for the current user,
adds Hex to the Start menu and to **Settings › Apps** for uninstalling, and
opens Hex, whose first run downloads a speech model. Neither the installer nor
`hex.exe` is code-signed yet, so SmartScreen may say it protected your PC;
choose **More info**, then **Run anyway**.

**x64** decodes on the GPU through Vulkan and needs a current NVIDIA, AMD, or
Intel graphics driver (it supplies `vulkan-1.dll`); the installer stops with an
explanation when it is missing. **ARM64** decodes on the CPU. Both are
self-contained: no Visual C++ Redistributable is required. Hex then keeps
itself current through the signed update feed. Uninstalling removes every
installed version, the shortcut, and Launch at login, and keeps settings, logs,
history, and models in `%APPDATA%\voice-control`.

The installer and the script below create the same managed layout, so either
can replace the other. For a scripted install:

```powershell
powershell -ExecutionPolicy Bypass -Command "iwr https://downloads.hex.kitlangton.com/install-windows.ps1 -OutFile install-windows.ps1; ./install-windows.ps1"
```

Uninstall a script install with `install-windows.ps1 -Uninstall`.

## Streaming Dictation

With Parakeet Unified English on a GPU, HEX streams audio into the model while
the shortcut is held, so release only decodes the last one to two seconds. The
same engine streams on macOS (Metal) and Linux (Vulkan). Each dictation logs
`source=stream` or `source=fallback` in `logs\process.log`; a stream that fails,
stalls past its deadline, or returns nothing falls back to transcribing the
whole clip. CPU devices (including ARM64) always use the whole-clip path. Set
`HEX_STREAMING=0` to force the whole-clip path. See
[dictation](features/dictation.md#streaming-finish).

## Requirements For Source Builds

- 64-bit Windows 10 or 11, x64 or ARM64.
- PowerShell 7 or Windows PowerShell 5.1.
- `winget` from Microsoft App Installer.
- Visual Studio C++ Build Tools, CMake, and the stable MSVC Rust toolchain.
- x64: the Vulkan SDK (`VULKAN_SDK` set), for the GPU backend.
- ARM64: Visual Studio's clang-cl and Ninja; ggml's ARM CPU backend refuses
  MSVC. Build with `CC=clang-cl`, `CXX=clang-cl`, `CMAKE_GENERATOR=Ninja`, and
  `TRANSCRIBE_CMAKE_ARGS=-DGGML_NATIVE=OFF -DGGML_OPENMP=OFF`, as
  `.github/workflows/check-windows.yml` does.
- A Windows 10 or 11 SDK containing the DirectX `fxc.exe` shader compiler.
- Bun is optional. It is required only for user-defined TypeScript
  transformations; the built-in Lowercase and SpongeBob case transformations
  run without it.

`.cargo/config.toml` links the C and C++ runtimes statically. Distributed builds
also set `TRANSCRIBE_CMAKE_ARGS=-DGGML_NATIVE=OFF`; without it ggml tunes its
CPU kernels to the build machine and the binary can crash on older CPUs.

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

The Settings card reads **Enabled** when the shortcut works system-wide in every
app and **Disabled** when it does nothing; its button and the tray's
**Enable / Disable Hex** switch between them. HEX enables itself on launch by
default (**Enable on launch**). The microphone records only while you dictate.
Changing the
transcription model, microphone, dictation shortcut, double-tap behavior, or
Paste Last setting automatically restarts the listener; you do not need to
stop it first. Text replacements and feedback volume apply without a model
reload. Mode rules, OpenCode profiles, and ordered transformations are
snapshotted by the output worker and apply to the next completed dictation
without restarting capture. The command-line listener uses the settings saved
by the app:

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

## Custom Transformations

The Transformations card in Modes always offers the built-in transformations.
Choose **Set up** there to create `~/.config/hex/hex.config.ts`, install the
managed `@hex/commands` SDK with Bun, and load any transformations registered by
that config. The Windows release executable embeds the checked-in SDK source,
so a managed single-executable install does not depend on a repository checkout.

Transformations run in the displayed order after mode corrections and the
optional OpenCode rewrite. Reordering, adding, or removing them persists in the
global or contextual mode and applies to the next output job. Host startup,
registration, IPC, execution time, and status files are bounded; a failed step
preserves the previous pipeline output. Use **Retry** in the card after fixing a
Bun, dependency, or config error. Literal voice commands in the same config
remain unavailable on Windows until a real streaming command recognizer lands.

## Local Transcription Service

Not available on Windows in this build. The `windows-shell` branch generalized
the authenticated local API (`service` and `service --embedded`) across
platforms, but `main` has since reshaped it for macOS; porting it onto that
version is follow-up work.

## macOS Parity

The target is product and visual parity with macOS, using native Windows
adapters where the operating-system APIs differ. A direct copy of AppKit,
CoreAudio event taps, Metal, `SMAppService`, Accessibility, or
ScreenCaptureKit code would not run on Windows; the Windows implementation must
preserve the same user contract through Win32, WASAPI, GPUI/DirectX, the
current-user startup registry, UI Automation, and Windows capture APIs.

| Area | Windows status |
| --- | --- |
| Local hold-to-dictate, Toggle mode, timestamped boundaries, Escape, paste | Implemented |
| Live model, shortcut, and microphone selection | Implemented |
| Double-tap lock and Paste Last Dictation | Implemented |
| Clipboard preservation and generation-safe restoration | Implemented |
| Recording tones and click-through recording/processing HUD | Implemented; the renderer is native GPUI/DirectX rather than the macOS Metal shader |
| History retention, list, detail, search, copy, delete, and clear | Implemented |
| Text replacements | Implemented in the Windows Modes pane using the shared phrase-boundary replacement engine |
| Application picker in Modes | Implemented; lists Start menu shortcuts, App Paths, and apps with an open window, each with its executable's icon |
| Interface language | Implemented on every OS; English, Polish, Chinese, Japanese, German, and Spanish |
| Resident tray and Launch at login | Implemented |
| Application modes with per-mode corrections, OpenCode rewriting, and transformations | Implemented on the shared ordered processing policy; global and contextual profiles persist their own corrections, model, reasoning variant, deadline, and ordered built-in/TypeScript transformations, while modes match by application name — physical end-to-end paste/UI validation remains |
| Voice Action with selected-text capture and in-app OpenCode model selection | Implemented; selection is read through a clipboard round trip rather than UI Automation |
| Recognition hints for Whisper-family models | Implemented |
| Release microphone while idle | Implemented; mutually exclusive with audio pre-roll, as documented in Settings |
| While-dictating audio control (mute other apps or pause media) | Implemented via WASAPI session volumes and GSMTC |
| Signed automatic self-update for managed installs | Implemented for x64 and ARM64; the same ed25519 key and `downloads.hex.kitlangton.dev` origin as Linux, one feed per architecture, activating into a user-local versions directory with a restart button. Source builds keep the GitHub release link |
| Web-domain mode rules and browser context via UI Automation | Implemented; the page URL comes from the browser's UIA document element, bounded so a hung provider degrades to application-only context |
| Opt-in voice Commands | Missing; blocked on a Windows streaming command model |
| Managed installer with Start Menu integration and clean uninstall | Implemented as `scripts/install-windows.ps1`; it creates the self-updating versions layout, and updates retarget the managed shortcut and Launch-at-login entry |
| First-run onboarding | Implemented; a fresh install picks a dictation language, downloads its recommended model with live progress, and starts listening only after the dialog closes — shared with the Linux shell |
| Activity pane with the live session and recent transcripts | Implemented; one shared read-only pane on both port shells |
| Developer Meetings, live drafts, and meeting paste | Missing |
| Local transcription API and public TypeScript SDK host lifecycle on Windows | Missing; see Local Transcription Service |
| Streaming finish with whole-clip fallback | Implemented on GPU devices with Parakeet Unified English |

Settings live in `%APPDATA%\voice-control\settings.json`, the same file macOS and
Linux use. On first launch, an existing `windows-settings.json` is migrated once
and kept as `windows-settings.json.migrated`.

The Windows window uses the shared design system
([design-system.md](design-system.md)): the same tokens, components, and text
input as macOS, with Segoe Fluent Icons glyphs for inline icons. The sidebar
runs the full window height as on macOS; the caption row above the content
holds the window buttons, and the sidebar's part of it holds the app name.
Mode processing, OpenCode rewriting, Voice Action, and transformations run
through the same `dictation_processor` and `personal_commands` modules as
macOS; Windows reaches OpenCode through the `opencode2 api` CLI. Like macOS,
an unset OpenCode model tries the ranked recommended models before the
OpenCode default.
Remaining panes should be added only with their complete Windows behavior;
empty look-alike screens do not count as parity.

## Current Port Boundary

The Windows listener preserves physical press/release timestamps, keeps audio
capture off the transcription and paste worker, and restores the previous
clipboard only when no newer clipboard change supersedes it. The optimized
MSVC release build is verified with GPUI's DirectX shaders; `check-windows`
builds and tests x64 and ARM64 on every pull request. The remaining code
parity slice is the developer-only Meetings surface after the physical matrix
is green; opt-in Commands wait on a Windows streaming command model. Shared
first-run onboarding, complete OpenCode mode rewriting controls, and the bounded
TypeScript transformation host already ship. Windows releases are built for
x64 and ARM64 by
[`release-desktop.yml`](../.github/workflows/release-desktop.yml) with
[`scripts/release-windows.sh`](../scripts/release-windows.sh), which signs
the update feeds with the same release key as Linux and publishes
[`scripts/install-windows.ps1`](../scripts/install-windows.ps1). Users install
with the command under [Install](#install).
