# Windows Beta

The Windows port is a beta with a Rune desktop app (a tray process; see
[The app](#the-app)), WASAPI capture,
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
modifier on its own does nothing. Hex consumes the chord while it matches, so
applications and input methods no longer see `Ctrl+Space`; rebind it in Settings
if you rely on that chord elsewhere. In Hold mode, tap the shortcut twice within
300 ms to lock recording, then press it again to finish. Paste Last defaults to
`Ctrl+Alt+V`. Settings changes apply live or through an automatic listener
restart.

The Settings card reads **Enabled** when the shortcut works system-wide in every
app and **Disabled** when it does nothing.

## Install

Download and run the installer for your PC:

- x64: [`HEX-Setup-2.3.1-x64.exe`](https://anoma.ly/hex/files/releases/HEX-Setup-2.3.1-x64.exe)
- ARM64: [`HEX-Setup-2.3.1-arm64.exe`](https://anoma.ly/hex/files/releases/HEX-Setup-2.3.1-arm64.exe)

[anoma.ly/hex](https://anoma.ly/hex/) picks the right one. The
installer needs no administrator rights: it installs for the current user,
adds Hex to the Start menu and to **Settings › Apps** for uninstalling, and
opens Hex, whose first run downloads a speech model. Releases built once
Anomaly's Azure Artifact Signing is configured are Authenticode-signed
([windows-signing.md](windows-signing.md)). Until then, and while a new
publisher builds SmartScreen reputation, SmartScreen may say it protected your
PC; choose **More info**, then **Run anyway**.

**x64** decodes on the GPU through Vulkan and needs a current NVIDIA, AMD, or
Intel graphics driver (it supplies `vulkan-1.dll`); the installer stops with an
explanation when it is missing. **ARM64** decodes on the CPU. Both are
self-contained: no Visual C++ Redistributable is required. Hex then keeps
itself current through the signed update feed. Uninstalling removes every
installed version, the shortcut, and Launch at login, and keeps settings, logs,
history, and models in `%APPDATA%\hex`.

The installer and the script below create the same managed layout, so either
can replace the other. For a scripted install:

```powershell
powershell -ExecutionPolicy Bypass -Command "iwr https://anoma.ly/hex/files/install-windows.ps1 -OutFile install-windows.ps1; ./install-windows.ps1"
```

Uninstall a script install with `install-windows.ps1 -Uninstall`.

## Streaming Dictation

With Parakeet Unified English on a GPU, Hex streams audio into the model while
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

`.cargo/config.toml` links the C and C++ runtimes statically. Distributed builds
also set `TRANSCRIBE_CMAKE_ARGS=-DGGML_NATIVE=OFF`; without it ggml tunes its
CPU kernels to the build machine and the binary can crash on older CPUs.

Run the checked-in setup script from the repository root. It keeps an existing
compatible Visual Studio installation and installs only missing prerequisites.

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\setup-windows.ps1
```

The script also installs `rustfmt`, Clippy, and rust-analyzer, fetches Cargo
dependencies, and runs `cargo check --all-targets`.

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
**Enable / Disable Hex** switch between them. Hex enables itself on launch by
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

Runtime data is stored under `%APPDATA%\hex`. Observations are in
`logs\live.ndjson`, and native/runtime diagnostics are in `logs\process.log`,
including every panic with its thread and backtrace. A worker that stops
unexpectedly leaves its cause there. The app detaches from its console at
launch, so it has no stdout or stderr.
Successful pasted dictations are retained, subject to the selected retention
window and hard size limits, in `history.json`. Captured audio is never retained
by default.

## Custom Transformations

The Transformations card in Modes always offers the built-in transformations.
Choose **Set up** there to create `~/.config/hex/hex.config.ts` and load any
transformations registered by that config. Hex runs the config in its embedded
JavaScript engine (QuickJS), so nothing else needs installing.

Transformations run in the displayed order after mode corrections and the
optional OpenCode rewrite. Reordering, adding, or removing them persists in the
global or contextual mode and applies to the next output job. Each
transformation call is limited to one second and the engine to 64 MiB; a
failed step preserves the previous pipeline output. Saving the config reloads
it; a config that fails to load keeps the previous one and shows its error in
the card. Literal voice commands in the same config remain unavailable on
Windows until a real streaming command recognizer lands.

## The App

`hex app` (`--hidden` for a login launch) runs the tray app on Rune. Snapshot
fixtures without starting anything: `hex app -- --snap out.png --scene modes`.

```ts
hex app
├── named event already held -> signal it (the running app shows its window) -> exit
└── becomes the app                                   // windows_rune::open
    ├── runtime thread: WindowsDesktopHost (listener, model preparation, OpenCode, transformations, updater), publishes every 250 ms
    ├── main thread: rune::App's winit loop, the window, the tray (rides winit's message pump)
    ├── --hidden -> windowless start; close -> hide (keep_running)
    ├── tray Show or a relaunch -> AppHandle::open (shows and focuses)
    └── tray Quit / Ctrl-C / restart into an update -> AppHandle::quit -> runtime stops the listener
```

- Quit Hex in the window or the tray stops the listener and ends the app;
  closing the window only hides it.
- The window uses the shared Rune panes (General, Modes, Voice Action,
  History, the transcription picker, first-run setup) with Windows rows:
  Launch at login (the current-user Run key), no terminal-paste row. The
  sidebar footer holds Check for updates, and Download update for unmanaged
  installs. Shortcuts are recorded by
  polling the keyboard with the listener's hook suspended, because modifier-only chords never reach the window as key events.
- Models are prepared in-process through `transcription_preparation`; the
  runtime saves the selection when it is ready and restarts the listener.
- Enable on launch waits until first-run setup has recorded completion.
- The dictation HUD is a Rune overlay (`windows_rune_hud.rs`): a HUD thread
  steps the shared indicator model and shades each frame with the CPU port of
  the macOS shader into `rune::Pixels`; Rune shows it in a topmost tool window
  with `WS_EX_NOACTIVATE|WS_EX_TRANSPARENT|WS_EX_LAYERED`, shown with
  `SW_SHOWNOACTIVATE`, 12 pt from the top or 24 pt above the taskbar. The
  same model, shader, and overlay path ran in a headless sway on apollo (a
  test binary including the two shared modules); on Windows it is only
  type-checked.
- The window gets the executable's icon from Rune's `App::on_window` hook
  (its HWND through `WindowHandle::with_raw_handle`) for the taskbar,
  Alt-Tab, and the caption. The native title bar keeps its icon and title,
  blended into the window: DWM paints the caption the theme's `bg_deep`
  (`DWMWA_CAPTION_COLOR`, Windows 11; Windows 10 keeps its own caption
  color). The view repaints it when the system theme switches.
- Without a tray (creation failed) closing the window quits Hex.
  See the [single-process plan](plans/single-process.md#windows-under-rune).

**This app has never run on Windows.** Earlier releases shipped a GPUI app
that a person did run; that app is gone, and the Rune app replaces it in the
next release, so a person must run it on Windows (x64 and ARM64) before that
release. It is type-checked by cross-compiling on Linux with
[cargo-xwin](https://github.com/rust-cross/cargo-xwin):

```sh
rustup target add x86_64-pc-windows-msvc && cargo install cargo-xwin
# clang-cl, lld-link, and llvm-lib must be on PATH (llvm-lib can be a
# symlink to rustup's llvm-tools llvm-ar).
mkdir -p /tmp/no-transcribe/lib
echo '{"libraries":[],"shared":false}' > /tmp/no-transcribe/lib/transcribe-link.json
# TRANSCRIBE_DIR skips the transcribe.cpp/Vulkan CMake build; nothing links in check.
TRANSCRIBE_DIR=/tmp/no-transcribe cargo xwin clippy --target x86_64-pc-windows-msvc \
  --all-targets -- -D warnings
```

For ARM64, add the `aarch64-pc-windows-msvc` target and set
`XWIN_CROSS_COMPILER=clang`: ring's aarch64 build forces the GNU-style
`clang` driver, which rejects the clang-cl `/imsvc` flags cargo-xwin passes
by default. Also set `CARGO_FEATURE_NO_NEON=1`: blake3 (through Rune) builds
NEON C that the xwin sysroot's `arm64_neon.h` macros break under that driver.

That checks every Rust path, including Rune, wgpu's DX12 and Vulkan
backends, and the Windows modules; it compiles C build scripts (ring) for
the target but never links, builds transcribe.cpp, or runs anything.

## Local Transcription Service

Not available on Windows in this build. The `windows-shell` branch generalized
the authenticated local API (`service` and `service --embedded`) across
platforms, but `dev` has since reshaped it for macOS; porting it onto that
version is follow-up work.

## macOS Parity

The target is product and visual parity with macOS, using native Windows
adapters where the operating-system APIs differ. A direct copy of AppKit,
CoreAudio event taps, Metal, `SMAppService`, Accessibility, or
ScreenCaptureKit code would not run on Windows; the Windows implementation must
preserve the same user contract through Win32, WASAPI, Rune (wgpu), the
current-user startup registry, UI Automation, and Windows capture APIs.

| Area | Windows status |
| --- | --- |
| Local hold-to-dictate, Toggle mode, timestamped boundaries, Escape, paste | Implemented |
| Live model, shortcut, and microphone selection | Implemented |
| Double-tap lock and Paste Last Dictation | Implemented |
| Clipboard preservation and generation-safe restoration | Implemented |
| Recording tones and click-through recording/processing HUD | Implemented; a CPU port of the macOS Metal shader drawn in a Rune overlay, never run on Windows |
| History retention, list, detail, search, copy, delete, and clear | Implemented |
| Text replacements | Implemented in the Windows Modes pane using the shared phrase-boundary replacement engine |
| Application picker in Modes | Implemented; lists Start menu shortcuts, App Paths, and apps with an open window, with each executable's icon; "Choose a file…" opens `IFileOpenDialog` |
| Interface language | Implemented on every OS; English, Polish, Chinese, Japanese, German, and Spanish |
| Resident tray and Launch at login | Implemented |
| Application modes with per-mode corrections, OpenCode rewriting, and transformations | Implemented on the shared ordered processing policy; global and contextual profiles persist their own corrections, model, reasoning variant, deadline, and ordered built-in/TypeScript transformations, while modes match by application name — physical end-to-end paste/UI validation remains |
| Voice Action with selected-text capture and in-app OpenCode model selection | Implemented; selection is read through a clipboard round trip rather than UI Automation |
| Recognition hints for Whisper-family models | Implemented |
| Release microphone while idle | Implemented; mutually exclusive with audio pre-roll, as documented in Settings |
| While-dictating audio control (mute other apps or pause media) | Implemented via WASAPI session volumes and GSMTC |
| Signed automatic self-update for managed installs | Implemented for x64 and ARM64; the same ed25519 key and `https://anoma.ly/hex/files` origin as Linux, one feed per architecture, activating into a user-local versions directory. Checks 10 s after launch and every 6 h (or Check for updates); a staged update shows Restart to update in the tray menu and the window's sidebar footer, and the tray tooltip says an update is available. Source builds keep the GitHub release link (Download update); CI test builds never check |
| Web-domain mode rules and browser context via UI Automation | Implemented; the page URL comes from the browser's UIA document element, bounded so a hung provider degrades to application-only context |
| Opt-in voice Commands | Missing; blocked on a Windows streaming command model |
| Managed installer with Start Menu integration and clean uninstall | Implemented as `scripts/install-windows.ps1`; it creates the self-updating versions layout, and updates retarget the managed shortcut and Launch-at-login entry |
| First-run onboarding | Implemented; a fresh install picks a dictation language, downloads its recommended model with live progress, and starts listening only once setup is recorded — the shared Rune tour |
| Activity pane with the live session and recent transcripts | Implemented in developer builds; the shared read-only pane |
| Developer Meetings, live drafts, and meeting paste | Missing |
| Local transcription API and public TypeScript SDK host lifecycle on Windows | Missing; see Local Transcription Service |
| Streaming finish with whole-clip fallback | Implemented on GPU devices with Parakeet Unified English |

Settings live in `%APPDATA%\hex\settings.json`, the same file macOS and
Linux use.

The Windows window uses the shared design system
([design-system.md](design-system.md)): the same Rune panes as macOS and
Linux under the native title bar. Mode processing, OpenCode rewriting, Voice Action, and transformations run
through the same `dictation_processor` and `personal_commands` modules as
macOS; Windows reaches OpenCode through the `opencode2 api` CLI. Like macOS,
an unset OpenCode model tries the ranked recommended models before the
OpenCode default.
Remaining panes should be added only with their complete Windows behavior;
empty look-alike screens do not count as parity.

## Current Port Boundary

The Windows listener preserves physical press/release timestamps, keeps audio
capture off the transcription and paste worker, and restores the previous
clipboard only when no newer clipboard change supersedes it.
`check-windows` builds and tests the optimized MSVC release for x64 and ARM64
on every pull request. The remaining code
parity slice is the developer-only Meetings surface after the physical matrix
is green; opt-in Commands wait on a Windows streaming command model. Shared
first-run onboarding, complete OpenCode mode rewriting controls, and the bounded
TypeScript transformation host already ship. Windows releases are built for
x64 and ARM64 by
[`release.yml`](../.github/workflows/release.yml) with
[`scripts/build-windows.sh`](../scripts/build-windows.sh), Authenticode-signed,
and published to the Anomaly release service with the other platforms
(`docs/releasing.md`); [`scripts/install-windows.ps1`](../scripts/install-windows.ps1)
installs from its stable manifest. Users install
with the command under [Install](#install).
