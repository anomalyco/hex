# HEX Feature Map

What can the user do, how do they reach it, and what would prove it works?
This maps existing behavior. [ROADMAP.md](../../ROADMAP.md) owns future work;
[AGENTS.md](../../AGENTS.md) owns implementation invariants.

Initial source baseline: September 1, 2026, `9e9da53aa9ed`. Public macOS release:
[2.1.16](../releases/2.1.16.md), with the long-form Cohere correction and menu-bar
model switching. The initial map was source-only. Listed checks are
locators unless an executed result is explicitly recorded, as in the
[keyboard-layout verification](recovery.md#keyboard-layout-resolution).

## Product Map

```ts
HEX                                      // macOS release
├── Get ready                            // install, permissions, local model
├── Dictate                              // hold OR double-tap lock
├── Use the result                       // paste, Send, paste-last, History
├── Adjust behavior                      // shortcut, microphone, recording preferences
├── Process text                         // context-selected Modes
├── Opt in                               // Voice Action and Commands: separately off
├── Recover                              // prove the next attempt works
└── Maintain the app                     // menu bar, login, updates, quit
```

[Dictation](dictation.md), [Voice Commands](commands.md),
[Voice Action](voice-action.md), and [Recovery](recovery.md) have detailed maps.
The other capabilities stay here until they need their own verification recipe.

## Get Ready

Install the signed macOS app with `brew install --cask anomalyco/tap/hex` or the
[manual DMG](../../README.md#install-hex). The cask uses a versioned, checksum-pinned
DMG, requires Apple silicon/macOS 15+, and keeps Sparkle updates enabled. It does
not require Bun or OpenCode, launch the app automatically, migrate the legacy
Swift app, or forcibly replace an existing app destination. Homebrew uninstall
leaves runtime settings, models, and retained data intact.

```ts
Launch Hex                               // setup
  -> Grant Microphone / Input Monitoring / Accessibility
  -> Choose language and local model
  -> Ready for ordinary dictation         // no Bun, OpenCode, or Commands required

Settings > Local transcription
  -> Prepare candidate
     ├── Success -> Save selection
     └── Failure -> Preserve previous selection and show error

Permissions revoked later
  -> Settings shows the missing access and repair action
```

Apple silicon, macOS 15+. Choices come from the compiled model/language catalog;
Apple Speech is currently excluded from the desktop picker.

```ts
Choose a supported language              // setup.language-conditioning; macOS + Linux picker
├── Parakeet v3 -> Detect language from audio; selected language does not constrain output
└── Whisper -> Use selected language to guide decoding; accuracy is not guaranteed
```

Parakeet v3 remains available for Portuguese and its other existing language
choices. Its card explains the limitation and suggests Whisper for
language-guided transcription. This does not change saved selections or expand
the Auto picker choices. Multilingual audio support is not language conditioning:
the pinned v3 GGUF has no `stt.parakeet.prompt.num_prompts` metadata, which gates
language prompting in `transcribe-cpp-sys` 0.1.3.

Sources: [model catalog](../../src/transcription_models.rs),
[shared picker](../../src/desktop_transcription_picker.rs), and the runtime
options in [parakeet.rs](../../src/parakeet.rs) and
[linux_transcriber.rs](../../src/linux_transcriber.rs). The regression
`parakeet_v3_supports_portuguese_without_language_conditioning` checks that the
Portuguese selection remains valid and available, sends no runtime language
hint, and contrasts Whisper's `pt` hint. It does not run inference.
The regression was observed failing with the old metadata and passing after the
correction. The macOS release picker preview built, but screenshot capture was
blocked by missing Screen Recording access; visual layout remains unverified.

**Unreproduced recognition report:** [#68](https://github.com/anomalyco/hex/issues/68)
reports Portuguese speech misrecognized as English. Source and pinned-model
metadata inspection establish the prompting limitation, not the cause or repair
of those transcripts. No v3 audio reproduction or transcription-quality fix has
been demonstrated.

Checks start in [onboarding.rs](../../src/onboarding.rs),
[transcription.rs](../../src/transcription.rs), and
[transcription_models.rs](../../src/transcription_models.rs). Setup/picker previews
prove rendering, not clean-account permission grants or model preparation.

```ts
Menu bar > Transcription Model            // setup.quick-model-switch; macOS
├── Downloaded models -> Last selected language shown beside each model
├── Choose -> Verify installed artifact -> Prewarm -> Save selection
│   ├── Success -> Checkmark moves; Settings reflects the saved selection
│   └── Failure -> Previous selection remains; open Settings for the error
├── Preparing -> Cancel Model Switch      // never cancels dictation/output jobs
└── Manage Models… -> Settings picker      // downloads and language changes
```

The menu does not open Settings merely to switch models, download a missing model,
or replace a corrupt artifact. Each model retains one last-used language/hints
selection in `transcription_recents`; the current selection takes precedence.
Older settings start with no remembered alternatives. Select an alternative's
language in Settings once to establish it. No additional model is loaded at startup
just to populate the menu, and the checkmark means selected, not live-worker readiness.

[transcription_preparation.rs](../../src/transcription_preparation.rs) is shared
by Settings and the menu bar: one worker plus one replaceable pending choice.
Closing Settings does not abandon preparation. A superseded/cancelled completion
cannot save, and delivered menu controls are drained before accepting completions.
The desktop root commits through an existing editor to retain unrelated unsaved
settings, or through persisted settings when no window exists.

Sources: [status_item.rs](../../src/status_item.rs),
[meeting_watcher.rs](../../src/meeting_watcher.rs), and
[app_window.rs](../../src/app_window.rs). Key checks are
`preparation_has_one_worker_and_only_keeps_the_latest_pending_choice`,
`cancelling_a_completed_unpolled_preparation_never_commits_it`,
`model_switches_remember_language_and_hints_without_changing_other_settings`, and
`failed_model_selection_save_preserves_selection_recents_and_unsaved_edits` in
the preparation owner and [app_settings.rs](../../src/app_settings.rs).
Additional checks cover worker failure/retry, installed-only checksum verification,
and menu choice/tag projection. These do not establish a physical menu click or
an installed-app switch. Remembering a language does not change the language
conditioning limitations described above.

**Observed September 4, 2026 (2.1.16):** all 455 Rust tests
passed in debug and release, with ten opt-in tests skipped per profile. All twelve
keyboard-layout child scenarios passed per profile; formatting, strict
all-target/all-feature Clippy in both profiles, and the release build passed.
The Cohere fixture inference check was run separately; see
[dictation model windows](dictation.md#ongoing-jobs-and-output). Native menu
interaction, installed-app switching, and Linux runtime behavior were not exercised.

**Known gap:** `is_installed` checks size, not checksum. Not every native-load
path requires verification. Picker Active means selected/installed, not live
worker readiness; a later worker-load failure does not roll back saved selection.

## Dictate

```ts
Saved shortcut                           // dictation; Option by default
├── Hold -> Speak -> Release
├── Tap, tap -> Speak hands-free -> Press again   // Double-tap to lock enabled
└── Escape
    ├── Capture active -> Cancel that capture
    └── Otherwise -> Cancel newest cancellable job

Settings > Double-tap only               // optional for key bindings
  -> Disable single-hold activation
  -> Keep the double-tap hands-free path
```

The [detailed map](dictation.md) covers timing, successive captures, output
commitment, named checks, and native-proof gaps. Optional
[spoken entry and controls](commands.md#spoken-dictation-controls) belong to Commands.
The macOS CLI entry is `hex listen`, with an optional authoritative `--device`.
Do not start it beside an existing listener.

## Use The Result

```ts
Completed output                         // output
├── Paste -> Current focus, in submission order
├── Spoken Send -> Paste, then Enter      // requires Commands and voice capture
├── Paste Last Dictation                  // menu or Option-Shift-V / saved binding
│   └── Last completed dictation, not Voice Action
└── History
    ├── Inspect / search / copy / delete
    ├── Retention -> Seven days by default, always hard-capped
    ├── Off -> Stop new entries; preserve existing entries
    └── Clear all -> Explicit deletion
```

Checks in [paste.rs](../../src/paste.rs), [parakeet.rs](../../src/parakeet.rs), and
[history.rs](../../src/history.rs) cover output and retention boundaries.
They do not prove that an arbitrary target consumed its clipboard contents.
Send is not an atomic target-app transaction.

**Easy to misread:** History Off does not stop diagnostic logging. Diagnostic
logs are separate and currently unbounded on disk. See the
[privacy guide](../../README.md#privacy-and-local-data).

## Adjust Behavior

```ts
Settings                                 // settings
├── Dictation shortcut -> Capture a replacement binding
├── Microphone -> Automatic or saved device
├── Microphone mode
│   ├── Keep ready (fast) -> Open while idle; pre-roll available
│   └── Release when idle -> Open on press; no pre-roll; startup delay
│       └── Commands enabled? -> Confirm turning Commands off
├── While dictating -> Mute / Pause media / Do nothing
│   └── Intentional capture only, not ordinary shortcut chords
└── Sound volume -> Immediate feedback setting; zero suppresses tones
```

Successful settings saves persist and project changes at safe runtime boundaries.
Persistence, conflict, and ownership checks live in
[app_settings.rs](../../src/app_settings.rs),
[recording_environment.rs](../../src/recording_environment.rs), and
[audio.rs](../../src/audio.rs). Settings previews do not prove physical device
switching or native mute support; muting is best-effort, not universal.

**Easy to misread:** an open microphone is not an active recording. Sleeping
Commands still needs open input; it is not Release when idle.

## Process Text With Modes

```ts
Modes                                    // modes
├── Select a row -> Edit that mode        // does NOT manually activate it
└── Foreground context -> Select processing
    ├── Matching Brave website
    ├── Otherwise matching application
    └── Otherwise Global

Selected mode                            // replaces Global, does not layer over it
  -> Corrections
  -> OpenCode transformation             // optional provider-backed rewrite
  -> Transformations                     // ordered text chain
  -> Paste or Send

Stage fails -> Preserve its input        // failed chain discards partial results
```

Built-in text transformations run natively. Custom transformations require the
managed Bun workspace, not Commands enabled. OpenCode rewriting sends documented
context to the configured provider; speech transcription stays local.

Selection and stage checks start in
[dictation_processor.rs](../../src/dictation_processor.rs),
[text_replacements.rs](../../src/text_replacements.rs), and
[personal_commands.rs](../../src/personal_commands.rs). They do not prove live
provider availability or real application/Brave context changes.

Application activations compare the picker's bundle name with the foreground
application's localized name. When Finder shows all filename extensions, the
picker name arrives as `Ghostty.app`; [context.rs](../../src/context.rs) strips
that suffix on both sides and [app_settings.rs](../../src/app_settings.rs)
rewrites already persisted activations on load, checked by
`application_matching_ignores_finder_bundle_extensions` and
`loading_strips_finder_bundle_extensions_from_mode_applications`.

**Fixed in 2.1.14:** through 2.1.13, a mode saved with that Finder
preference enabled never activates and every dictation falls back to Global.
Update to repair saved selections automatically. For older builds, the workaround
is to quit HEX, remove the `.app` suffix from
`dictation_processing.modes[].applications` in `settings.json`, then relaunch.

## Use Voice Action

[Voice Action](voice-action.md) maps the separate opt-in, hold-only shortcut,
optional selection, local transcript-to-OpenCode-to-paste path, and cancellation
boundary. It owns the mode-bypass/last-result rules, checks, and
[cold modifier-order gap](voice-action.md#cold-modifier-order).

## Speak Commands

[Voice Commands](commands.md) maps the command opt-in and separate Moonshine
model, awake/sleep state, contextual catalog, personal config, and
[spoken dictation controls](commands.md#spoken-dictation-controls).
It owns the checks and [host/startup gaps](commands.md#gaps-and-constraints);
actual capture and output remain in [Dictation](dictation.md).

## Recover And Maintain

```ts
Cancellation / interruption / failure    // recovery
  -> Scoped stop, fallback, or error
  -> Retry or fresh gesture
  -> Next capture succeeds               // this is the proof, not a cleared error

Menu bar and Settings                    // maintenance
├── Settings / Paste Last -> Reachable with Dock icon hidden
├── Launch at Login -> macOS owns registration state
├── Check for Updates -> Signed packaged-app Sparkle lifecycle
└── Quit -> Orderly shutdown
```

```ts
Launch Hex                               // maintain.startup
├── Setup incomplete / permissions or model missing -> Open setup or Settings
├── Show Dock icon on -> Open Settings
└── Show Dock icon off + usable menu-bar item -> No startup window
    ├── Dictation starts independently of the Settings window
    └── Menu-bar Settings / Finder or Spotlight reopen -> Open or focus the window

Menu-bar installation fails -> Show Dock icon and open the window // recovery access
```

The existing **Show Dock icon** preference controls quiet startup; there is no
additional launch-window setting. This applies to normal and login launches,
does not change login registration, and does not hide an already open window
when the preference changes. Explicit previews always open their requested pane.

Checks in [meeting_watcher.rs](../../src/meeting_watcher.rs):
`dockless_startup_stays_quiet_only_when_setup_and_menu_bar_are_ready` and
`dock_visible_startup_always_opens_the_app` cover the startup decision. The
existing `on_reopen` and status-item `OpenSettings` paths bypass that decision.
These checks do not establish signed-app login behavior or native Finder/Spotlight
reopening; those still need an installed-app smoke test. This addresses the
menu-bar-only case in [#64](https://github.com/anomalyco/hex/issues/64), not an
independent window preference for users keeping the Dock icon visible.

**Executed September 3, 2026:** 441 Rust tests passed (nine opt-in tests ignored),
along with all twelve keyboard-layout child scenarios, formatting, and Clippy.
The release Settings preview built and launched, but window screenshot capture
failed; no visual or installed-app startup verification is claimed.

```ts
Update available                         // maintain.updates
├── Sidebar Update -> Sparkle update dialog
└── Menu bar > Check for Updates -> Same dialog

Remind Me Later -> Dismiss this offer -> Manual update -> Reopen the offer
```

The sidebar uses window-local deferred action dispatch in
[app_window.rs](../../src/app_window.rs). The executed
`sidebar_update_click_dispatches_only_outside_preview` regression clicks the
production control in an active in-memory GPUI window: zero actions before the
fix, one afterward, and no action in a preview. It does not start Sparkle.

**Fixed in 2.1.13:** older releases through 2.1.12 used app-global dispatch from
inside the sidebar click, losing the action while the active window was borrowed.
The click regression passed in debug and optimized release builds. The menu-bar
path was exercised on the installed local 2.1.11 build and opened the native
Software Update window; no installation was performed. That path remains the
workaround for older versions, including after Remind Me Later. A repaired
installed-sidebar click remains unverified.

Release `3675206`, build `20113`, was signed, notarized, stapled, and accepted by
Gatekeeper. DMG and ZIP payloads matched across 207 entries. Public DMG, ZIP, and
latest-DMG downloads matched their prepared SHA-256 checksums; the public feed
led with `20113`, and its ZIP signature verified against the app's public key.

**Published September 3, 2026:** [2.1.14](../releases/2.1.14.md), release commit
`cc7f843`, build `20114`. The combined release passed 442 Rust tests (nine opt-in
tests ignored), all twelve keyboard-layout scenarios in debug and release,
46 command-SDK tests, Clippy, formatting, and app identity guards. The first
optimized harness attempt exceeded the command timeout during compilation;
the rerun completed and all twelve scenarios passed.

The app and DMG were Developer ID signed, notarized, and stapled; Gatekeeper
accepted both. DMG and ZIP app payloads matched across 207 entries, including
file bytes, permissions, and symlink targets. After artifact-first/feed-last
publication, public DMG, ZIP, and latest-DMG downloads matched the prepared
SHA-256 hashes. The public feed led with `20114`, and its ZIP signature verified
against the app's public key. The publication script's recursive diff emitted
framework directory-loop warnings; the separate no-follow manifest comparison
verified the complete payloads without traversing symlinks.

The installed app and live settings were not modified. The live quiet-startup,
Finder/Spotlight reopen, and physical dictation smoke test was explicitly waived
for this release; those paths remain unverified on the installed candidate.
No Sparkle installation or Linux binary release was performed.

**Published September 3, 2026:** [2.1.15](../releases/2.1.15.md), release commit
`9ec1b51`, build `20115`. The current-version checks passed 444 Rust tests in each
of debug and release (nine opt-in tests ignored per profile), all twelve
keyboard-layout scenarios in each profile, 46 command-SDK tests, strict Clippy in
both profiles, formatting, and app identity guards.

The app and DMG were Developer ID signed, notarized, and stapled; Gatekeeper
accepted both. A no-follow manifest comparison matched all 208 entries including
the app root, file bytes, modes, and symlink targets across DMG and ZIP. After
artifact-first/feed-last publication, public DMG, ZIP, and latest-DMG downloads
matched their prepared SHA-256 hashes. The public feed led with `20115`, and its
ZIP signature verified against the app's public key. The existing recursive-diff
warnings in the publication script remain; the separate no-follow check passed.

[Homebrew cask PR #16](https://github.com/anomalyco/homebrew-tap/pull/16) passed
style, strict online audit, Sparkle livecheck, and checksum-verified download.
Installation and uninstall succeeded with an isolated temporary application
directory. The installed candidate passed identity, signature, stapled-ticket,
and Gatekeeper checks. An existing-app fixture was refused without replacement;
the real installed app's Info.plist and executable hashes were unchanged.

Screen Recording preflight reported unavailable, so no screenshot is claimed.
The installed app was not replaced or launched, and no physical dictation,
Sparkle installation, or Linux binary release was performed.

**Published September 4, 2026:** [2.1.16](../releases/2.1.16.md), release commit
`76dc552`, build `20116`. Both profiles passed 455 Rust tests and all twelve
keyboard-layout scenarios, with ten opt-in tests skipped per profile. The real
Cohere synthetic-audio regression passed separately. Both profiles passed strict
Clippy; formatting, app-identity fixtures, 46 command-SDK tests, and 45 public-SDK
tests passed, along with SDK typechecks and the public SDK build.

The app and DMG were Developer ID signed, notarized, stapled, and accepted by
Gatekeeper. The DMG and Sparkle ZIP matched across 208 no-follow payload entries.
After artifact-first/feed-last publication, fresh public DMG, ZIP, and latest-DMG
downloads matched the prepared artifacts byte-for-byte; the public feed led with
`20116`, and its Ed25519 signature verified against the app's public key. The
existing recursive-diff framework-loop warnings were independently covered by
the no-follow comparison.

[Homebrew cask PR #17](https://github.com/anomalyco/homebrew-tap/pull/17) updated
the version/checksum and was merged after style, strict online audit, livecheck,
checksum fetch, isolated installation/uninstallation, app identity/signature,
stapled-ticket, and Gatekeeper checks. An existing-app fixture was refused without
replacement. The test cask was removed; the real installed app's Info.plist and
executable hashes were unchanged. Release replies were posted and
[#73](https://github.com/anomalyco/hex/issues/73) and
[#74](https://github.com/anomalyco/hex/issues/74) were closed after publication.

Screen Recording preflight remains unavailable. This release has no physical
menu-click, installed-app model switch, microphone/paste, or Sparkle installation
proof. No Linux binary is published. The
[Linux/Nix CI run](https://github.com/anomalyco/hex/actions/runs/33891943549)
passed for release commit `76dc552`: the Linux job completed in 3m50s and Nix in
40m10s. This includes isolated X11 grabs, Wayland paste into GTK, virtual-microphone
capture/inference/paste, signed-installer tamper rejection, and the installed Nix
package checks. These CI checks remain separate from native macOS and physical
Linux verification.
The [performance investigation](../research/performance-2026-09-04.md) changed
no runtime defaults.

[Recovery](recovery.md) separates supported recovery from known defects.
[login_item.rs](../../src/login_item.rs), [status_item.rs](../../src/status_item.rs),
[sparkle.rs](../../src/sparkle.rs), the
[identity test](../../scripts/test-app-identity.sh), and
[app validation](../../scripts/validate-app.sh) are maintenance evidence starting
points. Preview badges do not prove installed-app login registration or a signed
update from an older supported build.

## Other Platforms And Consumers

```ts
Linux beta                               // not macOS feature parity
├── hex start -> Per-user service -> Alt-Space capture -> Transcribe -> Paste
├── hex app -> Settings client; closing it leaves the runtime running
├── Recording sounds -> Shortcut press / capture stop / active cancellation
├── X11 -> No tray or recording HUD; service status and sounds remain available
├── Wayland -> evdev + compositor protocols; keys observed, not suppressed
├── Escape -> Cancel active capture, not newest accepted job
├── Microphone failure -> Listener exits, not macOS automatic recovery
└── Paste -> Retain transcript clipboard, not restore previous contents
```

No Linux Commands, Voice Action, Modes processing, retained History, or meetings.
`hex listen` instead prints Moonshine transcripts. Wayland needs explicit broad
input-device access; physical reconnect and click-through still need native
evidence. Its [smoke](../../scripts/test-wayland-paste.sh) explicitly isolates
`HEX_APPLICATION_SUPPORT_DIR` and checks that Settings can exit without stopping
the service.

See the [Linux guide](../linux.md), [linux_app.rs](../../src/linux_app.rs),
[linux_wayland_input.rs](../../src/linux_wayland_input.rs), and
[Linux CI](../../.github/workflows/check-linux.yml) for existing checks.
The [direct installer/updater](../../src/linux_updater.rs) and
[installer tests](../../scripts/test-install-linux-release.sh) do not establish
a published, supported-host-validated signed release. [Nix](../nix.md) owns its
package updates; HEX updates only managed direct installs.

```ts
Linux runtime                            // maintain.linux-service
├── hex start / desktop autostart -> systemd --user hex.service; no Settings or tray
├── Settings / CLI -> Same-user, owner-only Unix socket -> Typed commands and snapshots
├── Close/crash Settings -> Keep microphone, accepted jobs, and model preparation
│   └── Uncommitted shortcut capture -> Cancel and restore the prior listening state
├── Service restart -> Client reconnects; uncertain commands are not replayed
└── hex stop / systemctl --user stop hex -> Stop workers and release devices
```

Sources: the service owner and client in [linux_app.rs](../../src/linux_app.rs),
[linux_service.rs](../../src/linux_service.rs), and the managed installer/Nix
user units. `hex status` queries the running service; `--lines` explicitly reads
historical observations. `hex app --hidden` is a compatibility alias for starting
the service. GUI exit does not mean Stop Listening; the explicit listener and
service controls retain separate meanings. No root daemon or new input grants.

Checks: [test-linux-service.py](../../scripts/test-linux-service.py) exercises
real IPC and process lifetime without a display, installed model, or audio device.
`closing_a_client_does_not_stop_normal_dictation` and
`client_disconnect_cancels_its_edit_and_restores_previous_listening` use controlled
runtime workers. The virtual-microphone and Wayland scripts now run separate
service/client processes. These checks do not establish a physical desktop logout,
cross-version signed update, or microphone/target-app success on the user's devices.

**Observed September 4, 2026, local service build:** 139 Rust tests passed (seven
native/opt-in tests skipped), together with strict Linux bin/tests Clippy, the
release build, the release-binary IPC/lifecycle script, and installer fixtures.
The managed Arch/i3 installation passed `systemd-analyze --user verify` after
correcting the unit's `EnvironmentFile` syntax. Its environment file cleared the
user manager's stale Wayland values; the service reported X11 and Listening.
The installed executable SHA-256 was
`cbfa91894c5b592254da41a3d2791f9461e5995abdac3da90a2639aefe3e52f1`.

The installed Settings client had no audio handles or runtime locks. Closing its
window left the same systemd service PID running and Listening. The screenshot
at `/tmp/opencode/hex-service-settings.png` also retained an empty-transcript
failure; it was not dismissed to make the lifecycle check pass. This validates
client-independent ownership, not transcription accuracy. The modified virtual
microphone/Wayland smokes, Nix evaluation, physical logout, and signed
cross-version service updates were not run on this host.

```ts
Linux Settings > Sound volume             // dictate.feedback.linux; X11 + Wayland
├── Default / older settings -> 50%
├── Off -> No recording tones
└── 25% / 50% / 75% / 100% -> Save and apply live; preview the start sound

Capture                                  // hex app and hex dictate
├── Shortcut starts capture -> Queue start sound immediately, before audio arrives
├── Finish retained recording -> Stop sound, before transcription/paste
├── Brief discarded tap -> Start sound, but no stop sound or transcription
└── Cancel active capture -> Cancel sound; idle Escape stays silent
```

The shared [feedback player](../../src/feedback.rs) uses the bundled recording
sounds on both platforms; only macOS wake/sleep/error tones use `afplay`.
Decoding and output initialization happen before the Linux capture loop, and
playback admission is bounded and nonblocking. Output preparation failures are
logged without blocking dictation. Sound-volume saves in
[linux_app.rs](../../src/linux_app.rs) preserve listener ownership; failed saves
leave the previous selection active, and shortcut/model edits temporarily disable
the control to prevent overlapping settings writes. The start sound does not wait
for the 300 ms capture-retention threshold; input delivery and audio-device
latency still apply.

Checks: `bundled_recording_sounds_decode_without_an_audio_device` and
`feedback_admission_never_waits_for_playback` in the player, plus
`recording_volume_defaults_for_new_and_legacy_settings_and_round_trips` and
`invalid_sound_volume_is_rejected_before_saving` in
[linux_settings.rs](../../src/linux_settings.rs). The desktop host checks
`invalid_volume_preserves_settings_and_does_not_stop_the_listener` and
`volume_changes_cannot_overwrite_an_in_flight_settings_edit` cover invalid-volume
and overlapping-edit guards, not real disk failures.
`start_sound_precedes_audio_and_does_not_change_short_tap_discard` in
[linux_dictation.rs](../../src/linux_dictation.rs) exercises the production start
path with a sound spy: immediate feedback without any audio, while a 100 ms tap
still discards. Existing timing checks in [dictation.rs](../../src/dictation.rs)
cover the unchanged intentional-hold boundary. These are not proof of audible
native playback or physical hotkey-to-sound timing.

**Observed September 4, 2026, initial Linux sound build (before immediate-start
feedback):** 134 Rust tests passed
(seven native/opt-in tests skipped), along with formatting, strict Linux
bin/tests Clippy, and the release build. The locally modified `4ff6c4c` build
(executable SHA-256 `609f16cac11eb66cea7054092e7c4caee3f02c3bd1b8828f5232cf3b2b67df31`)
was installed and launched with `hex app` on Arch/i3/X11. It reached Listening
and opened a PipeWire ALSA playback stream; existing settings were unchanged.
The installed Settings capture at `/tmp/opencode/hex-linux-sounds.png` showed
the new volume control with 50% selected. Actual tone audibility, physical
hotkey-to-sound timing, Wayland UI, and macOS regressions were not exercised.

**Immediate-start refinement, same day:** 135 Rust tests passed (seven skipped),
including the new start-before-audio regression. Formatting, strict Linux
bin/tests Clippy, and the release build passed. The replacement executable
(SHA-256 `470be7569cf9fa8be60e73554c8311df2b68487b221cefffc7cfd26cb7e11629`)
was reinstalled and restarted on the same Arch/i3/X11 host and reached Listening.
There is no measured native key-to-sound latency or additional macOS/Wayland proof.

Both Linux CI triggers include `tests/**`, and the
[Nix source fileset](../../nix/package.nix) includes those sources. The
`keyboard_layout` harness skips on Linux; packaging it does not provide macOS
layout evidence.

```ts
TypeScript SDK + macOS local service
├── Caller records audio -> Explicit helper -> Raw transcription
├── Running desktop capture API -> Separate microphone ownership protocol
└── Distribution
    ├── SDK 0.3.0 -> Published
    └── Turnkey signed helper -> Unfinished
```

The [SDK guide](../../sdk/typescript/README.md),
[service contract](../specs/local-transcription-service.md), and
[SDK tests](../../sdk/typescript/test) distinguish those paths. Helper-only service
mode is not a physical hotkey/microphone test. Embedded/packaged service smokes
still expect API 1 while service/SDK require API 2. Low-level blocking-peek and
FIN/half-close limitations in [local_api.rs](../../src/local_api.rs) must not be
generalized to every SDK cancellation path.

## Developer And Prototype Surfaces

```ts
macOS debug runtime                      // not release navigation
├── Meetings -> Explicit recording, separate artifacts, recoverable drafts
│   └── Detection may offer recording, never start it automatically
└── Activity / HUD Lab / hex dev -> Observation and developer controls

Desktop previews -> Rendered fixtures    // not proof capture/download/update happened
iOS prototype -> Record/transcribe/copy and keyboard handoff
Unmerged Windows / deferred features -> Not part of this map
```

Sources: [meeting.rs](../../src/meeting.rs),
[meeting_watcher.rs](../../src/meeting_watcher.rs),
[developer_control.rs](../../src/developer_control.rs),
[capture-preview.sh](../../scripts/capture-preview.sh), [iOS guide](../../ios/README.md).
iOS physical-device proof and temporary-WAV failure cleanup remain gaps;
best-effort deletion is not guaranteed deletion.

`default_paste_hotkeys_follow_build_capabilities` in
[app_settings.rs](../../src/app_settings.rs) checks the production default runtime
and settings projections: Paste Last remains available in both build profiles;
meeting paste is present only in debug. `meeting_paste_is_only_active_in_developer_builds`
in [suppression.rs](../../src/suppression.rs) checks the corresponding constructed
hotkey event, including ordinary-chord discard in release. These replace an
unconditional debug-only expectation and a test-only copy of the reservation rule;
they do not establish native keyboard delivery or meeting insertion.

`structural_event_equality_preserves_serialized_identity` in
[events.rs](../../src/events.rs) covers Activity's full-event equality across
event variants, equal timestamps, non-ASCII text, and optional/error payloads.
It does not exercise native Activity selection or scrolling.

**Executed September 3, 2026, source simplification pass:** 444 Rust tests passed
in both debug and release (nine opt-in tests ignored in each), with all twelve
keyboard-layout child scenarios passing in each profile. Formatting and strict
Clippy passed in both profiles. These checks cover the working-tree cleanup, not
a new published app; no installed-app, microphone, or native Linux test was run.

## Grow The Map

```ts
One feature file
├── Sub-features -> Small action/outcome diagrams with stable IDs
├── Entry points -> Every user route and its prerequisites
├── Existing checks -> Source/test links and what they do not prove
└── Gotchas -> Exceptions, known defects, missing native proof
```

This is the map-only stage of the verification workflow, not a working driver.
Keep checks beside the behavior and tests with the implementation. Split new
files only when useful; link shared failure behavior to Recovery.

Future evidence should name the behavior ID, entry point, revision/binary,
platform, prerequisites, outcome, and artifact location. Report skipped paths.
Never publish tokens, personal transcripts, or unredacted diagnostics as proof.
This map does not authorize operating a live installation.
