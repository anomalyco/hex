# Nix

The flake packages HEX for `x86_64-linux`. It supports the Linux X11 beta
and native Wayland on compatible wlroots compositors. Wayland requires
layer-shell for the recording overlay, the clipboard protocol used by
`wl-copy`, and the virtual-keyboard protocol used by `wtype`. Global shortcuts
come from the desktop's GlobalShortcuts portal, or from your own compositor
keybindings running the `hex` commands. This is not a claim of support for every
Wayland desktop; GNOME and KDE are not covered by this contract.

## Run Or Install

From a checkout, with Nix flakes enabled:

```sh
nix build
nix run . -- model install
nix run . -- app
```

Or add `hex.url = "github:anomalyco/hex"` to your configuration's flake inputs
and install `hex.packages.x86_64-linux.default`. The package supplies the
`hex` command and desktop launcher, native libraries, and the `curl`,
`wl-copy`, and `wtype` runtime dependencies. Your system still needs a
working audio session and a Vulkan driver for Settings, even when inference
falls back to the CPU. On non-NixOS hosts, Nix GUI
applications may require the host's usual Nix graphics integration.

Models are downloaded into your user data directory, not the Nix store.
Run `hex model install` once, then launch HEX as your normal desktop user.
Do not run HEX as root.

The Nix store is not HEX's user-local managed installation layout. In-app
updates do not replace a Nix installation. Update the flake input and
rebuild or switch your Nix/Home Manager configuration instead. Remove any
old source-install XDG autostart entry before enabling a second autostart
mechanism; do not run two HEX listeners.

## NixOS

Import `hex.nixosModules.hex` and enable the package:

```nix
{
  imports = [ hex.nixosModules.hex ];
  programs.hex.enable = true;
}
```

This installs HEX system-wide. It does not start a root service or enable
autostart for every account. Use the
Home Manager module below for per-user autostart. Both modules accept an
explicit `programs.hex.package` override. The flake also exports
`hex.overlays.default` for configurations that prefer `pkgs.hex`.

## Home Manager

```nix
{
  imports = [ hex.homeManagerModules.hex ];
  programs.hex = {
    enable = true;
    autostart = true;
    # Use your compositor's session target when appropriate:
    # systemdTarget = "sway-session.target";
  };
}
```

The module always installs the systemd **user** service. Autostart defaults to
`false`; when enabled, the unit is wanted by `graphical-session.target` by default.
It waits for a Wayland socket or an X11 window manager before launching `hex service`,
which reports readiness to systemd (`Type=notify`) once its socket answers.
Hex is one process: `hex` opens its window, starting the unit when it is not
running, and `hex start` starts it without the window. Closing the window never
stops Hex; a tray icon shows when the desktop has a tray host. The unit stops with the configured target
and retries failed starts without a tight restart loop; a crash restart opens no
window. `hex quit` (or **Quit Hex** in the window) leaves it stopped until the
next session or the next launch.

Your session manager must import its current `DISPLAY`, `XAUTHORITY`,
`WAYLAND_DISPLAY`, and `XDG_RUNTIME_DIR` values as applicable into the systemd
user environment **before** starting the target, and stop the target at
logout. Use your compositor's systemd integration, such as Home Manager's
Sway integration, rather than starting the target from a shell login.
The module does not create a graphical session or grant permission to use
one. A bare `startx`/i3 session without systemd session integration will not
start this service automatically.

`hex start` records the current display/session environment in an owner-only
file read by the unit; it clears stale variables when switching between X11 and
Wayland. There is one Hex per user. Launching it from a different desktop
explicitly restarts the unit for that session.

Home Manager's normal service-switch policy applies: newly enabled services
can start during a switch if the session target is already active, and
package changes can restart a running service. Do not switch during an
active recording. Inspect failures with:

```sh
systemctl --user status hex.service
journalctl --user -u hex.service
```

## Wayland Shortcuts

HEX reads no input devices, so neither module adds users to the `input`
group, installs udev rules, or grants capabilities. With a GlobalShortcuts
portal, the desktop grants the shortcuts Settings records; the portal
identifies HEX by the package's `hex.desktop` entry, so keep the package's
`share/applications` on `XDG_DATA_DIRS` (NixOS and Home Manager profiles do).
Without the portal, bind the commands in your compositor, for example in Sway:

```sh
bindsym F9 exec hex dictation start
bindsym --release F9 exec hex dictation stop
```

`hex dictation toggle`, `hex dictation cancel`, `hex paste-last`, and
`hex voice-action start|stop|toggle` are also available; see the
[Linux guide](linux.md#shortcuts). The paste helper is `wtype`; no privileged
input-injection fallback is installed.

## Development

```sh
nix develop
cargo build --locked
cargo test --locked
cargo run --locked -- app
```

The package and shell share the native build environment and runtime
library/plugin paths. OpenBLAS link ordering is handled by `build.rs`, not
by Nix-only `RUSTFLAGS`. The shell also supplies the paste and download
helpers, so it does not rely on the installed app's wrapper. Desktop commands
still require a real graphical/audio session and the permissions described above.

`nix flake check` evaluates the module and shell-environment regressions and
builds the package with its normal display-free Rust tests. Tests explicitly
marked ignored because they require a desktop or installed model remain
ignored; the Nix build does not download models or exercise live input.

The initial packaging design builds on [PR #29](https://github.com/anomalyco/hex/pull/29).
