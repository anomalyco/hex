# Install The Linux X11 Beta

HEX currently supports one Linux beta target: x86_64 Arch Linux running i3 on
X11. Audio uses ALSA, including PipeWire systems with ALSA compatibility.
Inference uses Vulkan when available and can fall back to the CPU.

The beta does not support voice commands, application or browser context,
meetings, native Wayland, or package-manager installation. A Nix flake
packages the same X11 binary for `nix build` / `nix run`.

## Install A Published Build

After the first signed Linux release is published, download and inspect the
installer before running it:

```sh
sudo pacman -S --needed alsa-lib curl gtk3 jq libappindicator-gtk3 libxkbcommon \
  libxkbcommon-x11 libx11 libxcb openblas openssl util-linux vulkan-icd-loader xxd
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
  util-linux gtk3 libappindicator-gtk3 libxkbcommon \
  libxkbcommon-x11 libx11 libxcb openblas vulkan-headers vulkan-icd-loader \
  shaderc spirv-headers clang cmake pkgconf
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

## Install With Nix

The flake packages the Linux X11 beta. It does not change the desktop
contract. The Nix package is not the user-local managed layout, so HEX does
not apply its signed in-app updater.

```sh
nix build
nix run . -- model install
nix run . -- app
```

`nix develop` provides the native build inputs. Optional NixOS and Home
Manager modules are `hex.nixosModules.hex` and `hex.homeManagerModules.hex`.
After install, run `hex model install` once, then `hex app`.

## Dictate

Hold **Alt-Space**, speak, then release. Double-tap the shortcut to keep
recording, press it again to finish, or press Escape to cancel. Stop the listener
before changing the shortcut in Settings.

Managed installs check for signed updates at startup and every 24 hours. HEX
downloads, verifies, and installs an available update, then asks before
restarting into it. The signed update path is implemented but still awaiting a
complete cross-version validation on the supported Arch/i3 host.

See [`plans/linux.md`](plans/linux.md) for the engineering contract and remaining
validation work.
