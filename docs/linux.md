# Install The Linux Beta

The Linux beta targets x86_64 Linux: i3/X11 and compatible wlroots-based Wayland
compositors such as Hyprland or Sway. Audio uses ALSA, including PipeWire systems
with ALSA compatibility. Inference uses Vulkan when available and can fall back
to the CPU. See the Wayland requirements below before enabling native input.

The beta does not support voice commands, application or browser context, or
meetings. This is not universal Wayland support: GNOME and KDE are not covered
by the current clipboard, virtual-keyboard, and overlay protocol contract.

For Nix/NixOS, use the [Nix guide](nix.md). Nix owns updates to that installation;
HEX's signed updater owns only the direct-install layout below.

## Install A Published Build

After the first signed Linux release is published, download and inspect the
installer before running it:

```sh
sudo pacman -S --needed alsa-lib curl gtk3 gtk-layer-shell jq libappindicator-gtk3 \
  libxkbcommon libxkbcommon-x11 libx11 libxcb openblas openssl util-linux \
  vulkan-icd-loader wl-clipboard wtype xxd
curl --proto '=https' --tlsv1.2 -fsSLO \
  https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/install-linux.sh
less install-linux.sh
sh install-linux.sh
~/.local/bin/hex model install
~/.local/bin/hex app
```

The installer verifies the signed release feed, exact artifact size, and
SHA-256 before installing. It uses no root access. It creates the managed
version layout, `~/.local/bin/hex`, a desktop launcher, and an XDG autostart
entry.

Remove the application while preserving logs and settings with:

```sh
sh install-linux.sh uninstall
```

## Install From Source

Install the native dependencies and, for a source build, the Rust toolchain:

```sh
sudo pacman -S --needed base-devel git rustup python alsa-lib curl jq openssl xxd \
  util-linux gtk3 gtk-layer-shell libappindicator-gtk3 libxkbcommon \
  libxkbcommon-x11 libx11 libxcb openblas vulkan-headers vulkan-icd-loader \
  shaderc spirv-headers clang cmake pkgconf wl-clipboard wtype
rustup default stable
```

Build and install HEX as your desktop user:

```sh
git clone https://github.com/anomalyco/hex.git
cd hex
./scripts/install-linux.sh
~/.local/bin/hex model install
~/.local/bin/hex app
```

Do not run the installer as root. It installs a user-local binary, desktop
launcher, and autostart entry.

## Dictate

Hold **Alt-Space**, speak, then release. Double-tap the shortcut to keep
recording, press the same chord again to finish, or press Escape to cancel the
active recording. Settings shows listener state, Start/Stop, and operational
errors. Changing the shortcut or double-tap setting temporarily stops listening
and restores the previous running state after the edit. Cancelling shortcut
capture restores the old binding and running state.

With a working X11 tray, closing Settings hides the window. Without a working
X11 tray, closing Settings quits after stopping its workers; it never leaves an
unmanageable microphone running. `--hidden` is honored only with a usable X11
tray. On Wayland, a StatusNotifier tray icon is shown when a host is present,
and closing Settings leaves dictation running until Quit HEX or SIGTERM. The
recording/processing pill uses layer-shell without taking focus or intercepting
clicks.

Insertion uses the clipboard and **Ctrl-V** for new installations. Enable
**Terminal paste shortcut** in Settings for targets that require **Ctrl-Shift-V**.
Existing X11 settings retain their previous Ctrl-Shift-V behavior until changed.
The target application must accept the selected binding. The beta leaves the
transcript on the clipboard; arbitrary previous
clipboard formats are not restored. A short settling delay prevents immediate
back-to-back replacement, but is not an acknowledgment from a stalled target
application (see [#24](https://github.com/anomalyco/hex/issues/24)).

## Native Wayland Requirements

- Export the compositor's nonempty `WAYLAND_DISPLAY` in the launch environment.
  HEX, GTK, and GPUI select the same native backend even when XWayland's
  `DISPLAY` is also present. `XDG_SESSION_TYPE` alone does not select Wayland.
- Install `wl-clipboard` and `wtype`. The compositor must support their clipboard
  and virtual-keyboard protocols. There is no silent XWayland, direct-typing, or
  privileged input-injection fallback. Transcript text travels on stdin, not
  command-line arguments.
- The overlay requires layer-shell. Settings reports when the HUD is unavailable
  rather than presenting a normal focus-taking window as an overlay.
- The evdev backend requires read access to **every `/dev/input/event*` node**
  so it can inspect which devices may hold modifier keys. An unreadable node
  causes an explicit startup error, not a listener that reports ready but cannot
  paste. Configure this deliberately using your distribution's input-group or
  equivalent policy; HEX does not grant permissions or run as root.
- **This access exposes all keyboard input to processes running as your user,
  not only HEX's shortcut.** Do not make event devices world-readable. X11 does
  not require this additional access.
- Shortcut labels use physical evdev keys with US-layout names. The backend
  observes keys without suppressing delivery to other applications. Reserve the
  chosen chord with a no-op compositor binding if it would otherwise reach the
  focused client. Modifier-only shortcuts are not supported.
- Devices are rescanned once per second. Device loss or an input-stream gap
  cancels an active recording rather than fabricating a release; a reconnected
  keyboard can be used after the next scan.

## Updates And Validation

Managed installs check for signed updates at startup and every 24 hours. HEX
downloads, verifies, and installs an available update, then asks before
restarting into it. The signed update path is implemented but still awaiting a
complete cross-version validation on the supported Arch/i3 host.

See [`plans/linux.md`](plans/linux.md) for the engineering contract and remaining
validation work. The native Wayland implementation builds on the feature
proposed in [PR #28](https://github.com/anomalyco/hex/pull/28), with separate
input-state, lifecycle, and paste regression coverage. Physical device,
compositor, and target-application smoke tests remain necessary before a release.
`scripts/test-wayland-paste.sh /path/to/compiled-test-executable /path/to/voice-control` runs the real
paste helpers against a GTK target under an isolated headless Sway compositor.
It also launches Settings with `--hidden`, repeats paste while Settings remains
open, and verifies that the tray-less native window closes without exiting the
process until SIGTERM. It requires the GTK development
libraries, Sway, `wl-clipboard`, `wtype`, `dbus-run-session`, and a Vulkan driver
(Mesa's software driver is sufficient), and must run as a non-root user. It uses
isolated app data and no microphone; CI runs it with the display-free suite and
Xvfb hotkey tests.

`scripts/test-linux-dictation.sh /path/to/voice-control /path/to/parakeet-unified-en-0.6b-Q8_0.gguf /path/to/jfk.wav`
checks the complete X11 app dictation path with a private PulseAudio virtual
microphone and Xvfb display. It verifies Settings stays visible without a tray,
replays the public JFK sample from whisper.cpp, holds and releases the real
shortcut, verifies the transcribed words in a GTK editor, and checks orderly
application shutdown and microphone release. It needs PulseAudio, its ALSA
plugin, `xdotool`, Xvfb, and the GTK development libraries. It runs as a normal
user with temporary settings; it does not use physical audio devices or personal
recordings. CI downloads and verifies the pinned model and sample for this check.
