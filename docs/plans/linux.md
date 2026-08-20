# Linux Plan

**Status:** Active validation and capability plan. The x86_64 Arch/i3 X11 beta
has the source-complete dictation loop, live global replacements, EWMH
application modes with corrections and optional OpenCode rewriting, ordered
built-in/TypeScript transformations through the shared host, retained History,
and a signed update path. The immediate release blocker is validating a genuine
signed update on the target machine.

## The Supported Contract Is Narrow

HEX currently supports one Linux beta contract:

| Area | Contract |
| --- | --- |
| Distribution | User-local direct install with signed automatic updates |
| Host | x86_64 Arch Linux rolling |
| Desktop | i3 on X11 |
| Audio | CPAL through ALSA, typically backed by PipeWire |
| Inference | Vulkan with CPU fallback |
| Local API | Authenticated loopback service with owner-only XDG discovery or SDK-owned direct-child handoff |
| Shortcut | Configurable key-containing chord; default `Alt+Space` |
| Insertion | X11 clipboard and synthetic paste |
| Text replacements | Persisted case-insensitive phrase-boundary rules applied live before paste |
| Application context | `_NET_ACTIVE_WINDOW`, `_NET_WM_PID` plus `/proc`, `WM_CLASS` fallback, and bounded window title |
| Modes | Ordered executable-name substring rules with phrase-boundary corrections, optional deadline-bounded OpenCode rewriting, and built-in/TypeScript transformations; no browser rules yet |
| Retained History | Bounded owner-only text/metadata store after successful paste |
| UI | GPUI Settings, Modes, Activity, model catalog, onboarding, and History panes plus tray |

The beta does not claim voice commands, browser context, meetings, native
Wayland support, or package-manager installation. Developer builds contain a
bounded command-recognition prototype for bringing up the portable engine and
X11 executor; it is deliberately outside this release contract. XWayland is not
native Wayland support.

## What Is Implemented

- Global hold/release dictation, Escape cancellation, and double-tap lock.
- Configurable persisted shortcut binding.
- Local model installation and transcription.
- Shared local transcription service with bounded WAV admission, explicit model
  preparation, owner-only discovery, and stdin-owned embedded lifecycle.
- Automatic clipboard insertion and restoration.
- Persisted global phrase-boundary replacements and ordered application-mode
  corrections. The shared Modes list, basic editor, and phrase/output editor
  expose only executable-name rules; a running listener sees edits without
  restarting.
- Persisted global and per-mode OpenCode model, reasoning-variant, deadline, and
  prompt controls. Rewriting uses the shared ordered policy after corrections,
  preserves corrected text on failure, and records bounded History metadata.
- Persisted ordered global and per-mode transformations. Built-ins run without
  Bun; custom steps use the shared bounded Bun supervisor, watch-reloaded
  `~/.config/hex/hex.config.ts`, embedded managed SDK, and shared drag-order UI.
- A concrete EWMH context adapter reads the active X11 client, resolves its
  executable through `_NET_WM_PID` and `/proc` with `WM_CLASS` fallback, and
  captures a bounded title. Context failure degrades to global processing and
  never blocks paste.
- Shared retained-History pane with persisted retention, search, selectable
  detail, copy, delete, and confirmed clear. Only successfully pasted output is
  recorded; raw and replaced text remain distinct and captured audio is never
  retained.
- CLI microphone override.
- GPUI shell, tray integration, desktop launcher, and autostart entry.
- XDG paths, diagnostics, and exclusive listener ownership.
- Signed user-local updates with bounded download, exact size and SHA-256
  verification, atomic version activation, and restart handoff.

The update path is implemented but is not yet proven by a complete signed
cross-version update on the supported Arch/i3 host.

The local API passes its portable server suite and source-build standalone and
direct-child lifecycle smokes under Linux. A signed architecture-specific SDK
helper package and physical Arch/i3 validation remain distribution work, not a
second service implementation.

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

### 2. Continue Desktop Parity In Complete Vertical Slices

Retained History, global replacements, application-mode corrections, OpenCode
rewriting, and built-in/TypeScript transformations are implemented with the same
store, processing policy, bounded host, and editor components used on macOS and
Windows. Physically validate their X11 clipboard, focus, EWMH identity, OpenCode
and Bun lifecycles, and UI behavior on the supported host. Continue in dependency
order:

1. Add Voice Action with a real selected-text and focus-preserving X11 contract.

Do not expose a pane before its worker, persistence, and native validation path
exist.

### 3. Add X11 Commands And Context

The developer-only prototype now provides bounded, generation-safe Moonshine
projection, wake/sleep resolution, a small compiled command set, and XTest key
execution without blocking authoritative dictation capture. Finish the product
slice by adding:

- packaged and verified Moonshine native/model assets for direct installs;
- project the existing EWMH application/title adapter into command decisions
  and visible context health;
- the complete compiled and TypeScript command registry;
- platform-appropriate command shortcuts, application launch, and failure
  feedback;
- MPRIS pause/resume for players HEX actually paused;
- logind or XDG idle-sleep inhibition;
- visible context age and failure;
- physical pressure, reset, dictation-pre-roll, action, and shutdown validation.

Only after those gates pass may release builds expose the persisted Commands
setting or load Moonshine while idle.

Keep browser-host commands unavailable until a real browser adapter provides
the active URL. Never infer a URL from a window title.

### 4. Add Manual PipeWire Meetings

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

### 5. Add Metadata-Only Meeting Offers

After manual capture is reliable, observe PipeWire registry nodes and active
links without recording samples. Normalize process and media metadata into the
existing meeting-candidate model. Detection remains offer-only and never starts
recording automatically.

### 6. Choose A Native Wayland Contract

Wayland support requires an explicit capability decision:

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

### 7. Add Distribution Channels Deliberately

The app-managed updater owns only the user-local direct-install layout. A future
Arch package must leave updates to the package manager. Add architectures,
distros, or package formats one validated channel at a time.

## Preserve The Portable Core

Do not fork these modules by platform:

- audio capture, mono conversion, bounded delivery, and drop accounting;
- dictation pre-roll, growable capture, controls, and resampling;
- transcription models, inference, processing, ordering, and cancellation;
- command grammar and pure resolution;
- event schema and bounded projections;
- retained-history bounds, persistence, search, and pane behavior;
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
```

Run behavior tests against real adapters rather than mocks that repeat
implementation assumptions.

| Capability | macOS | X11 target | Wayland target |
| --- | --- | --- | --- |
| Command microphone and wake/sleep | Required | Required when shipped | Required when shipped |
| Hold/release, lock, cancel | Required | Required | Required if advertised |
| Foreground Paste and Send | Required | Required | Required if advertised |
| Retained History after successful paste | Required | Required | Required if advertised |
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
| Foreground and browser context lack universal APIs | High | EWMH application context is implemented; add a real browser adapter later |
| PipeWire output monitoring includes HEX feedback | High | Suppress feedback or construct a filtered capture graph |
| PipeWire metadata varies by package format | Medium | Build identity rules from observed fixtures |
| Packaging multiplies native dependencies | Medium | Keep one distro and channel until validated |

## Recorded Decisions

- Target x86_64 Arch Linux rolling on i3/X11.
- Ship through a signed user-local direct-install channel.
- Require automatic insertion and a key-containing shortcut.
- Exclude meetings and native Wayland from the first beta.
- Keep package-manager and app-managed update ownership separate.
- Do not install a privileged helper for the X11 contract.
