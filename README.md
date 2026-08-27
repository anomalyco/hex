# HEX

HEX is local-first voice dictation for Apple silicon Macs running macOS 15 or
newer. Hold a shortcut, speak, and release. HEX transcribes on your Mac and
pastes the result into the app you are using.

[Download the latest signed DMG](https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/releases/HEX-latest-arm64.dmg)

## Install HEX

1. Download and open the DMG.
2. Drag HEX into Applications, then launch it.
3. Complete setup:
   - **Microphone** lets HEX record while you dictate.
   - **Input Monitoring** lets HEX detect its shortcut globally. Drag the HEX tile into the list.
   - **Accessibility** lets HEX paste into the foreground app. Drag the HEX tile into the list.
   - **Transcription model** downloads the speech model that runs locally.

HEX checks for signed updates automatically. Use **HEX > Check for Updates...**
to check immediately.

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
visibility. Replacements correct names and preferred spellings before text is
pasted.

## Optional OpenCode Features

[OpenCode](https://v2.opencode.ai/) is optional. Ordinary dictation does not
require it.

- **Voice Action:** hold **Option-Command** and describe what you want. HEX sends
  that instruction, selected text when Accessibility makes it available, and
  the foreground application and Brave website hostname. It pastes the result
  at the current focus.
- **Post-processing:** enable OpenCode in Modes to rewrite a transcript before
  paste. HEX sends the transcript, foreground application, and Brave website
  hostname to the configured model provider. Modes can target applications and
  websites in Brave Browser.

If post-processing fails, HEX pastes the local transcript. A failed Voice Action
pastes nothing.

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

Diagnostics are stored at:

- macOS: `~/Library/Application Support/voice-control/logs/`
- Linux: `~/.local/share/voice-control/logs/`, or under `XDG_DATA_HOME`

## Linux Beta

The Linux beta targets x86_64 Linux. On X11 it grabs the shortcut and pastes
with xtest. On Wayland it observes evdev and pastes with wl-copy and wtype.
It does not support voice commands or meetings.

See the [Linux installation guide](docs/linux.md) for the verified user-local
installer, source-build fallback, requirements, and limitations.

## Contributing

Architecture, development commands, active plans, and historical research are
indexed in [`docs/README.md`](docs/README.md). Start with
[`CONTRIBUTING.md`](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md) before changing
the native application. Report vulnerabilities through the process in
[`SECURITY.md`](SECURITY.md).

HEX is available under the [MIT License](LICENSE).
Vendored icon licenses are listed in [Third-Party Notices](THIRD_PARTY_NOTICES.md).
