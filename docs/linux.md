# Install The Linux Beta

The Linux beta targets x86_64 Linux: i3/X11 and compatible wlroots-based Wayland
compositors such as Hyprland or Sway. Audio uses ALSA, including PipeWire systems
with ALSA compatibility. Inference uses Vulkan when available and can fall back
to the CPU. The GPUI Settings window still needs a Vulkan driver, even with CPU
inference; `vulkan-icd-loader` alone is not a driver. Install a driver for your GPU
or a software Vulkan driver. See the Wayland requirements below before enabling
native input.

The beta does not support voice commands, application or browser context, or
meetings. This is not universal Wayland support: GNOME and KDE are not covered
by the current clipboard, virtual-keyboard, and overlay protocol contract.

For Nix/NixOS, use the [Nix guide](nix.md). Nix owns updates to that installation;
HEX's signed updater owns only the direct-install layout below.

## Background Service And Settings

The dictation runtime is a **per-user systemd service**, `hex.service`, not a
system-wide/root daemon. It owns the microphone, hotkeys, model workers, sounds,
paste, and settings. Settings is a separate client: closing it does not stop
dictation or model preparation. There is no Linux tray icon.

```sh
hex start                       # start the service and listen; no window
hex app                         # open Settings; connect to/start the service
hex status                      # query live service state, not an old log entry
hex stop                        # stop the service and release microphone/hotkeys
hex restart                     # restart using this desktop's environment
journalctl --user -u hex.service # service diagnostics
```

Settings also offers Start/Stop for the listener without stopping the service.
An existing paused listener stays paused when Settings opens. A crashed client
does not stop dictation. A disconnected client loses its uncommitted shortcut
capture; the old binding and prior listening state are restored. The client
reconnects after a service restart and never replays uncertain mutation requests.

The direct installer creates the user unit and an XDG autostart entry that runs
`hex start`. The launcher writes an owner-only `session.env` containing only
display/session variables before starting systemd; missing variables explicitly
clear stale Wayland/X11 values inherited by the user manager. No permissions are
granted. The service is `PartOf=graphical-session.target`; desktops managing a
custom session target must bind its lifetime to their login/logout lifecycle.
Restart-on-failure belongs to systemd, not the Settings process.
There is one active desktop service per user: `hex start` hands it over with a
restart if the display/session changed. Opening Settings alone never silently
moves an existing runtime from another desktop.

IPC uses an owner-only Unix socket in HEX's data directory, same-user peer
credentials, versioned bounded frames, bounded queues, and I/O deadlines. It is
not a network API or the macOS embedded transcription SDK service.

For development or a non-systemd supervisor, run `cargo run -- service` in the
foreground, then `cargo run -- app` separately. `hex dictate` remains an explicit
foreground diagnostic listener and cannot coexist with the service's listener.
With `HEX_APPLICATION_SUPPORT_DIR` set, clients only connect to an explicitly
started service at that data root; they never start/restart the installed user
unit. `hex stop` then shuts down only that custom service through its socket.
Legacy `hex app --hidden` now means `hex start`; it never creates a hidden UI.
`hex status --lines 40` retains the explicit diagnostic-log view.

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
entry and a `hex.service` user unit. It does not start recording during installation.

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
launcher, and autostart entry in the same managed layout as published builds.
Newer signed releases can therefore replace this installed source build through
the in-app updater. `cargo run --release -- app` runs an unmanaged development
binary instead.

## Dictate

Hold **Alt-Space**, speak, then release. Double-tap the shortcut to keep
recording, press the same chord again to finish, or press Escape to cancel the
active recording. Settings shows listener state, Start/Stop, and operational
errors. Changing the shortcut, double-tap, or terminal-paste setting temporarily
stops listening and restores the previous running state after the edit.
Cancelling shortcut capture preserves the old binding unless the new binding
has already been saved; listening resumes only if it was previously running.

Recording sounds are enabled at 50% volume by default, including when upgrading
older settings. A start sound is queued immediately when the shortcut starts capture,
a stop sound marks the end of a retained recording (not completed transcription
or paste), and a cancel sound acknowledges cancellation of an active capture.
Brief taps still play the start sound, but recordings shorter than 300 ms remain
discarded and do not play the stop sound. **Sound volume** in Settings
offers Off, 25%, 50%, 75%, and 100%; changes save immediately without restarting
the listener and preview the start sound unless set to Off. `hex dictate` uses
the same saved volume. Playback uses the default audio output through a bounded
background worker; unavailable audio output does not prevent dictation.

Closing Settings leaves the service running on both X11 and Wayland. On Wayland,
the service-owned recording/processing pill uses layer-shell without taking
focus or intercepting clicks, even with Settings closed. X11 still has no HUD;
recording sounds and `hex status` remain available.

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

Managed installs check for signed updates when the service starts and every
24 hours until an update is ready. HEX downloads, verifies, and activates an
available update, then offers a **Restart** button in Settings for the service. The next launch
through `~/.local/bin/hex` also uses the activated version. The signed update
path is implemented but still awaiting a complete cross-version validation on
the supported Arch/i3 host.

See [`plans/linux.md`](plans/linux.md) for the engineering contract and remaining
validation work. The native Wayland implementation builds on the feature
proposed in [PR #28](https://github.com/anomalyco/hex/pull/28), with separate
input-state, lifecycle, and paste regression coverage. Physical device,
compositor, and target-application smoke tests remain necessary before a release.
`scripts/test-wayland-paste.sh /path/to/compiled-test-executable /path/to/voice-control` runs the real
paste helpers against a GTK target under an isolated headless Sway compositor.
It also launches an isolated service and Settings client, repeats paste while
Settings remains open, and verifies that closing Settings leaves the service
reachable. It requires the GTK development
libraries, Sway, `wl-clipboard`, `wtype`, `dbus-run-session`, and a Vulkan driver
(Mesa's software driver is sufficient), and must run as a non-root user. It uses
isolated app data and no microphone; CI runs it with the display-free suite and
Xvfb hotkey tests.

`scripts/test-linux-dictation.sh /path/to/voice-control /path/to/parakeet-unified-en-0.6b-Q8_0.gguf /path/to/jfk.wav`
checks the complete X11 app dictation path with a private PulseAudio virtual
microphone and Xvfb display. It opens and closes Settings while leaving the service
running, replays the public JFK sample from whisper.cpp, holds and releases the real
shortcut, verifies the transcribed words in a GTK editor, and checks orderly
application shutdown and microphone release. It needs PulseAudio, its ALSA
plugin, `xdotool`, Xvfb, the GTK development libraries, and a Vulkan driver for
Settings (Mesa's software driver is sufficient). It runs as a normal user with
temporary settings; it does not use physical audio devices or personal recordings.
CI downloads and verifies the pinned model and sample for this check.

`python3 scripts/test-linux-service.py /path/to/voice-control` is a display-free,
device-free IPC/lifecycle check: same-user socket permissions, sole service
ownership, client disconnects, shortcut-edit cleanup, invalid/oversized/stalled
frames, persisted volume, crash/stale-socket recovery, and orderly shutdown.
