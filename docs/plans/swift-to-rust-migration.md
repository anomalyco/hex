# Migrate Swift HEX Users To Rust HEX

**Status:** Planned. Rust HEX is the replacement product, but the public Swift
population must not receive it until the identity, preference migration, and
agreed parity work in this plan have shipped and soaked on the existing Rust
channel.

## Outcome

HEX 2.1.0 replaces the Swift application for eligible users without creating a
second permanent macOS identity. The resulting product has one public name,
bundle identity, executable, data root, and update channel:

```text
Published app bundle: Hex.app
Display name:        HEX
Bundle identifier:   com.kitlangton.Hex
Executable:          hex
Application Support: ~/Library/Application Support/com.kitlangton.Hex
Update feed:         R2
```

Existing Rust installations at `/Applications/HEX.app` retain that case-only
filesystem name after the transition because Sparkle replaces the host at its
current path. Fresh, Swift-origin, and Homebrew installations use `Hex.app`.
Once both populations share `com.kitlangton.Hex`, later `Hex.app` archives are
discoverable by bundle identifier from either installed path.

The Rust crate, source modules, and repository do not need a broad internal
rename. Rename only user-facing `voice-control` identities and paths.

Intel Macs and Macs running macOS 14 remain on Swift HEX 0.8.4. The legacy S3
appcast remains available indefinitely as a transition doorway for dormant
Swift installations, but ongoing Rust releases and artifacts live on R2.

## Product Decisions

- Adopt the established Swift identity rather than making the larger public
  population move to `com.kitlangton.voice-control.agent`.
- Update directly from Swift 0.8.4 to Rust 2.1.0. Do not ship a final Swift
  bridge release.
- Present 2.1.0 as a Sparkle major upgrade requiring explicit confirmation.
- Import a small allowlist of useful Swift preferences. Do not import legacy
  transcript history or retained audio.
- Never delete legacy Swift settings, history, audio, caches, or models as part
  of migration. Do not build timed cleanup or legacy-data management UI.
- Existing Rust settings are authoritative when both Rust and Swift data exist.
  Do not merge Swift preferences over an existing Rust `settings.json`.
- Relocate an existing Rust Application Support tree as one unit. This is a
  path takeover, not a field-level settings migration.
- Add audio playback for new Rust history as explicit, bounded retention. This
  is a new opt-in capability, not preserved consent from Swift history, and it
  does not make legacy Swift recordings visible in Rust.
- Complete every parity item in this plan before exposing 2.1.0 through the
  legacy S3 appcast.
- Use Sparkle's native seven-cohort phased rollout with a 48-hour interval.
  Manual checks bypass phasing and may receive the update immediately.

## Current Compatibility Boundary

| Property | Swift 0.8.4 | Rust 2.0.23 | Rust 2.1.0 target |
| --- | --- | --- | --- |
| App bundle | `Hex.app` | `HEX.app` | `Hex.app` |
| Bundle ID | `com.kitlangton.Hex` | `com.kitlangton.voice-control.agent` | `com.kitlangton.Hex` |
| Executable | `Hex` | `voice-control-watch` | `hex` |
| Data root | sandbox container and historical `com.kitlangton.Hex` roots | `voice-control` | `com.kitlangton.Hex` |
| Feed | S3 | R2 | R2 after transition |
| Architecture | universal | arm64 | arm64 |
| Minimum macOS | 14 | 15 | 15 |
| Sandbox | enabled | disabled | disabled |
| Sparkle key | `mIek27lttJe8cIBqVZFhh6reRKjpTx1h9ZY9OKWPtuM=` | same | same |
| Signing team | `QC99C9JE59` | same | same |

The stable Sparkle Ed25519 key authorizes each bundle-identity transition. The
common Developer ID team independently satisfies Gatekeeper and App Management
expectations; it does not by itself make the old designated requirement accept a
new bundle identifier. The stable Swift bundle identifier gives Swift-origin
users the best continuity conditions, but TCC and login-item continuity remain
release test requirements rather than assumptions.

Current Rust users necessarily cross bundle identities in 2.1.0. Their Rust
data remains authoritative, normal onboarding repairs missing TCC grants, and a
one-time banner offers `Enable Launch at Login` because the old Rust login-item
registration cannot be assumed to follow the new identity.

## Take Over Application Support Before Startup

Migration runs in a preflight bootstrap before command dispatch, logger
creation, settings, models, local API discovery, command hosting, or background
workers open files. Its stable lock lives outside every movable tree:

```text
~/Library/Application Support/.com.kitlangton.Hex-migration.lock
```

Before relocating current Rust data, preflight also acquires the shipped
source-root listener, local API, and model-download locks non-blockingly and
rejects a live local API discovery endpoint. Runtime lock and discovery files
are coordination state, not durable user data; stale discovery is removed only
after destination publication while the migration lock remains held.

Move every runtime path behind `app_paths` before takeover. In particular,
replace the current hard-coded meeting root and audit setup, fixture, and API
smoke scripts so no secondary process recreates `voice-control` after migration.

Known source locations are:

```text
Current Rust:
~/Library/Application Support/voice-control

Current sandboxed Swift:
~/Library/Containers/com.kitlangton.Hex/Data/Library/Application Support/com.kitlangton.Hex

Historical unsandboxed Swift:
~/Library/Application Support/com.kitlangton.Hex

Legacy sandboxed Documents:
~/Library/Containers/com.kitlangton.Hex/Data/Documents

Legacy unsandboxed Documents:
~/Documents

External caches that migration neither moves nor deletes:
~/Library/Containers/com.kitlangton.Hex/Data/Library/Application Support/FluidAudio/Models
~/Library/Caches/moonshine_voice
```

### Existing Rust Installation

If `voice-control/settings.json` is structurally a Rust settings document, Rust
data wins. Structural recognition requires the expected filename plus at least
one Rust-specific known key of the expected JSON type; `{}` and unknown-only
objects do not qualify. An unsupported model is repairable Rust state, not
evidence that the root belongs to another product.

1. Preserve any existing historical Swift root without interpreting it as Rust.
2. Relocate the complete `voice-control` tree to the canonical
   `com.kitlangton.Hex` root before starting any file owner.
3. Preserve settings, history, logs, downloaded GGUF models, meetings, and
   durable user data. Recreate runtime discovery and lock state.
4. Write an atomic migration receipt containing source, destination, schema,
   and completion state.
5. Create a temporary owner-controlled
   `voice-control -> com.kitlangton.Hex` compatibility symlink so the actual
   previous Rust build can be used for manual rollback during the transition.
6. Do not import Swift preferences into the relocated Rust settings.

The implementation records a unique non-destructive staging name when the
target directory already contains historical Swift data. It never merges the
two directory trees blindly because both products use names such as `models`
for incompatible artifacts. The receipt records the exact displaced Swift path
and a manual rollback operation that restores it.

### Swift-Only Installation

When no valid Rust `settings.json` exists:

1. Preserve an existing historical unsandboxed Swift root under a distinct
   non-destructive staging name.
2. Create the canonical Rust root and its owner-only settings file.
3. Select the first structurally recognized Swift settings source in this
   precedence order:
   sandbox Application Support, unsandboxed Application Support, sandbox
   Documents, then user Documents.
4. Decode through a dedicated legacy schema, validate every field separately,
   and import only the allowlist below.
5. Record the selected source and a content fingerprint so startup is
   idempotent.
6. Leave every legacy source and staged tree untouched.

Do not choose a source only because it has the newest modification date. A
candidate must be a JSON object containing at least one Swift-specific known key
with the expected type. Decode allowlisted fields independently so one malformed
field does not discard valid siblings.

### Failure And Recovery

- Refuse automatic migration when a source root, target root, settings file, or
  staging parent is a symlink, except for the exact compatibility symlink this
  migration created and recorded.
- Verify source and destination parents are on the same filesystem. Abort
  non-destructively on `EXDEV`; never fall back to recursive copy.
- Create the canonical root with mode `0700`. Create receipts and imported
  settings with mode `0600`, sync file contents, atomically rename, and sync the
  parent directory.
- Persist an owner-only intent receipt outside both roots before mutation. The
  receipt records source and destination identities, unique staging path, and a
  state machine: `planned`, `destination_staged`, `source_published`, and
  `complete`.
- After each namespace rename, sync the Application Support parent directory.
  Recovery uses receipt state plus filesystem identity to continue forward; it
  never guesses from path names alone.
- A failed migration leaves the source tree usable and does not start HEX from
  a partially populated destination.
- Temporary directories and receipts identify whether startup may resume,
  retry, or preserve a failed destination for diagnosis.
- No migration operation rewrites a legacy Swift file.
- A missing or malformed Swift settings file falls back to normal Rust defaults
  and onboarding instead of blocking launch.
- Only one process may migrate or start the listener against these roots.

## Import Only Useful Swift Preferences

| Swift setting | Rust destination | Rule |
| --- | --- | --- |
| `soundEffectsEnabled` | `sound_effects` | Direct Boolean |
| `soundEffectsVolume` | `sound_effect_volume` | Convert the old `0...0.2` scale to `0...1` and clamp |
| `hotkey` | `dictation_hotkey` | Preserve key, modifiers, and left/right/either side |
| `doubleTapLockEnabled` | `double_tap_lock` | Direct Boolean |
| `useDoubleTapOnly` | new double-tap-only setting | Preserve activation semantics |
| `pasteLastTranscriptHotkey` | configurable paste-last shortcut | Import a valid persisted binding; an omitted/null value is indistinguishable from an older default and therefore uses the Rust default |
| `showDockIcon` | `show_dock_icon` | Direct Boolean after the menu-bar item exists |
| `recordingAudioBehavior` | `recording_audio_behavior` | Translate `pauseMedia`, `mute`, and `doNothing` |
| `superFastModeEnabled` | `release_microphone_while_idle` | Invert: disabled Super Fast Mode releases the microphone |
| `selectedMicrophoneID` | `microphone` | Resolve the CoreAudio UID to a current device name; otherwise leave automatic selection |
| `selectedModel` | `transcription.model` | Parakeet v2 to v2, v3 to v3, every Whisper variant to Whisper Large v3 Turbo |
| `outputLanguage` | `transcription.language` | Preserve explicit supported languages and Whisper Auto |
| `saveTranscriptionHistory` | `history_retention` | Off to Off; enabled to Week |
| enabled literal `wordRemappings` | global mode replacements | Preserve literal boundary behavior and replacement escapes |
| `lowercaseTranscripts` | global mode transformation | Add the built-in lowercase transformation when enabled |

Unknown model selections fall back to Parakeet v2. Core ML model files are not
reused; onboarding downloads and verifies the mapped GGUF artifact.

Swift hotkey keys map through their serialized raw key plus the active macOS
layout. Unsupported keys are reported and defaulted, never guessed. Swift
remappings retain case-insensitive phrase boundaries and escape expansion;
detect overlapping rules whose Swift list order would differ from Rust's
longest-match order and report them as unsupported rather than claiming exact
preservation.

Do not import:

- `openOnLogin`; macOS remains the authority for the stable Swift identity;
- old history entries, WAV paths, or history entry limits;
- regex word removals or punctuation stripping; users may recreate these with
  custom transformations;
- `minimumKeyTime`; Rust retains its fixed 300 ms intentional-hold threshold;
- `useClipboardPaste` or `copyToClipboard`; Rust keeps one generation-safe
  clipboard insertion contract;
- `preventSystemSleep`; Rust scopes idle-sleep prevention to intentional
  capture;
- old model files, bootstrap flags, or storage-migration flags.

## Close The Agreed Product Gaps

### Preserve Hotkey Semantics

- Extend macOS hotkey bindings, capture, persistence, matching, labels, and
  tests with independently selected left, right, and either sides for each
  modifier. Either overlaps both specific sides for conflict detection. Fn and
  standalone function keys have no side.
- Apply one shared conflict algebra to Dictation, Voice Action, Paste Last, and
  meeting paste bindings. Side-specific bindings must not bypass protected
  shortcut collisions.
- Add double-tap-only mode without weakening ordinary double-tap lock.
- Make Paste Last Dictation configurable and disableable rather than permanently
  reserving Option-Shift-V.
- Preserve the fixed 300 ms intentional-hold safety threshold.

Double-tap-only applies only to key-plus-modifier bindings and matches Swift's
event contract: the first complete tap records intent without audio, HUD, tone,
or recording-environment effects; a second complete tap inside 300 ms starts a
locked recording; the next matching press finishes it; Escape cancels it. A
timeout or unrelated chord returns to idle without producing a capture.

Paste Last continues to mean the last completed ordinary dictation, survives
History being Off only for the current process, excludes Voice Action, and does
not become durable across restart. When unavailable, both shortcut and status
item produce the existing error feedback rather than a stale history paste.

### Add Automatic Language Detection

**Implemented on main; signed-release soak remains.**

- Represent `Auto` explicitly for transcription runtimes that actually support
  language detection, not as a magic language string.
- Preserve explicit language validation for fixed-language models.
- Migrate Swift Whisper Auto to Rust Whisper Large v3 Turbo Auto.
- Test preparation, persistence, model switching, local API presentation, and
- Switching from Auto to an incompatible model requires an explicit supported
  language before activation. Whisper Auto omits the runtime language hint.

### Restore The macOS Menu-Bar Item

**Implemented on main; signed-release soak remains.**

Provide a persistent native status item with:

- Settings;
- Paste Last Dictation;
- Check for Updates;
- Quit.

It remains available when the Dock icon is hidden. Login launches stay quiet
and discoverable through the status item. If status-item installation fails,
HEX forces the Dock icon visible for that process. Settings always opens the
Settings pane; Paste Last reports unavailable state; Quit follows normal worker
shutdown rather than terminating workers out from under pending work.

### Keep Permission Failures Diagnosable

**Implemented on main; signed-release soak remains.**

After onboarding, show a compact Settings warning only while Microphone, Input
Monitoring, or Accessibility is missing or requires action. Healthy permissions
do not occupy permanent Settings space.

Refresh health while Settings is visible and whenever the application becomes
active. A revoked permission warns and links to the exact system pane; it does
not fabricate listener readiness or silently rewrite settings.

Restore `NSAccessibilityUsageDescription` and
`NSInputMonitoringUsageDescription` in the packaged Rust `Info.plist` alongside
the existing microphone and Apple Events descriptions.

### Release The Microphone While Idle

Add `Release microphone while idle`, effective only when voice commands are
disabled. It removes pre-roll and adds first-capture startup latency.

Commands and idle microphone release use an explicit prerequisite interaction:

- enabling Commands while idle release is active offers
  `Keep Microphone Ready & Enable Commands`;
- enabling idle release while Commands is active offers
  `Disable Commands & Release Microphone`;
- neither action silently changes the other setting.

The settings owner also enforces this invariant below the UI: persisted or
programmatic state may never activate Commands while idle release is enabled.
Opening the microphone, first-capture buffering, cancellation before stream
readiness, device switching, and recovery remain owned by `dictation_audio`, not
coordinated by the Settings UI.

When idle release is enabled:

- `dictation_audio` closes the stream only after active capture is idle; already
  accepted jobs own their audio and do not keep the microphone open;
- a physical shortcut press starts opening the current selected device without
  fabricating pre-roll or moving the source event timestamp;
- the 300 ms intentional-hold decision still uses physical event time, not
  stream-open completion time;
- release before the stream is ready cancels the open and discards without
  inference, paste, feedback side effects, or a fabricated release boundary;
- release after readiness submits only audio actually captured between stream
  readiness and the physical release;
- device-open failure produces bounded error feedback and returns to idle;
- microphone selection while closed applies on the next open, and recovery
  retries only while a capture still owns the stream;
- changing the setting during active or locked capture takes effect after that
  capture finishes or cancels.

### Restore Bounded Audio Playback For New History

Add `Retain audio for playback`, off by default.

- Retain 16 kHz mono WAV only for successful Dictation, Send, and Voice Action
  entries admitted to History.
- History Off prevents audio retention.
- Cancelled, failed, discarded, and non-pasted captures delete disposable audio.
- Audio expires or is deleted with its history entry.
- A 1 GB hard cap deletes the oldest retained audio first without deleting the
  text history entry.
- History rows expose Play/Pause only, and only one entry plays at a time.
- Deleting a playing entry stops playback and deletes its WAV.
- Do not import or play Swift WAVs.

Swift `saveTranscriptionHistory=true` does not imply consent for new Rust audio
retention. The migration banner explicitly says future history is text-only
until the user enables playback retention.

This deliberately supersedes the current shipped invariant that product history
never retains audio. Update `AGENTS.md`, privacy documentation, settings copy,
and release notes only when the feature ships.

## Publish Two Transition Artifacts

Release the completed work as HEX 2.1.0, build 20100. Build one signed app
payload with the target identity, then package transition DMGs appropriate to
each installed population.

### Gate Work Before The Identity Transition

Do not first expose every behavioral change inside the identity-changing build.
Use one or more ordinary 2.0.x R2 releases under the current Rust identity to
soak completed parity slices before 2.1.0:

- low-risk presentation and model-selection slices, including the status item,
  permission health, and Auto language, pass the complete release suite and run
  for at least 48 hours on current Rust installations;
- input and capture-lifecycle slices, including side-aware hotkeys,
  double-tap-only, configurable Paste Last, and idle microphone release, pass
  physical event-trace tests and run for at least seven days;
- retained audio playback ships off by default as an independently disableable
  2.0.x capability and runs for at least seven days with storage, cancellation,
  crash-recovery, and privacy validation.

The legacy S3 feed sees none of these intermediate releases. The 2.1.0 release
gate then contains the already-soaked behavior plus identity, preflight storage
takeover, Swift preference import, migration presentation, and transition
packaging. A failure in an intermediate capability blocks S3 exposure without
requiring the identity migration itself to debug that feature.

Extend release tooling rather than hand-editing this transition. It must:

- package `HEX.app` and `Hex.app` archives from the same signed payload;
- compute an independent length and Ed25519 signature for each archive;
- generate the R2 and S3 transition items with exact version gates;
- assert each archive's inner application name before publication;
- support a dry run that downloads both generated feeds and public artifacts
  into a clean directory;
- publish immutable artifacts first and each appcast last as distinct auditable
  operations.

### Existing Rust R2 Channel

Current Rust hosts look for `HEX.app`. Publish an R2 transition DMG whose inner
directory remains `HEX.app` for this one update even though the incoming bundle
uses `com.kitlangton.Hex` and executable `hex`. The unchanged Ed25519 key
authorizes the identity transition.

Sparkle installs this update back at the current host path, so Rust-origin
installations remain `HEX.app` on disk. This is an accepted case-only legacy
path, not convergence that Sparkle can perform by itself.

The app imports no Swift preferences when valid Rust settings exist. Normal
onboarding repairs TCC permissions under the new identity. The migration banner
offers a one-time Launch at Login action because the previous
`com.kitlangton.voice-control.agent` registration may remain stale.

### Swift S3 Doorway

Legacy clients continue requesting:

```text
https://hex-updates.s3.amazonaws.com/appcast.xml
```

Publish the transition DMG itself to immutable R2 storage. Add an S3 appcast item
whose enclosure points to that R2 artifact and whose inner application is
`Hex.app`.

The transition item must:

- require Swift build 91 so older clients first reach 0.8.4;
- require macOS 15;
- require arm64;
- be a Sparkle major upgrade that cannot install automatically;
- contain complete migration release notes;
- use a full DMG, never a Swift-to-Rust binary delta.

Its gate is explicit and verified through Sparkle 2.9.4:

```xml
<sparkle:version>20100</sparkle:version>
<sparkle:shortVersionString>2.1.0</sparkle:shortVersionString>
<sparkle:minimumUpdateVersion>91</sparkle:minimumUpdateVersion>
<sparkle:minimumAutoupdateVersion>20100</sparkle:minimumAutoupdateVersion>
<sparkle:minimumSystemVersion>15.0</sparkle:minimumSystemVersion>
<sparkle:hardwareRequirements>arm64</sparkle:hardwareRequirements>
<sparkle:phasedRolloutInterval>172800</sparkle:phasedRolloutInterval>
```

Builds 89 and 90 must select Swift build 91 first. Build 91 sees 2.1.0 as a
major upgrade. Manual checks bypass the native seven-cohort rollout.

The installed Rust app embeds the R2 feed URL, so all subsequent checks use R2.
Keep the S3 appcast and all referenced Swift artifacts available indefinitely
for dormant and unsupported clients. S3 does not become a second ongoing Rust
release channel.

### Homebrew

Update `kitlangton-hex` only after both Sparkle transitions validate:

- point to an immutable Rust 2.1.0 artifact;
- keep the installed artifact name `Hex.app`;
- require arm64 and macOS 15;
- preserve auto-update declaration and legacy data during upgrade;
- test `brew upgrade --cask kitlangton-hex` from 0.8.4.

The authoritative cask is
`Homebrew/homebrew-cask/Casks/k/kitlangton-hex.rb`, not the stale `hex.rb` in the
Swift repository. Test both ordinary and `--greedy` upgrades for its
`auto_updates true` policy. Decide explicitly whether a full cask zap removes
the new canonical Application Support root; normal upgrade never does.

## Migration Presentation

Do not add a migration wizard. Import supported state before opening the normal
application, then use existing onboarding only for missing permissions and model
preparation.

Show one dismissible `HEX has been upgraded` banner that:

- links to the complete 2.1.0 release notes;
- explains the new local model download;
- states that old transcript and audio history was not imported and remains on
  disk;
- identifies retired regex-removal and punctuation settings when they were
  present;
- identifies Auto language, hotkey, and replacement settings that were
  preserved;
- offers Launch at Login repair only for installations migrated from the old
  Rust identity.

The migration receipt therefore stores a bounded summary of origin, imported
fields, repaired/defaulted fields, retired settings detected, and banner
dismissal. The banner appears after required onboarding succeeds; its release
notes remain reachable from the normal About/update surface after dismissal.
Launch at Login actions always resolve live `SMAppService` state rather than a
recorded Boolean.

## Release Order

1. Implement identity and Application Support takeover behind deterministic
   migration tests.
2. Implement and test the Swift preference importer.
3. Ship every agreed parity feature in ordinary 2.0.x R2 releases and satisfy
   the slice-specific soak gates above.
4. Physically validate capture, release boundaries, custom chords, lock,
   cancellation, paste, and configurable paste-last behavior.
5. Validate clean-account installation and both genuine Sparkle transitions.
6. Build one signed app, then package and independently Sparkle-sign the
   `HEX.app` Rust-transition archive and `Hex.app` Swift/Homebrew archive.
7. Record the signing certificate identity and compare both mounted app trees
   after normalizing only their top-level directory names.
8. Build and validate the R2 and S3 appcast items, including exact version
   gates, enclosure lengths, signatures, URLs, and inner names.
9. Upload and verify every immutable R2 artifact before changing either feed.
10. Publish the R2 appcast transition for current Rust users.
11. Soak the completed release on current Rust installations.
12. Publish the legacy S3 transition item last with
    `sparkle:phasedRolloutInterval` set to 172800 seconds.
13. Allow Sparkle's seven automatic cohorts to advance every 48 hours if
    diagnostics remain healthy. Manual checks remain immediately available.
14. Update Homebrew after the Sparkle migration is healthy.

Pause new discovery by removing the transition item from the S3 appcast. This
does not recall an update already downloaded or resumed. Never publish an
appcast enclosure before its immutable artifact is publicly fetchable and
verified.

Before exposure, prepare a signed emergency roll-forward build with a build
number greater than 20100 and publication paths for both R2 and S3. The Swift
0.8.4 DMG remains a manual downgrade artifact, not an automatic rollback;
Sparkle will not install build 91 over build 20100.

## Validation Matrix

### Upgrade Sources

- Swift 0.8.2 or 0.8.3 first updates to 0.8.4 build 91.
- Swift 0.8.4 build 91 updates directly to Rust 2.1.0 build 20100.
- Rust 2.0.23 build 20023 updates to the new permanent identity.
- Clean Rust 2.1.0 installation has no migration source.

### Machines

| Hardware and OS | Expected result |
| --- | --- |
| Intel, macOS 14 | Remains on Swift 0.8.4 |
| Intel, macOS 15 or newer | Remains on Swift 0.8.4 |
| Apple silicon, macOS 14 | Remains on Swift 0.8.4 |
| Apple silicon, macOS 15 | Eligible explicit migration |
| Apple silicon, current macOS | Eligible explicit migration |
| Clean Apple-silicon account | Normal 2.1.0 onboarding |

### Data States

- only sandbox Swift settings exist;
- only unsandboxed Swift settings exist;
- only Documents-era Swift settings exist;
- multiple Swift candidates disagree;
- malformed and partially written Swift settings;
- valid current Rust root plus every Swift source;
- valid current Rust settings with models, history, meetings, logs, and API
  state;
- target canonical root already contains historical Swift files;
- symlinked roots and cross-filesystem roots abort without mutation;
- current Rust source locks held by a live process;
- stale and live local API discovery state;
- migration interrupted before and after destination publication;
- migration reruns after a successful receipt;
- actual Rust 2.0.23 rollback through the recorded compatibility symlink;
- actual Swift 0.8.4 manual rollback through the recorded staging path;
- no prior data exists.

### Settings

- left-, right-, and either-side modifier-only shortcuts;
- key-plus-modifier shortcuts and Fn/function keys;
- double-tap lock and double-tap-only;
- custom paste-last shortcuts; omitted/null Swift values use the Rust default;
- every recording audio behavior;
- Super Fast Mode enabled and disabled;
- resolvable and missing microphone UIDs;
- Parakeet v2, Parakeet v3, every Whisper variant, and unknown model values;
- explicit and Auto languages;
- history enabled and disabled;
- literal remapping escapes and lowercase;
- retired regex and punctuation settings reported but not imported.

### System Integration

- record exact pre/post Microphone, Accessibility, and Input Monitoring status;
- every missing TCC grant enters normal onboarding and remains diagnosable;
- record pre/post `SMAppService` status, `sfltool dumpbtm`, registered URL, and
  launched executable, then verify one real login launch;
- exercise the complete sandboxed Swift updater transaction: download,
  authorization, termination, replacement, relaunch, and a second update check
  from the unsandboxed Rust app;
- verify the final Rust app has no sandbox or temporary Sparkle Mach lookup
  entitlements and can read the sandbox settings source on a clean account;
- current Rust migration offers login-item repair without duplicate launches;
- menu-bar controls work with the Dock visible and hidden;
- status-item failure forces a visible Dock icon;
- idle microphone release and Commands enforce their confirmed prerequisite;
- update from `/Applications`, `~/Applications`, and a moved application;
- Homebrew upgrade preserves the final `Hex.app` artifact.
- Rust-origin Sparkle upgrade intentionally preserves its installed `HEX.app`
  path while adopting the permanent bundle identifier.

### Audio Playback

- successful Dictation, Send, and Voice Action retention;
- failed, cancelled, discarded, and non-pasted deletion;
- history Off and each retention window;
- oldest-audio pruning at 1 GB without text-entry deletion;
- one-at-a-time Play/Pause;
- delete while playing;
- crash-safe WAV publication and orphan cleanup;
- no Swift WAV discovery or import.

### Release Security

- exact version, build, architecture, and minimum OS metadata;
- stable Ed25519 public key and valid archive signatures;
- a correctly notarized same-team archive with an invalid Sparkle signature is
  rejected;
- Developer ID signing, notarization, stapling, and Gatekeeper assessment;
- correct inner app filename for each transition artifact;
- corrupted, wrong-name, wrong-ID, and unavailable artifacts are rejected;
- S3 and R2 appcasts are published only after public artifact verification;
- exact Sparkle item selection for hosts 89, 90, 91, 20023, and 20100 in manual
  and background checks;
- full and range GETs of immutable public R2 bytes match enclosure length and
  signature from an external network;
- both downloaded transition app trees match after normalizing only the outer
  application directory name;
- emergency roll-forward publication succeeds on both feeds;
- manual downgrade DMGs remain permanently addressable.

## Completion Criteria

- All target user-facing identity and path references say HEX rather than Voice
  Control.
- Existing Rust data relocates without field loss and always wins over Swift
  preferences.
- Swift-only users retain every allowlisted behavior and receive normal model
  onboarding.
- No legacy history, audio, model, or unknown file is deleted.
- Every agreed parity feature ships before migration exposure.
- A signed 0.8.4-to-2.1.0 update succeeds on a clean eligible account.
- A signed 2.0.23-to-2.1.0 update succeeds and repairs identity-owned system
  state explicitly.
- Intel and macOS 14 clients remain on a working 0.8.4 release.
- Current Rust, migrated Swift, Homebrew, and clean installations all converge
  on one bundle identity, data root, and R2 update channel; existing Rust
  installations may retain the case-only `HEX.app` host path.
