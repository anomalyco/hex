# Roadmap

HEX ships the macOS product loop: configurable hold-to-talk local dictation,
explicit selected-text voice editing, optional context-aware OpenCode
post-processing, a Metal HUD, bounded paste workers, settings, and a signed GPUI
app bundle. Streaming commands are a disabled-by-default experimental opt-in;
meetings remain a developer prototype until their product surface is redesigned.
An x86_64 Arch/i3 X11 beta now provides hotkey dictation, automatic paste, a
GPUI shell, and signed user-local updates. Developer builds have the first
bounded Moonshine command prototype, but release builds do not yet claim
commands, context, meetings, native Wayland support, or a package-manager
channel.
The Windows source-build alpha now has the native GPUI shell and complete global
hold-to-dictate loop: timestamped Win32 input, WASAPI capture, checksum-pinned
local models, generation-safe paste, live settings, double-tap lock, Paste Last,
History, replacements, tones, click-through HUD, tray residency, and Launch at
login. Modes with corrections and browser-host selection, Voice Action,
onboarding, and managed signed updates are implemented. OpenCode mode rewriting,
TypeScript transformations, streaming commands, the local API host, and
developer Meetings remain.
The authenticated local transcription service now implements discovery,
direct-child embedding, model preparation, and bounded host-audio transcription;
Promise and Effect TypeScript wrappers pass fake-helper tests. Signed helper
package validation, an Electron bridge, and a first real consumer remain.

## Cross-Platform Convergence Plan

This is the authoritative execution order for the macOS, Linux X11, and Windows
port. Detailed platform plans remain useful for adapter-specific work, but a
stale status sentence there must not override this matrix. “Implemented” means
the behavior exists in source; “release” additionally requires its assets and
distribution contract; “validated” requires the physical-host evidence listed
below.

| Surface | macOS | Linux X11 | Windows |
| --- | --- | --- | --- |
| Dictation loop | Product implementation; physical timestamp regression recheck remains | Beta implementation; Arch/i3 smoke test remains | Alpha implementation; Windows 10/11 smoke matrix remains |
| Desktop presentation | Full root still owns macOS composition | Shared primitives and panes, but a separate Linux root remains | Shared primitives and several panes, but a separate Windows root remains |
| Commands | Persisted release opt-in | Bounded developer-only prototype; no packaged/context-complete release contract | Unavailable until a real streaming command model exists |
| Modes and replacements | Full ordered pipeline on the shared processing policy | Not implemented | Corrections, replacements, and web-domain selection use the shared policy; OpenCode settings and TypeScript transformation hosting remain |
| Voice Action | Implemented | Not implemented | Implemented with clipboard-backed selected-text capture |
| HUD | Product Metal HUD using the shared state model | Embedded developer lab only | Product GPUI/DirectX HUD using the same shared state model |
| Packaging and updates | Sparkle flow implemented; clean-account validation remains | Signed direct-install flow implemented; real cross-version update remains | Managed installer and signed update flow implemented; signed-host validation remains |
| Meetings | Developer-only implementation | Not implemented | Not implemented |
| Local transcription API | Implemented | Not implemented | Not implemented |

### Phase 1: Make macOS A Consumer Of The Portable Core

- [x] Extract platform-neutral command keys, typed grammar, pure resolution,
  and bounded action dispatch for the desktop shells.
- [x] Make the macOS keyboard executor consume the neutral key and modifier
  vocabulary.
- [x] Share the command context snapshot and selector vocabulary while keeping
  `NSWorkspace`/Accessibility/AppleScript, EWMH, and UI Automation capture in
  their native adapters.
- [x] Make macOS consume the shared command grammar and resolution engine, then
  delete the duplicate macOS copies without changing compiled protected
  commands or TypeScript personal-command behavior.
- [x] Move macOS HUD transitions onto the shared indicator state model while
  retaining Metal on macOS and GPUI/DirectX on Windows.
- [x] Share processing-pipeline policy—corrections, OpenCode rewriting, and
  TypeScript transformations—without moving platform context, selection, or
  paste I/O into the portable layer.

Exit gate: the macOS release behavior is unchanged, duplicate semantic engines
are gone, command overlap/catalog tests run against the one implementation, and
native macOS tests plus authoritative release previews pass.

### Phase 2: Converge Desktop Presentation By Capability

- [x] Finish portable microphone, update, shortcut, error, and listener actions
  behind the existing deep `DesktopHost` seam.
- [ ] Have macOS and Linux open the same production GPUI root, then remove the
  remaining Linux Settings/render composition.
- [ ] Extract each behavior-complete History, Modes, Replacements, Voice Action,
  Commands, and onboarding pane once; Windows should consume those pane modules
  rather than grow parallel render trees.
- [ ] Keep native lifecycle outside the shared root: AppKit/Dock/Sparkle on
  macOS, GTK tray/X11/updater on Linux, and Win32 tray/caption/startup on
  Windows.

Exit gate: every common pane has one renderer, capability flags omit behavior
that is genuinely absent, minimum and wide previews match on all supporting
hosts, and no shared renderer branches on an operating-system name.

### Phase 3: Promote Linux In Complete Vertical Slices

- [ ] Physically validate the current beta install, model download, shortcut,
  lock/cancel, paste, tray/autostart, and signed cross-version update on the
  supported x86_64 Arch/i3 host.
- [ ] Finish the command slice with packaged Moonshine assets, EWMH application
  and title context, native action/media/idle adapters, visible context health,
  and pressure/recovery tests; only then enable Commands in release builds.
- [ ] Add History and replacements before Modes; add full mode processing before
  Voice Action so each new pane owns real behavior.
- [ ] Add the authenticated local transcription host before advertising SDK
  lifecycle support on Linux.
- [ ] Keep PipeWire meetings and native Wayland as later explicit contracts from
  `docs/plans/linux.md`; neither blocks the supported X11 beta.

### Phase 4: Finish And Validate Windows Parity

- [ ] Run the physical Windows 10/11 matrix for timestamped shortcuts, live
  microphone changes and recovery, lock/cancel, Paste Last, clipboard restore,
  History/replacements, audio control, HUD/tones, tray, startup, installer, and
  signed update restart.
- [ ] Complete OpenCode mode rewriting and ordered TypeScript transformations
  using the same portable processing policy as macOS.
- [ ] Add opt-in Commands only after selecting and measuring a real Windows
  streaming recognizer; do not introduce a placeholder model abstraction.
- [ ] Port the authenticated local transcription host and direct-child handoff.
- [ ] Add developer Meetings only after the ordinary dictation and distribution
  matrix is physically green.

### Phase 5: Prove Releases Instead Of Inferring Them

For every promoted capability, require all of the following evidence:

1. `cargo fmt --check`, target-native tests, all-target/all-feature Clippy with
   warnings denied, and `git diff --check`.
2. An optimized native build and deterministic release-mode previews for every
   affected pane and HUD state.
3. Physical input, audio, focus, clipboard, tray/login, and recovery tests on
   the supported operating-system versions; mocks do not prove native adapters.
4. Clean-install and real cross-version signed-update tests for the distribution
   channel that claims the feature.
5. Updated capability matrices and diagnostics that distinguish source-complete,
   release-enabled, and physically validated behavior.

## Validate The macOS Release

Runtime logs now live in Application Support, first-run setup installs the
selected dictation model, and the release flow signs, notarizes,
staples, Gatekeeper-assesses, and publishes Sparkle updates through R2.

The quarantined public DMG and a genuine automatic 2.0.0-to-2.0.1 Sparkle update
have been validated on the development Mac. Before broadly sharing it, validate
the DMG from a clean macOS account, including every TCC permission, model
installation, and source-free launch. Preserve the stable bundle identity that
owns Microphone, Accessibility, Input Monitoring, and Automation permissions.

## Finish Settings And First-Run Health

Microphone selection, idle-safe live switching, unavailable-device fallback,
sound-effect volume, application/browser-host processing modes, and the native
`SMAppService` login-item toggle now ship. Runtime settings changes project live
and persist atomically; launch-at-login remains owned by macOS rather than the
settings file. A persistent menu-bar item keeps Settings, Paste Last, updates,
and Quit reachable when the Dock icon is hidden. Settings now surfaces revoked
Microphone, Accessibility, or Input Monitoring access after onboarding, and
supported transcription runtimes expose automatic language detection. Keep true
processor chains as future work only if real profiles need composition.

Replace the public Swift app through the direct, explicitly confirmed migration
in [`docs/plans/swift-to-rust-migration.md`](docs/plans/swift-to-rust-migration.md).
Converge both installed populations on `Hex.app`, `com.kitlangton.Hex`, the
existing Rust `voice-control` Application Support root, and the R2 update channel
only after the allowlisted preference import and agreed parity work are complete.
Transition tooling now builds both host-name archives and validates the R2 and
legacy-S3 feeds; signed upgrade testing and the physical soak remain.

## Validate The Linux X11 Beta

Run a genuine signed update between two published versions on the target
Arch/i3 machine. Verify exact artifact validation, atomic activation, restart,
and retained-version rollback. Smoke-test installation, model download, the CLI
microphone override, hotkey rebinding, locked capture, cancellation, automatic
paste, tray behavior, autostart, and startup without a tray host.

Keep app-managed updates limited to the user-local direct-install layout. A
future Arch package must leave updates to the package manager. The developer
command prototype is not a release capability until its context, packaged
assets, native actions, and physical validation land. Add meetings or Wayland
only as explicit later capability slices.

Converge the separate macOS and Linux GPUI roots on one capability-driven
product shell without merging their lifecycle or platform behavior. Follow
[`docs/plans/shared-desktop-ui.md`](docs/plans/shared-desktop-ui.md); preserve
both settings formats and delete the duplicate Linux render tree only after the
shared Settings and Activity slices work on both hosts.

## Establish The Windows Alpha

The source-build product loop and desktop host are implemented. Physically
smoke-test the new double-tap lock, Paste Last, live microphone switching,
clipboard restoration, recording tones, click-through HUD, close-to-tray, and
Launch at login on Windows 10 and 11. Include the implemented mutually exclusive
release-microphone-while-idle policy in that matrix. Add bounded automatic stream
recovery without moving capture or paste work onto the UI thread.

Continue toward macOS product parity in complete vertical slices. Modes with
per-application and web-domain corrections, Voice Action with in-app OpenCode
model selection, the UI Automation browser-context adapter, the signed
installer/update contract, and shared interface translations now ship; the
Linux shell renders the same shared settings surface and model catalog.
Shared first-run onboarding now ships as well. Remaining slices are OpenCode
mode rewriting with TypeScript transformations, the persisted opt-in command
engine (waiting on a Windows streaming model), the authenticated local
transcription host, and developer-only Meetings. The detailed live matrix is in
[`docs/windows.md`](docs/windows.md).

Do not advertise a missing capability merely because its macOS pane can be
drawn. Each Windows pane must own its real platform behavior and validation
contract. Keep DirectML, CUDA, and other accelerators out of the baseline until
the CPU backend and one real accelerated backend can be compared on supported
hardware.

## Harden The Real-Time Boundary

Append-only observation serialization and flushing now run on a bounded writer
instead of the semantic coordinator. Replaceable partial transcript and context
observations may be dropped under pressure; completed transcripts, state
transitions, command outcomes, and failures retain bounded backpressure.

The authoritative dictation timeline now drains timestamped CoreAudio on its own
thread; Moonshine receives a duration-bounded, generation-safe projection that
may reset without affecting active capture. Physically smoke-test delayed
shortcut boundaries, custom modifier and key chords, lock behavior,
cancellation, foreground insertion, and both reserved paste shortcuts.

The first installed timestamped-timeline build regressed physical dictation:
shortcut input arrived, but long captures discarded and one accepted job reached
inference with zero milliseconds of audio. The source treated raw CGEvent Mach
ticks as nanoseconds; on the development Mac's `125/3` timebase, a three-second
hold appeared to last roughly 72 ms and CoreAudio release trimming removed the
whole clip. Source timestamps now convert through the Mach timebase. Physically
verify non-empty captures, exact delayed release boundaries, and immediate HUD
onset before considering the new owner validated.

Keep the microphone and dictation model warm by default. Add an explicit
`Release microphone while idle` option that is effective only when commands are
disabled; document that it removes pre-roll and adds first-capture latency. `Do nothing` is the
default recording-audio behavior. Idle-sleep prevention is automatic and scoped
to intentional dictation and active meeting capture rather than persisted as a
user setting.

Add end-of-speech detection only if it preserves explicit Send, Cancel, and
locked-capture controls.

## Make Diagnostics Incremental

Ratatui and GPUI now share a bounded incremental event reader with session and
partial-write handling, and observation writes run on a bounded background
writer. Add heartbeat or process-liveness evidence so a crash cannot leave an
old `Listening` event looking live.

Add an inspectable audit that separates capture failures, recognition errors,
sleeping or discarded utterances, context mismatches, resolution misses, queue
pressure, and action failures. Recommendations may propose command-taxonomy
changes but must not mutate compiled configuration automatically.

Authenticated developer control now rides the existing loopback API through the
debug-only `hex dev` subcommand: it inspects app state, drives semantic HUD
scenarios, opens/focuses panes, and toggles command mode on the GPUI thread.
Extend the same bounded typed channel with fixture-audio capture, microphone
recovery, model switching, permission snapshots, deterministic screenshots,
and assertions over capture/job/UI state. MCP may wrap this protocol later; it
must not become a second control server or settings authority.
When that wrapper exists, publish it as a standard npm stdio package with a
`bin` entry so clients can run it through `npx -y`, and publish matching MCP
Registry metadata. Do not publish an MCP package before the protocol exposes a
real supported user capability.

## Generalize Context From Evidence

Foreground application identity now comes from
`NSWorkspace.frontmostApplication`, and focused window titles come from the
bounded Accessibility bridge rather than repeated System Events scripts. Keep
command and dictation-profile semantics expressed in application and browser-host
terms rather than Brave-specific types.

Add one real second browser adapter before extracting a generalized adapter
framework. Candidate implementations include Chromium-family AppleScript,
Safari AppleScript, browser extensions, or Accessibility APIs. Diagnose Slack
Huddle detection separately from generic browser context. Surface context age
and failures instead of silently retaining an indefinitely stale snapshot.

Keep installed-application discovery lazy. Opening Settings must not recursively
scan application bundles or rasterize their icons; start that work only after
the user opens `Add application`, and verify ordinary startup requests no
Downloads, Calendar, or other unrelated TCC access.

## Complete Meeting Lifecycle Recovery

When meeting microphone activity disappears, ask `Keep Recording` or
`Stop & Transcribe`; never stop automatically. Add EventKit correlation for
titles and reminders, explicit input-device selection, structured capture-gap
observations, active-WAV repair, and resumable final transcription. Status and
final-publication recovery already exist; do not replace them.

Keep active recording discoverable after the main window closes through a
persistent menu-bar/status affordance. Add transcript search; scrolling and
follow-live behavior are already implemented.

Give durable transcript entries stable identities so live-to-final reconciliation,
incremental UI refresh, and meeting-delta paste can advance without rereading
broad history. Keep `You` and `Computer` source labels and the recoverable live
draft when final transcription fails.

Add end-to-end coverage for voice-triggered start/stop, shutdown during capture,
forced final-transcription failure, and live provider detection. Evaluate
synchronized playback, echo annotation, summarization, and person-level
diarization only against real meeting recordings.

## Deliberate Deferrals

Keep protected lifecycle commands and typed captures in compiled Rust. Ordinary
literal commands and dictation control phrases already use the explicit
TypeScript user config. Do not generalize that config into a plugin framework.

Do not generalize the concrete macOS, Linux X11, and Windows adapters into a
platform framework. Add seams only when implemented adapters require one.
Follow the remaining capability contracts and exit criteria in
[`docs/plans/linux.md`](docs/plans/linux.md).
