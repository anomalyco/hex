# Migrate Swift HEX Users To Rust HEX

**Status:** In progress. The preference importer and transition-aware package
and feed tooling are implemented. The public identity transition remains gated
on parity soak and signed upgrade testing.

## Outcome

Rust HEX replaces Swift HEX in place. There is one product and no bridge or
second application:

```text
Published app bundle: Hex.app
Display name:         HEX
Bundle identifier:    com.kitlangton.Hex
Executable:           hex
Rust data root:       ~/Library/Application Support/voice-control
Update feed:          R2
```

The Application Support directory is not part of the macOS application
identity. Keeping the established Rust root avoids a risky filesystem takeover
and preserves every existing Rust installation without migration code.

Existing Rust installations at `/Applications/HEX.app` may retain that
case-only filesystem name because Sparkle replaces the current host path. Fresh,
Swift-origin, and Homebrew installations use `Hex.app`.

Intel Macs and Macs running macOS 14 remain on Swift HEX 0.8.4. The legacy S3
appcast remains available indefinitely as a doorway for dormant eligible Swift
installations, but Rust artifacts and all ongoing releases live on R2.

## Product Decisions

- Adopt the established Swift bundle identity instead of moving Swift users to
  `com.kitlangton.voice-control.agent`.
- Update directly from Swift 0.8.4 build 91 to Rust 2.1.0 build 20100. Do not
  ship a final Swift bridge release.
- Keep the public name HEX. This is a normal major upgrade, not a separate HEX 2
  application.
- Ship a full application archive, never a binary delta across implementations.
- Permanently keep Rust data in `~/Library/Application Support/voice-control`.
- Import only a small preference allowlist when Rust settings do not exist.
- Do not import Swift transcripts, audio, models, caches, microphone IDs,
  launch-at-login state, or text-processing rules.
- Never modify or delete Swift data automatically.
- Existing Rust settings always win because an existing Rust `settings.json`
  prevents the Swift importer from running.
- Product History remains text-only. Diagnostic retained dictation audio remains
  developer-only and is not migration data.
- Use Sparkle's native seven-cohort phased rollout with a 48-hour interval.
  Manual update checks may bypass phasing and receive the update immediately.

## One-Shot Preference Import

`AppSettings::load` owns the import boundary. When
`~/Library/Application Support/voice-control/settings.json` is absent, it checks
the one known eligible Swift source:

```text
~/Library/Containers/com.kitlangton.Hex/Data/Library/Application Support/com.kitlangton.Hex/hex_settings.json
```

The importer starts from current Rust defaults and independently applies valid
values for:

- Dictation shortcut, including left/either/right modifier sides.
- Paste Last shortcut. Swift 0.8.4 omits this optional field when disabled, so
  an absent or null value disables the Rust shortcut.
- Double-tap lock and double-tap-only behavior.
- Recording audio behavior.
- Microphone lifecycle policy by inverting Swift `superFastModeEnabled` into
  Rust `release_microphone_while_idle`.
- Sound enabled and volume, normalized from Swift's `0...0.2` stored gain to
  Rust's `0...1` user-facing scale.
- Dock icon visibility.

Character shortcuts resolve through the active macOS keyboard layout. Stable
special and function keys use their macOS virtual key codes. Unsupported,
ambiguous, empty, or malformed shortcuts preserve the Rust default. Invalid
individual scalar fields also preserve their Rust defaults without blocking
other fields.

A successful import writes the ordinary Rust `settings.json` atomically. Each
writer uses a process-unique temporary publication path, so concurrent first
loaders cannot collide on `settings.json.tmp`. The settings file is the
idempotence marker; there is no migration receipt. Explicit
`HEX_APPLICATION_SUPPORT_DIR` instances skip import so previews, tests, and
embedded services stay isolated and deterministic.

The importer deliberately ignores:

- `openOnLogin`
- `selectedModel` and downloaded model files
- `outputLanguage`
- `selectedMicrophoneID`
- `saveTranscriptionHistory`, history limits, transcripts, and audio
- word removals, word remappings, lowercase conversion, and punctuation removal
- clipboard and legacy sleep settings

Malformed or missing Swift JSON falls back to normal Rust defaults. The source
file remains untouched in every case.

## Storage Contract

Rust continues to own:

```text
~/Library/Application Support/voice-control
```

Swift data remains untouched in its sandbox container and any historical
unsandboxed locations. The two implementations no longer contend for an
Application Support directory, so the migration requires no:

- directory relocation or merge;
- migration lock or receipt state machine;
- crash recovery across filesystem renames;
- displaced Swift staging directory;
- compatibility symlink;
- rollback restoration procedure;
- schema recognition for deciding which product owns a directory.

Manual rollback to Swift 0.8.4 can continue using the untouched Swift data.
Manual rollback to a pre-transition Rust build continues using the unchanged
`voice-control` root.

## Identity Transition

The 2.1.0 application payload changes the current Rust package identity to:

```text
Bundle identifier: com.kitlangton.Hex
Executable:        hex
Bundle name:       Hex.app
```

The crate and internal module names do not need a broad rename. Rename only
packaged and user-facing identities required by the permanent application.

The signed payload must use the established Developer ID team and existing
Sparkle EdDSA key. Validate TCC continuity for Microphone, Accessibility, Input
Monitoring, and Automation with a genuine Swift-origin upgrade. Validate
`SMAppService` registration separately because launch-at-login state belongs to
macOS and is not imported from Swift's Boolean preference.

Swift users still pass through Rust onboarding health checks. Existing
permissions should remain healthy under the established identity; the selected
Rust transcription model may still require installation.

## Release Channels

Release HEX 2.1.0 build 20100 only after the soak gates pass. Build one signed
payload and package the host-name variants required by the two installed
populations:

- The R2 transition archive keeps inner `HEX.app` for existing Rust hosts.
- The Swift transition archive contains inner `Hex.app` and is referenced by
  the legacy S3 appcast.
- Both archives contain the same signed application identity and executable.
- Every later public archive uses `Hex.app` and the R2 feed.

The S3 appcast is a permanent transition doorway, not a second release channel.
Its final eligible item points to an immutable R2-hosted artifact. Publish and
verify the immutable artifact before changing either feed.

Roll out through Sparkle's native seven cohorts. Pause the rollout immediately
if crash, permission, model-install, input, or paste telemetry indicates a
release blocker.

The transition item must:

- be offered only to Swift 0.8.4 build 91;
- require Apple silicon and macOS 15 or newer;
- require explicit major-upgrade confirmation;
- explain that HEX is replacing its engine and will download a local model;
- state that old Swift history and files remain untouched;
- use the exact archive length and EdDSA signature.

Intel and macOS 14 clients must continue to see Swift 0.8.4 as their latest
compatible release.

## Soak Gates

Do not first expose capture-lifecycle changes inside the identity-changing
build. Before 2.1.0 reaches the Swift appcast:

- Low-risk presentation, permission-health, and model-selection slices pass the
  complete release suite and run for at least 48 hours on current Rust installs.
- Side-aware hotkeys, key-based double-tap-only, configurable Paste Last, and
  idle microphone release pass physical event-trace tests and run for at least
  seven days.
- `Release microphone while idle` passes physical tests for idle closure, first
  capture without pre-roll, quick-release discard, post-capture closure, and the
  explicit Commands prerequisite transition.
- The preference importer passes fixture tests for the exact Swift 0.8.4 JSON
  representation and a manual import from a copied real settings file.

The legacy S3 feed sees none of the intermediate 2.0.x Rust releases.

## Release Tooling

Extend release tooling rather than hand-editing the transition. It must:

- build and sign the permanent `com.kitlangton.Hex` payload;
- package and assert the required inner app name for each transition archive;
- calculate each archive's exact length and EdDSA signature;
- generate version- and platform-gated R2 and S3 items;
- support a dry run that downloads both generated feeds and public artifacts
  into a clean directory;
- publish immutable artifacts before feeds;
- refuse publication if bundle identity, executable, signing team, Sparkle key,
  minimum OS, architecture gate, version, or build number is wrong.

## Validation Matrix

Before publishing the Swift transition item, test signed and notarized artifacts
from clean snapshots:

- Swift 0.8.4 build 91 on Apple silicon/macOS 15 with ordinary preferences.
- Swift 0.8.4 with side-specific and key-based shortcuts.
- Swift 0.8.4 with Paste Last disabled and malformed optional fields.
- Existing Rust 2.0.x with populated settings, models, history, meetings, and
  logs; none may move or be replaced by Swift preferences.
- A machine with both Swift data and existing Rust settings; Rust settings win.
- A clean machine with no Swift or Rust data; normal Rust defaults win.
- An explicit `HEX_APPLICATION_SUPPORT_DIR`; import must not run.
- Intel and macOS 14 Swift clients; no incompatible Rust update appears.
- Interrupted download, failed signature, failed model install, and first-launch
  restart; no source data changes and the app remains recoverable.
- Rust-origin Sparkle replacement at an existing `HEX.app` path.
- Swift-origin Sparkle replacement and fresh DMG/Homebrew installation as
  `Hex.app`.

After installation, verify permissions, onboarding, hotkeys, dictation, Paste
Last, idle microphone policy, Dock behavior, updates, launch-at-login state, and
that Swift files remain byte-for-byte untouched.

## Rollback

Before the S3 appcast changes, rollback is simply withholding the transition
item. After publication:

- Stop publishing newer Rust items if a release blocker appears.
- Point eligible users to the last known-good signed Rust artifact or provide
  manual Swift 0.8.4 reinstall instructions when implementation rollback is
  required.
- Do not delete Rust settings during rollback.
- Do not restore, merge, or delete Swift data because the migration never
  changed it.

The unchanged, separate data roots make rollback a release operation rather
than a filesystem recovery operation.
