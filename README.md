<p align="center">
  <img src=".github/assets/hex-icon.png" width="96" height="96" alt="HEX app icon" />
</p>

<h1 align="center">HEX</h1>

<p align="center">
  <strong>Speak. It appears.</strong><br />
  Local-first voice dictation, built in Rust.
</p>

<p align="center">
  <a href="https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/releases/HEX-latest-arm64.dmg"><strong>Download for macOS</strong></a>
  · <a href="https://hex.kitlangton.dev/">Website</a>
  · <a href="docs/linux.md">Linux beta</a>
  · <a href="sdk/typescript/README.md">TypeScript SDK</a>
  · <a href="CONTRIBUTING.md">Contributing</a>
</p>

HEX is local-first voice dictation for Apple silicon Macs running macOS 15 or
newer. Hold a shortcut, speak, and release. HEX transcribes on your Mac and
pastes the result into the app you are using.

- **On-device transcription.** Download a local speech model during setup;
  ordinary dictation does not require OpenCode or a cloud transcription service.
- **Your words, your way.** Choose a shortcut, language, and model. Use Modes for
  corrections, application-specific behavior, and optional text processing.
- **Optional voice tools.** Enable Voice Action or voice commands when you need
  them. Both start off; provider-backed rewriting is separate from local speech
  recognition.

The macOS app is available now. The [Linux beta](#linux-beta) has a smaller feature
set and requires a source build or Nix. See [Privacy and local data](#privacy-and-local-data)
for what stays on your device and what optional features send to a provider.

## Install HEX

1. [Download the latest signed DMG](https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/releases/HEX-latest-arm64.dmg) and open it.
2. Drag `Hex.app` into Applications, then launch it.
3. Complete the permission and model steps in setup.

- **Microphone** lets HEX record while you dictate.
- **Input Monitoring** lets HEX detect its shortcut globally. Drag the HEX tile into the list.
- **Accessibility** lets HEX paste into the foreground app. Drag the HEX tile into the list.
- **Transcription model** downloads the speech model that runs locally.

HEX checks for signed updates automatically. Use **HEX > Check for Updates...**
to check immediately.

**Looking for the Mac app in GitHub Releases?** Those releases currently contain
the TypeScript SDK. Use the DMG link above for the app and its built-in updater
for subsequent releases.

Coming from the original Swift app? Install the new app manually and complete
setup. Settings and history are not transferred. Quit the old app before
dictating in the new one so their shortcuts do not compete.

## Dictate Anywhere

Hold **Option**, speak, then release. HEX transcribes and pastes at the current
focus.

- Double-tap Option to keep recording; press Option again to finish.
- Press Escape to cancel.
- Press Option-Shift-V to paste the last completed dictation again.

Settings let you change the shortcut, microphone, language, local model,
recording behavior, feedback volume, launch-at-login behavior, and Dock
visibility. Corrections in Modes fix names and preferred spellings before text
is pasted; modes can also apply optional rewriting and text transformations.

## Optional OpenCode Features

[OpenCode V2 beta](https://v2.opencode.ai/) (`opencode2`) is optional. Ordinary
dictation does not require it.

- **Voice Action:** enable it in the Voice Action pane, then hold
  **Option-Command** and describe what you want. It defaults off, independently
  of your dictation shortcut. HEX sends
  that instruction, selected text when Accessibility makes it available, and
  the foreground application and Brave website hostname. It pastes the result
  at the current focus.
- **Post-processing:** enable OpenCode in Modes to rewrite a transcript before
  paste. HEX sends the transcript, foreground application, and Brave website
  hostname to the configured model provider. Modes can target applications and
  websites in Brave Browser.

If OpenCode rewriting fails, HEX keeps the corrected local transcript and still
runs any selected text transformations. If that transformation stage fails, HEX
keeps the text from before the stage, not partial transformation results. A
failed Voice Action pastes nothing.

## Experimental Voice Commands

Voice commands are available on macOS and disabled by default. Enabling them
starts continuous local command recognition and requires a separate local
command model; HEX prompts to install it when needed. Dictation remains
available while command recognition is off or asleep.

The Commands pane lists built-in phrases and filters them by global, application,
and Brave website context. Advanced users can create
`~/.config/hex/hex.config.ts` for custom TypeScript commands. Custom commands
require [Bun](https://bun.sh/); HEX provides an Edit Config action and a prompt
you can paste into a local coding agent.

Custom command files and dependencies are executable local code with your user
permissions. Review agent changes and install only dependencies you trust.

To teach a coding agent how to configure personal commands and transformations,
install the official HEX skill:

```sh
npx skills add anomalyco/hex --skill hex-personal-commands
```

## Privacy And Local Data

- Speech transcription and command recognition run locally.
- HEX does not save dictation audio by default.
- On macOS, History retains successfully pasted dictation and Voice Action text
  for seven days by default, including the local transcript, final text, and
  limited application and timing metadata. It never stores audio, full browser
  URLs, or window titles. Every retention choice has entry and byte limits.
- Use **History > Keep: Off** to stop adding entries, or **Clear all** to remove
  existing history. Turning retention off does not delete existing entries or
  disable diagnostic logging.
- An explicit `HEX_RETAIN_DICTATION_AUDIO` diagnostic setting can retain a
  bounded number of owner-only WAV files.
- Local diagnostic logs can contain transcript text, recognized command speech,
  foreground application names, and Brave URLs. HEX does not upload these logs.
- OpenCode features send the data described above to your configured model
  provider.
- Model installation and software updates require network access.

Review and redact diagnostics before sharing them.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| HEX appears to disappear | Open HEX from its menu-bar hexagon. If the status item could not start, HEX keeps its Dock icon visible. |
| The shortcut does nothing | Open HEX and verify Microphone and Input Monitoring access. |
| Recording works but text does not paste | Verify Accessibility access. |
| Transcription does not start | Confirm that the selected local model is installed. |
| OpenCode features fail | Verify the separate OpenCode installation and model provider. |
| Voice commands do not respond | Confirm Commands is enabled and voice control is awake. |
| Website context does not match | Website-aware modes and commands currently require Brave Browser. |

Permission changes may require quitting and reopening HEX.

Still stuck? [Report a bug](https://github.com/anomalyco/hex/issues/new?template=bug_report.yml)
with your HEX version, operating system, and steps to reproduce. Review and
redact any attachments; do not upload private dictation, recordings, or full logs.

Diagnostics are stored at:

- macOS: `~/Library/Application Support/voice-control/logs/`
- Linux: `~/.local/share/voice-control/logs/`, or under `XDG_DATA_HOME`

## Linux Beta

The Linux beta targets x86_64 Linux on i3/X11 and compatible wlroots Wayland
compositors. It supports local hotkey dictation, a GPUI settings shell, and
signed updates for direct installs. Native Wayland requires explicit input-device
access and compositor support for clipboard, virtual-keyboard, and layer-shell
protocols. Voice commands and meetings are not included.

Use the [Linux installation guide](docs/linux.md) to build from source until
the first signed Linux release is published. It also describes the planned
user-local installer, requirements, and limitations.
For Nix/NixOS packaging and per-user autostart, see the [Nix guide](docs/nix.md).

## Build With HEX

[`@kitlangton/hex`](sdk/typescript/README.md) brings local transcription to your
own application through Promise and optional Effect APIs. Your app supplies the
recorded audio; HEX handles model preparation and transcription.

The SDK is published on [npm](https://www.npmjs.com/package/@kitlangton/hex), but
a bundled native helper is not yet published. You must supply a compatible HEX
executable. Follow the [SDK guide](sdk/typescript/README.md) for requirements and
examples.

## Contributing

Architecture, development commands, active plans, and historical research are
indexed in [`docs/README.md`](docs/README.md). Start with
[`CONTRIBUTING.md`](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md) before changing
the native application. Report vulnerabilities through the process in
[`SECURITY.md`](SECURITY.md).

HEX is available under the [MIT License](LICENSE).
Vendored icon licenses are listed in [Third-Party Notices](THIRD_PARTY_NOTICES.md).
