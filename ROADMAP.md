# Roadmap

Hex ships the macOS product loop: configurable hold-to-talk local dictation,
optional context-aware OpenCode post-processing, a Metal HUD, bounded paste
workers, settings, and a signed app bundle. Streaming commands are a
disabled-by-default experimental opt-in, and Meetings (beta) is a separate
persisted opt-in (`meetings.enabled`, default off). Windows (x64 and ARM64) and
x86_64 Linux (i3/X11 and compatible wlroots Wayland compositors) betas provide
hotkey dictation, Modes, History, the shared Rune window, and signed user-local
updates; they do not yet include voice commands or meetings. Nix packaging for
Linux is implemented and awaiting supported-host validation before release.
The authenticated local transcription service now implements discovery,
direct-child embedding, model preparation, and bounded host-audio transcription;
`@kitlangton/hex@0.3.0` publishes ready transcribers through Promise and Effect
entrypoints. Local Electron capture and native transcription have been exercised;
signed helper packaging and a signed first-consumer integration remain. Follow the
[embedded desktop delivery plan](docs/plans/typescript-sdk.md); Apple Silicon
macOS is the proposed first target, not a cross-platform availability promise.

## Validate The Rune Desktop App

The window, menu-bar and tray hosts, HUDs, meeting capsules, and onboarding now
run only on Rune (GPUI is gone). Linux has been run in a real session; macOS
has been compiled, tested, and launched without being seen on screen; Windows
has only been cross-compiled. Before releasing `dev`, a person runs each
OS through the checklist in [the Rune port plan](docs/plans/rune-ui.md#needs-a-person-to-look), including
the Windows x64 and ARM64 tray app, the macOS HUD and meeting capsules, and
shortcut capture on every OS. Restore application icons in the Modes picker
(removed with GPUI) on Rune's raster images, and add native file dialogs on
macOS and Windows.

## Validate The macOS Release

Runtime logs now live in Application Support, first-run setup installs the
selected dictation model, and the release flow signs, notarizes,
staples, Gatekeeper-assesses, and publishes Sparkle updates through R2.

The quarantined public DMG and a genuine automatic 2.0.0-to-2.0.1 Sparkle update
have been validated on the development Mac. Before broadly sharing it, validate
the DMG from a clean macOS account, including every TCC permission, model
installation, and source-free launch. Preserve the stable bundle identity that
owns Microphone, Accessibility, and Automation permissions.

## Finish Settings And First-Run Health

Later, split features from settings into two windows (Kit, 2026-10-24; deferred
in favour of the one-window feature-owned panes): the Hex window keeps what you
use, History and Meetings recordings, and a separate Settings window (⌘, and
the menu bar's Settings…) holds General, Modes, Commands,
Meetings settings, and Experiments.

Microphone selection, idle-safe live switching, unavailable-device fallback,
sound-effect volume, application/browser-host processing modes, and the native
`SMAppService` login-item toggle now ship. Runtime settings changes project live
and persist atomically; launch-at-login remains owned by macOS rather than the
settings file. A persistent menu-bar item keeps Settings, Paste Last, updates,
and Quit reachable when the Dock icon is hidden. Settings now surfaces revoked
Microphone or Accessibility access after onboarding, and
supported transcription runtimes expose automatic language detection. Modes own
corrections, optional OpenCode rewriting, and ordered text transformations;
stage failures preserve the preceding pipeline output. Do not add a generalized
processor framework without a concrete need beyond this shipped sequence.

Keep Swift and Rust separate. Prefer the old app's existing Sparkle update window
with a website-only informational item to install the new app manually, with no
data import or automatic replacement.
Follow [`docs/plans/swift-app-handoff.md`](docs/plans/swift-app-handoff.md).
The new app uses identity `com.kitlangton.hex2`, display name `Hex`, and filename
`Hex.app`, with data in `hex`. Use Kit's approved
Developer ID signing team and validate the new identity before release. The legacy
S3 feed must never install a Rust payload. Publish
the updated Rust artifact first, then the separately validated legacy feed notice.
No Swift rebuild should be necessary for the notice.

## Validate The Linux Beta

Run a genuine signed update between two published versions on the target
Arch/i3 machine. Verify exact artifact validation, atomic activation, restart,
and retained-version rollback. Smoke-test installation, model download, the CLI
microphone override, hotkey rebinding, locked capture, cancellation, automatic
paste, systemd autostart, launch forwarding (`Activate`), and closing the window
without stopping accepted work. Linux no longer uses a tray.

Keep app-managed updates limited to the user-local direct-install layout. A
future Arch package must leave updates to the package manager. Nix owns its
packaged installation and optional Home Manager service. Validate the Wayland
beta on a compatible compositor with physical portal shortcuts, the `hex`
commands bound in a desktop without the portal, microphone, foreground paste,
click-through HUD, and window close leaving the process running while `hex quit`
drains and exits. See [`docs/plans/linux.md`](docs/plans/linux.md). Add commands,
context, meetings, or broader Wayland support only as explicit later capability
slices.

macOS, Windows, and Linux now share one window and one `settings.json` (see
[`docs/plans/shared-desktop-ui.md`](docs/plans/shared-desktop-ui.md)). Remaining
desktop parity: website mode rules and the focused application on Wayland and
for Linux browsers, and voice commands outside macOS.

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

The macOS stop tone now sounds at the release, from the audio owner before it
drains the tail, after a muted output is restored. That change was written
without a Mac: compile it, run the `recording_environment` and `dictation_audio`
tests, and hear the tone at key-up with While dictating set to Mute.

The first installed timestamped-timeline build regressed physical dictation:
shortcut input arrived, but long captures discarded and one accepted job reached
inference with zero milliseconds of audio. At the HID tap, physical timestamps
arrived as raw Mach ticks; on the development Mac's `125/3` timebase, treating
them as nanoseconds made a three-second hold appear to last roughly 72 ms.
The 2.1.7 move to annotated-session taps retained that conversion even though
timestamps at this boundary are already nanoseconds. This stretched short taps
by 41.67 times, breaking double-tap lock and admitting accidental short captures.
A simultaneous passive HID/annotated probe confirmed the different units for
real modifier events. Annotated-session timestamps now enter the audio timeline
without scaling; callback-level regressions cover lock, short-tap discard, and
exact delayed release trimming. Keep physical shortcuts, assistive modifier and
key chords, non-empty captures, and immediate HUD onset in the release smoke
checks; changing tap location requires revalidating the clock as well as routing.

Version 2.1.11 also recovers blocked modifier gestures after missed releases and
navigation-key metadata. Recovery requires a neutral keyboard and emits no
capture actions. Keep the callback regressions for delayed releases and Fn
evidence alongside physical Option-hold checks; a physical Fn/Globe smoke test
and general cross-tap delivery-order validation remain outstanding.

Settings > Shortcut mode now offers Toggle (press to start, press again to
finish) beside Hold on macOS, Linux, and Windows; see
[dictation](docs/features/dictation.md#shortcut-mode). Physically check it with
modifier-only, key-chord, Fn/Globe, and standalone function-key bindings, and
with cold-microphone capture, before calling #77 done.

Track the remaining shortcut requests explicitly:

- [#82](https://github.com/anomalyco/hex/issues/82): menu-bar Pause/Resume must
  remain deferred for now; quitting Hex is sufficient for the current scope.
  If revisited, it must
  suspend capture shortcuts and command activation, cancel only active capture,
  retain accepted output, and require key release before resuming. Test event-tap
  suppression as well as the recognition state. App exclusions are a later slice.
- [#77](https://github.com/anomalyco/hex/issues/77): Toggle mode is implemented;
  it still needs physical chord-discard and cold-microphone checks.
- [#67](https://github.com/anomalyco/hex/issues/67): standalone keypad/character
  bindings need explicit key-consumption, cancellation, and rebinding semantics.
- [#37](https://github.com/anomalyco/hex/issues/37): remaining Caps
  Lock-to-Control behavior needs physical remapping evidence. Side-less flags
  and actual Right Control flags are distinct.

On-demand rewrite [PR #63](https://github.com/anomalyco/hex/pull/63) remains under
review: publish the shared last transcript only after successful paste, require
explicit shortcut opt-in, prove generation while automatic processing is off at
the generation boundary, and reuse the isolated native keyboard test harness.
Do not pre-publish an uncommitted transcript and attempt to repair it afterward.

The microphone and dictation model remain warm by default. The shipped
`Release when idle` microphone option opens the device on shortcut press, removes
pre-roll, and adds first-capture latency. It is mutually exclusive with Commands
through confirmed settings transitions. `Do nothing` is the default
recording-audio behavior. Idle-sleep prevention is automatic and scoped to
intentional dictation and active meeting capture rather than persisted as a
user setting.

Add end-of-speech detection only if it preserves explicit Send, Cancel, and
locked-capture controls.

## Make Diagnostics Incremental

Ratatui and the Activity pane now share a bounded incremental event reader with session and
partial-write handling, and observation writes run on a bounded background
writer. Add heartbeat or process-liveness evidence so a crash cannot leave an
old `Listening` event looking live.

Add an inspectable audit that separates capture failures, recognition errors,
sleeping or discarded utterances, context mismatches, resolution misses, queue
pressure, and action failures. Recommendations may propose command-taxonomy
changes but must not mutate compiled configuration automatically.

Authenticated developer control now rides the existing loopback API through the
debug-only `hex dev` subcommand: it inspects app state, drives semantic HUD
scenarios, opens/focuses panes, and toggles command mode.
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
Huddle detection separately from generic browser context. Capture failures
already log a warning and invalidate the cached context. Surface context age and
failure health in the UI rather than relying only on diagnostics.

Keep installed-application discovery lazy. Opening Settings must not recursively
scan application bundles or rasterize their icons; start that work only after
the user opens `Add application`, and verify ordinary startup requests no
Downloads, Calendar, or other unrelated TCC access.

## Complete Meeting Lifecycle Recovery

Add EventKit correlation for titles and reminders, explicit input-device
selection, structured capture-gap observations, active-WAV repair, and resumable
final transcription. Status, final-publication recovery, the call-ended prompt,
and the menu-bar recording state already exist; do not replace them.

Search over titles, notes, and final transcripts, scrolling, and follow-live
are implemented. Echo suppression is tuned on one real speaker-mode call
(`hex meeting echo <dir>`); validate it on more rooms, volumes, and
headphones, and record capture arrival times so offline replays need no
`--microphone-offset-ms`.

Acoustic echo cancellation (Voice Processing IO) was evaluated and deferred:
it would replace ScreenCaptureKit's raw microphone with an AVAudioEngine or
`kAudioUnitSubType_VoiceProcessingIO` input, which ducks other audio, switches
Bluetooth headsets to the low-quality call profile, applies its own gain and
noise suppression to the transcribed track, runs a second voice processor
beside the call app's, and drops ScreenCaptureKit's shared capture clock. Revisit
only if signal-level suppression proves insufficient, with a per-device opt-in.

`hex mcp` serves meetings to agents read-only over stdio (full-text BM25
with stemming and fuzzy matching, transcripts, stored notes). Next: on-device
semantic search only when full-text misses real paraphrased queries
(NaturalLanguage embeddings through a Swift bridge, cached under
`~/Library/Caches/hex/meeting-index`), EventKit attendees in
`list_meetings`, and a one-shot Granola CSV import
([granola-import.md](docs/plans/granola-import.md)). Keep it a CLI subcommand;
the npm-package guidance above applies only to a developer-control wrapper.

Give durable transcript entries stable identities so live-to-final reconciliation,
incremental UI refresh, and meeting-delta paste can advance without rereading
broad history. Keep `You` and `Them` source labels and the recoverable rolling
transcript when final transcription fails.

Add end-to-end coverage for voice-triggered start/stop, shutdown during capture,
forced final-transcription failure, and live provider detection. Evaluate
synchronized playback, echo annotation, summarization, and person-level
diarization only against real meeting recordings.

## Recommend Better Models To Existing Users

After the Rune port is fully done (the macOS app has passed its smoke test),
add a gentle, dismissible "recommended update" hint in the model picker and
Settings for users whose saved model is no longer recommended for their
language, explaining how to switch. Language routing already keeps their model
and lists it in the picker ([plan](docs/plans/language-recommendations.md)); the
hint never switches or downloads anything itself.

## Shared Engine Service

Later, not started. Move the engine into a launchd service
(`anomaly.hex.engine`) serving the same `/streams` and `/transcriptions` API;
the Hex UI becomes a client. Today the running app serves
[`/streams`](docs/streams.md) on dictation's loaded model, with dictation
first; the service keeps that contract, so stream clients such as Seer don't
change.

## Deliberate Deferrals

The iOS prototype needs explicit accepted-job behavior when its keyboard session
expires. `DictationController` can clear active-job state while an unretained
transcription task still owns completion and temporary-file cleanup. Define the
expiry/restart contract, then give the job its task and generation-scoped
completion; verify that a stale job cannot reset a newer capture or delete its
file. This is a source-identified risk, not a reproduced device failure.

Keep protected lifecycle commands and typed captures in compiled Rust. Ordinary
literal commands and dictation control phrases already use the explicit
TypeScript user config. Do not generalize that config into a plugin framework.

Do not generalize the concrete macOS and Linux X11 adapters into a platform
framework. Add seams only when a second implemented adapter requires one.
Follow the remaining capability contracts and exit criteria in
[`docs/plans/linux.md`](docs/plans/linux.md).
