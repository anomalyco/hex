# HEX Feature Map

What can the user do, how do they reach it, and what would prove it works?
This maps existing behavior. [ROADMAP.md](../../ROADMAP.md) owns future work;
[AGENTS.md](../../AGENTS.md) owns implementation invariants.

Initial source baseline: September 1, 2026, `9e9da53aa9ed`. Public macOS release:
[2.3.1](../releases/2.3.1.md), with custom-domain downloads, a GitHub DMG mirror,
and model-download recovery guidance. The initial map was source-only. Listed checks are
locators unless an executed result is explicitly recorded, as in the
[keyboard-layout verification](recovery.md#keyboard-layout-resolution).

## Product Map

```ts
HEX                                      // macOS release
├── Get ready                            // install, permissions, local model
├── Dictate                              // hold OR double-tap lock
├── Use the result                       // paste, Send, paste-last, History
├── Adjust behavior                      // shortcut, microphone, recording preferences
├── Process text                         // context-selected Modes (experiment `modes`, off by default)
├── Opt in                               // Voice Action, Commands, Meetings: separately off
├── Recover                              // prove the next attempt works
└── Maintain the app                     // menu bar, login, updates, quit
```

[Dictation](dictation.md), [Voice Commands](commands.md),
[Voice Action](voice-action.md), [Meetings](meetings.md), and [Recovery](recovery.md)
have detailed maps.
Agents can [read retained transcripts](transcript-cli.md) and
[control the running app](control-cli.md) with the release CLI.
[Experiments](experiments.md) turn unfinished features on per person with a flag;
the [vocabulary](vocabulary.md) is one, and macOS builds with the `capture`
build option add [Captures](capture.md): Scrawl's capture engine hosted in Hex,
with its shortcut, input, and Settings › Experiments › Scrawl pane.
The other capabilities stay here until they need their own verification recipe.

## Get Ready

Install the signed macOS app with `brew install --cask anomalyco/tap/hex` or the
[manual DMG](../../README.md#install-hex). The cask uses a versioned, checksum-pinned
DMG, requires Apple silicon/macOS 15+, and leaves updates to Hex's own updater. It does
not require Bun or OpenCode, launch the app automatically, migrate the legacy
Swift app, or forcibly replace an existing app destination. Homebrew uninstall
leaves runtime settings, models, and retained data intact.

```ts
Download Hex                              // setup.download-recovery
├── anoma.ly/hex names the visitor's platform  // setup.download-button
│   ├── Mac -> DMG | Intel Mac -> disabled, "needs Apple silicon"
│   ├── Windows x64 / ARM64 (UA-CH) -> Hex-Setup-<version>-windows-<arch>.exe + SmartScreen note
│   ├── Linux -> copyable `curl … install-linux.sh | sh` + Arch/glibc requirement
│   ├── Phone -> Send yourself the link
│   └── Other platforms -> every build, Homebrew, the GitHub release
├── Website, README, installers, apps, feeds, appcast -> anoma.ly/hex/files -> R2 bytes (Worker hex-production-site)
├── Releases up to 2.3.1, Homebrew -> downloads.hex.kitlangton.dev -> redirect to the same bytes
├── GitHub v<version> (app-v through 2.3.1) -> identical DMG, Setup.exe, and Linux bytes
└── Older app cannot reach its update feed -> manually install the DMG

Install on Windows                        // setup.windows-installer
├── Setup.exe, per user, no admin -> managed versions\<version>\hex.exe
│   ├── x64 without vulkan-1.dll -> stop with driver guidance
│   ├── another version installed (e.g. self-updated) -> install this one over it
│   └── finish -> Open Hex -> first-run tour downloads a model
└── Settings › Apps -> uninstall every version, shortcut, Launch at login; keep data

Install on Linux                          // setup.linux-installer
└── curl … | sh -> signed feed -> verified artifact -> launcher + hex.service
    └── cannot start -> name the glibc floor or the missing Arch packages

Prepare local models                      // separate network dependency
├── Dictation -> huggingface.co and its download CDN
└── Commands -> download.moonshine.ai
    └── Transfer fails -> host-specific guidance -> restore access -> Retry
```

The website shows the pinned macOS version and platform requirements beside its
download links. App mirrors use explicit `v…` release tags (`app-v…` through 2.3.1) rather than GitHub's
repository-wide latest release, which may refer to the SDK. Builds on the release
service update from `https://anoma.ly/release` (docs/releasing.md); the 2.3.1
feeds, appcast, and the legacy `r2.dev` endpoint stay frozen for installed
clients. Hosting does not guarantee access through every corporate policy.

The site's hero is an animated, optional show: an idle head and an ⌥ keycap until
the visitor presses it, then a voiced demo of messages, OpenCode prompts and fixes
(WebGPU, with a Canvas 2D fallback). Audio loads only after that press; the website
has no microphone button or microphone access. Reduced motion keeps a still until
the press and then plays only the calm stage. See [site authoring](../../site/README.md).
Headless Chromium renders of the WebGPU, Canvas 2D, reduced-motion and phone paths
showed no console errors; a deterministic 60 fps capture was reviewed frame by
frame. These checks do not exercise real speakers or a physical phone.

The Windows installer is silently installed, reinstalled, installed over
another version, and uninstalled on x64 and ARM64 runners by
[test-windows-installer.ps1](../../scripts/test-windows-installer.ps1); no
interactive install, SmartScreen prompt, or first launch on a real PC has been
observed. The Linux library diagnosis has no automated check. The site's
platform states were captured once in headless Chromium with `?platform=`.

Sources: [website](../../site/src/Download.tsx), [installer](../../packaging/windows/hex.iss),
[release publisher](../../.github/release/release-service.sh),
[mirror publisher](../../scripts/publish-app-mirror.sh),
[macOS release publisher](../../scripts/release-app.sh),
[Linux installer](../../scripts/install-linux-release.sh), and
[updater](../../src/updater.rs).
Model transfer failures in [transcription_models.rs](../../src/transcription_models.rs)
and [moonshine.rs](../../src/moonshine.rs) explain the source host, retry, and
separate prerequisites. These messages do not diagnose a particular firewall
or establish a successful subsequent model download.
Executed hosting checks and deployment details are recorded in
[Download Hosting](../downloads.md). In-app feed defaults and recovery copy ship
with 2.1.17; the initial 2.1.16 mirror preserves that release's original bytes.

```ts
Launch Hex, setup incomplete              // setup.tour; full-window, own column
├── Tour opens -> speech model download starts at once      // setup.background-download
│   ├── Model -> the selection recommended for the system's first preferred
│   │            language (English -> Parakeet Unified English; e.g. French ->
│   │            Parakeet v3, Japanese -> Qwen3-ASR); no picker in the tour
│   ├── Progress sits beside the step dots on every step; failure -> Retry
│   └── Change or delete the model later in Settings      // setup.delete-model
├── macOS: opens centred at one fixed size, so tiling managers float it
├── Quit part-way -> next launch resumes at the saved step  // setup.resume
├── Welcome -> Get started                // setup.welcome-hero
│   ├── The Hex glyph backlit by a cool corona, drawn live on the GPU
│   │   (`rune_ui/hero.rs`); core, middle ring, outer ring light in turn,
│   │   then the lamp breathes. Light theme: ink rings over a pale blue glow
│   └── Reduce Motion -> the settled frame, lamp at rest
├── Setup: one screen, every permission   // setup.checklist; skipped when granted
│   ├── Each row leads with its glyph: mic, keyboard, text cursor, sparkle (model)
│   ├── Every row -> Allow; asked -> pulse + Allow; granted -> Allowed  // setup.permission-control
│   │   └── Grant: the pill narrows into the green label, the dot shrinks into
│   │       the check as it draws on (0.3 s, no bounce; Reduce Motion: no travel)
│   ├── Microphone -> macOS prompt; refused -> Privacy pane + "switch Hex on" guide
│   ├── Input Monitoring / Accessibility -> Privacy pane + drag guide
│   │   ├── The app tile lifts out of the row and flies, blurred, into the guide
│   │   └── macOS 27 names the list "Device Control and Data Access"; row and guide follow
│   ├── Rows re-read every 400 ms and flip to a check as macOS grants
│   ├── On in System Settings, unusable until relaunch -> "Turns on when Hex restarts"
│   │   └── Every permission on -> primary "Restart Hex" -> relaunch at setup  // setup.restart
│   ├── The first unfinished row carries the screen's one accent
│   └── Last grant -> 0.7 s beat -> Give it a try (Continue also works)
├── Give it a try                         // setup.practice; macOS only
│   ├── Model still downloading -> "Hex starts listening once…" + footer progress
│   ├── Focused practice field; hold the shortcut, speak, release
│   ├── Started -> "Listening…" ; Transcribing -> "Transcribing…"
│   ├── Discarded before transcribing -> quick-tap hint
│   ├── Discarded after transcribing -> nothing-heard hint (mic, distance)
│   ├── Microphone line under the hint: input in use + live level while recording
│   │   ├── Click -> General's microphone menu; a choice saves the same setting
│   │   ├── 1.5 s of silence while recording -> "No sound. Pick another mic?"
│   │   └── Lid closed, no other input -> "The lid is closed, so the built-in mic is off."
│   ├── Pasted -> words land in the field + success -> Continue
│   └── Skip -> Done                      // practice is never required
├── Listener cannot start after setup     // setup.relaunch; event tap refused
│   ├── First time -> Hex records a marker, reopens itself, resumes at Give it a try
│   └── Within 2 min of that relaunch -> "Reopen Hex" button; dev binaries ask to reopen
└── Done -> paste last, Voice Action, Modes tips -> Start using Hex
    // Windows and Linux: Welcome -> Setup (the model row only) -> Done

General > Speech model or Language
  -> Prepare candidate
     ├── Success -> Save selection
     └── Failure -> Preserve previous selection and show error

Permissions revoked later
  -> General > Permissions needed: the same rows and Allow control as the tour
     └── On, waiting for a relaunch -> "Restart Hex"
```

Apple silicon, macOS 15+. Choices come from the compiled model/language catalog;
Apple Speech is currently excluded from the desktop picker.

```ts
General > Dictation > Speech model, then Language below it   // setup.language-select; every desktop root
├── Speech model -> Picker lists every available model; never changes the language
│   ├── Saved language's recommendations first with their tags, then the
│   │   models that support it, then those that do not
│   ├── Model without the saved language -> Card: "Doesn't support Japanese;
│   │   switches the language to English"
│   └── Choose it -> ChooseModel(model, model's default: English, else its first)
│       └── Saved -> Language row: "<Model> doesn't support Japanese, so the
│           language changed to English" until the next choice
├── Language -> Select of the saved model's languages (catalog order, Auto first
│   when it detects); one language -> the field shows it, disabled
│   └── Choose -> ChooseModel(same model, language): the same preparation as a
│       model choice, so per-language assets (Apple Speech locales) install
└── The saved TranscriptionSelection is always a pair the model supports
```

Sources: [shared picker](../../src/rune_ui/picker.rs) (`choose`,
`choose_language`, `language_row`) and `picker_models`, `language_choices`,
`language_for` in [transcription_models.rs](../../src/transcription_models.rs).
Checks: `the_picker_lists_every_model_with_the_language_s_recommendations_first`,
`a_model_switch_keeps_a_supported_language_or_falls_back_to_the_model_s_default`,
`each_model_offers_exactly_the_languages_it_supports`, and the Rune tests
`a_model_without_the_saved_language_falls_back_to_its_own_visibly`,
`browsing_the_picker_never_changes_the_language`,
`changing_only_the_language_prepares_the_same_model_again`, and
`settings_opens_the_picker_and_names_the_model_and_language_apart`. Scenes:
`picker-japanese`, `language-fallback`, `language-menu`. Fixture renders and
recorded requests only; a live model switch with a fallback has not been
observed. Apple Speech stays excluded, so its per-locale install through the
Language row is unexercised.

```ts
Choose a supported language              // setup.language-conditioning; macOS + Linux picker
├── Parakeet v3 -> Detect language from audio; selected language does not constrain output
└── Whisper -> Use selected language to guide decoding; accuracy is not guaranteed
```

```ts
Settings picker or setup > model card     // setup.model-card; shared by every desktop root
├── Title row -> Name, recommendation tag, "Streams" tag    // at most two tags
│   ├── Recommendation -> For the saved language from the catalog (English: Unified English first)
│   └── Streams -> Catalog `streams` (pinned artifact advertises streaming)
├── Detail line -> "Transcribes while you speak" (non-macOS: "… on a GPU"),
│                  coverage, benchmark, timestamps, Installed
├── Saved language unsupported -> Warning line naming the language it switches to
├── Trailing edge -> Active as dot + label; Cancel while preparing; else Use/Download
├── Preparing -> Progress bar with its "Downloading 37%" / "Loading model" label
└── Speed · Size · Accuracy -> 100% minus the benchmark word error rate
```

Only Parakeet Unified English is marked `streams`
(`only_the_buffered_parakeet_artifact_is_marked_streaming`). The mark describes
the artifact; dictation still streams only after the load probe succeeds on a GPU
device ([Streaming Finish](dictation.md#streaming-finish)). The menu bar lists
names only and shows no tags. Scenes: `hex app -- --snap OUT --scene picker`
(also `picker-downloading`, `picker-japanese`).

Parakeet v3 remains available for Portuguese and its other existing language
choices. Its card explains the limitation and suggests Whisper for
language-guided transcription. This does not change saved selections or expand
the Auto picker choices. Multilingual audio support is not language conditioning:
the pinned v3 GGUF has no `stt.parakeet.prompt.num_prompts` metadata, which gates
language prompting in `transcribe-cpp-sys` 0.1.3.

Sources: [model catalog](../../src/transcription_models.rs),
[shared picker](../../src/rune_ui/picker.rs), and the runtime
options in [parakeet.rs](../../src/parakeet.rs) and
[local_transcriber.rs](../../src/local_transcriber.rs). The regression
`parakeet_v3_supports_portuguese_without_language_conditioning` checks that the
Portuguese selection remains valid and available, sends no runtime language
hint, and contrasts Whisper's `pt` hint. It does not run inference.
The regression was observed failing with the old metadata and passing after the
correction. The picker's layout is checked by Rune goldens (`rune-picker-*`), not on a
Mac screen.

**Unreproduced recognition report:** [#68](https://github.com/anomalyco/hex/issues/68)
reports Portuguese speech misrecognized as English. Source and pinned-model
metadata inspection establish the prompting limitation, not the cause or repair
of those transcripts. No v3 audio reproduction or transcription-quality fix has
been demonstrated.

Checks start in [onboarding.rs](../../src/onboarding.rs),
[transcription.rs](../../src/transcription.rs), and
[transcription_models.rs](../../src/transcription_models.rs). Setup/picker previews
prove rendering, not clean-account permission grants or model preparation.

The tour is the pure `onboarding::Flow` (steps, row states, background
download, practice hints, one-relaunch guard) drawn by
[rune_ui/onboarding.rs](../../src/rune_ui/onboarding.rs); `permission_control`
in [rune_ui/lists.rs](../../src/rune_ui/lists.rs) is the one Allow / dot /
Allowed control Settings and the tour share (`a_grant_gathers_then_releases_into_allowed`,
`reduce_motion_swaps_allow_for_allowed_at_once`; scene `meetings-allow`). The
welcome hero is a compositor pass after the UI, so goldens show its stage
empty; `hex app -- --scene onboarding-hero` (`-still` for Reduce Motion) runs
it live, and `--video` records it. Its springs passed motion-lint as level
channels; native visual review is still by eye. The guide
beside System Settings is [permission_guide.m](../../native/permission_guide.m).
Unit tests (`onboarding::tests`) cover step order, the download starting on the
welcome screen and retrying from any step, permissions-only setup with the
model waiting for practice, the auto-advance beat, live row flips, a refused
microphone, platforms without permissions or practice, practice hints, resume,
the relaunch guard and resume marker, grants waiting for a restart
(`a_grant_the_process_cannot_use_yet_waits_for_a_restart`,
`setup_offers_a_restart_once_every_permission_is_on`), and the saved step
(`the_tour_step_is_saved_for_the_next_launch`). Rune tests drive the tour
(`the_mac_tour_asks_each_permission_then_practices`,
`a_listener_that_will_not_start_relaunches_once_then_asks_to_reopen`,
`a_refused_microphone_points_to_system_settings`,
`a_grant_waiting_for_a_restart_makes_setup_restart_hex`,
`onboarding_downloads_the_model_through_the_service`), and every macOS step
is a scene (`mac-onboarding`, `-setup`, `-progress`, `-denied`, `-restart`,
`-download-failed`, `-try`, `-listening`, `-landed`, `-reopen`, `-done`).
Reaching the last step in a live tour plays the onboarding success sound
(`remember_step`; fixtures stay silent), checked by reading the code.
The default model comes from `desktop_i18n::preferred_language` (macOS:
`NSLocale.preferredLanguages`) through `recommended_selection`; existing
settings keep their saved model.
Not yet observed: a clean-account run with real grants, a real practice paste
into the field, or a real self-relaunch after an event-tap refusal. Windows and
Linux show the model row only and skip the practice step.

```ts
Menu bar > Transcription Model            // setup.quick-model-switch; macOS
├── Downloaded models -> Last selected language shown beside each model
├── Choose -> Verify installed artifact -> Prewarm -> Save selection
│   ├── Success -> Checkmark moves; Settings reflects the saved selection
│   └── Failure -> Previous selection remains; open Settings for the error
├── Preparing -> Cancel Model Switch      // never cancels dictation/output jobs
└── Manage Models… -> Settings picker      // downloads; languages in the Language row
```

The menu does not open Settings merely to switch models, download a missing model,
or replace a corrupt artifact. Each model retains one last-used language/hints
selection in `transcription_recents`; the current selection takes precedence.
Older settings start with no remembered alternatives. Select an alternative's
language in Settings' Language row once to establish it. No additional model is loaded at startup
just to populate the menu, and the checkmark means selected, not live-worker readiness.

[transcription_preparation.rs](../../src/transcription_preparation.rs) is shared
by Settings and the menu bar: one worker plus one replaceable pending choice.
Closing Settings does not abandon preparation. A superseded/cancelled completion
cannot save, and delivered menu controls are drained before accepting completions.
The desktop root commits through an existing editor to retain unrelated unsaved
settings, or through persisted settings when no window exists.

Sources: [status_item.rs](../../src/status_item.rs) and
[macos_rune.rs](../../src/macos_rune.rs). Key checks are
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

```ts
Settings picker > Downloaded model card   // setup.delete-model; macOS
├── Delete -> "Delete <size>?" -> Delete again
│   ├── Success -> Model file, receipt, and partial download removed; card offers Download
│   └── Download in progress -> Nothing removed; "Model could not be deleted." with reason
├── Selected dictation model (any language) -> No Delete
├── Preparing or Apple Speech model -> No Delete
└── Model choice or closing the picker -> Confirmation cleared
```

Deletion takes the shared model-directory download lock without waiting, so it
cannot race a Settings, menu-bar, or local API download. The menu bar drops a
deleted model because it reads installation state when it builds the menu. Linux
Settings does not offer deletion: the service owns model files and has no delete
request yet.

Sources: [shared picker](../../src/rune_ui/picker.rs) and `remove_installed`
in [transcription_models.rs](../../src/transcription_models.rs). The check is
`removing_a_model_deletes_its_artifacts_unless_a_download_is_active` (real files
and lock); no Rune test covers the picker's two-step confirmation yet.

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

General > Advanced > Double-tap only     // optional for key bindings
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
Send is not an atomic target-app transaction. Chromium 155 and 156 get the text
typed instead of pasted for a [Chrome paste bug](dictation.md#ongoing-jobs-and-output).

**Easy to misread:** History Off does not stop diagnostic logging. Diagnostic
logs are separate and currently unbounded on disk. See the
[privacy guide](../../README.md#privacy-and-local-data).

The shared `EventReader` streams a file-length snapshot in 16 KiB chunks rather
than copying the entire log into memory. It retains up to 1,024 event rows and
the latest context, even when that context predates the retained rows.
Diagnostic records over 1 MiB are skipped through their terminating newline;
the original log and dictation output remain intact. Partial UTF-8 records carry
over to the next refresh, and truncation or file replacement resets the reader.

Checks: the `reader_*` regressions in [events.rs](../../src/events.rs). These
exercise synthetic files, not an installed app. The GPUI window's
on-demand-loading regression went with GPUI; when the Rune window reads the
log is not covered by a test. A first Commands/Activity refresh still scans the
full log snapshot; disk rotation and background parsing remain separate work.

Observed September 26, 2026: the reader and pane-loading regressions passed.
An optimized isolated harness using the production reader and a synthetic
64 MiB log reduced peak process RSS from about 204 MB to 2.4 MB; refresh took
254 ms before and 222 ms after. This is reader evidence, not a native Settings
latency measurement.

## Adjust Behavior

```ts
General                                  // settings
├── Dictation card -> Shortcut, Shortcut mode, Double-tap to lock, Speech model, Language
│   ├── Shortcut mode > How it works -> Hold/Toggle explainer modal (dictation.md)
│   └── A row that hides or shows (Double-tap to lock, Advanced, a microphone error)
│       -> Folds under a clip as the rows below slide over it; no fade in place
├── Microphone -> Field always names the input in use            // settings.microphone
│   ├── "System default" + faint "KIT PODS", or a saved device
│   ├── Re-listed on a device or default change while General shows (Windows: also every 3 s)
│   ├── System default -> The OS default input, followed live      // a Bluetooth default is kept
│   │   ├── Default changes -> Switch once capture is idle; new stream opens before the old drops
│   │   └── Default is Hex Microphone -> Built-in mic, else first real input; row says so
│   ├── Saved device missing -> System default stands in, field names it
│   │   ├── Row says "SHURE MV7 isn't connected; using your Mac's input"
│   │   ├── Menu keeps "SHURE MV7" checked with a faint "not connected"
│   │   └── Device returns -> Switch back once idle; the setting never changes
│   ├── Menu lists real inputs only                                // never Hex Microphone
│   └── Open menu covers the controls beneath it; click elsewhere dismisses
├── Advanced ▸ (Dictation header) -> rows at the end of the Dictation card
│   ├── Open while Release when idle or Double-tap only (Hold) is on
│   └── Collapse -> drops a pending release confirmation
├── Microphone mode (Advanced)
│   ├── Keep ready (fast) -> Open while idle; pre-roll available
│   └── Release when idle -> Open on press; no pre-roll; startup delay
│       └── Commands enabled? -> Confirm turning Commands off
├── Output card -> Paste last shortcut, While dictating, Dictation HUD
├── While dictating -> Mute / Pause media / Do nothing
│   ├── Intentional capture only, not ordinary shortcut chords
│   └── Pause/resume only players already reported running by macOS
├── Application card -> Sound volume: immediate feedback; zero suppresses tones
└── Voice Action shortcut -> Voice Action pane; Meetings -> Meetings › Settings

Intentional recording                    // recording.environment
├── Acquire native macOS no-idle-sleep assertion
│   └── Failure -> Warn and continue recording
└── Finish/cancel -> Release the assertion
    // no caffeinate child process or executable-policy dialog
```

Successful settings saves persist and project changes at safe runtime boundaries.
Immediate settings controls commit a candidate before changing the saved UI
choice or applying dependent native/history effects. A failed shortcut save
stays in capture instead of showing Saved; failed retention saves keep the old
choice and surface the error. Failed debounced text saves retain their drafts
for a subsequent edit or close-time flush; no Rune test injects a save failure
yet.
Persistence, conflict, and ownership checks live in
[app_settings.rs](../../src/app_settings.rs),
[recording_environment.rs](../../src/recording_environment.rs), and
[audio.rs](../../src/audio.rs). Settings previews do not prove physical device
switching or native mute support; muting is best-effort, not universal. The
recording-environment ownership checks cover overlapping acquisition and release,
not a managed-device policy or native power assertion. The pause-script regression
checks that inactive players are absent before AppleScript application resolution;
when no supported player is running, acquisition skips AppleScript entirely.
The script check does not prove subprocess avoidance or exercise Music, Spotify,
or VLC through Automation. An opt-in native
assertion smoke exercises IOKit without starting microphone capture.

**Changed after 2.3.1:** Automatic was a compiled preference list (Universal
Audio Thunderbolt, Studio Display Microphone), then the macOS default with a
Bluetooth default swapped for the built-in microphone (the 2.1.22 fix for
[#106](https://github.com/anomalyco/hex/issues/106) and
[#88](https://github.com/anomalyco/hex/issues/88)). That skipped a user's
AirPods default, and Hex Microphone then passed the wrong microphone on to
calls. Automatic is now System default: exactly the OS default input, followed
live, so a Bluetooth default does open the headset (and its call profile); pick
the built-in microphone to avoid that. Checks in [audio.rs](../../src/audio.rs):
`inputs_resolve_to_the_selection_then_the_system_default`,
`device_override_wins_and_never_falls_back`,
`a_default_change_switches_only_while_capture_is_idle`, and
`a_missing_selection_uses_the_default_until_it_returns`, with injected device
lists and opens. Not verified: the CoreAudio listeners against a real default
change, or Windows' default-change report on a real PC.

**Fixed after 2.1.20:** through 2.1.20 the microphone menu was painted without
occluding the panel, so hovering or clicking a device also reached the setting
under the pointer ([#94](https://github.com/anomalyco/hex/issues/94)), and it
only closed by choosing or reopening. The menu is now anchored to its button,
occludes the controls beneath it, and closes on any outside mouse-down. In the
Rune window the menu is a rune-kit `Menu` in the overlay layer (scene `menus`);
the GPUI occlusion regression went with GPUI and has no Rune counterpart.

**Easy to misread:** an open microphone is not an active recording. Sleeping
Commands still needs open input; it is not Release when idle.

## Process Text With Modes

```ts
Experiment `modes` off (the default)     // experiments.md; modes.experiment
├── Sidebar -> No Modes pane; onboarding -> No Modes tip
├── `window show --pane modes` -> Stays on the current pane
├── Processing -> Global replacements only    // literal, cheap, still applied
│   └── No per-app mode, corrections, OpenCode rewrite, or transformations
├── Transformations host -> Not started for modes; a hex.config.ts workspace still starts it
├── Voice Action and the OpenCode catalog -> Unchanged // the catalog is Voice Action's too
├── History -> No processing profile, since nothing processed
└── Saved modes, `hex settings dictation_processing.*` -> Untouched and working
    // `hex experiments enable modes` + restart -> everything below, as saved

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

The built-ins are Lowercase, SpongeBob case, and No trailing punctuation. The
last removes only the final contiguous run of sentence marks (including
full-width marks), preserving internal punctuation, trailing whitespace, and
closing quotes/brackets. Punctuation inside a final closing quote is preserved.
Select it after Lowercase for lowercase messages without the final period.
The `no_trailing_punctuation_*` and `built_in_transformations_chain_in_selected_order`
tests in [personal_commands.rs](../../src/personal_commands.rs) cover text behavior
and native dispatch, not a live custom-host or target-app paste.

Selection and stage checks start in
[dictation_processor.rs](../../src/dictation_processor.rs),
[text_replacements.rs](../../src/text_replacements.rs), and
[personal_commands.rs](../../src/personal_commands.rs). They do not prove live
provider availability or real application/Brave context changes.

OpenCode transformation shares Voice Action's OpenCode discovery, which accepts
`opencode2` or a V2 `opencode` and rejects V1
([Voice Action](voice-action.md)). While the switch can't be turned on, the
reason ("Requires OpenCode", "Requires OpenCode V2") shows beside it and the
setup/retry notice below; `unavailable_opencode_is_explained_beside_each_switch`
covers both panes headlessly.

GUI executable discovery also checks `~/.vite-plus/bin/opencode2` when it is
absent from the inherited PATH. `standard_install_location_finds_opencode_outside_the_gui_path`
in [dictation_processor.rs](../../src/dictation_processor.rs) covers this with
a temporary executable; it does not run an installed Vite Plus shim or provider.

`ordinary_processing_snapshots_modes_before_stage_and_keeps_transformations` in
[parakeet.rs](../../src/parakeet.rs) checks processing-time profile acquisition,
conditional Processing-stage emission, and retained transformation selection.
It uses controlled profiles, not a live provider or custom transformation host.

`modes_add_select_and_delete`, `mode_corrections_and_names_save_from_their_fields`,
and `mode_transformations_add_reorder_and_remove` in
[rune_ui/tests.rs](../../src/rune_ui/tests.rs) cover the Rune mode editor and
its debounced drafts headlessly. They do not exercise native typing or
foreground mode activation.

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

## Record Meetings

[Meetings (beta)](meetings.md) maps the opt-in (the Meetings pane's card, then
Meetings › Settings), call detection and the
focus-free offer, menu-bar recording state, rolling transcription, echo
suppression, and the Markdown copy in the user's transcript folder.

## Experimental: Hex Microphone And Voice Filter

```ts
voice-filter experiment, HEX_EXPERIMENTAL_MIC=1, experimental.hex_microphone = true,
or the driver installed                  // fun.hex-microphone; macOS
├── Sidebar > Experiments > Voice Filter  // the only home of these controls
├── Hex Microphone card
│   ├── Driver status -> Installed v<ver> / In use by <app> + Uninstall
│   │   ├── Version mismatch -> v<old> installed + Update + Uninstall
│   │   ├── Pre-Anomaly com.kitlangton driver at the same path -> Update replaces it in place
│   │   ├── Not installed -> Install -> macOS admin prompt -> copies bundled HexMicrophone.driver + restarts coreaudiod
│   │   └── Inline state -> Installing… / Uninstalling… / Cancelled / Failed with reason
│   ├── Mode while dictating -> Muffled (default), Muted, Quiet, Normal, Chipmunk, Robot, Walkie, Underwater, Far
│   ├── Key sliders -> Live mode tuning + Effect strength + Reset
│   └── Try it -> Example clip OR Record 5s (in-memory only)
│       ├── Dual waveform -> You vs Call hears with highlighted dictation span
│       └── Explicit playback -> Play what call hears | Play your voice
├── Live microphone capture -> Raw audio to Hex transcription (unmodified)
│   ├── Source -> Hex's dictation microphone (System default follows the OS default, never Hex Microphone itself), never a meeting's second stream
│   ├── Lock-free ring -> 48 kHz virtual sink -> Raised-cosine fade on intentional hold
│   │   └── Not dictating -> bit-exact pass-through; non-48 kHz inputs resample continuously
│   └── Ring underflows/overflows -> process.log each minute while an app reads it, and on sink stop
└── Offline CLI -> hex dev voice-filter render <in.wav> --mode <m> --dictation <start>-<end> -o <out.wav>
```

Sources: [voice_filter.rs](../../src/voice_filter.rs),
[hex_microphone.rs](../../src/hex_microphone.rs),
[HexMicrophoneDriver.c](../../macos/HexMicrophoneDriver/HexMicrophoneDriver.c),
[build-app.sh](../../scripts/build-app.sh),
and [rune_ui/fun.rs](../../src/rune_ui/fun.rs). Checks:
`default_mode_is_muffled`,
`all_nine_modes_render_finite_and_expected_levels`,
`raised_cosine_fade_has_no_step_discontinuity`,
`ring_buffer_transfers_samples_and_handles_underflow_and_overflow`,
`a_meeting_stream_on_the_same_microphone_does_not_double_the_feed`,
`a_callback_larger_than_the_latency_cap_passes_through_whole`,
`resampling_is_continuous_across_callbacks`,
`only_the_listeners_current_input_feeds_hex_microphone`,
`driver_plugin_loopback_streams_at_48khz_under_20ms_latency`,
`driver_plist_version_and_status_mismatch_are_detected`,
`osascript_failure_classifies_user_cancel_and_errors`,
`audio_service_restart_interrupts_stream_and_recovers`,
`voice_filter_saves_the_mode_and_applies_the_tuning_live`,
`try_it_records_and_plays_through_the_host`, and
`voice_filter_needs_its_experiment_or_hex_microphone`.
Scenes: `mac-voice-filter`, `mac-voice-filter-muted`,
`mac-voice-filter-driver-missing`.
Proof limit: in-process `dlopen` loopback proves the compiled driver's 48 kHz
`WriteMix`/`ReadInput` path at 5.33 ms (256 frames), not an interactive macOS
admin authorization prompt or an active Slack/Zoom call. The pass-through
checks simulate callback timing and sizes; clock drift between the microphone
and the virtual device is not corrected, so a skip or zero-fill click every
minute or two remains possible.

## Recover And Maintain

```ts
Cancellation / interruption / failure    // recovery
  -> Scoped stop, fallback, or error
  -> Retry or fresh gesture
  -> Next capture succeeds               // this is the proof, not a cleared error

Menu bar and Settings                    // maintenance
├── Icon -> 18 pt monochrome Hex mark template (recording variant + elapsed time)
├── Settings / Paste Last -> Reachable with Dock icon hidden
├── Launch at Login -> macOS owns registration state
├── Check for Updates -> The updater (maintain.updates)
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

Show Dock icon off                       // maintain.dock-policy
├── Settings window open -> Dock icon stays // hiding it would deactivate HEX
└── Settings window closes -> Menu-bar only
```

```ts
General > Launch at login                // maintain.login-item
├── Open -> Checking until macOS returns the initial state
├── Refresh / toggle / Open settings -> One persistent background worker
│   └── One active request; latest pending user action precedes the next refresh
├── Toggle -> Switch moves at once; macOS's answer settles it
│   ├── Answers behind a later click -> Switch keeps the last click
│   └── Settled answer differs -> Switch springs back
├── Failed toggle -> Show error and reconcile with actual macOS state when available
└── Close Settings -> Release channels without waiting for an active native call
```

[login_item.rs](../../src/login_item.rs) serializes ServiceManagement calls on one
window-owned worker, with bounded request and response channels and an autorelease
pool per operation. Settings reads responses without blocking on its 250 ms tick;
refreshes do not accumulate while macOS is busy. The toggle is unavailable until
the first successful status lookup. A click is held in memory
(`PlatformState::login_item_requested`) only until the worker is idle again, so
the switch need not wait for `SMAppService` (about 120 ms per toggle, measured
October 24, 2026, plus up to one tick). macOS owns registration state; no login
Boolean is persisted. Scenes never start the worker or call macOS.

The worker tests in `login_item.rs` exercise thread reuse, nonblocking polling,
latest-action coalescing, action priority, and closing during a blocked call.
`launch_at_login_asks_the_login_item_and_needs_approval_first` in
[rune_ui/tests/panes.rs](../../src/rune_ui/tests/panes.rs) checks the Rune row's
requests and the approval state, and the `login_item_*` tests there check the
switch while macOS answers: confirmed, refused, a status answer behind a click,
and rapid clicks. The GPUI regression for error rollback and
external enablement went with GPUI. Native `RequiresApproval` and error transitions remain unverified;
the simulated checks do not establish Developer ID signed distribution behavior.

Observed September 23, 2026, on `d1930b5` after the final simplifications: the
full debug suite passed 486 tests and the release suite passed 485, each with
twelve opt-in tests ignored and all twelve keyboard-layout child scenarios
passing. Formatting, strict debug all-target/all-feature Clippy, and diff
whitespace checks passed.

Computer-use checks on an ad-hoc signed temporary bundle of the optimized build
confirmed shortcut capture/cancellation, Settings/Modes navigation, registration
and unregistration, the final state after three consecutive toggle clicks, and
refresh after removing that bundle's login item in macOS Settings. The temporary
item was left disabled/removed; no new microphone, input, or Accessibility grants
were made. A five-second `sample` profile found `SMAppService` frames on the
`login-item` worker and none on the main thread. This is sampled-stack evidence,
not an input-to-frame latency measurement or a signed-release smoke test.

Closing Settings left the app process alive with zero `login-item` threads;
reopening produced one new worker. An earlier apparent surviving worker came
from computer-use observation reopening the window. Cmd+Q and Quit HEX each
terminated the process. Local profiles are `/tmp/hex-login-worker-current.sample.txt`
and `/tmp/hex-login-cycle-{before,closed-no-observe,reobserved}.sample.txt`.
The tested temporary executable's SHA-256 is
`d1a3be1e219e7371726273937eeef6c7e16434637e4297ae183f9bdaf4d28b04`.
No Developer ID signing identity was available for a new distribution build.

The existing **Show Dock icon** preference controls quiet startup; there is no
additional launch-window setting. This applies to normal and login launches,
does not change login registration, and does not hide an already open window
when the preference changes. Any open Settings window keeps the Dock icon: macOS
deactivates an app asynchronously when it becomes menu-bar only, which dropped
the window behind other applications ([#103](https://github.com/anomalyco/hex/issues/103)).
Turning the preference off therefore takes effect when Settings closes.

Checks in [meeting_watcher.rs](../../src/meeting_watcher.rs):
`dockless_startup_stays_quiet_only_when_setup_and_menu_bar_are_ready` and
`dock_visible_startup_always_opens_the_app` cover the startup decision. The
Dock/Finder reopen Apple event and the status item's `OpenSettings` bypass it.
These checks do not establish signed-app login behavior or native Finder/Spotlight
reopening; those still need an installed-app smoke test. This addresses the
menu-bar-only case in [#64](https://github.com/anomalyco/hex/issues/64), not an
independent window preference for users keeping the Dock icon visible.

**Executed September 3, 2026:** 441 Rust tests passed (nine opt-in tests ignored),
along with all twelve keyboard-layout child scenarios, formatting, and Clippy.
The release Settings preview built and launched, but window screenshot capture
failed; no visual or installed-app startup verification is claimed.

```ts
Hex checks the release service            // maintain.updates
├── 10 s after launch, hourly (Dev channel: every 10 min and on window focus), and Check for updates -> anoma.ly/release/hex/desktop/<channel>.json
│   └── <channel> -> updates.channel (General › Application › Update channel: Stable | Dev,
│       or `hex settings set updates.channel`), else the channel the build was published on
│       (HEX_BUILD_CHANNEL; older builds: dev for a dev version, else the receipt's);
│       package managers always stable
│       └── Choosing a channel -> checks now; a staged update stays staged
├── Versions never compared: channel's current release != running build -> take it
│   ├── Running build -> Linux/Windows: executable SHA-256 = target's; macOS: same version
│   ├── Another channel than the build's -> sidebar, tray, menu bar: Switch to Stable 2.3.1
│   │   └── Only after the user confirms -> download -> stage (never switches by itself)
│   ├── required -> install without waiting for the rollout
│   └── Rollout -> device-local bucket in <support>/update-bucket, never sent
├── Direct install -> download (resumable) -> size + SHA-256 -> code signature -> stage
│   ├── macOS -> unpack beside Hex.app -> Developer ID 5NZ4Q7NXJ4, anomaly.hex, notarized
│   ├── Windows -> Authenticode valid, signer AUTHENTICODE_SIGNER -> new version directory
│   ├── Linux -> SHA-256 only -> new version directory, `current` switched
│   └── Sidebar Update, menu bar / tray: Restart to update
│       └── macOS -> renamex_np swap (also at quit) -> relaunch -> old bundle removed at launch
├── macOS location not replaceable (read-only, translocated, root-owned) -> Download update (DMG)
├── Package manager (pacman, Nix) -> its command, once it serves another version
├── Source build, test build -> never checks
└── Failure -> keep the running version, back off, Check for updates retries

DisableUpdates at launch                 // maintain.managed-updates
├── No checks, downloads, or installs; sidebar: Managed by your organization
├── macOS app menu -> Check for Updates omitted
└── macOS menu bar -> Disabled "Updates Managed by Your Organization"
```

Administrators set `DisableUpdates` on macOS as a Boolean in the `anomaly.hex`
defaults domain, normally through a configuration profile
([#100](https://github.com/anomalyco/hex/issues/100)), and on Windows as a
non-zero DWORD under `SOFTWARE\Policies\Anomaly\Hex` in HKLM or HKCU. Hex
reads it once at launch, so a forced value wins and a change applies after
relaunch. Speech-model downloads are unaffected. Releases through 2.3.1 read
`com.kitlangton.hex2`; there is no fallback to it, so profiles must name
`anomaly.hex`.

[updater.rs](../../src/updater.rs) owns the worker and the shared status the
window, menu bar, and tray show; `updater/protocol.rs` passes the release
service's shared test vectors (`tests/fixtures/release-protocol`), and the
per-OS modules have tests for activation, the macOS bundle swap and
signature checks (ad-hoc fixtures against the real requirement), the rollout
bucket, receipts, the channel choice, never comparing versions (a legacy
2.3.2-dev.29 follows 0.0.0-dev-N; dev to stable is a switch), the switch
waiting for confirmation, and Windows Authenticode (a Microsoft-signed system file). The
Update channel row shows only while this install updates itself (not test,
source, managed, or package-manager installs); the `mac-settings-dev-channel`
scene renders it, and `rune_ui` tests cover saving and hiding it.
Not yet observed: a published dev channel release, a switch between channels on
a real install, a real update on any OS from the release service, the macOS
swap and relaunch of a Developer ID build, a Windows update of a signed build.
Manifests are unsigned; on Linux a compromised HTTPS origin could serve any
executable with a matching manifest, which Linux code signing cannot catch.

**Fixed in 2.1.13:** older releases through 2.1.12 used app-global dispatch from
inside the sidebar click, losing the action while the active window was borrowed.
The click regression passed in debug and optimized release builds. The menu-bar
path was exercised on the installed local 2.1.11 build and opened the native
Software Update window; no installation was performed. That path remains the
workaround for older versions, including after Remind Me Later. A repaired
installed-sidebar click remains unverified.

The [historical release verification](release-verification.md) records publication
evidence through 2.1.20, including signing, artifact checks, CI results, and skipped
native checks. Those release-specific results do not verify the current working
tree, installed-app behavior, or physical Linux compatibility; retain each entry's
stated limits when using it as evidence.

[Recovery](recovery.md) separates supported recovery from known defects.
[login_item.rs](../../src/login_item.rs), [status_item.rs](../../src/status_item.rs),
[updater.rs](../../src/updater.rs), the
[identity test](../../scripts/test-app-identity.sh), and
[app validation](../../scripts/validate-app.sh) are maintenance evidence starting
points. Preview badges do not prove installed-app login registration or a signed
update from an older supported build.

## Other Platforms And Consumers

```ts
Linux beta                               // not macOS feature parity
├── hex start -> One per-user Hex -> Alt-Space capture -> Stream (Vulkan GPU) -> Paste
│   └── CPU or a model that cannot stream -> Transcribe the whole clip after release
├── hex / hex app -> Window in the same process; closing it leaves Hex running
├── Recording sounds -> Shortcut press / capture stop / active cancellation
├── Shortcuts -> One state machine, three sources        // linux.shortcuts
│   ├── X11 -> Root-window key grabs; General records the chord
│   ├── Wayland + GlobalShortcuts portal -> Desktop grants the triggers
│   │   ├── General > Change shortcut -> Press chord (Esc cancels)
│   │   │   └── Saved -> New portal session asks for it as preferred_trigger
│   │   │       // observed on the niri fork: a new session gets it, a rebind in the same one fails
│   │   ├── Each row: "Your desktop granted <trigger>" (it may assign another)
│   │   └── Refused shortcut -> Danger row + how to free it (niri: config.kdl)
│   │       under that shortcut's own row (Dictation, Output, Voice Action pane);
│   │       the rest keep working
│   ├── Wayland without the portal -> Bind `hex` commands in the desktop
│   │   └── General hides shortcut rows and lists the commands
│   └── Listener starting or stopped -> `hex status` reports Unregistered,
│       never the X11 mode
├── hex dictation start|stop|toggle|cancel, hex voice-action start|stop|toggle,
│   hex paste-last -> Same events as a shortcut       // linux.commands
│   ├── Idempotent: start while recording / stop while idle -> Nothing
│   └── Hex missing or not listening -> One-line error, non-zero exit
│       // as for hex quit|restart|status
├── Escape -> Cancel active capture, not newest accepted job
│   ├── Bound only while recording (portal: own session, also with held modifiers)
│   └── X11 grab conflict / portal refusal -> Warn without stopping dictation
├── Voice Action (opt-in) -> Hold its shortcut or `hex voice-action` -> Instruction
│   └── Selected text from the primary selection -> OpenCode -> Paste the reply
├── Recording HUD -> "Recording" pill on the top or bottom edge, "Transcribing"
│   │   until accepted output finishes; Hidden position -> none   // linux.hud
│   └── Rune overlay in Hex's process (linux_rune_hud)
│       ├── Wayland -> layer-shell overlay layer, no keyboard, empty input region
│       ├── X11 -> override-redirect window, empty input shape
│       ├── No layer-shell -> `hex status` reports the HUD limitation
│       └── No window loop (no GPU or display, `hex dictate`) -> no HUD
├── Tray host (StatusNotifierWatcher) -> Tray icon: click opens the window; menu has Show Hex, Quit Hex (linux_tray)
│   └── No tray host -> No icon; status and sounds remain available
├── Microphone -> System default or a saved device; `--device` overrides
│   └── System default -> ALSA's default, resolved at each stream open // no live follow
├── Microphone failure -> Listener exits, not macOS automatic recovery
│   └── Overload (Xrun) glitch -> Logged; not a failure
└── Paste -> Retain transcript clipboard, not restore previous contents
```

```ts
Windows beta                             // x64 (Vulkan GPU) and ARM64 (CPU)
├── Install -> Setup.exe or install-windows.ps1 -> hex.exe + install.json receipt
├── HEX app -> General, tray, HUD; Enabled = shortcut works in every app
├── Launch again -> Running app shows its window; hidden (login) launch -> no-op
├── Ctrl+Space -> Hold or Toggle -> Capture -> Stream (GPU) -> Paste
├── Escape -> Cancel; a held modifier alone never ends or cancels a capture
├── Microphone -> System default or a saved device
│   └── Windows default input changes -> Switch once capture is idle // cpal's default-device report
├── Paste -> Waits for Shift, Alt, and Win; a held Ctrl is kept and harmless
├── Update staged (managed install) -> Tray, sidebar footer: Restart to update
├── Another release (unmanaged install) -> Tray, sidebar footer: Download update
├── Another channel chosen -> Tray, sidebar footer: Switch to Stable 2.3.1 (confirm)
├── Otherwise -> Sidebar footer: Check for updates
└── Modes, Voice Action, History, Activity, transformations -> as on macOS
```

Windows updates follow `maintain.updates` ([updater.rs](../../src/updater.rs),
`updater/windows.rs`). CI test builds never check. The tray item and tooltip
([windows_rune.rs](../../src/windows_rune.rs)) follow the published update
state. They pass the cross-compiled Clippy check, but no one has clicked them
on a real Windows PC. Releases are Authenticode-signed once Azure Artifact
Signing is configured ([windows-signing.md](../windows-signing.md)).

No Windows Commands, Meetings, or local transcription API. See the
[Windows guide](../windows.md) for the parity table, streaming notes, and
release flow; [check-windows](../../.github/workflows/check-windows.yml) builds,
lints, and tests x64 and builds and tests ARM64 on every pull request. Physical
smoke tests on Windows 10 and on ARM64 hardware have not been run.

```ts
Linux updates                            // maintain.linux-updates
├── Direct install (versions/ + install.json receipt) -> release service -> staged -> sidebar footer: Restart to update
├── Source install (source-build marker) -> Sidebar footer: Updated from your source checkout
├── Local build (HEX_LOCAL_BUILD=1) -> Sidebar footer: Local build · updates off; Version 2.3.1 (local <commit>); no checks, no channel row
├── pacman (AUR hex-bin: /usr/bin, root-owned) or Nix store -> Sidebar footer: Updated by your package manager
└── Anything else (a downloaded executable) -> Sidebar footer: Download update
```

The install kind comes from `updater/linux.rs`. The 2026-10-23 apollo
observation (a staged update and the sidebar button) predates the release
service and is not evidence for it.

No Linux voice Commands or meetings. `hex listen` instead prints Moonshine
transcripts. Wayland shortcuts need either the desktop's GlobalShortcuts portal
or the user's own keybindings for the `hex` commands; Hex reads no input
devices. Click-through and a physical portal or keybinding press still need
native evidence. Its [smoke](../../scripts/test-wayland-paste.sh) explicitly
isolates `HEX_APPLICATION_SUPPORT_DIR`, pastes while another keyboard holds
Ctrl and Alt, opens the window through a second launch, and checks that
closing it leaves the same process answering.

Shortcut rules are pure and tested once for every source in
[linux_input.rs](../../src/linux_input.rs): hold, double-tap lock, toggle,
double-tap only, Escape, Paste Last, Voice Action, and command idempotence,
plus X11 auto-repeat filtering and a portal release that never arrives
(Hyprland's Lua binds drop it when the modifier comes up first; see
[dictation.md](dictation.md)). The X11 grab tests run on an isolated display in
CI; `commands_and_grabs_share_the_x11_listener` drives grabs and commands
through one listener. [scripts/test-linux-service.py](../../scripts/test-linux-service.py)
checks that commands fail without a listening Hex.
On 2026-10-22 the portal path was observed on niri with xdg-desktop-portal-gnome
using synthetic uinput keys against an isolated service: granted triggers,
hold and Escape while held, double-tap lock, Paste Last, and commands. A
physical keyboard press has not been recorded.

`linux.hud`: [linux_rune_hud.rs](../../src/linux_rune_hud.rs) keeps the
listener's generation slot (an old listener cannot hide a restarted HUD) and
accepts reports before the window's app exists. The GTK HUD it replaced is
deleted. Observed on
2026-10-22 in a headless sway on apollo (isolated data root, real microphone,
`hex service`): `hex dictation start` showed the pill at the top edge and it
was gone after `hex dictation stop` and transcription. The Rune overlay itself
was also observed passing a virtual-pointer click through to the window below
and leaving keyboard focus where it was (Rune `examples/background`). Not
observed: X11 with Hex, niri, KWin, Mutter.

See the [Linux guide](../linux.md), [linux_app.rs](../../src/linux_app.rs),
[linux_portal.rs](../../src/linux_portal.rs), and
[Linux CI](../../.github/workflows/check-linux.yml) for existing checks.
The [direct installer/updater](../../src/updater/linux.rs) and
[installer tests](../../scripts/test-install-linux-release.sh) do not establish
a published, supported-host-validated signed release. [Nix](../nix.md) owns its
package updates; HEX updates only managed direct installs.

```ts
Linux process                            // maintain.linux-service
├── hex / hex app -> Hex running? -> Activate{activation token} over the socket -> exit 0
│   ├── No Hex, hex.service loaded -> session.env -> systemctl start (Type=notify) -> Activate
│   ├── No Hex, no unit or custom data dir -> This process becomes Hex
│   └── Two racing launches -> One Hex; the other forwards and exits 0
├── hex start / desktop autostart / hex service -> Same, without the window
├── hex dictation|voice-action|paste-last -> Socket worker -> Listener queue (never the UI thread)
├── hex status -> Published status; hex quit / Quit Hex / launcher action -> Drain, exit 0
├── Open the window -> One fixed 980×760 size (min = max) -> Tiling WMs float it
├── Close the window -> Keep microphone, accepted jobs, model preparation, and HUD
│   └── Uncommitted shortcut capture -> Cancel and restore the prior listening state
├── Crash -> systemd Restart=on-failure -> Hex without a window
├── Update -> Restart the unit -> Hex reopens the window
├── hex restart without a display (SSH) -> Keep the saved session.env
├── Window fails (no display, no GPU) -> Logged; Hex keeps running
└── Startup -> transcribe.cpp's Vulkan instance first, on the main thread
    └── then the model load and the window's wgpu instance never create one at once
```

Sources: the process, host, and window seam in [linux_app.rs](../../src/linux_app.rs),
the single-instance socket and systemd lifecycle in
[linux_service.rs](../../src/linux_service.rs), the CLI in
[linux.rs](../../src/linux.rs), and the managed installer/Nix user units.
`hex status` queries the running Hex; `--lines` explicitly reads historical
observations. `hex app --hidden` and `hex start` start Hex without the window;
`hex stop` equals `hex quit`. Closing the window does not mean Disable; the
General's listener card Enable/Disable controls the listener, Quit Hex the process. No
root daemon or new input grants. The window runs on Rune's app lifetime: it
opens or focuses with the forwarded activation token and keeps its state
across close and reopen. Observed in a headless sway on apollo on 2026-10-22
with [test-wayland-paste.sh](../../scripts/test-wayland-paste.sh), then with
GPUI and Rune builds (open in the same process, close, still answering,
reopen, quit); not rerun since GPUI was removed. On niri
(apollo, 2026-10-22, isolated data directory, listener off) launches spawned
through `niri msg action spawn` opened the window in the instance's process,
`close-window` left the same process answering, a relaunch opened a new
window in it, and `hex quit` exited; the window never took focus although niri
passed `XDG_ACTIVATION_TOKEN` (an IPC spawn's token, without user input, so
this does not show whether a launcher's token focuses it). Not yet on KWin,
Mutter, or i3.

Listener starts consume the host's settings snapshot; the foreground
`hex dictate` command loads its own snapshot once. Invalid startup settings leave
defaults available only for editing and block capture until a successful settings
commit or a restart with valid settings. Dismissing the load error does
not authorize recording with defaults. Feedback is initialized at these entry
points, so a delayed listener start cannot reset a newer live volume selection.

Checks: [test-linux-service.py](../../scripts/test-linux-service.py) exercises
the real socket and process lifetime without a display, installed model, or
audio device; it passed on apollo (Arch, niri) on 2026-10-22.
`closing_the_window_cancels_only_an_uncommitted_capture`,
`commands_route_only_to_a_started_listener`, and
`adopting_saved_settings_restarts_only_for_listener_settings` use controlled
runtime workers. The virtual-microphone and Wayland scripts open the window
through a second launch; the Wayland script passed on apollo (headless sway,
`WLR_RENDERER=gles2`, before GPUI was removed), the X11 virtual-microphone script
has not run since the single-process change. A throwaway `Type=notify` user unit
on apollo (`HEX_SYSTEMD_UNIT`) started ready through `hex start`, restarted
after SIGABRT and SIGKILL without a window, stopped cleanly on
`systemctl stop` and `hex quit`, and an older `Type=simple` unit still started.
Before the Vulkan ordering fix, a launch with an installed model and
`enable_on_launch` SIGSEGVed in the Vulkan loader 3/3 in a headless sway on
apollo (the model load raced the window's wgpu instance); with it, 6/6 ran.
These checks do not establish a physical desktop logout, cross-version signed
update, or microphone/target-app success on the user's devices.

`malformed_settings_cannot_start_even_after_dismissing_the_error` in
[linux_app.rs](../../src/linux_app.rs) checks the failure gate and recovery after a
simulated successful settings commit. It selects a model never installed on
Linux, so the resumed start stops at `Model required` instead of opening the
microphone on a machine with the real model; it passed on apollo on 2026-10-22
beside the live service.
`listener_startup_ignores_the_previous_session_until_a_new_one_arrives` checks the
listener owner's startup observation and stop signal; the exact production owner
and test passed in an isolated standard-library-only Rust harness on macOS. That
harness does not compile the Linux application or prove its runtime integration;
the test has **not been executed on Linux**. These checks do not exercise physical
devices or persistence failure on disk.

The [September 4 service-build evidence](release-verification.md#linux-service-build)
records an Arch/i3 client-independent ownership check, not transcription accuracy
or physical logout, Wayland, or signed cross-version update proof.

```ts
Linux General > Sound volume              // dictate.feedback.linux; X11 + Wayland
├── Default / older settings -> 50%
├── Off -> No recording tones
└── 25% / 50% / 75% / 100% -> Save and apply live; preview the start sound

Capture                                  // hex app and hex dictate
├── Shortcut starts capture -> Queue start sound immediately, before audio arrives
├── Hold passes 300 ms -> While dictating mute/pause begins, never at the press
├── Finish retained recording -> Stop sound at the release, while the 200 ms
│   tail is still captured; a muted output unmutes first   // dictate.stop-tone
│   └── Clip turns out near-silent -> Stop sound already played, nothing pasted
├── Brief discarded tap -> Start sound, but no stop sound or transcription
├── Dictation or Voice Action pasted -> Done sound; failures stay silent
└── Cancel active capture -> Cancel sound; idle Escape stays silent
```

The shared [feedback player](../../src/feedback.rs) uses Hex's bundled sounds
([Feedback](dictation.md#feedback)) on every platform; only macOS wake/sleep
use `afplay`.
Decoding happens before the Linux capture loop; the output device opens for
the first tone and is released five minutes after the last one, as described
in [macOS feedback](dictation.md#feedback). Playback admission is bounded and
nonblocking. Output preparation failures are
logged without blocking dictation. The Settings window saves the volume to
`settings.json` and the service reloads it without restarting the listener. The start sound does not wait
for the 300 ms capture-retention threshold; input delivery and audio-device
latency still apply. **While dictating** mute or pause does: the audio loop
requests it when the capture's audio reaches 300 ms past the press, and not
during a release tail. Until 2026-10-22 the service muted at the press. This
ordering is checked by reading the loop, not by a test or a muted device.

Checks: `bundled_sounds_decode_without_an_audio_device` and
`feedback_admission_never_waits_for_playback` in the player, plus
`files_that_predate_a_setting_keep_the_beta_defaults` and
`a_muted_beta_keeps_the_remembered_volume` in
[linux_migration.rs](../../src/linux_migration.rs) for settings that predate
`settings.json`.
`start_sound_precedes_audio_and_does_not_change_short_tap_discard` in
[linux_dictation.rs](../../src/linux_dictation.rs) exercises the production start
path with a sound spy: immediate feedback without any audio, while a 100 ms tap
still discards. Existing timing checks in [dictation.rs](../../src/dictation.rs)
cover the unchanged intentional-hold boundary. These are not proof of audible
native playback or physical hotkey-to-sound timing.

The [September 4 sound-build evidence](release-verification.md#linux-recording-sounds-builds)
records the initial and immediate-start builds. It does not establish audible
native playback, measured physical key-to-sound latency, or Wayland UI behavior.

Both Linux CI triggers include `tests/**`, and the
[Nix source fileset](../../nix/package.nix) includes those sources. The
`keyboard_layout` harness skips on Linux; packaging it does not provide macOS
layout evidence.

```ts
TypeScript SDK + macOS local service
├── Caller records audio -> Explicit helper -> Raw transcription
├── Caller streams audio -> Running app's /streams -> Partials, segments, final
│   └── Dictation first; uses the loaded model only (docs/streams.md)
├── Running desktop capture API -> Separate microphone ownership protocol
│   └── No silence gate                  // raw model output, unlike hotkey dictation
└── Distribution
    ├── SDK 0.3.0 -> Published
    └── Turnkey signed helper -> Unfinished
```

The [SDK guide](../../sdk/typescript/README.md),
[service contract](../specs/local-transcription-service.md), and
[SDK tests](../../sdk/typescript/test) distinguish those paths. Helper-only service
mode is not a physical hotkey/microphone test. Embedded/packaged service smokes
now expect API 2, matching service/SDK compatibility checks. Helper packaging
requires an explicit signing team and notarization profile, with the same
Developer ID identity validation as the app; identity fixtures do not establish
a signed helper distribution. Low-level blocking-peek and
FIN/half-close limitations in [local_api.rs](../../src/local_api.rs) must not be
generalized to every SDK cancellation path.

Live streams (`/streams`, [contract](../streams.md)) are exercised by fake
engines only: `transcription_stream` tests cover windows, partials, the
backlog during dictation, preemption and offline fallback, the lease, cancel,
and busy; `local_api::streams` tests cover routes, auth, and bounds; and
`transcription_engine` tests cover yielding streams closed by dictation. No
real-model or live-app stream has been run.

Model progress and dictation levels share bounded SSE framing in
[sse.ts](../../sdk/typescript/src/sse.ts). In-memory
[SDK regressions](../../sdk/typescript/test/sse.test.ts) cover transport-chunk
independence, line/event limits, callback cancellation, reader cleanup, and
bounded level observations. A CR-delimited terminal event is also checked with
an open response body, so completion does not depend on EOF or subsequent data.
These checks do not establish live service or
microphone behavior.

## Developer And Prototype Surfaces

```ts
macOS debug runtime                      // not release navigation
├── Meetings -> Always available; release builds need the Meetings opt-in (its pane's card or Meetings › Settings)
├── Activity / HUD Lab / hex dev -> Observation and developer controls
└── HUD Lab in release -> Only while dot-scope is on, Dot scope's section only

`hex app -- --snap OUT --scene NAME` -> Rendered fixtures    // not proof capture/download/update happened
Linux `hex` -> the Linux window on Rune, in the Hex process // dev.rune-ui
├── Calls the host directly: saves settings through it; woken on host changes
├── Sidebar: General, Modes, Voice Action, History
├── Transcription picker -> ChooseModel / CancelModel; progress from the snapshot
├── First-run setup -> onboarding::Flow without permissions or practice
├── Modes: "Choose a file…" through file_dialog (FileChooser portal); drag to reorder
├── Modes: app icons (hicolor theme or pixmaps, PNG or SVG) in the picker,
│   chips, and mode rows; monogram while loading or without an icon
├── Model and application searches: Up/Down highlight, Enter picks it
├── Not yet: right-click delete
└── macOS/Windows surfaces as scenes too (Capabilities + preview data):
    Commands, Meetings (Recordings | Settings, opt-in card), Voice Filter,
    HUD Lab, Activity, permission rows, Launch at login, Dock icon, shortcut
    sides, the full tour
macOS `hex app` -> one menu-bar process, Rune window // dev.rune-ui-macos
├── rune::App on the main thread: accessory, windowless unless setup/onboarding/Dock icon open it
├── Same AppKit loop: status item, updater, Hex/File/Window menu, Dock reopen event, Metal HUD (16 ms timer)
├── Runtime thread: model preparation (menu bar: installed only), starts the listener once setup is ready
├── Listener, meeting controller, local API: their own threads
├── macos_host: AppKit event monitor records chords (Option, Fn); NativeHost does permissions,
│   login item, Hex Microphone, meeting start/stop/rename/trash/reveal, commands workspace, relaunch
├── Meetings pane lists the store through meeting::query (1 s rescan, sent only when changed);
│   a controller failure shows in the pane; NSOpenPanel (file_dialog) chooses the meetings
│   folder and Modes' "Choose a file…" .app, modeless on the main thread
├── Tour: practice dictations from the listener's event log; a listener that failed to start
├── Sidebar and pane header move the window (Rune El::window_drag; the title bar is transparent)
├── While the window shows, its poll re-checks timed state every 250 ms (Rune Tx::wake_after); close hides it (Dock policy restored)
├── ⌘Q / menu-bar Quit / Ctrl-C quit and join
├── Meeting capsules: Rune interactive overlays (offer, call ended, ready, recording
│   with Copy new (N) / Copy all of the in-call transcript; compact until hovered,
│   dragged to any corner)
└── Modes app icons: Finder's icon (NSWorkspace) in the picker, chips, and mode rows
Windows `hex app` -> one tray process, Rune window // dev.rune-ui-windows
├── Runtime thread owns WindowsDesktopHost (listener, model preparation, OpenCode, transformations, updater)
├── Named event: a second launch shows the window; tray Show / Enable-Disable / Quit
├── rune::App: --hidden starts windowless, close hides (keep_running); Quit or no tray ends it
├── General adds Launch at login; no terminal paste; chord capture polls
├── HUD: shared indicator model + CPU shader in a Rune overlay
├── Modes: "Choose a file…" opens IFileOpenDialog (file_dialog) on its own STA thread, no owner window
├── Modes app icons: the executable's first icon (PrivateExtractIconsW)
└── Not yet: custom caption
iOS prototype -> Record/transcribe/copy and keyboard handoff
Unmerged Windows / deferred features -> Not part of this map
```

Sources: [meeting.rs](../../src/meeting.rs),
[meeting_watcher.rs](../../src/meeting_watcher.rs),
[developer_control.rs](../../src/developer_control.rs),
`SCENES` in [rune_ui.rs](../../src/rune_ui.rs), [iOS guide](../../ios/README.md).
iOS physical-device proof and temporary-WAV failure cleanup remain gaps;
best-effort deletion is not guaranteed deletion.

`dev.rune-ui` lives in [rune_ui.rs](../../src/rune_ui.rs) and its pane modules;
its tests drive the Rune runtime headlessly (navigation; General rows per
shortcut source, fitting the default window with Advanced collapsed, Double-tap to lock folding under the rows below, saves through the host, Quit Hex, microphone-release
confirmation, shortcut recording and Escape, conflicts; Voice Action opt-in
and its conflict refusal, model choice; History select, delete, two-step
clear, copy; Modes add/delete, corrections, transformations (drag reorder saves once
on release), application conflicts, a chosen file joining the mode, arrow
keys in the model and application searches; the picker's ChooseModel/CancelModel; setup's download; draft
saves; and, from macOS/Windows preview data, each OS's panes and rows, Launch
at login, permission warnings, the command model notice in Commands, the
Meetings opt-in card and Settings view, Commands search/opt-in card/prompt,
Meetings select,
search, copy, rename, start/stop, two-step trash, and the overflow menu, and
the macOS tour's permissions, practice, and relaunch; Voice Filter's
experiment gate, modes, sliders, and demo requests, HUD Lab's indicator requests, Activity, and shortcut
sides) and compare 62 offscreen
GPU renders with [tests/golden](../../tests/golden). In these tests the macOS and
Windows requests are recorded, never performed; the macOS native host
(`dev.rune-ui-macos`) has no automated test of its own. They skip the goldens without a GPU
adapter and do not prove a real window, input method, or physical shortcut.
Before the single-process change, a headless script against an isolated
service saved a new mode, a correction, and Voice Action. See
[the Rune port plan](../plans/rune-ui.md) for what was observed.

`dev.rune-ui-macos` lives in [macos_rune.rs](../../src/macos_rune.rs) and
[rune_ui/macos_host.rs](../../src/rune_ui/macos_host.rs). On hailmary
(macOS 26.2, Command Line Tools only) it compiles, passes clippy
`-D warnings` and the macOS unit tests (including `macos_host`'s chord
capture), and a debug `hex app` with an isolated data root started, showed
its 980×760 window (CGWindowList; `lsappinfo`: Foreground while open),
answered `hex dev status/show/hud`, and quit cleanly on Ctrl-C, also with
the meeting fixtures copied into its data root and the Meetings pane shown.
`macos_host`'s tests cover the meeting feed over `tests/fixtures/meetings`
(order, recovery to Interrupted, turns and pauses, no resend when unchanged)
and the practice feed over a written event log. After GPUI's removal the
same host built with a local Command Line Tools stub passed clippy and all
648 macOS tests on hailmary (2026-10-22); it was not launched again. Those
builds stub the HUD's Metal shader and Apple Intelligence (Xcode is missing),
so the HUD failed to load its shader there; window contents (including the
Meetings list and the practice step), dragging the window, the folder
panel, the status item, reopen, shortcut capture, and Sparkle have not been
seen or exercised, and no screenshot could be taken over SSH.

`dev.rune-ui-windows` lives in [windows_rune.rs](../../src/windows_rune.rs) and
[rune_ui/windows_host.rs](../../src/rune_ui/windows_host.rs). It has only
been cross-compiled (`cargo xwin clippy -D warnings` for
`x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`, with the
transcribe.cpp build stubbed out, last after GPUI's removal on 2026-10-22); it
has never run on Windows, and no test covers it. It replaces the GPUI app
earlier Windows releases shipped.

`each_os_lists_only_the_panes_it_can_run` and
`developer_panes_show_only_in_developer_builds` in
[rune_ui/tests/panes.rs](../../src/rune_ui/tests/panes.rs) check pane
availability from the capabilities, the developer flag, and the Meetings
opt-in; they do not exercise native navigation clicks.

Scenes and tests use fixture state (`Service::Fixture`, `PlatformHost::Fixture`):
requests are recorded, never performed. They do not establish native device
discovery or installed-app behavior.

`default_paste_hotkeys_follow_build_capabilities` in
[app_settings.rs](../../src/app_settings.rs) checks the production default runtime
and settings projections: Paste Last remains available in both build profiles;
meeting paste is present in debug or after the Meetings opt-in. `meeting_paste_is_only_active_in_developer_builds`
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

## September 22, 2026 Cleanup Verification

The working-tree cleanup passed 481 Rust tests in debug and 480 in release, with
twelve opt-in tests ignored in each profile and all twelve keyboard-layout child
scenarios passing per profile. Strict all-target/all-feature Clippy, formatting,
app/helper signing-input guards, and changed shell-script syntax checks passed.
The command SDK passed 52 tests; the public SDK passed 56; both typechecks and
builds passed. A packed public SDK imported both Promise and Effect entry points
from an isolated consumer.

The isolated embedded-service smoke passed API 2 handoff, health/models routes,
absence of discovery-file publication, and shutdown on host-lease closure. It did
not open a microphone or run inference. No signed helper artifact, installed-app
settings flow, provider, physical capture/paste, or Linux integration was tested.
The Linux listener-owner harness and source review do not replace Linux CI.
The optimized Modes preview built and launched, but screenshot capture failed
with `could not create image from window`; native visual layout is unverified.

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
