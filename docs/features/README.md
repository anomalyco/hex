# HEX Feature Map

What can the user do, how do they reach it, and what would prove it works?
This maps existing behavior. [ROADMAP.md](../../ROADMAP.md) owns future work;
[AGENTS.md](../../AGENTS.md) owns implementation invariants.

Initial source baseline: September 1, 2026, `9e9da53aa9ed`. Public macOS release:
[2.1.13](../releases/2.1.13.md), including the later cleanup, keyboard-layout
repair, and sidebar update fix. The initial map was source-only. Listed
checks are locators unless an executed result is explicitly recorded, as in
the [keyboard-layout verification](recovery.md#keyboard-layout-resolution).

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

Checks start in [onboarding.rs](../../src/onboarding.rs),
[transcription.rs](../../src/transcription.rs), and
[transcription_models.rs](../../src/transcription_models.rs). Setup/picker previews
prove rendering, not clean-account permission grants or model preparation.

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

**Released-version defect:** through 2.1.13, a mode saved with that Finder
preference enabled never activates and every dictation falls back to Global.
Workaround until the fix ships: quit HEX, remove the `.app` suffix from
`dictation_processing.modes[].applications` in `settings.json`, relaunch.

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
├── hex app / hex dictate -> Alt-Space capture -> Transcribe -> Paste
├── X11 -> Tray when usable; no recording HUD
├── Wayland -> evdev + compositor protocols; keys observed, not suppressed
├── Escape -> Cancel active capture, not newest accepted job
├── Microphone failure -> Listener exits, not macOS automatic recovery
└── Paste -> Retain transcript clipboard, not restore previous contents
```

No Linux Commands, Voice Action, Modes processing, retained History, or meetings.
`hex listen` instead prints Moonshine transcripts. Wayland needs explicit broad
input-device access; physical reconnect, click-through, and trayless shutdown
still need native evidence. Its [smoke](../../scripts/test-wayland-paste.sh)
inherits higher-priority `HEX_APPLICATION_SUPPORT_DIR`, weakening isolation.

See the [Linux guide](../linux.md), [linux_app.rs](../../src/linux_app.rs),
[linux_wayland_input.rs](../../src/linux_wayland_input.rs), and
[Linux CI](../../.github/workflows/check-linux.yml) for existing checks.
The [direct installer/updater](../../src/linux_updater.rs) and
[installer tests](../../scripts/test-install-linux-release.sh) do not establish
a published, supported-host-validated signed release. [Nix](../nix.md) owns its
package updates; HEX updates only managed direct installs.

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
