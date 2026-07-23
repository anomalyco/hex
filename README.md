# HEX

HEX is a private voice dictation app for macOS and Linux X11. Hold a shortcut,
speak, and release to transcribe and paste into the app you are using.

Speech transcription runs locally. Optional OpenCode processing uses the model
provider configured in OpenCode.

## Install On macOS

HEX requires an Apple silicon Mac running macOS 15 or newer.

1. [Download the latest signed DMG](https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/releases/HEX-latest-arm64.dmg).
2. Open the DMG and drag HEX into Applications.
3. Launch HEX from Applications.
4. Follow setup to grant Microphone, Input Monitoring, and Accessibility access
   and install a transcription model.

HEX checks for signed updates automatically. Use **HEX > Check for Updates...**
to check immediately.

## Dictate

Hold **Option**, speak, then release. HEX transcribes your speech and pastes it
into the foreground app.

- Tap and release in under 300 ms to discard the capture.
- Double-tap Option to lock recording, then press Option again to finish.
- Press Escape to cancel.
- Press Option-Shift-V to paste the last completed dictation again.

Change the shortcut, microphone, language, transcription model, sound behavior,
feedback volume, launch-at-login setting, and Dock visibility from HEX settings.

## Voice Action

Hold **Option-Command**, describe what you want OpenCode to produce, then
release. If text is selected, HEX includes it as context; otherwise OpenCode
generates the requested text. The paste-ready result is inserted at the current
focus. You can also begin with Option and add Command without restarting the
recording.

Voice Action requires the [OpenCode beta](https://v2.opencode.ai/). Choose its
dedicated model and deadline in Settings. Failed, empty, cancelled, or timed-out
actions paste nothing; ordinary dictation continues to use local transcription.

## Optional Processing

Enable **Post-process with OpenCode** in Modes to rewrite dictated text before
it is pasted. You can use the default mode or create modes for particular apps
and websites. If processing fails or times out, HEX pastes the local transcript
instead.

## Experimental Voice Commands

Voice commands are available on macOS but disabled by default. Open
**Commands** and enable recognition to install the local command model. The
Commands pane lists the phrases available in the current app or browser.

Dictation remains available when command recognition is disabled or asleep.

## Linux X11 Beta

The Linux beta targets x86_64 Arch Linux with X11, PipeWire or ALSA, and Vulkan
libraries; inference can fall back to the CPU. It includes global hotkey
dictation, automatic paste, shortcut rebinding, a tray app, and signed
user-local updates. Voice commands, meetings, Wayland, and package-manager
installation are not yet supported.

Linux currently installs from source and requires access to this repository.

Install the build dependencies and Rust toolchain:

```sh
sudo pacman -S --needed base-devel git rustup alsa-lib curl gtk3 libxkbcommon \
  libxkbcommon-x11 libx11 libxcb openblas vulkan-headers vulkan-icd-loader \
  shaderc spirv-headers clang cmake pkgconf
rustup default stable
```

Clone and install HEX:

```sh
git clone https://github.com/anomalyco/hex.git
cd hex
./scripts/install-linux.sh
~/.local/bin/hex model install
~/.local/bin/hex app
```

Hold **Alt-Space** and release to transcribe and paste. Double-tap the shortcut
to lock recording, press it again to finish, or press Escape to cancel. Change
the shortcut from the HEX window.

The installer adds a desktop launcher and an autostart entry. Installed beta
builds check for signed updates at startup and every 24 hours.

## Troubleshooting

If dictation does not start on macOS, open HEX and complete any permission or
model action shown in setup. Permission changes may require restarting HEX.

Runtime diagnostics are stored in:

- macOS: `~/Library/Application Support/voice-control/logs/`
- Linux: `~/.local/share/voice-control/logs/` by default, or under
  `XDG_DATA_HOME` when configured

Captured dictation audio is not saved.

## Project Documentation

Architecture, active plans, technical specifications, research, and historical
prototype material are organized in [`docs/README.md`](docs/README.md).
