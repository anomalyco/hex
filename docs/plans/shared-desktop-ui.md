# Share The Desktop UI Across macOS, Windows, And Linux

**Status:** Implemented, now on Rune. macOS, Windows, and Linux open the
same window (`src/rune_ui.rs`) and read and write the same `settings.json`.
Each OS supplies only its host and its native pieces; the GPUI `AppWindow`
this plan first described is deleted (see [rune-ui.md](rune-ui.md)).

## One Window, Per-OS Hosts

`src/rune_ui.rs` and its pane modules render every pane. `platform::Capabilities`
decides which panes and rows exist on an OS, so a missing capability omits a
row instead of showing a disabled imitation. Each OS connects the window to
its runtime through a host module:

```text
src/rune_ui/macos_host.rs    the menu-bar runtime: permissions, Meetings,
                             Commands, updater, Dock icon, chord capture
src/rune_ui/windows_host.rs  the tray runtime: listener host, updater,
                             keyboard-state shortcut capture, Launch at login
src/rune_ui/linux_host.rs    the in-process Linux host: shortcut capture,
                             model preparation, and settings adoption
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
keycaps and defaults. One-time changes,
such as moving English dictation to the streaming model, record their ID in
`migrations` so they never run twice.

## Shared Dictation Behavior

The Windows and Linux listeners share `dictation_modes::ModeRuntime` for mode
selection and processing, and both record History. On Linux, the single Hex
process owns the window, registers the shortcuts Settings records (X11 grabs or
the Wayland shortcuts portal), prepares models, applies saved settings live, and
shares `History` in-process (also rereading the file when a standalone CLI run
wrote it).

## Interface Language

Every string the shared window shows goes through `desktop_i18n::tr`,
`tr_fill`, or `tr_named`, with Polish, Chinese, Japanese, German, Spanish,
and Ukrainian translations. A test fails if any translated string lacks a
language.
