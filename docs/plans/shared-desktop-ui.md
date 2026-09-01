# Share The Desktop UI Across macOS And Linux

**Status:** In progress. The shared visual vocabulary and transcription picker
are implemented. macOS and Linux both render with GPUI, but they still use
separate root entities. This plan converges them on one product shell while
keeping platform behavior in the existing macOS and Linux X11/Wayland hosts.

## One Product Shell Should Represent The Same Product Concepts

macOS renders the full `AppWindow` from `src/app_window.rs`. Linux renders a
second, smaller `LinuxApp` tree from `src/linux_app.rs`. The duplicate tree makes
shared concepts such as listener state, shortcuts, settings, updates, and
activity look and behave differently even though both applications use GPUI.

The target is one shared `AppWindow` for navigation, layout, controls, and
presentation. The macOS and Linux hosts continue to own behavior that actually
varies by platform.

```text
macOS lifecycle ----\
                     +-- shared GPUI AppWindow -- desktop actions
Linux lifecycle ----/                              |
                              +--------------------+--------------------+
                              |                                         |
                        macOS adapter                              Linux adapter
```

The shared UI must depend on product capabilities such as `meetings` or
`launch_at_login`, never on `is_macos` or `is_linux`. A missing capability omits
the corresponding pane or row. It must not produce a disabled imitation of a
feature that the runtime cannot perform.

## The Shared Window Owns Presentation

The shared window owns:

- navigation, page frames, typography, colors, controls, and animations;
- UI-only state such as focus, open pickers, selections, and draft inputs;
- listener, microphone, transcription, shortcut, update, and activity
  presentation;
- panes whose runtime capability exists on the current host;
- conversion of user interaction into domain actions.

Shared rendering decides how every common concept looks. Platform adapters must
not inject arbitrary GPUI elements because that would preserve two visual
systems behind a nominally shared shell.

The first shared panes are Settings and Activity. Modes (including corrections),
Voice Action, History, Commands, and Meetings are later candidates; each pane
appears on Linux only when its underlying behavior is implemented there.

## Desktop Hosts Own Consequential Behavior

The outer application hosts remain separate:

- The macOS host owns native menus, Dock policy, reopen behavior, TCC setup,
  `SMAppService`, Sparkle, meeting lifecycle, and the existing recognition
  coordinator.
- The Linux host owns the GTK tray, X11 map and unmap behavior, close-to-tray,
  X11 shortcut registration or Wayland evdev input, listener lifecycle, the
  layer-shell HUD, and signed direct updates.

At completion, both hosts open the shared `AppWindow` and the Linux host stops
implementing its own `Render` tree. Today they still use separate roots.

Use one real seam between the shared window and the two existing hosts. The
exact Rust types may evolve during the first slice, but the interface should
retain this shape:

```rust
trait DesktopHost {
    fn capabilities(&self) -> DesktopCapabilities;
    fn snapshot(&self) -> DesktopSnapshot;
    fn dispatch(&mut self, action: DesktopAction) -> Result<()>;
}
```

Periodic refresh remains adapter-local until the shared root owns the timer;
then it can join this interface as one product-level operation.

`DesktopSnapshot` contains presentation state rather than platform handles.
`DesktopAction` names product operations such as capturing a shortcut,
selecting a microphone, toggling double-tap lock, checking for updates, or
restarting into an installed update. The host hides threads, X11 connections,
Core Graphics values, GTK, Sparkle, and persistence details.

Do not create one trait per settings row. The host interface should remain a
deep module: a small product-oriented surface over the platform implementation.

## Capabilities Control Composition

Capabilities describe available behavior rather than operating-system identity.
The initial set should cover only behavior that currently varies:

```rust
struct DesktopCapabilities {
    activity: bool,
    commands: bool,
    history: bool,
    hud_lab: bool,
    meetings: bool,
    modes: bool,
    replacements: bool,
    listener_control: bool,
    update_restart: bool,
    voice_action: bool,
}
```

Navigation and settings composition derive from these values. Adding Linux
command support later changes the Linux capability and adapter; it does not add
a second Commands UI.

Presentation state normalizes platform-specific behavior through
`DesktopUpdateStatus`. Both updaters use `Unavailable`, `Checking`, and `Current`.
macOS also exposes `Available`; Linux adds `Failed` and uses `ReadyToRestart`
after staging an update. Sparkle and the signed Linux updater retain independent
implementations.

## Shortcut Presentation Is Shared But Validation Is Not

The shared shortcut editor uses a portable shortcut draft and a common visual
control. The host validates and persists the result.

- macOS accepts modifier-only, Globe/Fn, function-key, and modifier-plus-key
  bindings and resolves active-layout key codes.
- X11 requires a key-containing chord and verifies that the binding can be
  registered on the active X server.
- Wayland uses explicit US-labeled physical keys through read-only evdev input,
  with user-granted access to every `/dev/input/event*` node.

The UI displays validation results without learning either representation.
Existing macOS `settings.json` and Linux `linux-settings.json` remain compatible.
Do not combine or migrate persisted formats as part of this refactor.

## Implementation Proceeds In Vertical Slices

### 1. Share The Visual Vocabulary

**Implemented.** `src/desktop_ui.rs` now owns the shared frame, sidebar,
navigation item, pane, settings row, toggle, button, message, and color
presentation. Both current root entities use it without moving lifecycle or
persistence behavior.

- Extract the page frame, navigation, settings rows, buttons, toggles, labels,
  and visual constants already used by `AppWindow`.
- Render the current Linux behavior with those shared primitives.
- Preserve behavior and persistence; this slice changes presentation only.

### 2. Share Activity And Listener Presentation

**In progress.** `src/desktop_activity.rs` projects the latest listener state,
device, session boundary, dictation failure, and read error from `EventReader`
for both roots. Transcript rows still read retained events directly; the unused
transcript cache has been removed. Linux no longer interprets the event stream
independently. The macOS Activity header renders the shared listener-status
control. The Linux root shows only Settings, with listener controls there and in
the X11 tray menu; the detailed Activity pane remains macOS developer-only until
the shared root window lands.

- Move the common `EventReader` projection into the shared window state.
- Render status, device, completed transcripts, and failures once.
- Keep starting and stopping the listener in each platform host.

### 3. Normalize Core Settings For Presentation

**In progress.** Shortcut keycaps, the double-tap toggle, and the complete local
transcription picker now use shared GPUI presentation. `DesktopSnapshot` carries
those shortcut values, activity, listener status, operation errors, update status,
and transcription selection/preparation state. It no longer duplicates a
shortcut label, paste-last shortcut, or observations path without a UI consumer.
Shortcut captures cross the host seam as portable values. The shared picker
delegates model preparation to each current root while macOS and Linux validate,
prewarm, and persist their native runtime selections independently. Portable
microphone view state still remains.

- Introduce portable view state for shortcut, double-tap lock, microphone, and
  transcription controls.
- Translate `AppSettings` and `LinuxSettings` through their host adapters.
- Keep platform validation and atomic persistence behind the host seam.

### 4. Open One Shared AppWindow From Both Hosts

**In progress.** `src/desktop_host.rs` now defines semantic capabilities,
portable activity and shortcut snapshots, and typed actions. Linux product
state has moved into a contained `LinuxDesktopHost`; its GPUI root now retains
presentation state and delegates listener lifecycle, update polling, activity,
shortcut validation, update restart, and settings persistence. Those remaining
Linux operations now use portable snapshots and typed actions; only periodic
refresh remains lifecycle-driven. The existing macOS root remains the
transitional macOS host. macOS navigation derives from capabilities. Moving the
remaining render state into one root entity is still outstanding.

- Move `AppWindow` and its portable dependencies out of macOS-only module gates.
- Have the macOS lifecycle coordinator and Linux tray host construct their
  adapters and open the same GPUI entity.
- Keep native menus, Dock and tray behavior outside the shared window.

### 5. Delete The Duplicate Linux Render Tree

**In progress.** The Linux production render now uses the shared navigation,
pane headers, settings panels and rows, keycaps, toggles, messages, and exact
transcription picker at the same default and minimum dimensions as macOS. The
superseded Linux dashboard implementation has been removed. The remaining
Linux Settings composition disappears when the shared root extraction lands.

- Remove `LinuxApp::render` and Linux-only visual constants and controls.
- Retain the Linux lifecycle, tray, X11 window mapping, listener, updater, and
  settings adapter.
- Add later Linux capabilities to the shared panes rather than recreating them.

## Invariants

- The refactor does not change dictation capture, audio boundaries, inference,
  paste, command resolution, or AppleScript behavior.
- Linux exposes only the capabilities in the X11 and compatible Wayland beta
  contracts.
- macOS retains onboarding, permissions, launch-at-login, Sparkle, commands,
  Voice Action, replacements, and developer-only meeting behavior.
- Platform I/O and blocking work stay off the GPUI thread.
- Existing settings and managed-install layouts remain readable without a
  migration.
- Shared rendering contains no scattered target checks. Platform selection
  occurs when the host adapter is constructed.
- Tests exercise behavior through the same host interface used by the window.

## Completion Criteria

- macOS and Linux open the same production `AppWindow` GPUI entity.
- The repository has one production `Render` implementation for the main
  desktop window.
- Settings and Activity use the same layout and controls on both platforms.
- Platform-only panes and rows derive from semantic capabilities.
- Linux retains working tray, close-to-tray, shortcut registration, listener,
  automatic paste, updates, and XDG persistence.
- macOS behavior and release validation remain unchanged.
- Wide and minimum-size screenshots verify both supported desktop hosts.
- `cargo fmt --check`, `cargo test`, Clippy, and `git diff --check` pass on the
  supported hosts.

## Rejected Directions

- Do not maintain separate macOS and Linux render trees with shared colors.
- Do not scatter `#[cfg(target_os = ...)]` through shared rendering.
- Do not let platform adapters return arbitrary GPUI elements.
- Do not create no-op adapters for unsupported features.
- Do not generalize beyond the implemented macOS and Linux X11/Wayland hosts.
