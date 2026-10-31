# One Hex Process Per User, Hosting Rune Windows

**Status:** Implemented on `dev` for every OS; GPUI is deleted (see
[`rune-ui.md`](rune-ui.md)). Linux runs as one process per user and data root
with launch forwarding, the single-instance socket, the `Type=notify` systemd
user unit, and the Rune window and HUD overlay (see
[`../linux.md`](../linux.md#one-hex-process)); macOS is one menu-bar process
(`macos_rune.rs`); Windows is one tray process (`windows_rune.rs`).

## Architecture

Hex is one long-lived process per user and data root on every OS:

```ts
hex (no instance)    -> become the instance (or the supervisor starts it) -> open window unless hidden
hex (instance)       -> forward Activate{activation token} -> exit 0
hex dictation start  -> forward Shortcut -> exit 0 | non-zero without an instance (never starts one)
close window         -> Rune window closed or hidden; runtime, listener, HUD, and models stay
quit                 -> explicit: tray / menu-bar Quit, "Quit Hex" in the window, `hex quit` / `hex stop`
```

The **runtime thread** owns all Hex state and non-UI work (`LinuxDesktopHost`,
`WindowsDesktopHost`, or the macOS runtime), publishes an `Arc<Published>`
snapshot after every change, and wakes subscribed UI handles. Keeping the
runtime off the UI thread ensures dictation, model preparation, and shortcut
commands continue with zero open windows and never stall rendering.

The **main thread** runs `rune::App` (`windowless`, `keep_running`, `on_close`,
`AppHandle::{activate, open, update, quit}`, and `App::overlay`). Closing the
window hides it on macOS, Windows, and X11, and destroys the surface on
Wayland while preserving `Runtime<Ui>` state in-process.

## Linux

### Decision: Single-Instance Socket + Lock Per Data Root

Exclusivity and forwarding are keyed to the **data root** (`settings.json`,
History, the listener lock, models, and isolated `HEX_APPLICATION_SUPPORT_DIR`
roots used by tests, Nix, and development). `linux_service.rs` guards
`service.sock` in that root with `desktop::instance::acquire("service")`, owner-only
permissions, peer-UID checks, bounded version-2 frames, deadlines, and
stale-socket recovery (`scripts/test-linux-service.py`):

```rust
enum Request {
    Activate(Activation),        // activation_token, startup_id, session, show
    Shortcut(ShortcutCommand),   // straight from socket thread to listener queue
    Status,                      // Published snapshot for `hex status`
    Quit,                        // drain accepted work and exit
}
```

Rejected alternatives:

- **D-Bus well-known name (`org.freedesktop.Application`):** scoped per session
  bus rather than per data root (breaking isolated test/dev instances), requires
  renaming `hex.desktop` (invalidating `linux_portal::APP_ID` `"hex"` and
  window rules), and requires a glib main loop on the UI thread.
- **Abstract-namespace socket:** scoped per network namespace rather than data
  root and accessible to any local process in that namespace.
- **Lock file + signals:** cannot carry activation tokens, status replies, or
  structured errors.

### Decision: Systemd Stays The Login Supervisor

Managed installs and Home Manager keep the `hex.service` user unit with
`Type=notify` (`READY=1` after `service.sock` binds),
`PartOf=graphical-session.target`, and `Restart=on-failure`:

```ts
connect(socket) ok        -> Activate{token from XDG_ACTIVATION_TOKEN / DESKTOP_STARTUP_ID, session, show} -> exit 0
no instance, managed unit -> write session.env; systemctl --user start hex.service (Type=notify) -> Activate{token, show}
no instance, otherwise    -> become the instance in this process (custom data root, cargo run, non-systemd)
```

- Crash restarts run `ExecStart=… service` without opening a window; only an
  explicit `Activate` with `show: true` opens one.
- `session.env` imports display, session, cursor-theme, IME, `XDG_DATA_DIRS`,
  and locale variables into the supervised process so the Rune window renders
  with the desktop's theme and input method, while clearing stale X11/Wayland
  variables when switching session types.
- `service`, `app`, `app --hidden`, `start`, `stop`, and `quit` remain valid
  entry points so self-updates only swap the executable.
- At startup on Linux, `transcription::local_transcriber::initialize_backends()` creates
  `transcribe.cpp`'s Vulkan instance on the main thread before any worker or
  Rune window starts, avoiding concurrent Vulkan instance creation in the
  loader.
- The recording/processing pill is a Rune `App::overlay` (`linux_rune_hud.rs`):
  layer-shell on Wayland and override-redirect on X11.

## Windows under Rune

[`windows/rune.rs`](../../src/windows/rune.rs),
[`windows/app.rs`](../../src/windows/app.rs), and
[`rune_ui/windows_host.rs`](../../src/rune_ui/windows_host.rs):

- Single instance uses the named auto-reset event `Local\anomaly.hex.app`.
  A second launch grants `AllowSetForegroundWindow`, signals the event, and
  exits (`--hidden` exits without signaling).
- The main thread creates the `tray-icon` (pumped by winit's Win32 message loop)
  and runs `rune::App` with `keep_running()` (or `windowless()` for `--hidden`).
  Tray Show and the relaunch waiter call `AppHandle::open`; tray Quit, the
  sidebar's Quit Hex, Ctrl-C, and update restarts call `AppHandle::quit`.
  Without a tray, closing the window quits Hex.
- The runtime thread owns `WindowsDesktopHost` (listener, `TranscriptionPreparation`,
  OpenCode catalog, personal transformations, and `WindowsUpdater`), publishes
  `Arc<Published>`, and wakes the window.
- Shortcut capture polls keyboard state in the window while suppressing the
  low-level hook, because modifier-only chords do not arrive as window key
  events.
- The HUD (`windows_rune_hud.rs`) steps `desktop_indicator_model` and shades
  `desktop_indicator_shader` into `rune::Pixels` inside a topmost, click-through,
  no-activate Rune overlay.

## macOS under Rune

[`macos_rune.rs`](../../src/macos_rune.rs) and
[`rune_ui/macos_host.rs`](../../src/rune_ui/macos_host.rs):

- LaunchServices enforces single instance. The main thread runs
  `rune_ui::app().accessory()`, starting windowless when setup is complete and
  Show Dock icon is off, or open otherwise.
- Because winit owns `NSApplicationDelegate`, Hex registers an
  `NSAppleEventManager` handler for `kAEReopenApplication` to answer Dock and
  Finder reopen events.
- An initial `AppHandle::update` closure on the main thread starts the updater,
  `NSStatusItem` (`macos_rune/status_item.rs`), the Hex/File/Window main menu (omitting an
  Edit menu so ⌘C/⌘V reach Rune text fields), the reopen handler, and the
  native Metal HUD (`dictation_indicator.rs`). A 16 ms `NSTimer` in common
  run-loop modes drains HUD, status-item, meeting, and developer-control
  channels even when no window is open.
- Opening the window shows the Dock icon and activates Hex; closing hides the
  window and restores the saved Dock policy.
- The runtime thread owns `TranscriptionPreparation` (shared with the menu bar's
  installed-only model picker), starts the listener once setup is ready, and
  publishes state to the window and menu bar. The listener, meeting controller
  (`meeting_watcher::controller_loop`), and `local_api` run on their own
  threads.
- `macos_host::MacHost` implements `NativeHost` for permissions and guides,
  System Audio, the login-item worker, Hex Microphone, meeting actions and
  1 Hz `meeting::query` rescanning, the command catalog and workspace, and
  tour practice events. Shortcut capture uses an AppKit local event monitor
  with the listener tap suspended.
- Meeting capsules (`rune_ui/meeting_capsules.rs`) are interactive,
  non-activating Rune overlays driven by controller events on the main loop.
  The dictation HUD remains the native Metal `NSPanel` in
  `dictation_indicator.rs`.
