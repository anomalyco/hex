# Install The Linux Beta

The Linux beta targets x86_64 and ARM64 (aarch64) Linux: i3/X11 and compatible wlroots-based Wayland
compositors such as Hyprland or Sway. Audio uses ALSA, including PipeWire systems
with ALSA compatibility. Inference uses Vulkan when available and can fall back
to the CPU. The window still needs a Vulkan driver, even with CPU
inference; `vulkan-icd-loader` alone is not a driver. Install a driver for your GPU
or a software Vulkan driver. See the Wayland requirements below.

x86_64 machines need a 2013 or newer processor with AVX2 (Intel Haswell / AMD Excavator or later); on an older processor Hex shows a message and quits instead of starting.

Linux opens the same window as macOS and Windows: Settings, Modes,
and History. Meetings are macOS-only.
Paste on Wayland uses `wtype` where the compositor offers the virtual keyboard
protocol (wlroots compositors, niri) and the desktop's RemoteDesktop portal
elsewhere (KDE, GNOME). Shortcuts are general: the desktop's GlobalShortcuts portal when it has one, otherwise
your desktop's own keybindings running `hex` commands (see
[Shortcuts](#shortcuts)).

For Nix/NixOS, use the [Nix guide](nix.md). Nix owns updates to that installation;
Hex's signed updater owns only the direct-install layout below.

## One Hex Process

Hex on Linux is **one process per user** (and per data directory), not a
system-wide/root daemon. It owns the microphone, shortcuts, model downloads and
workers, sounds, paste, History, the recording pill, and the window. The window
is optional: closing it stops neither dictation nor a model download, and
launching Hex again opens it. When the desktop has a tray (a
StatusNotifierItem host, such as Waybar, KDE, or Omarchy's bar), Hex shows a
tray icon: click it to open the window, or use its menu: **Show Hex**,
**Paste last dictation**, the downloaded speech models, the update once one
is ready (**Restart to update**), **Enable / Disable Hex**, and **Quit Hex**;
while Hex records or transcribes, the menu and tooltip say so. Without a
tray, quit with **Quit Hex** at the bottom of the window's sidebar, the launcher's **Quit Hex** action, or `hex quit`.

The window opens at 980×760 logical pixels and floats on tiling window
managers with no window rule: it maps with its minimum size equal to its
maximum, which sway, Hyprland, niri, and X11 managers such as i3 float, then
drops the maximum so you can resize it down to 860×560 or up to any size.
macOS and Windows open the same resizable window.

Settings live in `settings.json` in Hex's data directory, the same file macOS
and Windows use.

```sh
hex                             # open the window, starting Hex if needed
hex start                       # start Hex in the background; no window
hex status [--json]             # query the running Hex, not an old log entry
hex quit [--force]              # finish accepted work, release the microphone, exit
hex restart [--force]           # restart the systemd unit in this desktop's environment
                                # (over SSH: the one it last ran in)
hex settings set sound_effects off   # every control command (docs/cli.md)
journalctl --user -u hex.service # diagnostics of the supervised process
```

`hex app` is the same as `hex`, `hex start` runs `hex app --hidden` in the
background and waits until Hex answers, and `hex stop` is the same as
`hex quit`; older desktop and autostart entries keep working after an update
replaces only the executable. `hex quit` and `hex restart` refuse while a
dictation records or transcribes unless `--force`. Linux runs the same
control commands as macOS ([docs/cli.md](cli.md)): `settings`, `model`,
`window`, `vocabulary`, `history clear`, `check-for-updates`, `onboarding
reset`, `annotate status`, `launch-at-login` (the XDG autostart entry),
`logs`, `commands init|check`, `history`, `meetings`, and `mcp`.

A launch first asks the running Hex over its socket. If one answers, it
forwards `Activate` with the launcher's `XDG_ACTIVATION_TOKEN` or
`DESKTOP_STARTUP_ID` and exits 0; the running process opens or focuses its
window. If none runs and the installer's (or Home Manager's) `hex.service` user
unit is loaded, the launch writes an owner-only `session.env` and starts the
unit, which reports readiness to systemd (`Type=notify`) once its socket
answers, then forwards `Activate`. Otherwise (a custom data directory,
`cargo run`, a desktop without systemd) the launching process becomes Hex
itself. Two racing launches produce one Hex; the loser forwards and exits 0.

`session.env` holds the display and session variables, the cursor theme and
input method (`XCURSOR_THEME`, `XCURSOR_SIZE`, `XMODIFIERS`, `GTK_IM_MODULE`,
`QT_IM_MODULE`), and, when set, `XDG_DATA_DIRS` and the locale (`LANG`,
`LANGUAGE`, `LC_ALL`, `LC_CTYPE`, `LC_MESSAGES`). Missing display variables
explicitly clear stale Wayland/X11 values inherited by the user manager; missing
locale and data-directory variables are left to the user manager. A saved
`XAUTHORITY` whose file is gone (Xwayland writes a new cookie at each login, so
a reboot strands the old path) is replaced at startup by the user manager's
current value, or dropped so Xlib uses `~/.Xauthority`. A unit started with no
display at all (the desktop never imported its environment into systemd) adopts
the user manager's `WAYLAND_DISPLAY`, else the newest Wayland socket of yours in
`XDG_RUNTIME_DIR` that answers. Hyprland imports nothing by default; add this to
its config so the service, its paste helpers, and the HUD get your session:

```ini
exec-once = dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP
```

When the HUD can't show, Hex's log says why: no display, no layer-shell (and
no XWayland), or the overlay failing to open, with its error. No
permissions are granted. The unit is `PartOf=graphical-session.target`;
desktops managing a custom session target must bind its lifetime to their
login/logout lifecycle. Systemd restarts a crashed Hex (`Restart=on-failure`)
without a window; after **Update**, the restarted Hex reopens the window.
There is one Hex per user: a launch from another desktop session restarts the
supervised unit in the new session, and refuses to take over an unsupervised
one (quit it first).

The socket is owner-only in Hex's data directory, with same-user peer
credentials, versioned bounded frames, bounded queues, and I/O deadlines. It is
not a network API or the macOS embedded transcription SDK service. It carries
`Activate`, `Status`, and Annotate's recording commands. The `hex` control
commands reach Hex through its local API's `POST /control`; dictation
commands go straight from there to the listener, never through the window.

For development, run `cargo run --release`. `hex dictate` remains an explicit
foreground diagnostic listener without the window or HUD and cannot coexist
with Hex's listener. With
`HEX_APPLICATION_SUPPORT_DIR` set, launches never start, stop, or restart the
installed user unit; they forward to the Hex using that data directory or
become it. `HEX_SYSTEMD_UNIT` names another unit for isolated tests.
`hex logs -n 40` is the diagnostic-log view (`hex status --lines` is gone).

The window runs on Rune's app lifetime (`rune::App`): Hex starts it without a
window, each `Activate` opens or focuses it with the forwarded token (on
Wayland through `xdg_activation_v1`; without a token most compositors only
mark it urgent), and closing it keeps the window's state for the next open.

## Install A Published Build

Published builds exist for x86_64 and ARM64, as a bare executable, a `.deb`,
and an `.rpm`. They are built on Ubuntu 24.04 and need glibc 2.39 or later:
Ubuntu 24.04, Debian 13, Fedora 40, current Arch, or newer. ARM64 builds use
Vulkan like x64 and fall back to the CPU.

### Install script

The script installs the bare executable for your architecture into a
user-local layout (`~/.local/share/hex`) that updates itself. It installs the
stable release, or the newest dev build (on the Dev update channel) while
there is no stable release yet; set `HEX_INSTALL_CHANNEL=stable` or `=dev` to
choose:

```sh
curl -fsSL https://anoma.ly/hex/files/install-linux.sh | sh
```

If a runtime library is missing, the installer names the packages to install
(with Arch names). Then open Hex from your application launcher, or run
`~/.local/bin/hex`; its first run downloads a speech model.

To install the dependencies up front, or to inspect the installer before
running it:

```sh
sudo pacman -S --needed alsa-lib curl jq \
  libxkbcommon libxkbcommon-x11 libx11 libxcb openblas openssl util-linux \
  vulkan-icd-loader wl-clipboard wtype xxd
curl --proto '=https' --tlsv1.2 -fsSLO \
  https://anoma.ly/hex/files/install-linux.sh
less install-linux.sh
sh install-linux.sh
~/.local/bin/hex model install
~/.local/bin/hex
```

The installer verifies the signed release feed, exact artifact size, and
SHA-256 before installing. It uses no root access. It creates the managed
version layout, `~/.local/bin/hex`, a desktop launcher, and an XDG autostart
entry and a `hex.service` user unit. It does not start recording during installation.
General › **Launch at login** turns that autostart entry on or off
(`~/.config/autostart/anomaly.hex.desktop`); Hex leaves an entry it did not write alone.
Where the desktop has the Background portal (GNOME, KDE), Hex asks it at startup
to run in the background, and Launch at login goes through it: the portal writes
and removes that same autostart entry (marked `X-XDP-Autostart=anomaly.hex`).
Elsewhere Hex writes the entry itself. The launcher entry below is written only
when no installed package or data directory provides one, and never rewritten
while it's there.
The launcher entry is `~/.local/share/applications/anomaly.hex.desktop`, named
for Hex's app id `anomaly.hex`, which its windows also report. Earlier installs
named both entries `hex.desktop`; Hex and the installer rename Hex's own.
Its icon is `anomaly.hex`: packages install it in the hicolor theme, and a
direct install gets it in `~/.local/share/icons/hicolor` when Hex first starts.

Remove the application while preserving logs and settings with:

```sh
sh install-linux.sh uninstall
```

### Upgrading from 2.3.1

HEX 2.3.1 read every `/dev/input` device on Wayland, so it needed the `input`
group. Current builds don't: they use the GlobalShortcuts portal or `hex`
commands bound in your desktop (see [Shortcuts](#shortcuts)). Don't grant input
access; install the new build instead.

The new build is a separate install in `~/.local/share/hex` and imports
nothing from 2.3.1's `~/.local/share/voice-control`, so set up the model and
settings again. The installer refuses to replace 2.3.1's files, so remove them
first (the paths below assume the default XDG directories):

```sh
systemctl --user disable --now hex.service
rm -f ~/.config/systemd/user/hex.service ~/.local/bin/hex \
  ~/.local/share/applications/hex.desktop ~/.config/autostart/HEX.desktop
systemctl --user daemon-reload
rm -rf ~/.local/share/voice-control   # optional: 2.3.1's settings, logs, and models
curl -fsSL https://anoma.ly/hex/files/install-linux.sh | sh
~/.local/bin/hex model install
~/.local/bin/hex
```

On Hyprland, bind the shortcut in your config as described in
[Shortcuts](#shortcuts).

### Debian, Ubuntu, Fedora, and openSUSE

The `hex` package installs `/usr/bin/hex`, the desktop entry, and the systemd
user unit `hex.service` (the same files as the AUR package below). Download
it for your architecture from <https://anoma.ly/hex/download/linux-x64-deb>
(`linux-arm64-deb`, `linux-x64-rpm`, `linux-arm64-rpm`), then:

```sh
sudo apt install ./hex.deb            # Debian, Ubuntu
sudo dnf install ./hex.rpm            # Fedora (zypper install on openSUSE)
systemctl --user enable hex.service   # optional: start Hex at login
```

There is no apt or dnf repository yet. Hex never replaces a package-installed
executable: when a new stable release is out, Settings shows the command that
downloads and installs its package. Package installs follow the stable channel,
so a package from the dev channel learns of the next stable release only.

### AUR

From the next release on, the AUR package `hex-bin` installs `/usr/bin/hex`,
the desktop entry, and the systemd user unit `hex.service`, from the same
release executable, for `x86_64` and `aarch64`. It is not on the AUR yet. With
an AUR helper:

```sh
yay -S hex-bin
systemctl --user enable hex.service   # optional: start Hex at login
```

pacman updates it; Hex never replaces a package-installed executable, and
Settings says "Updated by your package manager". It is unrelated to the
official `hex` package (a hex dump tool that installs `/usr/bin/hx`).

## Install From Source

Install the native dependencies and, for a source build, the Rust toolchain:

```sh
sudo pacman -S --needed base-devel git rustup python alsa-lib curl jq openssl xxd \
  util-linux libxkbcommon libxkbcommon-x11 libx11 libxcb openblas \
  vulkan-headers vulkan-icd-loader \
  shaderc spirv-headers clang cmake pkgconf wl-clipboard wtype
rustup default stable
```

Build and install Hex as your desktop user:

```sh
git clone https://github.com/anomalyco/hex.git
cd hex
./scripts/install-linux.sh
~/.local/bin/hex model install
~/.local/bin/hex
```

Do not run the installer as root. It installs a user-local binary, desktop
launcher, and autostart entry in the same layout as published builds, and
marks the version as a source build (`source-build` beside the executable).
The in-app updater never replaces a source build; Settings shows "Updated from
your source checkout", and you update by pulling and running the installer
again. Installing a published release over it switches back to signed
updates. With a custom
`HEX_APPLICATION_SUPPORT_DIR`, `cargo run --release` runs an
unmanaged development Hex instead; without one it forwards to the installed Hex.

## Dictate

Hold **Alt-Space**, speak, then release. Double-tap the shortcut to keep
recording, and press the same chord again to finish. Escape never cancels:
every dictation finishes and pastes. **Double-tap only** waits for two complete taps before
recording, and **Toggle** mode starts and stops with single presses.
**Ctrl-Alt-V** pastes the last dictation again. Settings shows listener state,
Start/Stop, and operational errors. Changing a shortcut or a setting the
listener reads at start temporarily stops listening and restores the previous
running state after the edit.

Modes apply corrections, OpenCode rewriting, and transformations per
application or website; the application picker lists `.desktop` entries. On
X11, Hex reads the focused application from the active window's process name
(`code`, `chrome`). On Wayland, compositors with the foreign-toplevel protocol
(wlroots compositors such as Sway, niri, Hyprland) report the activated
window's app ID, which Hex names by its `.desktop` entry
(`google-chrome-stable`, `firefox`). GNOME and KDE don't report it, so the
Global mode applies there.

Website rules read the focused page's address through AT-SPI, the desktop
accessibility bus, on X11 and Wayland. Firefox exposes it whenever the
accessibility bus is running. Chrome and other Chromium browsers expose it
only with accessibility turned on: launch them with
`--force-renderer-accessibility` (for example in
`~/.config/chrome-flags.conf`), or run a screen reader. Without it, the
application rule applies. Completed dictations are kept in History.

**Microphone mode** can release the microphone between dictations; it then opens
on the shortcut press without pre-roll. **While dictating** can mute the default
output through `wpctl` (PipeWire) or `pactl` (PulseAudio), or pause playing
MPRIS media players over D-Bus, once the hold passes 300 ms, so shortcut chords
and short taps never do; Hex restores only what it changed.
Cancelling shortcut capture preserves the old binding unless the new binding
has already been saved; listening resumes only if it was previously running.

### Bluetooth headsets

A Bluetooth headset can't play high-quality audio (A2DP) and use its
microphone at once. While any application records from the headset's
microphone, PipeWire (WirePlumber) or the headset switches it to the
hands-free profile (HFP/HSP): mono, call-quality sound in both directions.
This is how Bluetooth audio works, not something Hex chooses, and how quickly
the headset switches back depends on the headset and WirePlumber's settings.

If Hex records from the headset's microphone, the two **Microphone mode**
choices trade off differently:

- **Release when idle** opens the microphone only when you press the shortcut,
  so each dictation switches the headset to hands-free and back. Playback
  drops out or pauses during the switch, and the microphone takes longer to
  open, so the first words can be lost.
- **Keep ready** keeps the microphone open, so the headset stays in hands-free
  mode, with call-quality playback, for as long as Hex listens.

The recommended setup is a separate microphone for input (the laptop's
built-in microphone or a USB microphone) and the Bluetooth earbuds for output
only. Pick that microphone in General's **Microphone** list, or make it the
system's default input (for example with `wpctl set-default`), so nothing
opens the headset's microphone.

General › Advanced's Bluetooth option (`avoid_bluetooth_microphone`, on by
default) helps only part of the way:

- It applies only while the microphone is **System default** and the default
  input is a Bluetooth headset. A Bluetooth microphone you picked in Settings,
  or `--device`, is always used as named.
- It needs another hardware input that isn't reported unplugged, and
  `pactl` (PipeWire's PulseAudio server or PulseAudio) to move the stream.
  With no other input, Hex records from the headset.
- On Linux, Hex can only open ALSA's default device, which is the Bluetooth
  source, and then moves its own stream to the other microphone. The stream
  is briefly on the headset first, so the headset may still switch to
  hands-free mode, and Hex doesn't switch its profile back. Forcing the
  headset's profile is up to your sound settings.
- It changes only Hex's own recording, never other applications or the
  system default.

To avoid the switch entirely, use the recommended setup above.

## Shortcuts

Hex gets its shortcuts one of three ways, and all of them drive the same
dictation rules:

- **X11:** Hex grabs the shortcuts Settings records.
- **Wayland with the GlobalShortcuts portal** (for example niri with
  `xdg-desktop-portal-gnome`, GNOME, or KDE): Hex asks the desktop for the
  shortcuts Settings records. The desktop decides: it may grant them, assign
  another key, or refuse one that it or another binding already uses. Settings
  lists what the desktop granted under **Desktop shortcuts**, with an error for
  any it refused. A desktop may keep an older binding when you record a new
  shortcut (KDE does); where its portal supports it, **Desktop shortcuts >
  Open** shows the desktop's own dialog for Hex's shortcuts, and Settings
  follows what you choose there. The portal identifies Hex by its app id
  `anomaly.hex`, its installed `anomaly.hex.desktop` entry, and by inspecting
  the Hex process. A downloaded executable has no entry, so Hex writes a minimal
  `~/.local/share/applications/anomaly.hex.desktop` that runs it (a managed
  install replaces it); if the portal still can't find it, restart the portal with
  `systemctl --user restart xdg-desktop-portal`. Run `hex service` directly:
  a setgid wrapper such as `newgrp`, once used to grant input-device access,
  makes the portal refuse Hex. The desktop delivers the keys to Hex, not to the
  focused application. On GNOME, new setups dictate with Ctrl+Alt+Space, because GNOME keeps
  Alt+Space for the window menu; Settings warns when a shortcut uses a key GNOME
  keeps for itself. Keys you change in GNOME Settings › Keyboard work at once,
  and Hex's Settings and `hex status --json` show them. Annotate's shortcut (F9
  by default) sits under the dictation shortcut in General. **Disable** (General or the tray) keeps Hex disabled
  across restarts until you enable it again. On GNOME, Disable also removes
  Hex's entries from Settings › Keyboard › Global Shortcuts; Enable then asks
  you to approve them again. Hex also removes the duplicate list that versions
  before `anomaly.hex` left under the id `hex`.
- **Hyprland** has the portal but takes every shortcut without a key, so
  nothing happens until you bind one in your config, and the key Settings
  records is not used. Hex can't tell whether your config binds it. Prefer
  keys without modifiers, because Hyprland adds every modifier you still hold
  to Hex's paste (see [Native Wayland Requirements](#native-wayland-requirements)). For example:

```lua
hl.bind("End", hl.dsp.global("hex:dictation"))
hl.bind("F9", hl.dsp.global("hex:paste-last"))
```

  Older Hyprland configs write `bind = , End, global, hex:dictation`.
  Remove any `hex:cancel-dictation` bind: Hex no longer has it. Hyprland 0.56's Lua binds send no release when the modifier comes
  up before the key (Alt before Z); 0.57 fixes this. On 0.56, Toggle is
  unaffected: the next press still finishes. In Hold, recording continues
  until the next press and release, so lift the key first or use Toggle.
- **Wayland without the portal** (for example Sway): nothing registers global
  shortcuts. Settings lists these commands; bind them in your desktop's own
  keyboard configuration:

```sh
hex dictation start     # bind to a key's press: start recording
hex dictation stop      # bind to the same key's release: stop and transcribe
hex dictation toggle    # one key: start, then stop and transcribe
hex paste-last          # paste the last dictation again
hex dictation toggle --raw  # dictate without processing: paste the words as heard
```

For example, hold-to-dictate on F9 in Sway, and a toggle in any desktop that
runs commands on key presses only:

```sh
bindsym F9 exec hex dictation start
bindsym --release F9 exec hex dictation stop
bindsym Ctrl+Alt+d exec hex dictation toggle
```

GNOME 46 and older (Ubuntu 24.04) have no shortcuts portal. There, Settings ›
Desktop shortcuts › **Add GNOME keyboard shortcuts** writes Hex's own entries
into GNOME Settings › Keyboard › Custom Shortcuts with your shortcuts:
`hex dictation toggle` (GNOME runs custom shortcuts on the key press only, so
dictation toggles), `hex paste-last`, and `hex annotate toggle` while Annotate
is on. Hex removes them while it's disabled and puts them back when you enable
it; **Remove** takes them out for good. Keys you change in GNOME Settings stay.
Hex never touches your other custom shortcuts.

`hex dictation stop --print` stops the same way, then waits up to 60 s and
writes the transcript to stdout instead of pasting it, after the same
processing a paste gets. Only that command waits; the listener stops
recording at once, so a following `start` isn't blocked. No speech exits 4
and prints nothing; a failure, cancel, or timeout prints nothing, exits 1,
and never pastes. History keeps the text as Printed, and Paste Last skips it.
See `docs/cli.md`.

The commands are safe to bind anywhere, including X11 beside the grabs. They
talk to the running Hex and never start it: without one, or while Hex is
not listening, they print a one-line error and exit non-zero. Starting while recording,
and stopping while idle do nothing, so key repeat is
harmless. Start and stop bypass Hold, Toggle, and double-tap: start records,
stop finishes. A shortcut press also finishes a recording a command started.

Dictation has no cancel (#786): Escape always stays with your applications,
and every dictation finishes and pastes. To drop one, finish it and delete the
text. On KDE, Hex removes the Cancel dictation shortcut older builds asked for,
so its key returns to your applications.

Shortcuts need a non-modifier key. Modifier-only shortcuts, such as Super
alone, are not supported on Linux.

On X11, Hex grabs Escape only while Annotate draws, where it stops drawing. If
another application holds it, drawing goes on and Hex logs it once.

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

Closing the window leaves Hex running on both X11 and Wayland. The
recording/processing pill never takes focus or intercepts clicks, with or
without the window open: a Rune overlay that is a layer-shell surface on
Wayland and an override-redirect window on X11. On a Wayland compositor without
layer-shell (GNOME), only the pill opens on XWayland, as that X11 window with an
empty input shape on the primary monitor; the rest of Hex, including the window
and portal shortcuts, stays on Wayland. It needs the window's event
loop, so it is unavailable when the window cannot start (no display or GPU)
and in `hex dictate`. **Recording indicator** places it at the top or bottom of the
screen, or turns it off.

Insertion uses the clipboard and **Ctrl-V** for new installations. Enable
**Terminal paste shortcut** in Settings for targets that require **Ctrl-Shift-V**.
Existing X11 settings retain their previous Ctrl-Shift-V behavior until changed.
The target application must accept the selected binding.

After a paste, Hex puts the previous clipboard back, as on macOS
([#391](https://github.com/anomalyco/hex/issues/391)). Before writing the
transcript it reads every format the clipboard offers (text, rich text, images),
up to 32 MiB within one second, and 500 ms after the paste shortcut serves them
again from Hex until something else copies. It restores only if nothing newer
replaced the transcript in the meantime: a later dictation, a copy, or another
application. An empty clipboard stays empty. A larger or unreadable clipboard
is left holding the transcript, and Hex's log says why. When automatic paste
fails, the transcript stays on the clipboard for Ctrl+V. Nothing confirms the
target read the paste, so a target stalled past the delay can paste the
restored clipboard instead (see [#24](https://github.com/anomalyco/hex/issues/24)).
On Wayland, restoring needs the compositor's `ext-data-control` protocol
(KDE Plasma 6, recent wlroots compositors) or `wlr-data-control` (wlroots
compositors, niri, Hyprland); without either the transcript stays.

On GNOME, Hex pastes through the RemoteDesktop portal, and GNOME shows its
remote-control indicator while that session is open. Hex closes it 15 seconds
after the last paste and reopens it at the next one with the grant you
approved, so you're asked only once.

## Native Wayland Requirements

- Export the compositor's nonempty `WAYLAND_DISPLAY` in the launch environment.
  Hex and its window (winit) select the same native backend even when XWayland's
  `DISPLAY` is also present. `XDG_SESSION_TYPE` alone does not select Wayland.
- Install `wl-clipboard`. Hex copies each transcript, then sends Ctrl+V (or
  Ctrl+Shift+V with terminal paste) through one of two general paths, chosen at
  the first paste:
  - `wtype`, when the compositor offers the virtual keyboard protocol
    (`zwp_virtual_keyboard_manager_v1`; wlroots compositors and niri) and
    `wtype` is installed.
  - Otherwise the desktop's RemoteDesktop portal
    (`org.freedesktop.portal.RemoteDesktop`; KDE, GNOME). The first paste asks
    you to allow Hex to control the keyboard; Hex keeps the portal's restore
    token (`remote-desktop-token` in Hex's data folder), so it asks once. Revoke
    it in the desktop's settings.
  - With neither, the dictation fails with "Copied to the clipboard; press
    Ctrl+V to paste" and the transcript stays on the clipboard.

  There is no silent XWayland, direct-typing, uinput, or privileged
  input-injection fallback. Transcript text travels on stdin, not command-line
  arguments.
- The overlay uses layer-shell. Without it (GNOME), only the HUD opens on
  XWayland as an override-redirect, click-through window that never takes
  focus. With neither, Settings reports that the HUD is unavailable rather than
  presenting a normal focus-taking window as an overlay.
- Shortcuts come from the GlobalShortcuts portal or from your own keybindings
  for the `hex` commands ([Shortcuts](#shortcuts)). Hex reads no input devices
  and needs no `input` group membership or other device permission. If the
  desktop closes the portal session, the listener stops with an error and an
  active recording is cancelled; enable it again from Settings or `hex start`.
- Hex can't see which keys are down on Wayland, and some compositors merge
  held physical modifiers into its paste: on Hyprland, Alt still held from
  Alt+Z turns `wtype`'s Ctrl+V into Ctrl+Alt+V, and a paste-last bound to
  Ctrl+Alt+V is matched by Hyprland again and never reaches the app. The
  RemoteDesktop portal shares the desktop's keyboard the same way. So when a
  shortcut with modifiers asks for a paste, Hex waits for its release (the
  portal's Deactivated) plus 250 ms, at most 2 s after a press whose release
  never comes. After a `hex` command, whose keybinding Hex can't see, it waits
  250 ms. A modifier still held past that changes the paste, so bind keys
  without modifiers (End, F-keys) or let go of the modifiers before the
  transcript arrives. Hex logs a dictation as pasted once it sent the paste
  shortcut; it can't confirm the application accepted it.

## Updates And Validation

Managed installs check for signed updates when Hex starts and every
24 hours until an update is ready. Hex downloads, verifies, and activates an
available update, then offers an **Update** button in the window that restarts
the unit and reopens the window. The next launch
through `~/.local/bin/hex` also uses the activated version. The signed update
path is implemented but still awaiting a complete cross-version validation on
the supported Arch/i3 host.

See [`plans/linux.md`](plans/linux.md) for the engineering contract and remaining
validation work. The native Wayland implementation builds on the feature
proposed in [PR #28](https://github.com/anomalyco/hex/pull/28), with separate
input-state, lifecycle, and paste regression coverage. Physical device,
compositor, and target-application smoke tests remain necessary before a release.
`scripts/test-wayland-paste.sh /path/to/compiled-test-executable /path/to/hex` runs the real
paste helpers against a GTK target under an isolated headless Sway compositor.
Its first paste runs while another virtual keyboard holds Ctrl and Alt.
It then checks that every format of the previous clipboard comes back after the
paste, and that an empty clipboard stays empty.
It also starts an isolated Hex, opens its window through a second `hex app`
(the window must belong to the same process), repeats paste while the window is
open, closes the window, checks that the same process still answers, opens the
window again (`HEX_WAYLAND_REOPEN=0` skips that), and quits. It requires the GTK development
libraries, Sway, `wl-clipboard`, `wtype`, `dbus-run-session`, and a Vulkan driver
(Mesa's software driver is sufficient), and must run as a non-root user. It uses
isolated app data and no microphone; CI runs it with the display-free suite and
Xvfb hotkey tests.

`scripts/test-linux-dictation.sh /path/to/hex /path/to/parakeet-unified-en-0.6b-Q8_0.gguf /path/to/jfk.wav`
checks the complete X11 app dictation path with a private PulseAudio virtual
microphone and Xvfb display. It opens the window in the running Hex through a
second `hex app`, replays the public JFK sample from whisper.cpp, holds and releases the real
shortcut, verifies the transcribed words in a GTK editor, and checks orderly
application shutdown and microphone release. It needs PulseAudio, its ALSA
plugin, `xdotool`, Xvfb, the GTK development libraries, and a Vulkan driver for
the window (Mesa's software driver is sufficient). It runs as a normal user with
temporary settings; it does not use physical audio devices or personal recordings.
CI downloads and verifies the pinned model and sample for this check.

`python3 scripts/test-linux-service.py /path/to/hex` is a display-free,
device-free single-instance check: commands fail fast without Hex, `hex start`
becomes Hex, second launches (`start`, `app --hidden`, `service`, `app`) forward
and exit 0, a window request without a display fails only the window,
same-user socket permissions, version-1 clients get an error, invalid,
oversized, and stalled frames, crash/stale-socket recovery, two racing launches
yield one Hex, `hex quit` drains and removes the socket, and a custom data
directory never runs `systemctl`.
