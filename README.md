<p align="center">
  <img src=".github/assets/hex-icon.png" width="96" height="96" alt="Hex app icon" />
</p>

<h1 align="center">Hex</h1>

<p align="center">
  <strong>Speak. It appears.</strong><br />
  Local-first voice dictation, built in Rust.
</p>

<p align="center">
  <a href="https://anoma.ly/hex/"><strong>Download for Mac, Windows, or Linux</strong></a>
  · <a href="docs/windows.md">Windows beta</a>
  · <a href="docs/linux.md">Linux beta</a>
  · <a href="sdk/typescript/README.md">TypeScript SDK</a>
  · <a href="CONTRIBUTING.md">Contributing</a>
</p>

Hex is local-first voice dictation for Apple silicon Macs running macOS 15 or
newer. Hold a shortcut, speak, and release. Hex transcribes on your Mac and
pastes the result into the app you are using.

- **On-device transcription.** Download a local speech model during setup;
  ordinary dictation does not require OpenCode or a cloud transcription service.
- **Your words, your way.** Choose a shortcut, language, and model. Use Modes for
  corrections, application-specific behavior, and optional text processing.
- **Optional voice tools.** Enable Voice Action or voice commands when you need
  them. Both start off; provider-backed rewriting is separate from local speech
  recognition.

The macOS app is available now. The [Windows beta](#windows-beta) covers x64 and
ARM64 PCs, and the [Linux beta](#linux-beta) covers x86_64 X11 and compatible
wlroots Wayland compositors. See [Privacy and local data](#privacy-and-local-data)
for what stays on your device and what optional features send to a provider.

## Install Hex

### Downloads

Each link serves the current release: the newest dev build (published on
every change to `dev`, and updating itself from the Dev channel) until Hex's
first stable release, then the stable release.

| Platform | Download |
| --- | --- |
| macOS (Apple silicon) | [DMG](https://anoma.ly/hex/download/macos-arm64-dmg) |
| Windows x64 | [Installer](https://anoma.ly/hex/download/windows-x64-setup) |
| Windows ARM64 | [Installer](https://anoma.ly/hex/download/windows-arm64-setup) |
| Linux x86_64 | [Executable](https://anoma.ly/hex/download/linux-x64) |
| Linux x86_64 (Debian, Ubuntu) | [.deb](https://anoma.ly/hex/download/linux-x64-deb) |
| Linux x86_64 (Fedora, openSUSE) | [.rpm](https://anoma.ly/hex/download/linux-x64-rpm) |
| Linux ARM64 | [Executable](https://anoma.ly/hex/download/linux-arm64) |
| Linux ARM64 (Debian, Ubuntu) | [.deb](https://anoma.ly/hex/download/linux-arm64-deb) |
| Linux ARM64 (Fedora, openSUSE) | [.rpm](https://anoma.ly/hex/download/linux-arm64-rpm) |

Updater files: [macOS ZIP](https://anoma.ly/release/hex/desktop/dev/macos-arm64),
[Windows x64 EXE](https://anoma.ly/release/hex/desktop/dev/windows-x64),
[Windows ARM64 EXE](https://anoma.ly/release/hex/desktop/dev/windows-arm64).
Windows links work once Windows signing is configured. On Linux you can also
use `yay -S hex-bin` or the install script below.

With Homebrew:

```sh
brew install --cask anomalyco/tap/hex
```

Then launch Hex from Applications and complete setup.

The cask installs the same signed app and keeps its built-in updater. Existing
DMG installations can keep using **Check for Updates**; Homebrew does not migrate
or forcibly replace another app already named `Hex.app`.

Or install manually:

1. [Download the signed DMG](https://anoma.ly/hex/download/macos-arm64-dmg) and open it.
2. Drag `Hex.app` into Applications, then launch it.
3. Complete the permission and model steps in setup.

- **Microphone** lets Hex record while you dictate.
- **Accessibility** lets Hex notice its shortcut in every app and paste into the foreground app. Drag the Hex tile into the list. Hex does not need Input Monitoring.
- **Transcription model** downloads the speech model that runs locally.

Hex checks for signed updates automatically. Use **Hex > Check for Updates…**
to check immediately.

Organizations that manage app versions themselves can turn off in-app updates,
including manual checks, with the `DisableUpdates` Boolean in the
`anomaly.hex` preference domain. Deploy it in a configuration profile
(payload type `anomaly.hex`, key `DisableUpdates`, value `true`), or for a
single Mac run `defaults write anomaly.hex DisableUpdates -bool true`. On
Windows, set the DWORD `DisableUpdates` to `1` under
`HKLM\SOFTWARE\Policies\Anomaly\Hex` (Group Policy) or the same path in HKCU.
Relaunch Hex to apply it. Model downloads still work. Releases through 2.3.1
used the `com.kitlangton.hex2` domain; profiles for those keep applying only to them.

**Download blocked?** Each app release on [GitHub Releases](https://github.com/anomalyco/hex/releases)
has the identical signed files. App releases use `v…` tags (`app-v…` through
2.3.1); the repository also publishes separate TypeScript SDK releases.
If an older installation cannot check for updates on your network, install the
DMG manually over the existing Rust app.

On managed networks, ask IT to allow `anoma.ly`: Hex downloads and updates
from the Anomaly release service at `https://anoma.ly/release` (releases up to
2.3.1 use `https://anoma.ly/hex/files`). Local model setup separately needs `huggingface.co` and its model
download CDN; optional Voice Commands needs `download.moonshine.ai`. A successful
app download does not establish that those model hosts are reachable. After
restoring access, retry model preparation in Settings (or Retry for Commands).

Coming from the original Swift app? Install the new app manually and complete
setup. Settings and history are not transferred. Quit the old app before
dictating in the new one so their shortcuts do not compete.

## Dictate Anywhere

Hold **Option**, speak, then release. Hex transcribes and pastes at the current
focus.

- Double-tap Option to keep recording; press Option again to finish.
- Press Escape to cancel. While recording, Escape discards the recording; after
  you release, it cancels the newest dictation that is still transcribing, so
  nothing is pasted for it.
- Press Option-Shift-V to paste the last completed dictation again.

Settings let you change the shortcut, microphone, language, local model,
recording behavior, feedback volume, launch-at-login behavior, and Dock
visibility. Corrections in Modes fix names and preferred spellings before text
is pasted; modes can also apply optional rewriting and text transformations.

Use **menu bar > Transcription Model** to switch between downloaded models without
opening Settings. Each model remembers its last selected language and, for
Whisper, recognition hints. Choose the language in Settings once, then switch
between combinations such as English with Parakeet and French with Whisper from
the menu. **Manage Models…** opens the full picker for downloads and language changes.

## Optional OpenCode Features

[OpenCode V2 beta](https://v2.opencode.ai/) (`opencode2`) is optional. Ordinary
dictation does not require it.

- **Voice Action:** enable it in the Voice Action pane, then hold
  **Option-Command** and describe what you want. It defaults off, independently
  of your dictation shortcut. Hex sends
  that instruction, selected text when Accessibility makes it available, and
  the foreground application and Brave website hostname. It pastes the result
  at the current focus.
- **Post-processing:** enable OpenCode in Modes to rewrite a transcript before
  paste. Hex sends the transcript, foreground application, and Brave website
  hostname to the configured model provider. Modes can target applications and
  websites in Brave Browser.

If OpenCode rewriting fails, Hex keeps the corrected local transcript and still
runs any selected text transformations. If that transformation stage fails, Hex
keeps the text from before the stage, not partial transformation results. A
failed Voice Action pastes nothing.

## Annotate

Annotate records your screen while you draw on it and talk, then hands the
recording to an agent: its keyframes, your drawing, and a transcript of what
you said. It is on by default on macOS and Linux (Settings › Annotate; not on
Windows yet). Nothing is recorded, and no screen permission is asked for,
until you first use it.

- **Start and stop:** **Option-Shift-S** on macOS, **Ctrl+Alt+S** on Linux.
  macOS asks for Screen Recording the first time; Linux asks through the
  desktop's screen-sharing dialog once, then remembers the screen. While it
  records, the dictation and Voice Action shortcuts do nothing.
- **Draw:** on macOS hold **Shift** and move the pointer; a Shift-click drops a
  numbered pin and **⌘Z** takes the last stroke back. On Linux hold
  **Ctrl+Alt+D** and move the pointer; a click while holding drops a pin, and
  a right-click (or Ctrl+Z) takes the last stroke back. Let go and clicks
  reach your apps again. Ink fades from the screen after about a second but
  stays in the recording.
- **Paste or send:** when it stops, the annotation is on your clipboard and
  the capsule offers **Paste** (the contact sheet, then the summary) and
  **Inspect**. With OpenCode running, **Send to…** lists its sessions (macOS
  also opens its suggestions by itself): click one to send it there, and Undo
  takes it back for a few seconds. A send carries the transcript and the
  pictures inline, so it works with an OpenCode server on another machine.
- `hex annotate list|show|send` (also `hex capture …`) reads and sends from the
  terminal.

## Experimental Voice Commands

Voice commands are available on macOS and disabled by default. Enabling them
starts continuous local command recognition and requires a separate local
command model; Hex prompts to install it when needed. Dictation remains
available while command recognition is off or asleep.

The Commands pane lists built-in phrases and filters them by global, application,
and Brave website context. Advanced users can create
`~/.config/hex/hex.config.ts` for custom commands and dictation
transformations, written in TypeScript or JavaScript. Hex runs the config
itself in an embedded JavaScript engine and reloads it on save; nothing else
needs installing. Hex provides an Edit Config action and a prompt you can paste
into a local coding agent, and `hex commands check` reports what the config
registers or why it fails.

Config scripts get no filesystem, network, or process access: handlers return
actions (open a URL, path, or app, press keys, type text) that Hex carries out,
so review agent changes before enabling commands.

To teach a coding agent how to configure personal commands and transformations,
install the official Hex skill:

```sh
npx skills add anomalyco/hex --skill hex-personal-commands
```

The skill targets current Hex: dev builds now, and the next stable release.
On Hex 2.3.1, install the matching skill with
`npx skills add anomalyco/hex#app-v2.3.1 --skill hex-personal-commands`, or
[switch to the dev build](https://github.com/anomalyco/hex#install-hex).

## Read Transcripts From The CLI

The macOS release binary provides read-only queries while the app runs:

```sh
/Applications/Hex.app/Contents/MacOS/hex history --since 3d --app 'OpenCode Beta' --grep problems --json
/Applications/Hex.app/Contents/MacOS/hex meetings list --since 7d --json
/Applications/Hex.app/Contents/MacOS/hex meetings show latest --md
/Applications/Hex.app/Contents/MacOS/hex meetings search 'release schedule'
```

History supports `--until`, `--limit`, `--raw`, and `--oldest-first`; queries
respect the saved retention window. Meetings share the read-only MCP query
layer. No microphone, GUI, model, or app service starts.
See [flags and the versioned JSON schema](docs/transcript-cli.md).

## Privacy And Local Data

- Speech transcription and command recognition run locally.
- Hex does not save dictation audio by default.
- History retains successfully pasted dictation and Voice Action text
  for seven days by default, including the local transcript, final text, and
  limited application and timing metadata. It never stores audio, full browser
  URLs, or window titles. Every retention choice is capped at 10,000 entries and
  16 MiB of raw plus final text, with oldest entries evicted first.
- Use **History > Keep: Off** to stop adding entries, or **Clear all** to remove
  existing history. Turning retention off does not delete existing entries or
  disable diagnostic logging.
- An explicit `HEX_RETAIN_DICTATION_AUDIO` diagnostic setting can retain a
  bounded number of owner-only WAV files.
- Local diagnostic logs can contain transcript text, recognized command speech,
  foreground application names, and Brave URLs. Hex does not upload these logs.
- OpenCode features send the data described above to your configured model
  provider.
- Model installation and software updates require network access.

Review and redact diagnostics before sharing them.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Hex appears to disappear | Open Hex from its menu-bar hexagon. If the status item could not start, Hex keeps its Dock icon visible. |
| The shortcut does nothing | Open Hex and verify Microphone and Accessibility access. |
| Recording works but text does not paste | Verify Accessibility access. |
| Transcription does not start | Confirm that the selected local model is installed. |
| OpenCode features fail | Verify the separate OpenCode installation and model provider. |
| Voice commands do not respond | Confirm Commands is enabled and voice control is awake. |
| Website context does not match | Website-aware modes and commands currently require Brave Browser. |

Permission changes may require quitting and reopening Hex.

Still stuck? [Report a bug](https://github.com/anomalyco/hex/issues/new?template=bug_report.yml)
with your Hex version, operating system, and steps to reproduce. Review and
redact any attachments; do not upload private dictation, recordings, or full logs.

Diagnostics are stored at:

- macOS: `~/Library/Application Support/hex/logs/`
- Windows: `%APPDATA%\hex\logs\`
- Linux: `$XDG_DATA_HOME/hex/logs/` when `XDG_DATA_HOME` is set to an absolute
  path, otherwise `~/.local/share/hex/logs/`

## Windows Beta

The Windows beta runs on Windows 10 and 11, x64 and ARM64. It covers hotkey
dictation (Hold or Toggle, `Ctrl+Space` by default), Modes, Voice Action,
History, a tray icon, and signed self-updates. On x64 with a Vulkan graphics
driver, Parakeet Unified English streams while you speak, so text arrives within
about 100 ms of release even after minutes of talking; ARM64 transcribes on the
CPU. Voice commands, meetings, and the local transcription API are not included.
Download the installer for
[x64](https://anoma.ly/hex/download/windows-x64-setup) or
[ARM64](https://anoma.ly/hex/download/windows-arm64-setup)
and run it; it installs for your user without administrator rights. It is not
code-signed yet, so SmartScreen may warn: choose **More info**, then **Run
anyway**. See the [Windows guide](docs/windows.md) for details or to build from
source.

## Linux Beta

The Linux beta targets x86_64 Linux on i3/X11 and compatible wlroots Wayland
compositors. It supports local hotkey dictation, Modes, Voice Action, History,
the Hex window, and signed updates for direct installs. With a Vulkan graphics driver, dictation
streams while you speak, as on macOS and Windows. Native Wayland requires
compositor support for clipboard and layer-shell protocols; paste uses `wtype`
with the virtual keyboard protocol, or the desktop's RemoteDesktop portal (KDE,
GNOME).
Shortcuts come from the desktop's GlobalShortcuts portal, or you bind
`hex dictation start` / `stop` / `toggle` in your compositor; see the
[Linux guide](docs/linux.md#shortcuts). Voice commands and meetings are not
included.

Install the signed release on x86_64 Arch Linux (glibc 2.44 or later) with:

```sh
curl -fsSL https://anoma.ly/hex/files/install-linux.sh | sh
```

Until Hex has a stable release on the release service, this installs the
newest dev build, which keeps updating from the Dev channel
(`HEX_INSTALL_CHANNEL=stable` or `=dev` picks one explicitly).

From the next release on, Arch users can instead install the AUR package
`hex-bin` (for example `yay -S hex-bin`), which pacman updates; it is not on the
AUR yet.

The [Linux installation guide](docs/linux.md) covers inspecting the installer
first, uninstalling, and building from source. It also describes
requirements and limitations.
For Nix/NixOS packaging and per-user autostart, see the [Nix guide](docs/nix.md).

## Testing Builds

Every push to `dev` produces unsigned test builds for macOS, Windows, and
Linux in the [Test builds](https://github.com/anomalyco/hex/actions/workflows/test-builds.yml)
workflow. Open the latest green run and download from its summary. The
[test build guide](docs/test-builds.md) explains how to install and remove each
one without disturbing an installed Hex.

## Build With Hex

[`@kitlangton/hex`](sdk/typescript/README.md) brings local transcription to your
own application through Promise and optional Effect APIs. Your app supplies the
recorded audio; Hex handles model preparation and transcription.

The SDK is published on [npm](https://www.npmjs.com/package/@kitlangton/hex), but
a bundled native helper is not yet published. You must supply a compatible Hex
executable. Follow the [SDK guide](sdk/typescript/README.md) for requirements and
examples.

## Contributing

Issues are welcome, including detailed diagnoses and proposed fixes. Hex's team
writes every change, so pull requests from outside the team aren't accepted; we
credit your analysis in the pull request that fixes it. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

Architecture, development commands, active plans, and historical research are
indexed in [`docs/README.md`](docs/README.md), with development rules in
[`AGENTS.md`](AGENTS.md). Report vulnerabilities through the process in
[`SECURITY.md`](SECURITY.md).

Hex is available under the [MIT License](LICENSE).
Vendored icon licenses are listed in [Third-Party Notices](THIRD_PARTY_NOTICES.md).
