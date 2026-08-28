## Linux Wayland And Nix Beta

Unreleased. Physical input, microphone, and supported-host validation remain
release gates; passing container tests alone does not establish those behaviors.

- Add native Wayland dictation on compatible wlroots compositors, with explicit
  input-device permissions and a click-through recording/processing indicator.
- Keep shortcut changes, cancellation, keyboard reconnects, and tray-less
  listener recovery manageable from Settings.
- Cancelling a new capture keeps older pending transcription visible in Settings.
- Add Nix packaging and a matching development shell, plus optional per-user
  Home Manager autostart. Nix installations leave updates to Nix.

The beta builds on the proposals in [#28](https://github.com/anomalyco/hex/pull/28)
and [#29](https://github.com/anomalyco/hex/pull/29). It does not claim GNOME or KDE
support and does not grant input-device permissions automatically. See
[`../linux.md`](../linux.md) and [`../nix.md`](../nix.md) for the supported setup.
