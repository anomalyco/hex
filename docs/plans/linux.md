# Linux Plan

**Status:** Active validation and capability plan. The x86_64 X11 beta,
wlroots-compatible native Wayland path, and Nix packaging are implemented.
Physical desktop/input validation and a genuine signed update remain release
gates; automated checks alone do not establish those behaviors.

## The Supported Contract Is Narrow

The Linux beta keeps its contracts explicit:

| Area | Contract |
| --- | --- |
| Distribution | Signed user-local direct install, or Nix-owned package updates |
| Host | x86_64 Linux; Arch/i3 reference and NixOS packaging |
| Desktop | i3/X11 or compatible wlroots Wayland compositor |
| Audio | CPAL through ALSA, typically backed by PipeWire |
| Inference | Vulkan with CPU fallback |
| Shortcut | Configurable key-containing chord; default `Alt+Space` |
| Insertion | Clipboard plus Ctrl-V, with a Ctrl-Shift-V terminal setting |
| UI | GPUI shell; X11 tray where available; Wayland layer-shell HUD |

The beta does not claim voice commands, application or browser context,
meetings, or universal Wayland support. Wayland needs explicit read access to
all event devices, `wl-copy`, `wtype`, and compatible compositor protocols.
There is no hidden XWayland or privileged injection fallback. See
[`../linux.md`](../linux.md) and [`../nix.md`](../nix.md).

## What Is Implemented

- Global hold/release dictation, Escape cancellation, and double-tap lock.
- Configurable persisted shortcut binding.
- Local model installation and transcription.
- Automatic clipboard insertion with bounded settling. The transcript remains
  on the clipboard; arbitrary previous formats are not restored.
- CLI microphone override.
- GPUI shell, tray integration, desktop launcher, and autostart entry.
- Native Wayland evdev input with explicit key mapping, cancellation, and
  device rediscovery; persistent click-through recording/processing overlay.
- Visible listener controls, transactional shortcut edits, and orderly tray-less
  shutdown instead of a detached invisible microphone.
- Nix package and matching development shell, NixOS installation, optional
  Home Manager user service, and evaluated session-readiness checks.
- XDG paths, diagnostics, and exclusive listener ownership.
- Signed user-local updates with bounded download, exact size and SHA-256
  verification, atomic version activation, and restart handoff.

The update path is implemented but is not yet proven by a complete signed
cross-version update on the supported Arch/i3 host.

The direct installer is published as `install-linux.sh`. It verifies the same
signed feed and content-addressed artifact as the in-app updater, creates the
managed version layout, and installs XDG desktop and autostart entries. An AUR
package is the preferred future Arch-native channel; pacman must own updates for
that layout.

## Remaining Work Is Sequenced By Capability

### 1. Validate The Signed Update Path

Run a genuine update between two published stable versions on the supported
Arch/i3 host. Verify:

- manifest signature and monotonic-version enforcement;
- exact artifact size and SHA-256 validation;
- atomic activation and application restart;
- retained-version rollback;
- refusal to overwrite development, root, or package-manager installs;
- startup with and without a tray host.

This validation blocks calling the direct-install update channel proven.

### 2. Add X11 Commands And Context

Only begin this slice when command parity is a product priority. Add:

- Moonshine command recognition and wake/sleep behavior;
- X11 foreground application and title through EWMH;
- platform-appropriate command shortcuts and application launch;
- MPRIS pause/resume for players HEX actually paused;
- logind or XDG idle-sleep inhibition;
- visible context age and failure.

Keep browser-host commands unavailable until a real browser adapter provides
the active URL. Never infer a URL from a window title.

### 3. Add Manual PipeWire Meetings

Manual meeting recording precedes automatic detection. Capture two explicit
sources:

1. The selected or default microphone source.
2. The monitor of the selected or default output sink.

Each callback must provide source, presentation timestamp, monotonic arrival
timestamp, sample rate, and mono float samples. Reuse the existing bounded WAV
writers, live drafts, gap accounting, manifests, recovery, and atomic final
publication.

Handle route replacement, Bluetooth profile changes, clock alignment, queue
pressure, and HEX feedback appearing in the output monitor. Route loss must
produce a visible gap or failure rather than silent corruption.

### 4. Add Metadata-Only Meeting Offers

After manual capture is reliable, observe PipeWire registry nodes and active
links without recording samples. Normalize process and media metadata into the
existing meeting-candidate model. Detection remains offer-only and never starts
recording automatically.

### 5. Expand Wayland Only With Explicit Capabilities

The implemented beta uses read-only evdev observation plus compositor-provided
clipboard and virtual-keyboard protocols. It does not suppress the shortcut;
users may need a compositor no-op binding. It does not grant input permissions.
Broader or less-privileged support requires a separate capability decision:

| Contract | Input and insertion behavior |
| --- | --- |
| Basic | Portal shortcut, native clipboard, limited context, manual paste where injection is unavailable |
| Appliance | Automatic insertion and broader shortcuts through an explicitly approved privileged helper |

Unprivileged Wayland does not guarantee arbitrary foreground insertion, global
activity observation, foreground application identity, or bare-modifier
shortcuts. Do not hide those limitations behind XWayland.

If appliance parity is required, design a minimal versioned evdev/uinput helper
that handles keyboard hotplug, crash-safe key release, access policy, and exact
re-injection of non-suppressed events. Do not build a general-purpose root
daemon.

### 6. Validate Distribution Channels Deliberately

The app-managed updater owns only the user-local direct-install layout. Nix owns
its packaged installation and optional user service; a future Arch package must
also leave updates to the package manager. Add architectures,
distros, or package formats one validated channel at a time.

## Preserve The Portable Core

Do not fork these modules by platform:

- audio capture, mono conversion, bounded delivery, and drop accounting;
- dictation pre-roll, growable capture, controls, and resampling;
- transcription models, inference, processing, ordering, and cancellation;
- command grammar and pure resolution;
- event schema and bounded projections;
- paste continuation and clipboard restoration state transitions;
- meeting manifests, locking, WAV writing, transcript publication, and
  recovery.

Platform adapters own native handles, permissions, and cleanup. Do not create
one application-wide `Platform` trait.

## Add Seams Only With Real Implementations

| Work beginning | Introduce then | Do not introduce |
| --- | --- | --- |
| X11 commands | Target action executor | Automation plugin registry |
| Foreground context | Private provider behind `ContextMonitor` | Browser framework before a browser adapter |
| PipeWire meetings | Capture session emitting existing packet facts | Generic media graph |
| PipeWire detection | Metadata-only microphone activity source | Sample capture for detection |
| Native Wayland | Runtime X11/Wayland capability selection | XWayland presented as native support |
| Privileged parity | Minimal versioned helper protocol | General-purpose root daemon |
| Additional packaging | One concrete channel and ownership rule | Universal package abstraction |

## Validation

Automated checks remain:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
./scripts/test-install-linux-release.sh
nix flake check
```

Run behavior tests against real adapters rather than mocks that repeat
implementation assumptions.

| Capability | macOS | X11 target | Wayland target |
| --- | --- | --- | --- |
| Command microphone and wake/sleep | Required | Required when shipped | Required when shipped |
| Hold/release, lock, cancel | Required | Required | Required if advertised |
| Foreground Paste and Send | Required | Required | Required if advertised |
| Browser, terminal, editor, Electron insertion | Required | Required | Capability-dependent |
| Application context | Required | Required when shipped | Capability-dependent |
| GPUI shell and settings | Required | Required | Required |
| Manual two-track meeting | Required in developer builds | Required when shipped | Required when shipped |
| Offer-only meeting detection | Required in developer builds | Required when shipped | Required when shipped |
| Shutdown and route-loss recovery | Required | Required | Required |

## Risks

| Risk | Severity | Mitigation |
| --- | --- | --- |
| Generic Wayland parity is not a stable contract | Critical | Publish one constrained contract |
| Native Wayland insertion is compositor-controlled | High | Prove portal or libei behavior; otherwise use manual paste or an approved helper |
| Foreground and browser context lack universal APIs | High | EWMH first; add a real browser adapter later |
| PipeWire output monitoring includes HEX feedback | High | Suppress feedback or construct a filtered capture graph |
| PipeWire metadata varies by package format | Medium | Build identity rules from observed fixtures |
| Packaging multiplies native dependencies | Medium | Keep one distro and channel until validated |

## Recorded Decisions

- Target x86_64 Arch Linux rolling on i3/X11.
- Ship through a signed user-local direct-install channel.
- Require automatic insertion and a key-containing shortcut.
- Exclude meetings; scope native Wayland to compatible wlroots protocols and
  explicit raw-input permissions rather than claiming every compositor.
- Keep package-manager and app-managed update ownership separate.
- Do not install a privileged helper for the X11 contract.
- Rebuild the proposals in [#28](https://github.com/anomalyco/hex/pull/28) and
  [#29](https://github.com/anomalyco/hex/pull/29) with regression coverage for
  input state, GTK ownership, helper I/O, settings restart, and Nix environments.
