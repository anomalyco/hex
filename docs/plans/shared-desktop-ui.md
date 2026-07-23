# Share The Desktop UI Across macOS And Linux

**Status:** In progress. The shared visual vocabulary is implemented. macOS and
Linux both render with GPUI, but they still use separate root entities. This
plan converges them on one product shell while keeping platform behavior in the
existing macOS and X11 hosts.

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

The first shared panes are Settings and Activity. Replacements, Modes, Voice
Action, Commands, and Meetings can use the same shell immediately, but each pane
appears on Linux only when its underlying behavior is implemented there.

## Desktop Hosts Own Consequential Behavior

The outer application hosts remain separate:

- The macOS host owns native menus, Dock policy, reopen behavior, TCC setup,
  `SMAppService`, Sparkle, meeting lifecycle, and the existing recognition
  coordinator.
- The Linux host owns the GTK tray, X11 map and unmap behavior, close-to-tray,
  X11 shortcut registration, listener lifecycle, and signed direct updates.

Both hosts open the shared `AppWindow`. The Linux host stops implementing its
own `Render` tree.

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
    commands: bool,
    meetings: bool,
    voice_action: bool,
    apple_speech: bool,
    launch_at_login: bool,
    dock_icon: bool,
    recording_audio_behavior: bool,
}
```

Navigation and settings composition derive from these values. Adding Linux
command support later changes the Linux capability and adapter; it does not add
a second Commands UI.

Presentation state may normalize platform-specific behavior. For example, both
updaters can expose `Unavailable`, `Checking`, `Current`, `ReadyToRestart`, and
`Failed` while Sparkle and the signed Linux updater retain independent
implementations.

## Shortcut Presentation Is Shared But Validation Is Not

The shared shortcut editor uses a portable shortcut draft and a common visual
control. The host validates and persists the result.

- macOS accepts modifier-only, Globe/Fn, function-key, and modifier-plus-key
  bindings and resolves active-layout key codes.
- X11 requires a key-containing chord and verifies that the binding can be
  registered on the active X server.

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

**In progress.** `src/desktop_activity.rs` now projects the latest listener
state, device, completed transcripts, and read failure from `EventReader` for
both roots. Linux no longer interprets the event stream independently, and the
macOS Activity header and Linux sidebar use the same listener-status control.
The detailed Activity pane and Linux transcript list remain separate until the
shared root window lands.

- Move the common `EventReader` projection into the shared window state.
- Render status, device, completed transcripts, and failures once.
- Keep starting and stopping the listener in each platform host.

### 3. Normalize Core Settings For Presentation

**In progress.** Shortcut keycaps and the double-tap toggle now use shared GPUI
presentation. `DesktopSnapshot` now also normalizes shortcut labels, listener
status, operation errors, update status, and observation metadata. Shortcut
captures cross the host seam as portable modifier and key values while macOS
and Linux continue to validate and persist their native binding representations
independently. Portable microphone and transcription view state still remain.

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
pane headers, settings panels and rows, keycaps, toggles, messages, and Activity
layout at the same default and minimum dimensions as macOS. The superseded
Linux dashboard implementation is compile-disabled and still needs physical
deletion when the shared root extraction lands.

- Remove `LinuxApp::render` and Linux-only visual constants and controls.
- Retain the Linux lifecycle, tray, X11 window mapping, listener, updater, and
  settings adapter.
- Add later Linux capabilities to the shared panes rather than recreating them.

## Invariants

- The refactor does not change dictation capture, audio boundaries, inference,
  paste, command resolution, or AppleScript behavior.
- Linux exposes only the capabilities in the supported X11 beta contract.
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
- Do not generalize beyond the implemented macOS and X11 hosts.
