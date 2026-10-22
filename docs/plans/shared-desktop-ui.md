# Share The Desktop UI Across macOS, Windows, And Linux

**Status:** Implemented. macOS, Windows, and Linux open the same `AppWindow`
and read and write the same `settings.json`. Each OS supplies only its chrome
and its backends.

## One Window, Per-OS Modules

`src/app_window.rs` renders every pane. A `Capabilities` constant decides which
panes and rows exist on an OS, so a missing capability omits a row instead of
showing a disabled imitation. Each OS module provides the same free functions
and `impl AppWindow` hooks:

```text
src/app_window.rs            shared panes, rows, and settings edits
src/app_window/macos.rs      traffic-light chrome, permissions, Meetings,
                             Commands, Sparkle, Dock icon
src/app_window/windows.rs    40 px caption and Fluent icons, the listener host,
                             tray, HUD, updater, keyboard-state shortcut capture
src/app_window/linux.rs      the Settings client of hex.service: shortcut
                             capture, model preparation, and settings reloads
                             go through the service
```

| Capability | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Modes, Voice Action, History | Yes | Yes | Yes |
| Commands, Meetings, permissions, Dock icon | Yes | No | No |
| Listener card and Enable on launch | No | Yes | Yes |
| Website mode rules | Brave | Any browser (UI Automation) | No |
| Left/right modifier sides | Yes | No | No |
| Terminal paste | No | No | Yes |
| Launch at login | Yes | Yes | No (the installer or Nix owns autostart) |

## One Settings File

`AppSettings` is the only settings type. Shortcuts are one `HotkeyBinding`:
macOS keys by layout keycode, Windows and Linux keys by name, each with its own
keycaps and defaults. `windows-settings.json` and `linux-settings.json` migrate
once into `settings.json` and stay behind as `*.migrated`. One-time changes,
such as moving English dictation to the streaming model, record their ID in
`migrations` so they never run twice.

## Shared Dictation Behavior

The Windows and Linux listeners share `dictation_modes::ModeRuntime` for mode
selection and processing, and both record History. Linux's service registers
the shortcuts Settings records (X11 grabs or the Wayland shortcuts portal),
prepares models, and applies saved settings live;
History rereads its file when the other process wrote it.

## Interface Language

Every string the shared window shows goes through `desktop_i18n::tr`,
`tr_fill`, or `tr_named`, with Polish, Chinese, Japanese, German, and Spanish
translations. A test fails if any translated string lacks a language.
