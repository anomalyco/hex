# Install The Linux X11 Beta

HEX currently supports one Linux beta target: x86_64 Arch Linux running i3 on
X11. Audio uses ALSA, including PipeWire systems with ALSA compatibility.
Inference uses Vulkan when available and can fall back to the CPU.

The beta does not support voice commands, browser context, meetings, native
Wayland, or package-manager installation.

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

## Dictate

Hold **Alt-Space**, speak, then release. Double-tap the shortcut to keep
recording, press it again to finish, or press Escape to cancel. Stop the listener
before changing the shortcut in Settings.

## Modes And Text Replacements

The Modes pane uses the same collection, application-rule, and phrase/output
editors as the other desktop builds. Global replacements always run first. A
custom mode matches the first comma-separated executable-name substring against
the active X11 client, then applies its corrections and optional OpenCode rewrite
profile. Global and per-mode profiles persist their model, reasoning variant,
deadline, and instructions through the same shared processing card used by the
other desktop builds. Rules match
case-insensitively at phrase boundaries, prefer the longest overlapping phrase,
and take effect while the listener is running.

HEX reads the active client through `_NET_ACTIVE_WINDOW`, resolves its
executable through `_NET_WM_PID` and `/proc` with `WM_CLASS` as a fallback, and
keeps the window title bounded. The Linux pane deliberately has no Websites
field until a real browser adapter exists. A replacement that leaves the entire
output empty is not pasted or added to History.

OpenCode is optional. When enabled, HEX asks the separately installed
`opencode2` service to rewrite corrected text after local transcription. Catalog
loading and rewriting stay off the audio-capture thread and are deadline-bounded;
an unavailable model, timeout, or empty response preserves the corrected text.
The prompt includes the resolved foreground application but no browser host on
Linux because the X11 beta does not yet have a browser adapter. Successful
History entries include bounded processing latency and fallback metadata.

Transformations run last, after corrections and any successful OpenCode rewrite.
The shared ordered editor includes built-in Lowercase and SpongeBob case steps,
which do not require another runtime. Custom TypeScript transformations use the
same bounded, watch-reloaded `@hex/commands` host as macOS and Windows. Choosing
**Set up** creates `~/.config/hex/hex.config.ts`, installs the managed embedded
SDK, and requires [Bun](https://bun.sh/). A missing, failed, or timed-out custom
step preserves the previous pipeline output and records bounded fallback
metadata instead of blocking capture or paste.

## Retained History

The History pane uses the same bounded, owner-only store and complete
list/detail UI as macOS and Windows. HEX records an entry only after text has
been pasted successfully; it keeps both the raw transcript and final pasted
text, the resolved foreground application when available, and bounded timing
metadata, never captured audio. The retention control can turn recording off or keep
entries for 24 hours, 7 days, 30 days, or indefinitely, all still subject to
the store's hard entry and byte caps. Search, selectable text, copy, individual
delete, and confirmed clear are available in the pane.

Managed installs check for signed updates at startup and every 24 hours. HEX
downloads, verifies, and installs an available update, then asks before
restarting into it. The signed update path is implemented but still awaiting a
complete cross-version validation on the supported Arch/i3 host.

See [`plans/linux.md`](plans/linux.md) for the engineering contract and remaining
validation work.
