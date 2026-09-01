# Nix

The flake packages HEX for `x86_64-linux`. It supports the Linux X11 beta
and native Wayland on compatible wlroots compositors. Wayland requires
layer-shell for the recording overlay, the clipboard protocol used by
`wl-copy`, the virtual-keyboard protocol used by `wtype`, and read access to
every `/dev/input/event*` device for global hotkeys. This is not a claim of support
for every Wayland desktop; GNOME and KDE are not covered by this contract.

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

This installs HEX system-wide. It does not start a root service, enable
autostart for every account, or change input-device permissions. Use the
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

Autostart defaults to `false`. When enabled, HEX is a systemd **user**
service, wanted by `graphical-session.target` by default. It waits for a
Wayland socket or an X11 window manager before launching with `app --hidden`.
Settings stays visible on Wayland or without a usable X11 tray; closing it quits
HEX in those cases.
The service stops with the configured target and retries failed starts
without a tight restart loop. Quitting HEX normally leaves it stopped until
the next session or an explicit `systemctl --user start hex.service`.

Your session manager must import its current `DISPLAY`, `XAUTHORITY`,
`WAYLAND_DISPLAY`, and `XDG_RUNTIME_DIR` values as applicable into the systemd
user environment **before** starting the target, and stop the target at
logout. Use your compositor's systemd integration, such as Home Manager's
Sway integration, rather than starting the target from a shell login.
The module does not create a graphical session or grant permission to use
one. A bare `startx`/i3 session without systemd session integration will not
start this service automatically.

Home Manager's normal service-switch policy applies: newly enabled services
can start during a switch if the session target is already active, and
package changes can restart a running service. Do not switch during an
active recording. Inspect failures with:

```sh
systemctl --user status hex.service
journalctl --user -u hex.service
```

## Wayland Input Access

The native Wayland beta requires the intended user to be able to read
**every `/dev/input/event*` node**, as with deliberate `input`-group access.
Keyboard-only ACLs are insufficient: HEX cannot rule out held modifiers on
an unreadable device. Monitoring, shortcut capture, and paste fail with a
permission error if any event device is unreadable.

Neither module adds users to the `input` group, installs permissive udev
rules, or grants capabilities to HEX. Make this access decision explicitly
for the intended account; do not make event devices world-readable.
**Raw input access exposes all keystrokes on those keyboards, not just
HEX's hotkeys, and can expose other input devices.** Other processes running
as the same user receive the same broad access.

Wayland shortcuts use physical evdev key positions and US-style key labels,
not the current layout's characters. HEX observes the keys but does not
suppress them. The compositor or focused app can still act on the chord;
use an appropriate compositor binding if you need to reserve it. The paste
helper is `wtype`; no privileged input-injection fallback is installed.

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
