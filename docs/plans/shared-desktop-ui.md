# Share The Desktop UI Across macOS, Linux, And Windows

**Status:** In progress. The shared visual vocabulary and transcription picker
are implemented. All three desktop targets render with GPUI, but they still use
separate root entities. This plan converges them on one product shell while
keeping platform behavior in the existing macOS, X11, and Win32 hosts.

## One Product Shell Should Represent The Same Product Concepts

macOS renders the full `AppWindow`; Linux and Windows each render another root
tree. Those duplicate trees make shared concepts such as listener state,
shortcuts, settings, updates, and activity look and behave differently even
though all three applications use GPUI.

The target is one shared `AppWindow` for navigation, layout, controls, and
presentation. The macOS, Linux, and Windows hosts continue to own behavior that
actually varies by platform.

```text
macOS lifecycle ----\
Linux lifecycle -----+-- shared GPUI AppWindow -- desktop actions
Windows lifecycle --/                              |
                           +-----------------------+-----------------------+
                           |                       |                       |
                     macOS adapter            Linux adapter         Windows adapter
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

Settings and Activity established the shared vocabulary. History now has one
behavior-complete pane, and the reusable replacement/correction editor is the
first extracted piece of Modes. The rest of Modes, Voice Action, Commands, and
Meetings can use the same shell, but each pane appears on a platform only when
its underlying behavior is implemented there.

## Desktop Hosts Own Consequential Behavior

The outer application hosts remain separate:

- The macOS host owns native menus, Dock policy, reopen behavior, TCC setup,
  `SMAppService`, Sparkle, meeting lifecycle, and the existing recognition
  coordinator.
- The Linux host owns the GTK tray, X11 map and unmap behavior, close-to-tray,
  X11 shortcut registration, listener lifecycle, and signed direct updates.
- The Windows host owns the Win32 tray and caption lifecycle, global shortcut
  hook, listener process, startup registration, and signed update restart.

All three hosts open the shared `AppWindow`. The Linux host stops implementing its
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
both roots, and Linux no longer interprets the event stream independently. The
macOS Activity header renders the shared listener-status control; the current
Linux root shows only the shared Settings pane and toggles listening from the
tray menu, so the detailed Activity pane remains macOS developer-only until the
shared root window lands.

- Move the common `EventReader` projection into the shared window state.
- Render status, device, completed transcripts, and failures once.
- Keep starting and stopping the listener in each platform host.

### 3. Normalize Core Settings For Presentation

**Implemented.** Shortcut keycaps, the double-tap toggle, and the complete local
transcription picker now use shared GPUI presentation. `DesktopSnapshot` also
normalizes shortcut labels, listener status, microphone devices/selection/errors,
operation errors, update status, observation metadata, and transcription
selection/preparation state. Shortcut captures, microphone refresh and
selection, update checks/restarts, error dismissal, and listener control cross
the host seam as portable actions. The shared picker delegates model preparation
to each current root while macOS, Linux, and Windows validate, prewarm, persist,
and apply their native runtime selections independently.

- Introduce portable view state for shortcut, double-tap lock, microphone, and
  transcription controls.
- Translate `AppSettings` and `LinuxSettings` through their host adapters.
- Keep platform validation and atomic persistence behind the host seam.

### 4. Open One Shared AppWindow From Both Hosts

**In progress.** `src/desktop/host.rs` now defines semantic capabilities,
portable activity, microphone, shortcut, transcription, error, listener, and
update snapshots plus typed actions. Linux product state has moved into a
contained `LinuxDesktopHost`; its GPUI root now retains presentation state and
delegates listener lifecycle, update polling, activity, microphone selection,
shortcut validation, update restart, and settings persistence. Windows and the
transitional macOS root consume the same core settings actions while keeping
their native validation and lifecycle. `src/desktop/shell.rs` now owns the one
stable pane identity, order, label, icon, and capability filter consumed by all
three roots, plus the one renderer for navigation rows and selection callbacks.
The current roots retain only their native frame/footer and pane-selection side
effects. Linux no longer advertises a Commands catalog pane merely because its
developer-only runtime toggle exists. `src/desktop/history_pane.rs` now owns the
History store handle, bounded search snapshot, selection reconciliation,
copy/delete behavior, confirmed-clear transition, selectable detail text, and
the complete list-and-detail renderer for macOS and Windows. The native roots
now provide only the search entity, retention setting, and one typed action
delegate while retaining platform settings persistence.
`src/desktop/replacement_editor.rs` likewise owns the phrase/output inputs,
add/remove action contract, focus behavior, and complete editor card used by
macOS mode corrections plus both Windows replacement collections. Each root
still decides which collection a target names and persists through its existing
schema. Periodic refresh and the remaining render state still need to move into
one root entity.

- Move `AppWindow` and its portable dependencies out of macOS-only module gates.
- Have the macOS lifecycle coordinator and Linux tray host construct their
  adapters alongside the Windows lifecycle host and open the same GPUI entity.
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

- macOS, Linux, and Windows open the same production `AppWindow` GPUI entity.
- The repository has one production `Render` implementation for the main
  desktop window.
- Settings and Activity use the same layout and controls on all supporting platforms.
- Platform-only panes and rows derive from semantic capabilities.
- Linux retains working tray, close-to-tray, shortcut registration, listener,
  automatic paste, updates, and XDG persistence.
- macOS behavior and release validation remain unchanged.
- Wide and minimum-size screenshots verify both supported desktop hosts.
- `cargo fmt --check`, `cargo test`, Clippy, and `git diff --check` pass on the
  supported hosts.

## Rejected Directions

- Do not maintain separate platform render trees with shared colors.
- Do not scatter `#[cfg(target_os = ...)]` through shared rendering.
- Do not let platform adapters return arbitrary GPUI elements.
- Do not create no-op adapters for unsupported features.
- Do not generalize beyond the implemented macOS, X11, and Win32 hosts.
