# Linux Port Plan

## Active Implementation Target

- Arch Linux rolling on x86_64.
- i3 on X11 with PipeWire and WirePlumber.
- User-local signed direct install for the beta and the X11 complete desktop
  contract; add a native Arch package as a separate package-managed channel.
- Full parity delivered sequentially: dictation, commands and context, manual
  meetings, automatic offers, packaging, then remaining UI hardening.
- Automatic insertion is required; no privileged helper is required for X11.
- The default dictation shortcut is `Alt+Space`, with persisted rebinding in
  the Linux settings UI. Shortcut conflicts must be reported visibly.

## Purpose

Port HEX to Linux without weakening the existing macOS product or replacing its
deep modules with a generic platform framework. The speech, command, dictation,
worker, event, transcript, and meeting-artifact cores remain shared. Linux work
adds concrete adapters only where the current implementation reaches macOS.

This is an execution plan, not a commitment to generic Linux support. Before
desktop integration begins, select one real distro, desktop environment,
compositor, CPU architecture, and feature contract.

## Recommended Product Sequence

1. Prove the native models, microphone, PipeWire, and GPUI on the target Linux
   machine.
2. Make headless recognition and diagnostics portable.
3. Deliver one complete desktop contract rather than partial support everywhere.
4. Add manual PipeWire meetings after dictation is a daily-usable loop.
5. Add automatic meeting offers only after manual capture is reliable.
6. Treat X11 and Wayland as different capability sets.

If no real target has been selected, use native x86_64 Linux with
PipeWire/WirePlumber and X11 as the reference complete desktop slice. Do not
claim native Wayland support from an XWayland implementation.

## Effort Envelope

These are cumulative estimates for one experienced engineer preserving macOS
behavior and validating on real desktops.

| Milestone | Cumulative effort |
| --- | ---: |
| Linux native-model and desktop feasibility spikes | 3-5 days |
| Headless recognition, logs, and Ratatui dashboard | 2-3 weeks |
| Daily-usable X11 command, dictation, and paste loop | 4-7 weeks |
| X11 GPUI shell, actions, and basic foreground context | 6-11 weeks |
| Manual PipeWire meeting recording | 8-15 weeks |
| PipeWire meeting detection and offers | 10-18 weeks |
| Constrained native Wayland support | 12-23 weeks |
| Hardened macOS, X11, and Wayland distribution | 14-27 weeks |

Exact Wayland parity may require a privileged evdev/uinput companion. Designing,
installing, and hardening that companion adds approximately 3-6 weeks and a
long-term distro maintenance surface.

## Initial Scope Contract

The first Linux release should target:

- One named distro and desktop environment.
- One CPU architecture.
- A user-local direct install rather than Flatpak for the beta.
- PipeWire/WirePlumber for meetings.
- CPAL over the desktop's ALSA compatibility path for the command microphone.
- A key-containing Linux dictation chord, not bare Alt.
- Manual meetings before automatic offers.
- No browser-host commands until a real browser adapter exists.
- No Linux HUD until the command and dictation loop is reliable.

The first target must choose one desktop contract:

| Contract | Input and insertion behavior |
| --- | --- |
| X11 complete | Global chord, suppression, activity observation, XTest insertion, and X11 clipboard |
| Wayland basic | Portal chord, native clipboard, limited context, and manual paste where injection is unavailable |
| Wayland appliance | Automatic insertion and broader shortcuts through an explicitly approved privileged helper |

## Preserve The Portable Core

The following modules are portable or mostly portable and should not be forked:

- `audio`: CPAL capture, mono conversion, bounded delivery, and drop accounting.
- `dictation`: pre-roll, capture bounds, control phrases, and resampling.
- `dictation_processor`: profile selection, OpenCode protocol, deadlines, and
  raw fallback.
- `command_grammar`: typed patterns, captures, overlap checks, and catalog facts.
- Pure command resolution in `commands`.
- `events` and the Ratatui projection.
- Moonshine stream ownership and C ABI calls.
- Parakeet inference, processor fan-out, output ordering, and meeting-delta logic.
- Paste continuation, casing, and clipboard-restoration state transitions.
- Meeting manifests, locking, WAV writing, transcript publication, recovery,
  live transcript projection, and final Parakeet transcription.

Port platform effects behind the existing deep module APIs. Do not create one
application-wide `Platform` trait.

## Add Seams Only With Their Linux Implementations

| Linux work beginning | Introduce at that point | Do not introduce |
| --- | --- | --- |
| Runtime bootstrap | `AppPaths` for logs, settings, models, meetings, and locks | Generic filesystem service |
| Native model loading | Target-selected library filename and search paths | Model plugin API |
| X11 input | Normalized physical key/modifier events and an extracted hotkey state machine | General event bus |
| Keyboard insertion | Narrow keyboard-output API consumed by paste and actions | Automation plugin registry |
| Linux actions | Split pure resolution from the target action executor | Data-driven command DSL |
| Foreground context | Private provider behind `ContextMonitor` | Browser framework before a browser adapter |
| Linux settings | Versioned, platform-tagged hotkey representation | Reinterpreting raw CG key codes |
| PipeWire meetings | Capture session that emits the existing conceptual `AudioPacket` | Generic media graph |
| PipeWire detection | Microphone-activity metadata source | Reusing capture samples for detection |
| Linux GPUI shell | Small lifecycle, reveal, and taskbar functions | Alternate UI abstraction |
| Linux HUD | One real renderer/window implementation | Renderer framework in advance |
| Native Wayland | Runtime X11/Wayland capability selection | Treating XWayland as native support |
| Privileged Wayland parity | Minimal versioned helper protocol | General-purpose root daemon |

## Phase 0: Feasibility Spikes

**Estimate:** 3-5 days

Run all spikes on the intended Linux architecture and desktop:

1. Load the Moonshine wheel's `libmoonshine.so` and transcribe one fixture.
2. Verify the Moonshine model lands under the XDG cache path used by
   `dirs::cache_dir()`.
3. Load the same Parakeet GGUF through `transcribe.cpp` using the intended
   Vulkan, CUDA, or CPU backend.
4. Capture the default microphone through CPAL.
5. Enumerate and record the default PipeWire sink monitor.
6. Open a minimal GPUI 0.2.2 window under the intended X11 and/or Wayland
   session.
7. Record native package dependencies and the target glibc baseline.

Do not add platform traits during this phase.

**Exit criteria**

- Moonshine and Parakeet both transcribe known fixtures.
- Microphone capture runs continuously without unexplained drops.
- A PipeWire monitor recording contains audible system output.
- GPUI opens and renders text on the target session.
- No unknown native binary or GPU blocker remains.

## Phase 1: Headless Linux Recognition

**Estimate:** 5-10 additional days

Deliver:

- Linux-compilable `listen` and `status` commands.
- XDG runtime, data, and cache paths through one `AppPaths` owner.
- Moonshine `.so` discovery.
- CPAL microphone input.
- Wake, sleep, transcript, command-resolution, and state observations.
- Ratatui diagnostics.
- Explicit unsupported-capability errors for actions not yet ported.
- Nonfatal degradation when sound output, global input, or clipboard is absent.
- Linux compile and pure-test CI alongside macOS validation.

Move existing macOS implementations behind target-selected modules without
changing their behavior.

**Exit criteria**

- Linux continuously recognizes commands without a display server.
- `status` projects the same event schema as macOS.
- A missing output sink does not prevent microphone recognition.
- A missing clipboard disables paste targets without disabling ordinary voice
  commands or Captain's Log.
- macOS tests and installed-app behavior remain unchanged.

## Phase 2: One Complete Desktop Dictation Loop

**Estimate:** 10-18 additional days for X11; reassess Wayland after Phase 0

For the X11 complete contract, deliver:

- A key-containing global shortcut with press and release events.
- Suppression of the registered shortcut.
- Escape cancellation and optional double-tap lock.
- Reserved last-dictation and meeting-paste shortcuts.
- XInput2 activity observation for continuation invalidation.
- XKB logical-key resolution.
- XTest Ctrl-V, Enter, and command shortcuts.
- X11 clipboard insertion and generation-safe restoration.
- Parakeet dictation, Paste, Send, and last-transcript paste.
- Voice-delimited dictation.

Extract normalized input events and the pure hotkey state machine only while
building this adapter. Do not silently map macOS Command and Option modifiers to
Linux Control and Alt; compile platform-appropriate actions explicitly.

**Exit criteria**

- Hold, release, lock, cancel, and maximum-duration behavior match macOS.
- The registered chord does not leak into the foreground application.
- Paste and Send work in at least a browser, terminal, editor, and Electron app.
- Clipboard restoration does not overwrite an external clipboard change.
- User activity invalidates continuation joins.
- macOS hotkey and paste tests remain green.

## Phase 3: Linux Actions And Context

**Estimate:** 5-10 additional days

Deliver:

- URL opening through the XDG default handler.
- Application launch by desktop application ID or executable.
- Platform-appropriate command shortcuts.
- X11 foreground application and title through EWMH.
- MPRIS pause and resume for players HEX actually paused.
- logind or XDG idle-sleep inhibition.
- PipeWire/WirePlumber mute and conditional restore if Mute remains supported.
- Linux application identity derived from observed desktop and PipeWire facts.

Keep browser-host commands unavailable until a real browser adapter can provide
the active URL. Do not infer a URL from a window title.

**Exit criteria**

- Global and application-scoped commands work on the selected desktop.
- Context failure is visible and cannot retain an indefinitely stale snapshot.
- Recording-environment cleanup does not overwrite user volume or media changes.
- Unsupported browser-host commands are absent from the generated catalog.

## Phase 4: GPUI Shell And Linux Packaging

**Estimate:** 7-12 additional days

Deliver:

- GPUI with the required Linux `x11` and/or `wayland` features.
- Meetings, Commands, Activity, and Settings panes.
- Linux editing and application keybindings.
- Platform-neutral file reveal.
- Linux-specific settings labels and capability health.
- A `.desktop` entry and icon installation.
- Native launch and optional systemd user-service installation.
- Platform-tagged persisted hotkeys with explicit legacy macOS migration.

Reuse the current GPUI views. Defer the click-through HUD and exact meeting-offer
placement until ordinary window behavior is proven.

**Exit criteria**

- The shell runs under the selected desktop without source-checkout paths.
- Settings survive restart and never reinterpret macOS key codes as Linux codes.
- The application starts from the desktop launcher and can own one listener.
- Capability health explains unavailable input, insertion, context, and meeting
  behavior.

## Phase 5: Manual PipeWire Meetings

**Estimate:** 12-22 additional days

Implement native PipeWire capture for two sources:

1. The selected or default microphone source.
2. The monitor of the selected or default output sink.

Each callback emits the shared packet facts:

```text
source
presentation timestamp
monotonic arrival timestamp
sample rate
mono float samples
```

Reuse the existing bounded writer, WAV tracks, dual-stream Moonshine draft,
packet-gap accounting, Parakeet finalization, manifest, locking, recovery, and
atomic publication.

Handle explicitly:

- PipeWire node and route replacement.
- Bluetooth profile changes.
- Clock alignment between microphone and system output.
- Queue duration under different PipeWire quantums.
- HEX feedback appearing in the system-output monitor.
- Host-package permissions and a clear error on unsupported PulseAudio-only
  environments.

**Exit criteria**

- A real Linux meeting produces the same manifest, separate WAV tracks, live
  draft, and final transcript shape as macOS.
- Route loss produces a visible gap or failure rather than silent corruption.
- A live or final transcription failure preserves captured tracks and the live
  draft.
- Meeting capture remains explicitly initiated.

## Phase 6: PipeWire Meeting Detection

**Estimate:** 6-12 additional days

Observe PipeWire registry nodes and active links without recording samples.
Normalize real properties such as process ID, application ID, binary, media role,
and client ID into Linux meeting candidates.

Convert detection debounce from observation counts to elapsed durations before
feeding event-driven PipeWire updates into it.

**Exit criteria**

- Supported Zoom, Teams, Slack, and browser calls produce one offer per
  activation.
- Five seconds of inactivity rearms detection.
- Browser calls remain conservatively labeled unless browser context exists.
- Detection never captures samples and never starts recording automatically.

## Phase 7: Native Wayland Support

**Estimate:** 12-25 additional days for constrained support

Deliver the selected Wayland contract:

- XDG Global Shortcuts session with a key-containing chord.
- Portal Activated and Deactivated signals for hold semantics.
- Wayland-native clipboard ownership.
- GPUI Wayland shell.
- Explicit capability diagnostics.
- Automatic insertion only through an approved mechanism.

Unprivileged Wayland does not guarantee arbitrary foreground insertion, global
activity observation, foreground application identity, or a bare modifier
shortcut. Do not hide these limitations behind XWayland.

If appliance parity is required, decide explicitly whether to build a minimal
evdev/uinput helper. That helper must handle keyboard hotplug, multiple devices,
crash-safe key release, access policy, installation, protocol versioning, and
reinjecting every non-suppressed event.

**Exit criteria**

- The advertised contract works on both the selected GNOME/KDE target and its
  actual portal backend.
- Unsupported operations fail visibly.
- Portal permission and shortcut sessions survive the documented lifecycle.
- No native Wayland application depends accidentally on XTest or X11 clipboard.

## Phase 8: Browser Context, HUD, And Hardening

**Estimate:** 10-20 additional days, excluding a privileged helper

Potential slices:

- Browser extension and native messaging for active host context.
- Browser navigation adapter.
- Portable GPUI HUD, followed by optional layer-shell placement where supported.
- Multi-monitor and fullscreen offer behavior.
- PipeWire route-switch recovery and system-audio self-exclusion.
- x86_64 and aarch64 artifacts.
- Distro package matrix and autostart health.
- Cross-platform release and regression validation.

Treat each as an independent product slice. None blocks the first useful Linux
release.

## Validation Strategy

Run shared pure tests on macOS and Linux. Keep platform behavior tests attached
to real adapters rather than mocks that merely repeat implementation assumptions.

Required automated checks:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- Pure unit tests on macOS and Linux.
- Signed Linux feed verification, artifact integrity, and atomic activation.
- Linux compile checks for every supported backend feature set.
- Fixture transcription for Moonshine, Parakeet, and meeting replay.
- Artifact and transcript recovery tests shared across platforms.

Required real-desktop smoke matrix:

| Capability | macOS | X11 target | Wayland target |
| --- | --- | --- | --- |
| Command microphone and wake/sleep | Required | Required | Required |
| Hold/release, lock, cancel | Required | Required | Required if advertised |
| Foreground Paste and Send | Required | Required | Required if advertised |
| Browser, terminal, editor, Electron insertion | Required | Required | Capability-dependent |
| Application context | Required | Required | Capability-dependent |
| GPUI shell and settings | Required | Required | Required |
| Manual two-track meeting | Required | Required | Required once shipped |
| Offer-only meeting detection | Required | Required once shipped | Required once shipped |
| Shutdown and route-loss recovery | Required | Required | Required |

## Risk Register

| Risk | Severity | Mitigation |
| --- | --- | --- |
| Generic Wayland parity is not a stable platform contract | Critical | Select and publish one constrained contract |
| Bare modifier hold is not portable | High | Use a key-containing Linux chord unless a helper is approved |
| Native Wayland insertion is compositor-controlled | High | Portal/libei spike; otherwise manual paste or helper |
| Foreground and browser context lack universal APIs | High | X11 EWMH first; real browser extension later |
| PipeWire sink monitor includes HEX feedback | High | Suppress feedback or construct a filtered capture graph |
| Native model artifact or ABI failure | Medium | Phase 0 fixture spikes before architecture work |
| PipeWire metadata differs by package format | Medium | Build identity rules from observed application fixtures |
| GPUI popup/HUD behavior differs by compositor | Medium | Ship shell first; defer exact overlay parity |
| Packaging multiplies native dependencies | Medium | One distro and direct-install channel first |

## Definition Of A Successful Migration

The migration is successful when:

- macOS behavior and release validation remain intact.
- Linux capabilities are explicit rather than inferred from the display server.
- Shared recognition, dictation, command, transcript, and artifact logic has one
  implementation.
- Each platform adapter owns its native handles, permissions, and cleanup.
- No unbounded worker or blocking platform action enters the microphone loop.
- Meeting detection remains metadata-only and offer-only.
- The supported Linux contract passes its real-desktop smoke matrix.
- Unsupported environments fail with actionable capability diagnostics.

## Decisions Required Before Phase 0

Record these decisions at the top of the implementation issue or pull request:

- Target distro and version.
- Desktop environment and compositor.
- CPU architecture.
- Distribution and update channel.
- X11 complete, Wayland basic, or Wayland appliance contract.
- Whether meeting recording is required in the first release.
- Whether automatic insertion is mandatory.
- Whether a key-containing Linux shortcut is acceptable.
- Whether a privileged helper is acceptable.
