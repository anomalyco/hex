# Roadmap

HEX ships the macOS product loop: configurable hold-to-talk local dictation,
explicit selected-text voice editing, optional context-aware OpenCode
post-processing, a Metal HUD, bounded paste workers, settings, and a signed GPUI
app bundle. Streaming commands are a disabled-by-default experimental opt-in;
meetings remain a developer prototype until their product surface is redesigned.
An x86_64 Arch/i3 X11 beta now provides hotkey dictation, automatic paste, a
GPUI shell, and signed user-local updates; it does not yet include commands,
meetings, native Wayland support, or a package-manager channel.
The authenticated local transcription service now implements discovery,
direct-child embedding, model preparation, and bounded host-audio transcription;
Promise and Effect TypeScript wrappers pass fake-helper tests. Signed helper
package validation, an Electron bridge, and a first real consumer remain.

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
`com.kitlangton.Hex` Application Support root, and the R2 update channel only
after the allowlisted preference import and agreed parity work are complete.

## Validate The Linux X11 Beta

Run a genuine signed update between two published versions on the target
Arch/i3 machine. Verify exact artifact validation, atomic activation, restart,
and retained-version rollback. Smoke-test installation, model download, the CLI
microphone override, hotkey rebinding, locked capture, cancellation, automatic
paste, tray behavior, autostart, and startup without a tray host.

Keep app-managed updates limited to the user-local direct-install layout. A
future Arch package must leave updates to the package manager. Add commands,
context, meetings, or Wayland only as explicit later capability slices.

Converge the separate macOS and Linux GPUI roots on one capability-driven
product shell without merging their lifecycle or platform behavior. Follow
[`docs/plans/shared-desktop-ui.md`](docs/plans/shared-desktop-ui.md); preserve
both settings formats and delete the duplicate Linux render tree only after the
shared Settings and Activity slices work on both hosts.

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

Do not generalize the concrete macOS and Linux X11 adapters into a platform
framework. Add seams only when a second implemented adapter requires one.
Follow the remaining capability contracts and exit criteria in
[`docs/plans/linux.md`](docs/plans/linux.md).
