# Hex Feature Map

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
Hex                                      // macOS release
├── Get ready                            // install, permissions, local model
├── Dictate                              // hold OR double-tap lock
├── Use the result                       // paste, paste-last, History
├── Adjust behavior                      // shortcut, microphone, recording preferences
├── Process text                         // Output tab + context-selected Modes
├── Opt in                               // Meetings: off by default
├── Recover                              // prove the next attempt works
└── Maintain the app                     // menu bar, login, updates, quit
```

[Dictation](dictation.md), [Meetings](meetings.md), and [Recovery](recovery.md)
have detailed maps.
[Accessibility](accessibility.md) maps what screen readers, Voice Control, and
automation (strobe's benches) read from the window, the HUD, and the capsules.
Agents can [read retained transcripts](transcript-cli.md) and
[control the running app](control-cli.md) with the release CLI.
[Labs](labs.md) gives every unfinished feature its own switch, off by
default, listed under collapsible groups (Dictation, Output, History,
Meetings, Annotate, Shortcuts, Models) with only this OS's labs.
Its Sort and filter models toggle (off by default) adds a sort and a saved-language filter to the
Local transcription picker (#645); its Model test details toggle (off by
default) shows each model's raw WER or CER with its language, dataset, source,
and conditions (#647); its Release notes
link toggle (off by default) puts a link to this build's release notes in the
sidebar footer; its Appearance toggle (off by default) forces the main window
light or dark instead of following the OS. The dictation HUD and meeting
recording pill always stay dark; other overlays follow the OS; its Words on screen
toggle (off by default, macOS) uses names and terms visible in the focused
window as temporary vocabulary for each dictation, read locally through
Accessibility and never saved ([vocabulary.md](vocabulary.md)). Its Rewrite
reads nearby text toggle (off by default, macOS) hands the mode's OpenCode
rewrite the text around the caret as context for names, terms, and where the
result lands; the text goes to the configured provider, which may be a cloud
service, password fields are never read, and off leaves the prompt byte for
byte unchanged. Its Clean up with Apple Intelligence toggle (off by default,
macOS) gives each dictation its mode does not rewrite one short on-device pass
that fixes clearly misheard words and spoken self-corrections, pasting the
corrected text whenever Apple Intelligence is unavailable, slow, or refuses.
Its Neutral voice toggle (off by default, macOS) rewrites a chosen mode's
dictations into plain, neutral wording on this Mac instead of that mode's
OpenCode rewrite and the cleanup, so the neutral pass never sends the text to
OpenCode; the mode's transformations still run afterwards and may use the
network if you wrote them that way, and Voice Action still uses OpenCode. A
number, negation, or protected number run, link, path, code, or quote that
does not survive verbatim keeps the corrected transcript.
General › Advanced › Interface language offers Português (Brasil), and Hex
uses it for any Portuguese system language (#867); it also offers Français,
used for any French system language (#892), Русский, used for any Russian
system language (#1317), and Magyar, used for any Hungarian system language
(#2667). None of these has had a native speaker's review yet. It offers
简体中文 (`zh`) and 繁體中文 (`zh-Hant`, #944);
Traditional Chinese is drafted from the Simplified strings by a character and
Taiwan-usage phrase converter (`desktop/i18n/traditional_chinese.rs`), not yet
reviewed by a native speaker. Meeting times in the Meetings pane, the macOS
calendar card, and the exported transcript and notes are 12-hour ("2:30 PM")
in English and 24-hour ("14:30") in every other interface language.
The [vocabulary](vocabulary.md) biases transcription toward names you list; its
Labs › Vocabulary files toggle (off by default, every OS) imports terms from a
CSV file into the Vocabulary sheet and exports the list as one.
[Your own speech models](own-models.md) covers adding a GGUF file and using
models another app already downloaded; its Labs › Smaller model files toggle
(off by default, macOS) also accepts imports quantized to Q6_K, Q5_K, or Q4_K.
[Annotate](capture.md) (macOS, Linux,
and Windows) records the screen while you draw and talk and sends it to an agent;
its Labs › Agent tools for captures toggle (off by default, every OS) adds
`hex annotate inspect`, `zoom`, `frames`, and `skill`, which read a bundle in
the CLI process so a coding agent can see its summary, marks, a dense frame
sheet, or a range of frames itself. Its Labs › Install agent skill from
Annotate toggle (off by default, every OS) adds an Agents section that writes
the `hex-annotate` skill into Claude Code, Codex, OpenCode, or Pi, and on macOS
links `hex` onto PATH.
[Screenshots](labs.md) (Labs, macOS) takes CleanShot-style ⌘⇧4 area and
window screenshots, and with Annotate on records from a ⌘⇧5 toolbar.
The other capabilities stay here until they need their own verification recipe.

## Get Ready

Install the signed macOS app with `brew install --cask anomalyco/tap/hex` or the
[manual DMG](../../README.md#downloads). The cask uses a versioned, checksum-pinned
DMG, requires Apple silicon/macOS 15+, and leaves updates to Hex's own updater. It does
not require OpenCode, launch the app automatically, migrate the legacy
Swift app's data, or forcibly replace an existing app destination. Homebrew uninstall
leaves runtime settings, models, and retained data intact.

```ts
Download Hex                              // setup.download-recovery
├── anoma.ly/hex names the visitor's platform  // setup.download-button
│   ├── Mac -> DMG | Intel Mac -> disabled, "needs Apple silicon"
│   ├── Windows x64 / ARM64 (UA-CH) -> hex-setup-<version>-windows-<arch>.exe + SmartScreen note
│   ├── Linux -> copyable `curl … install-linux.sh | sh` + Arch/glibc requirement
│   ├── Phone -> Send yourself the link
│   ├── Scripting off -> every OS and architecture, with no tabs or switches
│   └── Other platforms -> every build, Homebrew, the GitHub release
├── Website, README, installers, apps, feeds, appcast -> anoma.ly/hex/files (Worker hex-production-site):
│   installers and appcast from the site build, dev notes from the dev-notes branch, frozen feeds and artifacts from R2
├── Releases up to 2.3.1 -> downloads.hex.kitlangton.dev (Kit's frozen bucket) -> the same bytes
├── GitHub v<version> (app-v through 2.3.1) -> identical DMG, Setup.exe, and Linux bytes
└── Older app cannot reach its update feed -> manually install the DMG

Install on Windows                        // setup.windows-installer
├── Setup.exe, per user, no admin -> managed versions\<version>\hex.exe
│   ├── x64 without a Vulkan 1.2 driver -> installs; speech on the CPU + Settings note (#1240)
│   ├── another version installed (e.g. self-updated) -> install this one over it
│   ├── the setup engine is signed -> Smart App Control runs the copy extracted to %TEMP%
│   │   (unsigned, it blocks setup: "Unable to execute file in the temporary directory", error 4551)
│   └── finish -> Open Hex -> the first run downloads a model
└── Settings › Apps -> uninstall every version, shortcut, Launch at login; keep data
    └── the installed uninstaller is that same signed engine

Install on Linux                          // setup.linux-installer
└── curl … | sh -> signed feed -> verified artifact -> launcher + hex.service
    ├── cannot start -> name the glibc floor or the missing Arch packages
    └── finish without wl-copy -> print apt/dnf/pacman commands for wl-clipboard and wtype

Prepare local models                      // separate network dependency
└── Dictation -> huggingface.co and its download CDN
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
[test-windows-installer.ps1](../../scripts/test-windows-installer.ps1); with
`-SignedEngine` it also checks that the installed uninstaller (the same engine
bytes Setup extracts to `%TEMP%`) has a valid, timestamped Authenticode
signature, and Release runs it that way. No interactive install, Smart App
Control decision, SmartScreen prompt, or first launch on a real PC has been
observed. The Linux library diagnosis has no automated check. The site's
platform states were captured once in headless Chromium with `?platform=`, and
its scripting-off page with JavaScript disabled.

Sources: [website](../../site/src/Download.tsx), [installer](../../packaging/windows/hex.iss),
[release publisher](../../.github/release/release-service.sh),
[mirror publisher](../../scripts/publish-app-mirror.sh),
[macOS release publisher](../../scripts/release-app.sh),
[Linux installer](../../.github/release/install-linux.sh), and
[updater](../../src/updater.rs).
Model transfer failures in [transcription/models.rs](../../src/transcription/models.rs)
explain the source host, retry, and
separate prerequisites. These messages do not diagnose a particular firewall
or establish a successful subsequent model download.
Executed hosting checks and deployment details are recorded in
[Download Hosting](../downloads.md). In-app feed defaults and recovery copy ship
with 2.1.17; the initial 2.1.16 mirror preserves that release's original bytes.

```ts
Launch Hex, setup incomplete              // setup.tour; every build and OS, full window
├── One onboarding: the first-run story (rune_ui/onboarding_kit/live.rs) over
│   the tour state machine (desktop::onboarding::Flow); no flag, no other tour
├── Skip (quiet, top-right, only on the demo and the practice)    // setup.skip
│   ├── Passes over that part to the next step setup still needs: permissions,
│   │   then Almost ready while the model downloads, else Hex is ready
│   ├── Never closes onboarding or records it done
│   └── No Skip on permissions, the model step or download, or the paste
│       permission. Check: `onboarding_skip` (scripts/check-onboarding.sh)
├── GNOME/KDE Wayland (pastes through the RemoteDesktop portal, no wtype)
│   and no saved grant -> before Your turn, "Let Hex paste for you"     // setup.paste-access
│   ├── Says why: Hex only sends the paste keystroke, never sees the screen;
│   │   the desktop asks once and may call it remote desktop (#932)
│   ├── Allow -> opens the desktop's dialog; the grant's restore token is saved,
│   │   so the first paste needs no dialog
│   ├── Refused -> Try again, or Continue (the desktop asks at the first paste)
│   ├── X11, wlroots with wtype, macOS, Windows -> no page
│   └── Skipped or refused -> Settings › General shows it under "Permissions
│       needed" until allowed. Check: golden `onboarding-paste`; the real
│       portal dialog is not exercised
├── 2.3.1 or Swift Hex -> Sparkle offers the new Hex -> Install Update   // setup.legacy-update; macOS
│   ├── Replaces the old app in place (its file name kept) and opens the new one
│   ├── Accepted only because app/Info.plist keeps the old SUPublicEDKey;
│   │   scripts/validate-app.sh requires it. Feed items: docs/plans/swift-app-handoff.md
│   └── No automated check; a clean-Mac install from each old app is the proof
├── 2.3.1 Linux or Windows updates into this build     // setup.legacy-adopt
│   ├── It lands in <data>/voice-control/versions/<version>; its first app or
│   │   `service` launch from there moves it into the managed layout, then
│   │   relaunches from it (Linux: exec, so the old unit keeps its process)
│   ├── Linux -> 2.3.1's marked unit, ~/.local/bin/hex link, and hex.desktop
│   │   entries move aside -> the built-in .github/release/install-linux.sh
│   │   installs this executable -> session.env copied; failure puts them back
│   ├── Windows -> %APPDATA%\hex\versions, receipt, Hex.lnk recreated,
│   │   Launch at login repointed, 2.3.1's Apps & Features entry removed
│   ├── Failure -> logged, runs in place; the next launch tries again
│   ├── Settings, history, and models in voice-control are not carried over
│   └── Check: `a_legacy_linux_install_moves_into_the_installers_layout`,
│       `a_failed_move_puts_the_legacy_files_back`; Windows and a real 2.3.1
│       feed update are unverified
├── Upgrading from 2.3.1 or earlier (same `hex` folder)     // setup.upgrade; macOS
│   ├── `anomaly.hex` is a new identity: macOS kept none of the old grants
│   ├── Settings without `anomaly-hex-identity` in `migrations` -> checked once at launch
│   │   ├── Permissions or the selected model not ready -> the old completion
│   │   │   no longer counts -> onboarding opens from the start
│   │   └── Everything ready -> no onboarding; nothing but the ID changes
│   ├── Saved settings are kept; the ID is recorded either way
│   └── Setup ready, listener starting -> a running 2.3.1 or Swift Hex   // setup.quit-legacy
│       (retired bundle IDs) is asked to quit, so both never answer Option;
│       it stays installed. No check; `macos_rune::quit_legacy_hex`
├── Setting up Hex -> speech model download starts at once  // setup.background-download
│   ├── Model -> the selection recommended for the system's first preferred
│   │            language (English -> Parakeet Unified English; e.g. French ->
│   │            Parakeet v3, Japanese -> Whisper large-v3-turbo); when that
│   │            language isn't English, the language capsule switches between
│   │            English and its recommended model with one press, with no
│   │            dropdown chevron; with English as the system language there is
│   │            nothing to switch to, so the capsule just shows English,
│   │            whatever English model is selected
│   ├── The download is held while the Hugging Face scan runs (headers only)
│   ├── Models found here -> the model step instead      // setup.found-choice
│   │   ├── "Choose a speech model": one list, ours first and selected, then the
│   │   │   found files; a pick starts at once (ours downloads, a found one is
│   │   │   checked and used in place); Continue starts the selected row
│   │   ├── A found pick moves on with no download page or pill
│   │   └── Back on the pages after it returns to the list to change the pick
│   ├── Nothing found -> this page as always, the recommended model downloading
│   ├── Progress moves to the top pill once the story moves on; failure ->
│   │   why, in one line (a proxy or certificate, #894), + Use a model
│   │   file… + Try again (the model picked from the list, if one was)
│   ├── Failed, or stalled 30 s without bytes, with every permission ready
│   │   -> Continue without a model (quiet), saying dictation won't work
│   │   until a model is added with Add model file…      // setup.without-model
│   │   └── Records completion and closes onboarding; Settings › General's
│   │       Speech model row says no model is installed (with the download's
│   │       error), + Download + Add model file…. Check: golden
│   │       `onboarding-no-model`, `onboarding_continue_without_model`
│   └── Change or delete the model later in Settings      // setup.delete-model
├── The demo, while the model downloads: a second on Setting up Hex, then the
│   Launch Room's conversation is there, Jay's messages landing in it ->
│   "Without Hex" (typed, with typos) -> "With Hex" (the same reply, spoken,
│   fireworks lighting the page), then moves on by itself; Skip → (or
│   Escape) cuts to the end
│   └── Jay's face rides the newest message: it fades out on the older line as
│       it fades in on the new one, never popping off in one frame
├── Quit part-way -> next launch resumes at the saved step  // setup.resume
│   ├── The saved step is the one record of an unfinished tour; only Start
│   │   using Hex (with setup ready) or Continue without a model records it
│   │   finished, and that replaces the step. Setup becoming ready does not:
│   │   a run quit on the practice resumes at the practice, one quit on Hex
│   │   is ready resumes there. Check:
│   │   `a_tour_quit_part_way_stays_unfinished_until_it_finishes`
│   ├── Resumed at setup -> the first permission still missing; with none
│   │   (Linux and Windows) straight on to the practice or Almost ready.
│   │   Check: `a_run_resumed_at_setup_asks_only_for_what_is_missing`
│   └── Permissions are read again at launch: a setup they now finish (macOS's
│       Quit & Reopen after a grant) is passed; past setup, it waits until they do
├── Permissions, one page each           // setup.checklist; skipped when granted
│   ├── macOS: "Let Hex hear you" (Microphone), then "Make it work everywhere"
│   │   (Accessibility), both before Your turn: the shortcut needs Accessibility;
│   │   each card floats off once allowed
│   │   ├── No Input Monitoring row: Accessibility also lets the shortcut taps listen
│   │   ├── Microphone -> macOS prompt; refused -> "Open System Settings" + guide
│   │   │   ├── Until then, tones stay silent when the default output also records
│   │   │   │   (a USB headset, a Mac VM's one sound device): opening it would
│   │   │   │   make macOS prompt before the page asks
│   │   │   └── A grant macOS took another way (CoreAudio's prompt, the switch in
│   │   │       System Settings) reads from TCC, since AVFoundation keeps its first
│   │   │       answer. Check: `a_microphone_answer_avfoundation_has_not_seen_reads_from_tcc`
│   │   ├── Accessibility -> Privacy pane + drag guide
│   │   │   ├── Granted while System Settings is still in front (within 3 s) ->
│   │   │   │   Hex comes back in front; another app in front keeps it (also
│   │   │   │   System Audio and Screen Recording). Check:
│   │   │   │   `hex_returns_after_a_grant_only_while_system_settings_is_in_front`
│   │   │   ├── Retired Hex bundle IDs (`com.kitlangton.hex2`, `com.kitlangton.Hex`,
│   │   │   │   `com.kitlangton.Hex.debug`) cleared once in the background (`tccutil reset`,
│   │   │   │   ignoring `OSStatus -10814` when no bundle is on disk)
│   │   │   ├── Returned from System Settings still untrusted (2 s, or 6 s side-by-side) ->
│   │   │   │   "An older Hex entry can keep this off" + "Still grey? Fix it"
│   │   │   │   -> resets the running bundle ID + retired IDs, prompts, and reopens the guide
│   │   │   ├── macOS 27 names the list "Device Control and Data Access"; the row follows
│   │   │   └── macOS 27.3 renames it "System & App Protection Override" and takes no
│   │   │       dragged tile: a compact bar sits below System Settings (above it when there
│   │   │       is no room) out of the pane's + controls, and says to click the Privacy &
│   │   │       Security notification or add Hex with +; the row and the pane's name follow.
│   │   │       Check: `macos_27_3_names_accessibility_without_a_dragged_tile`
│   │   ├── On in System Settings, unusable until relaunch -> "Needs a restart"
│   │   │   └── Every permission on -> "Restart Hex" -> relaunch at setup  // setup.restart
│   │   └── Each grant -> a beat with its check -> the card floats off -> on
│   ├── Windows: only while the microphone privacy switch is off: "Let Hex hear
│   │   you", one Microphone row -> Open Settings (ms-settings:privacy-microphone)
│   └── Linux: no page
├── Almost ready: the model still downloads -> waits, then Your turn
├── Your turn: the practice game          // setup.practice; every OS
│   ├── "Clear your messages": hold the saved shortcut and answer the lit card;
│   │   Hex's own dictation (macOS listener, Linux and Windows event log) holds
│   │   the keys, flies the live words, and lightning burns the answered card
│   ├── A short, silent, or failed try -> a hint under the field
│   ├── Listener running but still loading its model, microphone, or shortcuts
│   │   -> "Getting Hex ready to listen…" in place of the hold prompt until its
│   │   first session starts (desktop/host.rs `warming`)
│   │   // setup.warming; Check: `the_practice_page_waits_for_the_listener`
│   ├── macOS, mic kept open (General's "Turn off the mic when not dictating"
│   │   off, read on every poll, so `hex settings set` updates it) -> a note at
│   │   the window's foot, here and on Hex is ready, says why macOS shows the
│   │   mic in use  // setup.mic-note
│   └── Done at any time -> Hex is ready  // practice is never required
├── Listener cannot start after setup     // setup.relaunch; event tap refused
│   ├── First time -> Hex records a marker, reopens itself, resumes at the practice
│   └── Within 2 min of that relaunch -> "Hex's shortcut didn't start." + Reopen Hex
│       in place of the practice's hold prompt, idle pill, and hint (never a
│       second Reopen Hex in the top banner); Hex is ready keeps the banner.
│       Check: `a_shortcut_that_did_not_start_shows_only_reopen`
└── Hex is ready -> where Hex lives (menu bar, system tray, or app launcher)
    ├── macOS: "Open Hex when your Mac starts?" + Launch at login, napping in your menu bar
    ├── Windows: "Open Hex when your computer starts?" + Launch at login, napping in your system tray
    ├── Linux: "Open Hex when your computer starts?" + Launch at login, napping in the background
    │   Check: `the_login_ask_speaks_each_platforms_wording` (all three)
    ├── Try again -> back to the practice game
    └── Start using Hex -> records completion once setup is ready; closes onboarding
Run setup again (Labs) or `hex onboarding reset` -> the same story from the start,
on every build; settings, models, and permissions stay, so their pages pass
```

General > Speech model, selected model not downloaded     // settings.model-missing (#676)
├── Muted "Hex can't listen until the speech model has downloaded"
├── Downloading -> progress bar beside the model
└── No download running -> Download (the same ChooseModel); a failure shows its error

General > Speech model or Language
  -> Prepare candidate
     ├── Success -> Save selection
     └── Failure -> Preserve previous selection and show error

Permissions revoked later
  -> General > Permissions needed: Allow rows like onboarding's
     ├── Returned from System Settings still untrusted -> "Still grey? Fix it"
     └── On, waiting for a relaunch -> "Restart Hex"
```

Observed on macOS 26.6.2 and macOS 27.0 VMs (2026-10-31, SIP enabled): unprivileged
`/usr/bin/tccutil reset` from inside `anomaly.hex` clears `Accessibility`,
`ScreenCapture`, and `AudioCapture` entries for retired Hex bundles on disk with
no password prompt (`rc=64` / `OSStatus -10814` when absent from disk), and
resetting `anomaly.hex` followed by `AXIsProcessTrustedWithOptions` /
`CGRequestScreenCaptureAccess` replaces a stale `CDHash` entry that stayed grey
or untrusted in System Settings. Checks:
`tccutil_exit_codes_distinguish_cleared_absent_and_failed_bundles`,
`stale_tracker_waits_for_return_from_system_settings_and_ignores_touch_id_focus`,
`stale_accessibility_row_resets_permission_and_returns_to_waiting` in
[desktop/onboarding.rs](../../src/desktop/onboarding.rs), and
`stale_permissions_offer_fix_it_and_reset_entries_across_panes` in
[rune_ui/tests/panes.rs](../../src/rune_ui/tests/panes.rs). Scenes:
`mac-onboarding-stale`, `mac-settings-permissions-stale`.

Apple silicon, macOS 15+. Choices come from the compiled model/language catalog;
Apple Speech is currently excluded from the desktop picker.

```ts
General > Dictation > Speech model, then Language below it   // setup.language-select; every desktop root
├── Speech model -> Picker: Compare, then every model; never changes the language
│   ├── Compare -> "Speech model for Japanese": three columns from the saved
│   │   language's models, recommendations first; "{n} more models" -> every model
│   ├── Every model -> the saved language's models, Your model files, then a fold
│   │   "{n} more that don't speak Japanese"
│   ├── Model without the saved language -> Row: "Choosing it switches the
│   │   language to English"
│   └── Choose it -> ChooseModel(model, model's default: English, else its first)
│       ├── Warmed on silence without the vocabulary prompt; a warm-up that
│       │   stops at its output limit still counts as warm (same as the menu bar)
│       └── Saved -> Language row: "<Model> doesn't support Japanese, so the
│           language changed to English" until the next choice
├── Language -> Select of the saved model's languages (alphabetical by name,
│   Auto first when it detects, including a model that takes no language
│   input); one language -> the field shows it, disabled
│   ├── Open the language menu -> It scrolls to the chosen language so its
│   │   check is in view (`the_language_menu_opens_on_its_chosen_language`)
│   └── Choose -> ChooseModel(same model, language): the same preparation as a
│       model choice, so per-language assets (Apple Speech locales) install
└── The saved TranscriptionSelection is always a pair the model supports
```

Sources: [shared picker](../../src/rune_ui/picker.rs) (`choose`,
`choose_language`, `language_row`) and `picker_models`, `language_choices`,
`language_for` in [transcription/models.rs](../../src/transcription/models.rs).
Checks: `the_picker_lists_every_model_with_the_language_s_recommendations_first`,
`a_model_switch_keeps_a_supported_language_or_falls_back_to_the_model_s_default`,
`each_model_offers_exactly_the_languages_it_supports`, and the Rune tests
`a_model_without_the_saved_language_falls_back_to_its_own_visibly`,
`browsing_the_picker_never_changes_the_language`,
`changing_only_the_language_prepares_the_same_model_again`, and
`settings_opens_the_picker_and_names_the_model_and_language_apart`. Scenes:
`picker-japanese`, `picker-japanese-all`, `language-fallback`, `language-menu`. Fixture renders and
recorded requests only; a live model switch with a fallback has not been
observed. Apple Speech stays excluded, so its per-locale install through the
Language row is unexercised.

```ts
Choose a supported language              // setup.language-conditioning; macOS + Linux picker
├── Parakeet v3, Orukeet, Fluid Speech Pro, and Parakeet Ultra -> Detect
│   language from audio; also offered under Auto; the selected language does
│   not constrain output
├── Granite Speech 5.0 -> No language input; transcribes whichever of en, fr,
│   de, es, pt, it, ja it hears, so it is offered under Auto too (#1252)
├── Whisper -> Use selected language to guide decoding; accuracy is not guaranteed
├── Whisper (ivrit.ai's, NB-Whisper's, and KB-Whisper's too), Qwen3-ASR 0.6B, QwenCleo-ASR 1.7B -> Vocabulary ->
│   context prompt (vocabulary.md); output that only repeats it is dropped,
│   and its listed "sounds like" spellings are corrected whole-word after
│   the run
├── Hebrew -> ivrit.ai Whisper large-v3-turbo, always with the `he` hint (it
│   lost Whisper's language detection); stock Whisper turbo also offers it
├── Russian -> GigaAM v3 (Russian only, no language input); Parakeet v3 and
│   Whisper turbo also offer it
├── Norwegian -> NB-Whisper Medium, always with the `no` hint (Bokmål is all
│   Hex measured); stock Whisper turbo also offers it (#1150)
├── Swedish -> KB-Whisper Medium, always with the `sv` hint; its 6.6% WER on
│   FLEURS Swedish is the model card's figure for its default checkpoint, not
│   Hex's run; Parakeet v3 is Fastest there, and stock Whisper turbo also
│   offers it (#1991)
├── Bangla -> Whisper only, with the `bn` hint; Auto lets Whisper guess per clip
├── Khmer -> Whisper only, with the `km` hint; Auto lets Whisper guess per clip
├── Azerbaijani -> Whisper only, with the `az` hint; Auto lets Whisper guess per clip
└── Catalan -> Whisper only, with the `ca` hint; Auto lets Whisper guess per clip
```

Whisper and Qwen3-ASR (including QwenCleo) read the shared
[Vocabulary](vocabulary.md) as their context prompt; recognition hints, a
separate per-model box, merged into it ([#129](https://github.com/anomalyco/hex/issues/129)).
Observed on apollo (Vulkan, 2026-10-31, LibriSpeech test-clean), when they were
still hints: with Qwen3-ASR 0.6B, "Kaffar, Rodolfo, Bragelonne, …" fixed Caffer,
Rudolpho, Redolfo, Rudolph, and Bragolon in 10 clips without changing the others
checked. Both families, on silence or noise, often return the prompt verbatim
(Qwen3-ASR 0.6B did on 3 s of silence and of noise). A segment whose text
equals the prompt after normalization (case, ASCII punctuation, spelled-out
digits; commas, semicolons and line breaks separate words) is removed, in
dictation, meetings, and the local API alike. The filter needs at least two
terms, so dictating the one listed name still pastes it; speech that contains
a term among other words is kept, and so is a repeat of only some terms.
Checks: `prompt_echo_is_dropped`, `speech_containing_the_hints_is_kept`,
`only_the_echo_segment_is_removed`, `single_hint_dictation_is_kept`, and
`hints_without_spaces_after_commas_are_matched` in
[transcription/gguf_session.rs](../../src/transcription/gguf_session.rs).

Models trained on subtitled video can end a clip with a subtitle credit nobody
said, such as "Sous-titrage Société Radio-Canada" from Cohere Transcribe in
French after a trailing pause (#1141), "İzlediğiniz için teşekkür ederim" and
"Altyazı M.K." from Whisper in Turkish (#1770), or "Продолжение следует",
"Субтитры сделал DimaTorzok" ("делал" too), and "Редактор субтитров
А.Семкин Корректор А.Егорова" from Whisper large-v3 and large-v3-turbo in
Russian. Every GGUF run and live stream drops a known credit only when it is
the very end of the text (letters and digits compared, case and punctuation
ignored, Unicode lowercasing so Cyrillic case folds and Turkish dotted and
dotless `i` fold together), with its segment and words, in
dictation, meetings, and the local API alike; the same words elsewhere stay.
A credit spoken mid-chunk and followed by more speech is kept. A dictation that
genuinely ends with a listed phrase, such as the Turkish thanks, loses it: the
list cannot tell it from the invention. Folding drops the combining marks in
U+0300–U+036F, not only the Turkish dot: a decomposed `é` folds to `e`, while a
precomposed one is unchanged. The list is in
[transcription/subtitle_credits.rs](../../src/transcription/subtitle_credits.rs).
Checks: `a_trailing_subtitle_credit_is_removed`,
`a_turkish_thanks_for_watching_is_removed`,
`a_truncated_turkish_credit_is_removed`,
`a_russian_continues_credit_is_removed`,
`a_russian_subtitle_credit_is_removed`,
`russian_credits_later_in_the_speech_are_kept`,
`the_same_words_mid_sentence_are_kept`,
`a_combining_mark_inside_a_word_does_not_split_it`,
`speech_without_a_trailing_credit_is_kept`, and
`a_trailing_subtitle_credit_is_dropped_from_text_segments_and_words`. No
inference reproduces the credit.

```ts
Settings picker > Compare                 // setup.model-card; shared by every desktop root
├── Column head -> Role (Recommended, Fewest wrong words, Fastest, Most languages,
│   Reads your vocabulary; from the catalog for the saved language), name, one-line blurb
├── Rows -> Wait when you stop (30 s ÷ realtime; "None" when it streams), Wrong
│   words or characters "1 in N" (benchmark in the saved language, else "Not
│   measured"; row hidden when none is measured), Languages, Reads your vocabulary,
│   Download size; one strictly best value is medium weight, ties get none
├── Action cell -> exactly one: In use dot, progress with Cancel, Use, or
│   Download <size>; Primary only on the recommended column when it can be chosen
├── The model in use or preparing replaces the last column when it would be left out
└── "{n} more models" -> Every model

Settings picker > Every model row          // fixed height; scrolls
├── Name, role, blurb, then "No wait · 1 in 50 words wrong · 25 languages · 2.5 GB"
├── Saved language unsupported -> Warning line naming the language it switches to
├── Trailing edge -> In use dot; progress with Cancel; else Use/Download; Delete when downloaded
├── Labs > Model test details (off) -> raw error rate "7.57% CER", a wrapping line:
│   language · dataset/split · source · conditions or "unknown" ·
│   speed qualified; "Most accurate" only between comparable records (labs.md)
├── Labs > Parakeet Ultra (off) -> its row with a Labs tag and Details: languages,  // setup.parakeet-ultra
│   measured memory, the CC BY 4.0 credit, notice, Source/License links, folded
│   published benchmarks "not measured in Hex" (labs.md)
├── Orukeet -> ordinary row with Details: languages, the CC BY-SA 4.0 credit  // setup.orukeet
│   and notice, Source/License links, folded published FLEURS benchmarks
│   "not measured in Hex"; never recommended or badged. Checks:
│   `orukeet_is_offered_for_exactly_parakeet_v3s_languages_and_never_badged`,
│   `orukeets_details_credit_its_base_and_cover_its_languages_once`
├── Fluid Speech Pro -> ordinary row with Details: languages, the CC BY 4.0  // setup.fluid-speech-pro
│   credit and notice, Source/License links, folded published benchmarks "not
│   measured in Hex": the card's FLEURS figures for 24 of its 25 languages
│   (greedy decoding, the reference NeMo model; no English figure); never
│   recommended or badged. Checks:
│   `fluid_speech_pro_is_offered_for_exactly_parakeet_v3s_languages_and_never_badged`,
│   `fluid_speech_pros_details_credit_its_bases_and_cover_its_languages_once`
├── "Add model file…" in the header -> imported cards after the catalog:  // setup.custom-models
│   Imported · family · languages · Not benchmarked · file · licence · SHA-256
│   prefix; Rename, Remove (not the active
│   model); missing or changed -> warning line and no Use (own-models.md)
└── Found on this computer -> a scan of the Hugging Face cache when the picker  // setup.found-models
    opens: other compatible files at the top (Not tested by Hex, Use); a catalog
    model already downloaded there says so and offers Use; a found file's
    Delete is Forget (own-models.md)
```

Parakeet Unified English (chunked attention) and Nemotron 3.5 ASR (cache-aware)
are the models marked `streams`
(`only_the_streaming_parakeet_artifacts_are_marked_streaming`). The mark describes
the artifact; dictation still streams only after the load probe succeeds on a GPU
device ([Streaming Finish](dictation.md#streaming-finish)). The menu bar lists
names only and shows no tags. Scenes: `hex app -- --snap OUT --scene picker`
(also `picker-downloading`, `picker-japanese`, `picker-japanese-all`,
`picker-custom-models`, `picker-found`).

Nemotron 3.5 ASR (#834; NVIDIA's `nemotron-3.5-asr-streaming-0.6b`, OpenMDW-1.1)
streams in 27 of Hex's languages and detects the language with Auto: Hex sends
its language as the locale its prompt takes (`de` -> `de-DE`,
`runtime_language`), and Auto sends none. It is not anyone's recommendation:
Hex's own runs of transcribe.cpp's WER harness on apollo (Vulkan, 7045d1c) put
it behind Parakeet v3 on every set measured: LibriSpeech test-clean 3.05%
against 1.95%, FLEURS English 7.91% against 4.88%, and FLEURS German 10.39%
against 5.17% (8.77% and 10.46% with Auto). It is there to stream with the
language set, or across English and German in one model. Hex converted the
pinned `.nemo` itself; the shipped file is handy-computer's, whose 653 tensors
match that conversion byte for byte (`NEMOTRON_3_5_ASR_ARTIFACT`). Checks:
`nemotron_takes_its_languages_as_locales_and_detects_with_auto`. Not observed:
macOS (Metal), Windows, and Japanese, Korean, Chinese, or Vietnamese audio.

Parakeet v3 remains available for Portuguese and its other existing language
choices. Its card explains the limitation and suggests Whisper for
language-guided transcription. It is also offered under Auto: it detects the
language from the audio and takes no hint, as do Orukeet, Fluid Speech Pro,
Parakeet Ultra, and Granite Speech 5.0. A hint-taking model without
`stt.capability.lang_detect` stays out of Auto. Multilingual audio support is
not language conditioning:
the pinned v3 GGUF has no `stt.parakeet.prompt.num_prompts` metadata, which gates
language prompting in `transcribe-cpp-sys` 0.1.3.

Sources: [model catalog](../../src/transcription/models.rs),
[shared picker](../../src/rune_ui/picker.rs), and the runtime
options in [parakeet.rs](../../src/parakeet.rs) and
[transcription/local_transcriber.rs](../../src/transcription/local_transcriber.rs). The regression
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

Bangla (`bn`) is offered because Whisper large-v3-turbo was trained on it; no
other catalog model supports it, so Whisper is its only, recommended choice and a
Mac whose first preferred language is Bangla starts on Whisper. Auto is not a
substitute: in a manual test, Whisper's per-clip detection labeled Bangla speech
as Arabic, Turkish, and Spanish. `bangla_is_transcribed_by_whisper_with_its_language_hint`
checks the recommendation, the `bn` hint, and that Qwen3-ASR rejects it. It
runs no inference, and Bangla accuracy has not been measured.

Khmer (`km`) is offered because Whisper large-v3-turbo was trained on it and no
other available catalog model supports it, so Whisper is its only, recommended
choice and a system whose first preferred language is Khmer starts on Whisper.
`khmer_is_transcribed_by_whisper_with_its_language_hint` checks the
recommendation, the `km` hint, that Whisper's picker offers it, and that
Qwen3-ASR rejects it. It runs no inference, and Khmer accuracy has not been
measured.

Azerbaijani (`az`) is offered because Whisper large-v3-turbo was trained on it
and no other available catalog model supports it, so Whisper is its only,
recommended choice and a system whose first preferred language is Azerbaijani
starts on Whisper. `azerbaijani_is_transcribed_by_whisper_with_its_language_hint`
checks the recommendation, the `az` hint, that Whisper's picker offers it, and
that Qwen3-ASR rejects it. It runs no inference, and Azerbaijani accuracy has
not been measured.

Catalan (`ca`) is offered because Whisper large-v3-turbo was trained on it and
no other available catalog model supports it, so Whisper is its only,
recommended choice and a system whose first preferred language is Catalan
starts on Whisper. `catalan_is_transcribed_by_whisper_with_its_language_hint`
checks the recommendation, the `ca` hint, that Whisper's picker offers it, and
that Qwen3-ASR rejects it. It runs no inference, and Catalan accuracy has not
been measured.

Checks start in [desktop/onboarding.rs](../../src/desktop/onboarding.rs),
[transcription.rs](../../src/transcription.rs), and
[transcription/models.rs](../../src/transcription/models.rs). Setup/picker previews
prove rendering, not clean-account permission grants or model preparation.

The model step's list and its rules are in
[own-models.md](own-models.md#onboarding-choice) (scenes
`onboarding-model-found`, `-downloading`, `-recommended`, `-back`).

The tour state is the pure `desktop::onboarding::Flow` (steps, row states,
background download, practice outcomes, one-relaunch guard), fed by
[rune_ui/onboarding.rs](../../src/rune_ui/onboarding.rs); what the person sees
is the story in [rune_ui/onboarding_kit](../../src/rune_ui/onboarding_kit.rs),
opened at launch by `Ui::new` and replayed by Run setup again. The guide
beside System Settings is [permission_guide.m](../../native/permission_guide.m):
it reads the Screen Recording grant from TCC, because the CoreGraphics
preflight keeps the answer it read at launch, closes that guide once the
watched read says granted or needs a restart, and stays out of the way while
another app is in front of System Settings.
On Windows the Microphone row shows only while the microphone switch in
Settings › Privacy & security is off (device, user, or desktop apps;
`CapabilityAccessManager\ConsentStore\microphone` says `Deny`); setup
finishes once it is on (`a_switched_off_windows_microphone_adds_its_row_until_setup_ends`,
`windows_onboarding_asks_only_for_the_microphone_switch`; the registry read is
cross-compiled only, not run on a Windows PC).
Unit tests (`desktop::onboarding::tests`) cover step order, the download starting on the
welcome screen and retrying from any step, the auto-advance beat, live row
flips, a refused microphone, practice outcomes, resume, the relaunch guard and
resume marker, resuming past a setup the relaunch finished
(`resuming_at_setup_after_a_relaunch_passes_grants_that_are_now_real`),
grants waiting for a restart
(`a_grant_the_process_cannot_use_yet_waits_for_a_restart`,
`setup_offers_a_restart_once_every_permission_is_on`), an Accessibility grant
advancing setup without one (`an_accessibility_grant_applies_without_a_restart`),
the saved step (`the_tour_step_is_saved_for_the_next_launch`), and the upgrade
check (`a_fresh_install_onboards_without_the_identity_check`,
`an_upgrader_without_permissions_gets_the_tour_once`,
`an_upgrader_whose_setup_is_ready_skips_the_tour`; a real 2.3.1 to
`anomaly.hex` upgrade on a Mac is not yet observed). Rune tests drive the
first run (`a_new_install_opens_the_first_run`,
`the_mac_first_run_asks_each_permission_then_practices`,
`the_practice_game_follows_hex_s_own_dictation_on_every_os`,
`a_listener_that_will_not_start_relaunches_once_then_asks_to_reopen`,
`a_refused_microphone_points_to_system_settings`,
`a_grant_waiting_for_a_restart_makes_setup_restart_hex`,
`onboarding_downloads_the_model_through_the_service`,
`a_found_model_moves_on_without_a_download_and_back_returns_to_it`), and every
page is a scene whose goldens `scripts/check-onboarding.sh` checks (its
`SCENES` list in `rune_ui/tests/onboarding_check.rs`). Scenes are fixtures
played on the story's own clock to one settled moment, not native proof.
The default model comes from `desktop::i18n::preferred_language` (macOS:
`NSLocale.preferredLanguages`) through `recommended_selection`; existing
settings keep their saved model.
Observed on Linux (apollo, 2026-11-01): a clean first run in a nested
session, from Setting up Hex through the real download to Hex is ready. Not
yet observed: a clean-account Mac run with real grants, a real practice
answer on macOS or Windows, or a real self-relaunch after an event-tap
refusal. The story's demo cards and the game's messages stay in English in
every interface language; its headings, buttons, and hints are translated.

```ts
Menu bar > Speech Model                   // setup.quick-model-switch; macOS
├── Downloaded models -> Last selected language shown beside each model
├── Choose -> Verify installed artifact -> Prewarm -> Save selection
│   ├── Prewarm runs silence without the vocabulary prompt, so a term list
│   │   can't be read back to Qwen3-ASR's token cap
│   ├── Warm-up still stops at its output limit (Qwen3-ASR 1.7B) -> Counts as
│   │   warm; saved (same as Linux and Windows, and as the Settings picker)
│   │   // gguf_session::RunKind; checks:
│   │   // a_warmup_leaves_the_vocabulary_out_and_counts_when_truncated,
│   │   // a_decode_stopped_at_its_output_limit_is_recognised_through_its_context
│   │   // Proof limit: the classifier test covers only error recognition and
│   │   // the warm-up test only the option choice; accepting a Mac prewarm has
│   │   // no automated check, and no Mac run was observed.
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

[transcription/preparation.rs](../../src/transcription/preparation.rs) is shared
by Settings and the menu bar: one worker plus one replaceable pending choice.
Closing Settings does not abandon preparation. A superseded/cancelled completion
cannot save, and delivered menu controls are drained before accepting completions.
The desktop root commits through an existing editor to retain unrelated unsaved
settings, or through persisted settings when no window exists.

Sources: [macos_rune/status_item.rs](../../src/macos_rune/status_item.rs) and
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

**Observed September 4, 2026 (2.1.16), existing model picker:** all 455 Rust tests
passed in debug and release, with ten opt-in tests skipped per profile. All twelve
keyboard-layout child scenarios passed per profile; formatting, strict
all-target/all-feature Clippy in both profiles, and the release build passed.
The Cohere fixture inference check was run separately; see
[dictation model windows](dictation.md#ongoing-jobs-and-output). Native menu
interaction, installed-app switching, and Linux runtime behavior were not exercised.

### Switch models with keyboard language

```ts
Labs > Switch model with keyboard language      // setup.input-source-model; macOS, Windows, Linux X11; off by default; General shows the mappings while on
├── Each enabled keyboard input source -> Don't switch, or a downloaded model and language
│   (alphabetical by name, Auto first)
│   ├── Open the language menu -> It scrolls to the chosen language so its
│   │   check is in view (`a_modes_language_menu_opens_on_its_chosen_language`)
│   ├── New mapping -> The source's own language when the model has it
│   ├── One-language model -> Its language shown fixed
│   └── New/unmapped source -> Don't switch; keep the current model
├── Source changes (shortcut, Input menu, automatic app/document switch)
│   └── 300 ms after the last change -> Existing installed-only picker preparation
│       ├── Same model and language already selected -> Nothing loads
│       ├── Success -> Save the selection, keeping the model's remembered hints
│       └── Failure -> Existing picker error; no download or guessed fallback
├── Manual model choice -> Supersede pending automatic work; do not reapply at recording start
├── Enable -> Apply the current source's mapping once it is observed
├── Edit the active mapping -> Apply now
├── Disable -> Cancel automatic work; stop observing; leave the selected model alone
└── Relaunch -> Observe current source without applying it; preserve the manual choice
```

Mappings are keyed by native input-source IDs (macOS input-source IDs, Windows
keyboard layout handles, X11 XKB layouts such as `de(neo)`), not inferred speech languages or
physical keyboard keys, and store a model with a language it supports. Only
downloaded models from the picker are offered (Apple Speech is never mapped);
downloads stay in the picker. A mapped model that is no longer downloaded is
labeled Not installed and can be cleared; a mapping to a model or language Hex no
longer offers is dropped when settings load. Removing a source hides its row
without erasing its saved mapping.

Hex reads input sources only while the setting is on: the Windows runtime's poll
of the keyboard-focused window's layout, which ignores Hex's own windows, and
the Linux host's XKB connection (polled every 250 ms) exist only then, and the
macOS main-thread TIS distributed-notification observer, which always runs to
rebuild the keyboard layout snapshot
([recovery](recovery.md#keyboard-layout-resolution)), reports the selected
source only then; all work without a Settings window, and the window lists
enabled sources only then. Windows lists the language bar's layouts, named by
language and keyboard. On Linux the row appears only in X11 sessions: Wayland
compositors keep the active layout to themselves, with no general protocol
(IBus and Fcitx report input-method engines, not the compositor's layout), so
Wayland is unavailable. Source changes cancel obsolete automatic work but keep
manual preparation until a mapped choice is due. An unmapped source cancels only
automatic work. This reuses the shared model selection, recording/stream
fallback, and meeting-engine behavior; it adds no recording-time routing, model
cache, or notifications.

Sources: observers for [macOS](../../src/keyboard_input_sources/macos.rs),
[Windows](../../src/keyboard_input_sources/windows.rs), and
[X11](../../src/keyboard_input_sources/x11.rs),
[switch policy](../../src/transcription/keyboard_model_switching.rs),
runtimes ([macOS](../../src/macos_rune.rs),
[Windows](../../src/windows/app.rs), [Linux](../../src/linux/app.rs)), and
[settings UI](../../src/rune_ui/settings.rs).
Checks: the switch policy and settings/CLI validation tests, the window-source
decision tests (Hex's own windows and a missing layout name no source), the
runtime scheduling tests in [macos_rune/tests.rs](../../src/macos_rune/tests.rs)
(macOS), the settings interaction tests in
[keyboard_model_tests.rs](../../src/rune_ui/keyboard_model_tests.rs), and the
`mac-input-source-models` golden, and the XKB symbols-name tests in the X11
observer. No check delivers native input-source notifications or switches
sources through the Input menu or a shortcut.

**Observed October 30, 2026, X11 observer:** on a headless Xvnc with
`setxkbmap -layout us,ru,de -variant ,,neo`, Hex listed English (US), Russian,
and German (Neo 2) as `us`, `ru`, `de(neo)` with languages en, ru, de, and
reported `us`, then `ru` after an XKB group lock. The Windows observer and both
new runtime paths compile (x64 clippy) but were not run on Windows or a live X11
desktop.

### Delete downloaded models

```ts
Settings picker > Every model > Downloaded row   // setup.delete-model; macOS
├── Delete -> "Delete <size>?" -> Delete again
│   ├── Success -> Model file, receipt, and partial download removed; row offers Download
│   └── Download in progress -> Nothing removed; "Model could not be deleted." with reason
├── Selected dictation model (any language) -> No Delete
├── Preparing or Apple Speech model -> No Delete
├── Model choice or closing the picker -> Confirmation cleared
└── Labs > Downloaded models only (off) -> "Downloaded models take <size>" (Hex's own   // labs.picker-downloaded
    files) + "Only downloaded models", which leaves just the downloaded rows, each
    with its Delete; reset when the picker opens (labs.md, #1018)
```

Deletion takes the shared model-directory download lock without waiting, so it
cannot race a Settings, menu-bar, or local API download. The menu bar drops a
deleted model because it reads installation state when it builds the menu. Linux
Settings does not offer deletion: the service owns model files and has no delete
request yet.

Sources: [shared picker](../../src/rune_ui/picker.rs) and `remove_installed`
in [transcription/models.rs](../../src/transcription/models.rs). The check is
`removing_a_model_deletes_its_artifacts_unless_a_download_is_active` (real files
and lock); no Rune test covers the picker's two-step confirmation yet.

**Known gap:** `is_installed` checks size, not checksum. Not every native-load
path requires verification. Picker Active means selected/installed, not live
worker readiness; a later worker-load failure does not roll back saved selection.

## Dictate

```ts
Saved shortcut                           // dictation; Option by default; Windows and X11 accept mouse buttons, macOS with Labs > Mouse shortcuts
├── Hold -> Speak -> Release
├── Tap, tap -> Speak hands-free -> Press again   // Shortcut mode: Hold or double-tap
├── Tap mode (Toggle) -> Cancel shortcut, Escape by default, while recording
│   -> Discarded: nothing pasted or saved; transcribing jobs finish (#1022)
├── Hold modes -> no cancel: Escape reaches the app, every dictation pastes (#786)
└── macOS recorder: a key alone needs a modifier, except F1–F20 and Home, End,
    Page Up, Page Down, Help (#1138; keys_without_modifiers_need_one_except_function_keys),
    and the microphone (dictation) key with Labs › Microphone key alone on (#181)

General > Shortcut mode -> Hold or double-tap (default), Hold, Tap, Double-tap
  -> Modal: each choice with its keys and a looping key/recording timeline
  -> Double-tap needs a key binding; with a shortcut that has no key (one or
     more modifiers) it says so, unless Labs > Double-tap Option, Command, or Globe
     is on (macOS)

Dictate without processing (Output pane; unbound)   // dictation.md
  -> records and pastes as heard, skipping Output; History kind Raw
  -> a shortcut on the dictation chord is cleared when settings load or save

Labs > Second dictation shortcut (off)   // labs.md; issue 627
  -> General shows "Second dictation shortcut" under Shortcut (unbound, clearable)
  -> it dictates like the shortcut: Hold/Toggle, double-tap lock, release
  -> the shortcut that starts a capture finishes it; the other stands down meanwhile
  -> Linux: an X11 grab or the shortcuts portal's "Second dictation shortcut"
     entry while on and bound; needs a key, as every Linux shortcut does
  -> Windows: the hook's second trigger, with the same rules; needs a key
     (Windows registers none without one)

Labs > More dictation shortcuts (macOS, off)   // labs.md
  -> General shows a numbered row per extra (recorder, shortcut mode, Remove) and Add shortcut under four
  -> each row dictates like the shortcut in its own mode; Double-tap also takes a lone modifier while Labs > Double-tap Option, Command, or Globe is on
  -> Escape cancels once it's locked, in Tap, or on a bare key or mouse button
  -> off, the saved rows stay and nothing matches; on, each needs its own keys

Labs > Terminal paste shortcut (Windows, Linux; off)   // labs.md
  -> General shows "Terminal paste shortcut" under Shortcut (unbound, clearable)
  -> it dictates like the shortcut and always pastes with Ctrl+Shift+V
  -> Paste last dictation keeps the global Terminal paste setting

Labs > Caps Lock dictation (macOS, off)   // labs.md
  -> a Caps Lock press starts a hands-free capture; the next press finishes it
  -> needs both the HID Caps Lock key and its AlphaShift edge; remapped to
     Control or Escape changes no AlphaShift, so it never toggles dictation
  -> Caps Lock stays off so typing is unchanged; its light is solid while any
     capture records (normal-shortcut and Annotate too), blinks while Hex
     transcribes or processes, off when done
  -> Escape cancels in any mode; in Toggle mode only Escape cancels a Caps
     Lock capture; the normal shortcut stands down while one runs; off or
     quit clears the light and restores Caps Lock

Labs > Free memory when idle (off; 5/15/30/60 min, default 15)   // labs.md; transcription/engine
  -> nothing used the model that long and nothing records -> the model unloads
  -> next press, meeting chunk, or API request -> reloads first; audio waits, nothing lost
```

The [detailed map](dictation.md) covers timing, successive captures, output
commitment, named checks, and native-proof gaps.
The macOS CLI entry is `hex listen`, with an optional authoritative `--device`.
Do not start it beside an existing listener.

## Use The Result

```ts
Completed output                         // output
├── Paste -> Current focus, in submission order
│   ├── Back-to-back dictations with no key, click, or window change between
│   │   -> joined with a space and sentence-aware capitalization (macOS, Windows;
│   │      Linux on Wayland where the compositor reports input, dictation.md)
│   ├── Previous clipboard restored 500 ms later, unless something newer replaced it
│   ├── Windows -> the transcript is marked out of clipboard history (Win+V) and cloud sync (#851)
│   ├── Windows -> a paste only goes to a window that can take it: the desktop
│   │   and taskbars keep the words on the clipboard and report "copied" (dictation.md)
│   ├── Labs > Keep dictation on clipboard (every OS, off) -> the dictation stays on the clipboard (labs.md)
│   │   └── Windows -> kept as an ordinary entry, in history too
│   └── Labs > Copy dictation only (macOS, Windows, off) -> clipboard only: no paste, typing, or restore (labs.md)
├── Paste Last Dictation                  // menu or Option-Shift-V / saved binding
│   ├── Last completed dictation
│   ├── A failed paste still keeps its text (every OS, unless cancelled; dictation.md)
│   ├── macOS menu item shows the saved binding (⌥⇧V by default); unbound,
│   │   modifier-only, or a mouse button -> no shortcut shown
│   └── A saved binding on the dictation chord is cleared (logged) when
│       settings load or save
└── History
    ├── Inspect / search / copy (row hover or the card) / Delete… (asks once)
    ├── Retention ("Kept 7 days ▾" menu) -> Seven days by default, always hard-capped
    ├── Off ("History off") -> Stop new entries; preserve existing entries
    ├── Same menu > Clear all history… -> asks in the header; only the red confirm clears
    ├── Select dictations (every OS, no setup)   // history.selection
    │   ├── Plain click, Shift-click on the visible (searched) rows, Cmd-click (Ctrl), or a drag -> one, a range, or a toggled set
    │   ├── Adjacent selected rows share one highlight; an unselected search result separates two highlights
    │   ├── Two or more selected -> "{count} dictations selected": Copy (final texts, oldest first, a blank line between each) / Delete… (asks once)
    │   ├── One selected -> the ordinary card, Copy, Delete… (asks once, then Cancel), Transcribe again with Keep recordings
    │   └── A search, new entries, or a delete prune the selection
    ├── Labs > Keep recordings (off) -> new entries keep their 16 kHz audio   // labs.keep-recordings
    │   -> also "Keep audio too" in the retention menu while the lab is offered
    │   -> the card's "Transcribe again" menu runs it through any installed model
    │   -> the new text with Copy and Replace; Clear all and retention delete the audio (labs.md, #1032)
    │   -> Replace keeps the old text as one of 5 earlier versions: Copy, Use this version (#1177)
    ├── Labs > Copy dictation audio (off) -> copies a kept WAV as a file   // labs.copy-dictation-audio
    │   -> the card's Copy audio beside Play, and the menu bar or tray's Copy last dictation audio
    │   -> needs Keep recordings; it stores no audio itself (labs.md)
    ├── Labs > Dictation stats (off) -> History's Stats dialog and hex history stats   // labs.history-stats
    │   -> headline tiles, time saved with a typing-speed slider, the busiest hour, a 30-day chart, top apps
    │   -> a 53-week heatmap and all-time totals, from a daily rollup in usage.json with no text or app names
    │   -> Clear all history deletes the rollup too (labs.md)
    └── Labs > Stats on History (off) -> one compact strip of those six headline numbers above the list   // labs.history-stats-strip
        -> needs Dictation stats and its own switch; off, History is exactly as before (labs.md)
Every entry since #1177 -> "Speech model" fact: the model that made its text (+ language when chosen)
```

Checks in [paste.rs](../../src/paste.rs), [parakeet.rs](../../src/parakeet.rs), and
[history.rs](../../src/history.rs) cover output and retention boundaries.
They do not prove that an arbitrary target consumed its clipboard contents.
Send is not an atomic target-app transaction. Chromium 155 and 156 get the text
typed instead of pasted for a [Chrome paste bug](dictation.md#ongoing-jobs-and-output).

**Easy to misread:** History Off does not stop diagnostic logging. Diagnostic
logs are separate. Each is capped at 10 MB and trimmed in place to its newest
5 MB at launch and hourly (`desktop::log_trim`), and they never hold what was
said: `live.ndjson` records each dictation's phase and timing with its text
left out (`events::VoiceEvent::without_words`,
`the_log_never_keeps_what_was_said`), so History is the only place dictated
words are kept. They are readable
only by their owner on macOS and Linux: Hex creates and tightens the logs folder
to 0700 and `live.ndjson` and `process.log` to 0600 (`app_paths::create_private_dir`,
`open_private_append`; `the_logs_folder_is_private`, `the_log_is_private`;
tightening is best-effort, never fatal), so other
accounts can't read them through a world-readable home folder.


The shared `EventReader` streams a file-length snapshot in 16 KiB chunks rather
than copying the entire log into memory. It retains up to 1,024 event rows and
the latest context, even when that context predates the retained rows.
Diagnostic records over 1 MiB are skipped through their terminating newline;
the original log and dictation output remain intact. Partial UTF-8 records carry
over to the next refresh, and truncation or file replacement resets the reader.

Checks: the `reader_*` regressions in [desktop/events.rs](../../src/desktop/events.rs). These
exercise synthetic files, not an installed app. The GPUI window's
on-demand-loading regression went with GPUI; when the Rune window reads the
log is not covered by a test. A first Commands/Activity refresh still scans the
full log snapshot; disk rotation and background parsing remain separate work.

Observed September 26, 2026: the reader and pane-loading regressions passed.
An optimized isolated harness using the production reader and a synthetic
64 MiB log reduced peak process RSS from about 204 MB to 2.4 MB; refresh took
254 ms before and 222 ms after. This is reader evidence, not a native Settings
latency measurement.

## Find a setting or lab

```ts
Focused Hex window → ⌘K (Ctrl+K off macOS; ⌘F / Ctrl+F also works on General)
├── Type a title, description or breadcrumb → one list of settings and Labs cards
├── ↑ / ↓, then Return, or click → reveal and focus the control; no value changes
│   ├── General › Advanced → unfold in this window only
│   └── Labs → show the destination’s group and outline its card, without opting in
└── Escape or press outside → close and restore focus
```

The [destination list](../../src/rune_ui/find_popover/catalog.rs) covers the
ordinary General controls (including shortcut bindings, microphone, speech
model and language, sound, HUD and application preferences), Output’s raw
dictation shortcut, and every available unlocked Labs card, including Annotate.
It does not index values, history, transcripts, files, commands, model choices,
per-device mappings, per-app rules, inline lab controls, diagnostic or
permission notices, or the contents of other panes. A focused editor, shortcut
recorder, onboarding or another modal keeps its keys. Labs keeps its own ⌘F.

## Adjust Behavior

```ts
Sidebar                                  // settings.nav
├── General, Output, Modes (Labs › Modes editor, while on), History, then each
│   feature that is on: Captures (Labs › Screenshots, macOS), Meetings and
│   Voice Filter (Labs toggles), Annotate, Playground (Labs, an item)
├── Labs -> a store of unfinished features, each off by default: search, categories, cards with their own settings inline; hex://labs/<feature> opens its group and scrolls to its card (labs.md)
└── What’s new -> recent features and their controls; Down from Labs selects it
    // Version, update actions, and Quit (Linux and Windows) are outside the navigation list
    // Command-1…9 follow the main panes' order (labs::order)

General                                  // settings
├── Basics card -> Shortcut, Speech model, Language, Microphone, Launch at login
│   ├── Each row leads with an icon; choices are inline dropdowns, not segments
│   ├── Language -> fixed text for a one-language model, else a dropdown
│   └── A row that hides or shows (Advanced, a microphone error)
│       -> Folds under a clip as the rows below slide over it; no fade in place
├── Microphone -> Value names the input in use, with live level bars // settings.microphone
│   ├── "System default · Kit Pods", or a saved device
│   ├── Level bars -> peak of the audio Hex receives, ~30 fps while General shows them above rest // input_level
│   │   ├── Peak measured around each chunk's mean, so a microphone's DC offset never lights them
│   │   ├── Read from one atomic the audio owner stores per chunk; never opens or blocks the mic
│   │   ├── Quiet room (bars at rest) -> no 30 fps frames; the window's 250 ms tick notices a rise.
│   │   │   An open General window with a warm mic woke 34.5 times a second in a quiet room before
│   │   │   (dev-364, headless sway on apollo, Oct 31) and 4.1 after; macOS was reasoned about only
│   │   ├── No audio for 250 ms (released while idle, failed, not allowed) -> flat, dimmed
│   │   └── Scenes: mac-settings-mic-level (a recorded WAV, `--video`), mac-settings-mic-released
│   │       // check: the_microphone_level_follows_the_voice (Linux)
│   ├── Re-listed on a device or default change while General shows (Windows: also every 3 s)
│   ├── System default -> The OS default input, followed live
│   │   ├── Default changes -> Switch once capture is idle; new stream opens before the old drops
│   │   ├── Default is Hex Microphone -> Built-in mic, else first real input; row says so
│   │   ├── Stand-ins never include virtual or aggregate inputs (macOS: e.g. Microsoft Teams Audio)
│   │   │   ├── Nothing real left -> "lid is closed" / "no microphone" error, not silence
│   │   │   └── A virtual default, saved device, or `--device` is still used as named
│   │   └── Default is Bluetooth (Advanced switch on) -> Built-in mic, else first wired/USB input
│   │       ├── Row says "KIT PODS is Bluetooth, so Hex listens to MacBook Pro Microphone"
│   │       ├── Windows: WASAPI `BTHENUM`, or a headset/handset endpoint of unknown bus (`BTHHFENUM`)
│   │       ├── Linux: a `bluez5` default source; Hex opens ALSA's default, then moves its own
│   │       │   stream to the PCI input, else the first other hardware source (pactl, PipeWire);
│   │       │   the stream is marked so WirePlumber never remembers the move. A Bluetooth
│   │       │   default chosen while Hex's stream is open is followed until the next open
│   │       ├── No other usable input (or lid closed, Bluetooth only) -> the Bluetooth default
│   │       ├── A saved Bluetooth mic or `--device` is always used as named
│   │       └── Dictation only: meeting recording keeps the Bluetooth default, which
│   │           a call app on the headset is using (#1086;
│   │           a_meeting_keeps_the_bluetooth_default_a_call_uses)
│   ├── Saved device missing -> System default stands in, value names it
│   │   ├── Row says "SHURE MV7 isn't connected; using your Mac's input"
│   │   ├── Menu keeps "SHURE MV7" checked with a faint "not connected"
│   │   └── Device returns -> Switch back once idle; the setting never changes
│   ├── Menu lists real inputs only                                // never Hex Microphone
│   └── Open menu covers the controls beneath it; click elsewhere dismisses
├── Advanced ▸ -> one fold per pane, closed by default, even while a setting in it is in use
│   ├── Collapse -> drops a pending release confirmation
│   ├── Dictation -> Shortcut side, keyboard-language mappings (while its Labs toggle is on),
│   │   Use the Mac's microphone instead of Bluetooth (Linux, Windows: "Use another
│   │   microphone instead of Bluetooth")
│   ├── Output -> Paste last shortcut, While dictating, Dictation HUD, Terminal paste (Linux)
│   │   (Dictate without processing sits in the Output pane; see dictation.md)
│   └── Application -> Enable on launch, Show Dock icon, Sounds,
│       Interface language (System first, then alphabetical by displayed name),
│       Update channel (self-updating installs)
│       // Interface language System: macOS's first preferred language (zh-Hans -> 简体中文,
│       // zh-Hant, zh-TW, zh-HK, zh-MO -> 繁體中文), Linux LC_ALL/LC_MESSAGES/LANG
│       // (zh_TW.UTF-8 -> 繁體中文), Windows the user locale; unknown -> English
│       // Text language (rune::text::set_language): 日本語 -> ja, 简体中文 -> zh-Hans,
│       // 繁體中文 -> zh-Hant (zh-HK in Hong Kong/Macau); another interface on a
│       // ja/ko/zh system locale -> that locale. Han text falls back to its fonts
│       // (Hiragino Sans, Yu Gothic, Noto Sans CJK JP for ja) and shapes with its
│       // locl forms (#1382)
├── Use the Mac's / another microphone instead of Bluetooth (Advanced, on)
│   └── Off -> System default follows a Bluetooth default too; re-resolves once idle
├── Labs > Share dictation shortcut with other apps (Windows, off)
│   └── On -> the hook passes the shortcut's key-down/up on instead of consuming it
├── Terminal paste (Advanced on Linux, Labs on Windows; off)
│   └── On -> paste sends Ctrl+Shift+V instead of Ctrl+V; applies to the next paste
├── Labs > Terminal paste shortcut (Windows, Linux; off)
│   └── On -> General shows its recorder; a capture it starts pastes with
│       Ctrl+Shift+V whatever Terminal paste says
├── Turn off the mic when not dictating (General, under Microphone; off)   // release_microphone_while_idle
│   ├── One switch, no confirmation; its line says what the state means (issue 61), on every OS
│   ├── Off (default) -> "The mic stays ready so dictation starts instantly; your system shows it in use"
│   │   Open while idle; pre-roll available
│   │   └── macOS: released while the display sleeps, the session is locked, or the Mac
│   │       sleeps, and back on wake or unlock   // dictate.warm-away
│   │       ├── Launch reads the current display and lock state, so it starts released
│   │       ├── Ignores Labs › Keep mic open after dictating's window (up to 30 min)
│   │       └── Turning the setting off reopens it with a plain open; a press
│   │           during that reopen becomes a pending capture
│   └── On -> "The mic opens only while you dictate; the first moment can be clipped"
│       Open on press; no pre-roll; startup delay
├── While dictating -> Mute / Pause media / Lower audio / Do nothing
│   ├── Lower audio -> Lower to slider, 0–100% of the previous volume (default 20%)
│   ├── Intentional capture only, not ordinary shortcut chords
│   ├── Pause/resume only players already reported running by macOS
│   ├── Linux restore -> Waits 300 ms; a new capture keeps it suppressed // double-tap lock
│   ├── Labs > Only on speakers (every OS; off by default)
│   │   ├── On, default output is speakers -> applies as chosen
│   │   ├── On, default output is wired or Bluetooth headphones -> no mute, lower, or media pause for that recording or capture
│   │   ├── Dictation and screen captures both ask the route as each starts, on the environment's or audio-control worker thread
│   │   ├── macOS -> an overlapping session that shares an already-held effect reuses the first session's decision
│   │   ├── Anything unclassified (unknown, USB, HDMI) counts as speakers
│   │   ├── macOS -> CoreAudio transport, jack data source, and UID
│   │   ├── Linux -> pactl's default sink: active port, form factor, or Bluetooth
│   │   ├── Windows -> the default output's WASAPI form factor through cpal
│   │   ├── Idle sleep is unchanged; pre-roll follows the setting, not the route
│   │   └── Limits: Bluetooth speakers read as headphones (macOS always; Linux unless PipeWire says speaker); a wpctl-only Linux reads unknown and applies
│   └── Labs > Also pause the browser (macOS, Pause media only; off by default); its permission row shows here while on
│       ├── Turning it on without System Audio Recording -> ask now, never while dictating
│       ├── On -> Screen & System Audio Recording row: Allow / waiting / Allowed
│       └── Dictation -> browser_media sequence below
├── Sounds -> one row ("Classic keys · 50%" or Off) opening a sheet: Sound volume slider in 5% steps, then each set to hear and pick
│   ├── Slider release saves and previews start unless Off; only the sheet's previews play while Off
│   ├── A saved zero displays Off; enabling sounds through the CLI restores the default level
│   └── Default output changes or disappears (macOS) -> next tone reopens on the current default
└── Linux / Windows -> listener card and desktop commands below the basics

Pause media + Also pause the browser     // browser_media
├── A browser uses the microphone (a call) -> leave it alone
├── No browser process has its audio output open -> leave it alone (no tap)
├── Listen to those processes' own output (private process tap), 3 windows of 100 ms
│   └── Any window at or below -60 dBFS, or no level -> leave it alone
├── Send the system pause command (MediaRemote 1; never the toggle)
├── Poll 5 windows of 100 ms
│   ├── 2 windows in a row below -90 dBFS -> Hex paused it
│   └── Otherwise (another tab, a sound that ended later, or the pause went elsewhere) -> no play later
└── Dictation ends and Hex paused it -> first, before the players: listen 300 ms to every browser process
    ├── Above -60 dBFS (you started it again) -> no play
    ├── No level, a browser runs and every one has closed its output -> still paused: play
    │   (a pause longer than about 10 s closes the output; a tap on it delivers nothing)
    ├── No level otherwise (browser quit, tap failed) -> no play
    └── Otherwise -> send the system play command (MediaRemote 0), then poll 20 windows of 100 ms
        ├── A window above -60 dBFS -> resumed
        ├── No level before the first level (the output is opening again) -> keep polling
        ├── No level after a level -> stop listening
        ├── No browser output open after 10 windows (tab closed: the play reached another app) -> send pause once
        ├── Never a level and a browser opened its output -> stop listening
        └── Silent throughout (tab closed or reloaded: the play reached another app) -> send pause once

Intentional recording                    // recording.environment, idle_sleep
├── Prevent idle sleep
│   ├── macOS -> IOKit no-idle-sleep assertion
│   ├── Linux -> logind `idle:sleep` inhibitor; `idle` only where polkit refuses a
│   │   process outside the seat session (Hex's user service) to block sleep
│   ├── Windows -> system-required power request (from the press, as its While dictating does)
│   └── Failure -> Warn and continue recording
└── Finish/cancel -> Release it
    // no caffeinate child process or executable-policy dialog
```

Feedback tones keep their output open briefly after the last one: about ten
seconds on macOS, where an open output is a live CoreAudio client coreaudiod
services and holds a sleep assertion for, and five idle minutes on Windows.
On Linux, where an open output stays listed in the desktop's sound controls,
it closes about a second after the last tone ends
([#720](https://github.com/anomalyco/hex/issues/720)); the existing
`output_opens_for_a_tone_and_releases_after_the_idle_grace` check covers the
grace logic, not a real PipeWire stream list. A fatal stream error (the output
became unavailable or invalid) sets a per-sink flag from the stream error
callback, which may run on a CoreAudio thread; overload glitches (`Xrun`) do
not. The worker drops a flagged sink on its next 250 ms observation or next
tone, and the next tone opens a fresh one
(`an_errored_sink_is_dropped_and_reopened_on_the_next_tone`). A default-output
change is not fatal on macOS, where cpal reports `DeviceChanged` after the
default-output unit has already rerouted the stream; on Windows cpal's WASAPI
backend never rebinds, so a default-device change ends the stream with
`StreamInvalidated`, which is fatal: the output is dropped and the next tone
opens the new default. On macOS the CoreAudio listeners on the default output,
the device list, and an audio service restart advance an output route
generation; the next tone drops an output opened under an older one and opens
the current default at once, so unplugging a USB or Bluetooth output no longer
silences tones until Hex restarts
([#428](https://github.com/anomalyco/hex/issues/428)). Check:
`a_route_change_reopens_the_output_on_the_current_default` in
[feedback.rs](../../src/feedback.rs), with injected opens; a real unplug has not
been observed by a test.

**Lower audio** (`recording.environment.duck`) is an explicit fourth
choice, off by default, on every OS. The slider appears only while selected and saves on
release. It remembers its value across behavior changes. Intentional captures
lower the original output channels to that fraction in about 120 ms,
preserving balance and silent channels, then restore on finish, cancel, or
orderly shutdown by easing back over one second. Changes apply to the next
capture. The stop tone waits for restoration. A manually changed volume after
settling is left alone;
a newly selected output device is not lowered. On macOS, a channel with no
volume control is left at its level, and a channel that can be read but not set
sends lowering to the virtual main volume so the balance stays intact; an
output with no controllable volume at all is left unchanged and logs a
warning. Meetings do
not acquire this dictation environment. Linux lowers the default sink's
channels through `pactl` (one volume through `wpctl` without it), so Hex's own
tones are lowered too and the stop tone waits, as on macOS. Windows lowers
every other app's audio session instead, leaving Hex's tones at full volume;
sessions that start while recording are left alone.

HEX snapshots the volumes and applies a smooth ramp in
[audio/output_ducking.rs](../../src/audio/output_ducking.rs) over the CoreAudio adapter in
[ducking.rs](../../src/audio/recording_environment/ducking.rs), the sink adapter in
[linux/audio_control.rs](../../src/linux/audio_control.rs), and the session
adapter in [windows/audio_control.rs](../../src/windows/audio_control.rs). The worker runs inside the app and cannot restore volume after a
hard process crash or SIGKILL. Changes during the ramp itself cannot be
distinguished from delayed Bluetooth acknowledgements. With the shorter
lowering ramp, a Bluetooth output that acknowledges an earlier ramp step late
could look like a manual change and stay lowered; not observed on a device.
Fake-device tests cover relative volume, balance, silence, interrupted ramps,
manual changes after settling, and partial-write rollback; settings/UI tests
cover defaults, bounds, persistence, conditional visibility and platform gating.

**Changed November 7, 2026 (issue 1468):** with Lower audio, lowering used to
take a full second, so the start of every dictation stayed loud, and Duck also
kept the 450 ms pre-roll, which captured audio from before the lower took
effect. It now reaches the ducked volume in about 120 ms (`LOWER` in
[audio/output_ducking.rs](../../src/audio/output_ducking.rs)), keeps no
pre-roll like Mute and Pause media, and still eases back over one second when
restored. Checks: `ducking_preserves_balance_silence_and_restores` and
`lowering_finishes_at_its_own_duration_while_restoring_is_still_ramping` in
that module, and `suppressing_audio_while_dictating_keeps_no_pre_roll` in
[dictation.rs](../../src/dictation.rs) (constructed audio; not run on a device
or physical output).

**Changed November 16, 2026 (issue 1915):** on macOS, Lower audio needed every
one of the output's stream channels to carry a settable volume scalar, so a USB
adapter reporting two buffers of two channels where only the first pair has one
was left unchanged, silently; Mute worked there because it uses the virtual main
volume. Lower audio now ramps the channels that can be read and set and leaves
channels without a volume control at their level, then falls back to the main
volume and then to the virtual main volume Mute uses when no channel reports a
volume at all. A channel that can be read but not set rules out both the
per-channel ramp and the plain main volume, since either could leave it
lopsided or flatten a balance that cannot be restored: the virtual main volume,
which macOS keeps balanced, is the only choice then. Restore writes the same
control it lowered, at the value it read. An output with no controllable volume
at all now logs a warning instead of doing nothing quietly. Checks:
`channels_without_a_volume_scalar_are_skipped_not_fatal`,
`a_read_only_channel_leaves_the_balance_to_the_virtual_main_volume`, and
`the_main_then_the_virtual_main_volume_are_the_fallbacks` in
[ducking.rs](../../src/audio/recording_environment/ducking.rs) (selection only;
not run on a device).

On 2026-10-25, the macOS logic/settings checks and strict Clippy passed.
The seven reference images were regenerated on Ubuntu 24.04 ARM64 with Mesa
lavapipe, using the unmodified Linux-only Rune test module and
`HEX_UPDATE_GOLDENS=1 scripts/check ducking`. A subsequent
`scripts/check rune_ui --nocapture` without the update flag passed all 113 tests,
including the unchanged control goldens; no golden skipped for lack of a GPU
adapter. The five existing images differ from the base branch above the existing
12-level channel tolerance only within the While dictating control; the sidebar,
Quit Hex, and shortcut layouts remain unchanged. Dark and light ducking previews
were visually inspected. The earlier macOS-generated references were replaced
because these fixtures retain build-OS-dependent layout and defaults. This
verifies headless Linux rendering, not audible playback, physical devices,
crash recovery, or native Windows execution.

The Linux and Windows ports (2026-10-30) were checked by unit tests (pactl and
wpctl volume parsing, the Linux Bluetooth stand-in choice), the Settings test
that Lower audio and its slider appear on every OS, Linux Clippy, and Windows
x86_64 cross-Clippy. On apollo (niri, PipeWire), the ignored
`native_idle_sleep_prevention_acquires_and_releases` took a logind `idle`
inhibitor (polkit refused `idle:sleep` over SSH), and a marked `arecord`
stream moved with `pactl move-source-output` without WirePlumber storing the
target. Not verified: audible Linux or Windows ducking, Windows idle-sleep
and Bluetooth detection on hardware, a real Bluetooth headset on Linux, and
Windows ARM64 (its cross-build stops in `rquickjs-sys` on this host).

Successful settings saves persist and project changes at safe runtime boundaries.
An invalid saved field falls back independently at launch, with its key and
reason in the log; the next save preserves the original first. See
[settings recovery](recovery.md#load-settings-with-invalid-fields).
Immediate settings controls commit a candidate before changing the saved UI
choice or applying dependent native/history effects. A failed shortcut save
stays in capture instead of showing Saved; failed retention saves keep the old
choice and surface the error. Failed debounced text saves retain their drafts
for a subsequent edit or close-time flush; no Rune test injects a save failure
yet.
Persistence, conflict, and ownership checks live in
[app_settings.rs](../../src/app_settings.rs),
[audio/recording_environment.rs](../../src/audio/recording_environment.rs), and
[audio.rs](../../src/audio.rs). Settings previews do not prove physical device
switching or native mute support; muting is best-effort, not universal. The
recording-environment ownership checks cover overlapping acquisition and release,
not a managed-device policy or native power assertion. The pause-script regression
checks that inactive players are absent before AppleScript application resolution;
when no supported player is running, acquisition skips AppleScript entirely.
The script check does not prove subprocess avoidance or exercise Music, Spotify,
or VLC through Automation.
The browser pause runs after the AppleScript players and only when System Audio
Recording is reported granted (an unknown answer skips it, because the tap would
open the macOS dialog mid-dictation). macOS 15.4 and later no longer tell other apps
what is playing, so Hex hears the browsers through a private process tap
(aggregate `anomaly.hex.browser-tap.<pid>`, apart from the meeting tap) and sends
play only after it saw the browser go silent right after its own pause. The
browsers are the ones [microphone_activity.rs](../../src/microphone_activity.rs)
knows: Chrome, Brave, Safari, Firefox, Edge, Arc, Vivaldi, Helium, Ardia,
Zen, and Aside, whose helpers count as their browser. The sequence checks in
[audio/browser_media.rs](../../src/audio/browser_media.rs) run on fakes:
`a_confirmed_pause_is_resumed`, `two_sounding_tabs_never_get_play`,
`a_browser_already_playing_gets_no_play`, `a_call_is_never_paused`,
`no_level_sends_nothing`, `a_play_the_browser_never_takes_is_paused_again`,
`a_browser_that_closed_its_output_gets_play`,
`a_closed_browser_that_never_opens_after_play_is_paused_again`; the ignored `native_tap_hears_a_playing_browser`
heard Brave at -25.4 dBFS on macOS 27.0.1 (2026-10-28). Which app receives the
system command is macOS's Now Playing choice; Safari is heard only when its
WebKit process reports Safari's name. An opt-in native
assertion smoke exercises IOKit without starting microphone capture.

**Changed after 2.3.1:** Automatic was a compiled preference list (Universal
Audio Thunderbolt, Studio Display Microphone), then the macOS default with a
Bluetooth default swapped for the built-in microphone (the 2.1.22 fix for
[#106](https://github.com/anomalyco/hex/issues/106) and
[#88](https://github.com/anomalyco/hex/issues/88)). That skipped a user's
AirPods default, and Hex Microphone then passed the wrong microphone on to
calls. Automatic is now System default: the OS default input, followed live.
A Bluetooth default (CoreAudio transport Bluetooth or Bluetooth LE) is passed
over for the built-in microphone, else the first other real input, so AirPods
keep their music out of call quality while you dictate; it is kept when nothing
else is usable. Only System default is affected: a saved Bluetooth microphone
and `--device` are honoured, and General › Advanced › "Use the Mac's microphone
instead of Bluetooth" (`avoid_bluetooth_microphone`, default on) turns it off.
Hex Microphone carries whichever input Hex records from. Linux and Windows
don't detect Bluetooth inputs yet and follow the default. Checks in
[audio.rs](../../src/audio.rs):
`system_default_passes_over_a_bluetooth_default`,
`a_bluetooth_default_is_kept_when_nothing_else_is_usable`,
`a_virtual_input_never_stands_in` (CoreAudio transport Virtual, Aggregate, or
AutoAggregate; [#429](https://github.com/anomalyco/hex/issues/429)),
`inputs_resolve_to_the_selection_then_the_system_default`,
`device_override_wins_and_never_falls_back`,
`a_default_change_switches_only_while_capture_is_idle`, and
`a_missing_selection_uses_the_default_until_it_returns`, with injected device
lists and opens; scene `mac-microphone-bluetooth` shows the row and switch. Not
verified: the CoreAudio listeners against a real default change, the transport
read against real AirPods, or Windows' default-change report on a real PC.

**Fixed after 2.1.20:** through 2.1.20 the microphone menu was painted without
occluding the panel, so hovering or clicking a device also reached the setting
under the pointer ([#94](https://github.com/anomalyco/hex/issues/94)), and it
only closed by choosing or reopening. The menu is now anchored to its button,
occludes the controls beneath it, and closes on any outside mouse-down. In the
Rune window the menu is a rune-kit `Menu` in the overlay layer (scene `menus`);
the GPUI occlusion regression went with GPUI and has no Rune counterpart.

**Easy to misread:** an open microphone is not an active recording.

### Scroll without a wheel

```ts
A pane, detail card or dropdown longer than its space    // ui.scroll-bar (#1209)
├── Pane and card bodies -> a wider, clearly visible thumb on the right edge, only while the content overflows
│   ├── Drag the thumb -> the content follows it
│   └── Press above or below it -> jumps there
├── Dropdowns and menus -> the kit's thin thumb, with the same drag and track press
├── Page Up / Page Down, Home / End -> page the pane around focus or the pointer
├── Open menu or dropdown -> ↑ ↓ move the highlight, Page Up / Down by the rows it
│   shows, Home / End to the ends; the highlight stays in view
├── Model search picker -> the same; Home / End only while the search is empty
└── Wheel and trackpad -> unchanged
```

The bar is Rune's `rune_kit::scroll_bar`, used by `scroll_body`, `faded_body`
and every kit `Menu`, `Select` and `Combobox`. Hex's pane and card bodies call
`scroll_bar_styled` instead, with an 8 pt `text_faint` thumb that turns
`text_muted` and widens to 10 pt under the pointer; dropdowns keep the kit's
thin thumb. Checks:
`a_long_transcript_scrolls_by_dragging_its_scroll_bar`,
`a_long_labs_store_scrolls_by_dragging_its_scroll_bar`, and in Rune the
`scroll_bar` tests and `page_keys_move_a_scrolling_listbox_highlight_by_what_it_shows`.
A touchscreen (Wayland, X11, Windows; a pen where the system reports it as a
touch) acts as the primary button (#1337): a tap clicks, dragging a slider or
the bar drags it, and any other drag pans the scroll area under the finger and
flings on; a second finger is ignored. Checks:
`a_finger_pans_the_transcript_and_taps_the_sidebar`, and Rune's touch tests.
Gaps: the bar and touch are not verified on a real Windows PC or touchscreen.

## Process Text With Modes

```ts
Output (main sidebar, replaces the Modes editor) // rune_ui/output.rs; scenes output*
├── Trust prompt -> Allow a config that uses fetch or programs (only when one asks)
├── Edit with agent -> Copy a prompt naming the steps' keys (the AI rewrite too), `hex settings`,
│   `hex vocabulary` for names, and the transforms docs
├── Open file -> settings.json in the default editor
├── Every dictation -> Global's steps, numbered, read-only
├── Try -> Global's corrections and built-ins on a typed sentence
│   ├── Scripts and rewriting run only when dictating
│   └── Labs > Fix results in Output (off, #989) -> Vocabulary (the shared sheet) and,
│       with a sentence typed, Fix this: copies Edit with agent's prompt ending
│       `When I say "<sentence>", Hex pastes "<result>". It should paste: ` (labs.md)
├── Labs > Transform typed text (Linux, off) -> a Typed text section with typed_text_hotkey,
│   unbound by default: copies the focused field, runs its Output steps, re-checks the
│   selection, and replaces it (labs.md, dictation.md)
└── In some apps -> Each mode's apps and its steps (Only: …), opens to list them
    // Labs > Modes editor (off) shows the old Modes editor as its own sidebar pane,
    // right after Output, with editable apps, corrections, rewriting, and
    // transformations; the Modes destination opens it instead of Output (labs.md)
    // Its drafts and delete confirmation follow the unique saved mode name
    // across reloads; removing, renaming or duplicating it drops the draft.
    // A dirty field stays; untouched fields adopt the new values individually.

Context retained when dictation stops -> Select processing
├── Matching website                     // macOS: scriptable browsers, then AXWebArea; Windows: UIA; Linux: AT-SPI
├── Otherwise matching application
├── Hex itself in front                  // application only: Hex never reads its own AX tree (#1402)
└── Otherwise Global

Selected mode                            // replaces Global's rewrite and transformations
  -> Its vocabulary, then the shared one   // features/vocabulary.md (vocabulary.modes)
  -> Speech Labs preparation              // formatting, symbols, English filler removal
  -> Global's corrections, then the mode's  // every OS
  -> Labs > Neutral voice, in a mode that opts in  // macOS; replaces the rewrite and cleanup; transformations still run
  -> Labs > Rewrite by voice's phrase          // Labs off, or no phrase, processes as before
     -> one spoken instruction replaces the rewrite, with the mode's model and deadline (labs.md)
  -> OpenCode transformation             // optional provider-backed rewrite; an unset model is Hex's ranked pick, shown Automatic
  -> Dictation style                     // Labs › Dictation style on: Polished, Casual, Notes, or Verbatim, after the rewrite and before transformations (labs.md)
  -> Transformations                     // ordered text chain
  -> Paste or Send

Stage fails -> Preserve its input        // failed chain discards partial results
```

Each mode may also carry a speech model and language (`speech`) from the saved
per-app model opt-in, which has no Settings editor; when it is on (macOS and
Windows), the frontmost app's mode switches the transcription model before the
next dictation ([`transcription/app_model_switching`](../../src/transcription/app_model_switching.rs)).

**Fixed November 1, 2026 (issue 949):** every OpenCode rewrite's fixed
instructions (`PROTOCOL_PROMPT`) now say to keep every number as said (no
dropped, merged, or reformatted numbers, no added thousands separators) and,
unless the mode's prompt says otherwise, to write a spoken enumeration
("point one … point two …") as a numbered list, one item per line. A
transcript of numbers alone (digits or English number words) skips the
rewrite and pastes after corrections as heard; transformations still run.
Check: `numbers_only_transcripts_skip_the_rewrite` in
[dictation/processor.rs](../../src/dictation/processor.rs). The prompt rules
were not run against a live model.

Built-in text transformations run natively. Custom transformations come from
`~/.config/hex/hex.config.ts` and run in Hex's embedded script engine, each
under its own `timeout` (1 s by default, 30 s when the config uses host APIs,
60 s cap). OpenCode rewriting sends documented context to the configured
provider; speech transcription stays local. With Labs › Rewrite reads nearby
text on (macOS), that context also includes the focused text element's text
around the caret, read through Accessibility and never saved; password fields
are skipped, and off leaves the prompt unchanged.

```ts
Discover custom transformations           // personal_commands::host_needed; every OS
├── A config exists (Set up, hex commands init, or a manual file) -> start the host
├── A custom transformation is selected -> start, even if its config is missing
├── Only built-ins selected, no config -> native transforms only, no script host
└── DisableTransforms policy -> no host, regardless of config or selection
```

The macOS listener rechecks discovery every 250 ms; Linux and Windows use
their host's refresh. `host_startup_discovers_a_config_without_selecting_a_transformation`
in [personal_commands.rs](../../src/personal_commands.rs) covers the shared
decision, policy denial, built-ins, and custom selection in global and other
modes. This is a pure decision check, not a live Set up walkthrough.
`workspace_fingerprint_tracks_every_loaded_file_kind` in
[personal_commands.rs](../../src/personal_commands.rs) covers which file edits
trigger the reload watcher; it is not a live filesystem notification.

```ts
Transform host APIs                      // personal_commands/{io,fetch,process,trust}.rs
├── fetch(url, { method, headers, body, signal }) -> Response (ureq, platform TLS)
├── node:child_process execFile / exec (+ node:util promisify, input option)
├── setTimeout, AbortController, AbortSignal.timeout
└── First use -> Output pane trust prompt (hosts, programs, or capability)
    ├── Allow -> hash of every loaded file pinned in transform-trust.json
    ├── Any edit -> asks again
    └── Not allowed -> call throws -> chain fails -> text pasted unchanged
Timeout or settle -> requests abandoned, process group / job object killed
```

The API is documented for agents at
[anoma.ly/hex/docs/transforms](../../site/public/docs/transforms.md), linked
from `llms.txt`, the managed `AGENTS.md`, and Edit with agent's prompt. Checks:
[tests/host_apis.rs](../../src/personal_commands/tests/host_apis.rs) runs
`fetch` against a local server (approval, re-ask on edit, abort, network
error) and `exec`/`execFile` (exit codes, stdin `input`, promisify, `timeout`,
`maxBuffer`, process-tree kill on Linux, fallback through dictation's
processing stage); [trust.rs](../../src/personal_commands/trust.rs) covers the
scan and hash; `the_trust_prompt_lists_what_a_config_reaches_until_it_is_allowed`
and the `output-trust` golden cover the prompt. Windows `cmd` quoting is a
pure-function test only; the job object and macOS paths compile on CI but are
not run against real programs there. No check reaches a remote TLS host.

The built-ins are Lowercase, SpongeBob case, and No trailing punctuation. The
last removes only the final contiguous run of sentence marks (including
full-width marks), preserving internal punctuation, trailing whitespace, and
closing quotes/brackets. Punctuation inside a final closing quote is preserved.
Select it after Lowercase for lowercase messages without the final period.
The `no_trailing_punctuation_*` and `built_in_transformations_chain_in_selected_order`
tests in [personal_commands.rs](../../src/personal_commands.rs) cover text behavior
and native dispatch; `transformations_chain_with_built_ins_and_failures_keep_nothing`
and `runaway_scripts_stop_at_the_time_limit_and_the_config_keeps_working` run
real scripts. None is a live target-app paste.

Selection and stage checks start in
[dictation/processor.rs](../../src/dictation/processor.rs),
[dictation/text_replacements.rs](../../src/dictation/text_replacements.rs), and
[personal_commands.rs](../../src/personal_commands.rs). They do not prove live
provider availability or real application/browser context changes.

Every OS compiles settings through `Profiles::from_settings` and uses the same
selection and correction pipeline. Website rules accept bare hosts, URLs,
ports, and internationalized domains; the first matching website wins over
the first matching application. Application matching retains each adapter's
policy: exact names ignoring `.app` on macOS, case-insensitive substrings on
Windows and Linux. Global corrections run once, after speech preparation.
The corpus comparison `shared_pipeline_preserves_the_macos_corpus_output`,
`speech_preparation_precedes_global_and_mode_corrections`, and
`website_selectors_accept_saved_urls_ports_and_idn` cover these rules without
inference or a provider call.

Linux and Windows pin the foreground window when stop is handled, carry its
pending accessibility read through the release tail and output queue, and
resolve it before processing. macOS retains its capture-end monitor snapshot.
Switching apps while inference or an older rewrite runs no longer selects a
different mode for queued speech. Paste still targets the current focus.
This handoff has unit coverage; native focus switching has not been observed.

Website context per OS: no OS reads a page until something uses it: a mode
chosen by website, an OpenCode rewrite (its prompt names the site), or a
script transformation (it receives the address)
(`dictation::config::reads_browser` on macOS, `ModeRuntime::reads_page` on
Windows and Linux; a dictation without processing never reads it). On macOS,
dictation's page reads then never prompt (Meetings and media pause still ask
browsers when on). Windows starts no UI Automation walk of the browser for a
dictation without processing (meeting detection reads a microphone-holding
browser's windows itself, see [Meetings](meetings.md)), and
Linux asks AT-SPI only for the process of a window the display server names no
other way. Check: `the_page_is_read_only_when_processing_uses_it` in
[dictation/modes.rs](../../src/dictation/modes.rs); not run against a live
browser. On macOS the front window's title is read only by meeting detection
and while a script transformation is set up (`reads_window_title`). When
something uses the page, macOS asks the front tab of Safari, Orion, Chrome,
Brave, Edge, Arc, Vivaldi, Opera, Helium, Chromium, and Aside over
AppleScript (one Automation prompt per browser; a refusal keeps the
application context) and reads any other app's focused AXWebArea `AXURL`
(Firefox, Zen, web views).
That AppleScript read is not part of the 500 ms foreground-context poll: the
poll reads it once when a scriptable browser becomes the frontmost process
(caching its address, or no address after a failed read, per pid), so
switching away and back reads the page again. A capture asks for one more
read only once it is real, never on every press: the shortcut held past its
hold threshold, a finish that lands before that, or a control start
(`hex dictation start`). A press a chord discards before that hold threshold
reads nothing; holding the shortcut past the threshold first still reads the
page once.
A finish waits at most 1 s for the refreshed page and otherwise hands its
capture the same app and window title with no page, never the previous
dictation's. Checks:
`the_scripted_page_is_read_once_per_pid_and_on_refresh_only`,
`a_refresh_the_full_channel_held_back_completes_once_delivered`, and
`a_press_discarded_as_a_chord_never_reads_the_page` in
[dictation/context.rs](../../src/dictation/context.rs); not run against a live
browser, and the recognition loop's wiring of that refresh
([recognition/context_watch.rs](../../src/recognition/context_watch.rs)) is
covered only by the macOS build, not by a test.
Linux matches the focused window from X11 or the Wayland foreign-toplevel list
to an AT-SPI window by title and process and reads its showing web
document's `DocURL` (Firefox) or `URI` (Chromium, only with
`--force-renderer-accessibility`). `linux_context_reads_the_focused_window`
(ignored, manual) printed `google-chrome-stable` with `https://example.com/path`
under headless Sway and `chrome` with the page under its XWayland, and `hex`
on niri (Oct 30); the macOS adapters are compiled on CI only and not run.
With Hex's own window frontmost, its application name still matches but no
Accessibility tree of its own process is read, so its main thread owns that
tree alone (#1402); `hex_never_reads_its_own_accessibility_tree` in
[accessibility.rs](../../src/accessibility.rs) covers the skip (macOS only),
though the race it prevents is not reproduced in a test.

OpenCode transformation talks only to an already running OpenCode service,
found through its registration file ([desktop/opencode_service.rs](../../src/desktop/opencode_service.rs));
Hex runs no OpenCode executable and starts nothing. On Windows, Labs › OpenCode
in WSL adds the default WSL distribution's registration and a Linux
`x-opencode-directory` workspace; with the lab on, a WSL service that answers
is used ahead of a Windows one, and the Windows service is the fallback when
WSL answers nothing. Off, only the Windows registration is read. The catalog
loads only while the Modes pane shows. Without a service, "OpenCode isn't
running" shows beside the switch and the setup/retry notice below;
`unavailable_opencode_is_explained_beside_each_switch` covers it headlessly, and
the `desktop/opencode_service` tests cover registration, stale-PID, loopback, chunked,
deadline, and cancellation handling against a local fixture, not a live
provider.

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
picker name arrives as `Ghostty.app`; [dictation/context.rs](../../src/dictation/context.rs) strips
that suffix on both sides and [app_settings.rs](../../src/app_settings.rs)
rewrites already persisted activations on load, checked by
`application_matching_ignores_finder_bundle_extensions` and
`loading_strips_finder_bundle_extensions_from_mode_applications`.

The picker saves the name Finder shows for the bundle's file, which can differ
from the name the running app reports (`Rex Beta.app` runs as `Rex`,
`Visual Studio Code.app` as `Code`). The macOS frontmost read also keeps
Finder's name for the frontmost bundle (`ContextSnapshot::bundle_name`, set
only when it differs), and `application_is` accepts either, so Modes,
Labs › YOLO mode, and the per-app model opt-in match an app under both
names. Checked by
`an_application_selector_matches_the_bundles_finder_name_too` (context.rs),
`a_picked_app_sends_under_its_finder_name_or_its_own` (yolo_mode.rs), and
`a_mode_picked_under_its_finder_name_switches` (app_model_switching.rs).
**Fixed on dev, Nov 19 2026:** before, a picked app whose file name differs
from its own name never activated its Mode, YOLO mode never sent in it, and
the per-app model opt-in never switched to its mode's model.

**Fixed in 2.1.14:** through 2.1.13, a mode saved with that Finder
preference enabled never activates and every dictation falls back to Global.
Update to repair saved selections automatically. For older builds, the workaround
is to quit Hex, remove the `.app` suffix from
`dictation_processing.modes[].applications` in `settings.json`, then relaunch.

## Record Meetings

[Meetings (beta)](meetings.md) maps the opt-in (the Meetings pane's card, then
Meetings › Settings), call detection and the
focus-free offer, menu-bar recording state, rolling transcription, echo
suppression, and the Markdown copy in the user's transcript folder.

## Experimental: Hex Microphone And Voice Filter

```ts
Labs › Voice Filter (experimental.hex_microphone = true), HEX_EXPERIMENTAL_MIC=1,
or the driver installed; off, the pane is hidden  // fun.hex-microphone; macOS, Linux
├── Sidebar > Voice Filter  // the only home of these controls
├── What callers hear: the effect applies only while dictating; normal speech otherwise
│   ├── Select a name → apply it live: Muffled (default), Muted, Quiet, Normal, Chipmunk, Robot, Walkie, Underwater, Far
│   ├── Play on a tile → preview that effect, never save it or change the virtual input
│   │   ├── Example clip or an explicit 5 s recording (in memory only); Play original to compare
│   │   └── Two waveforms highlight the dictation span. Switching previews stops the previous one
│   └── Adjust effect → per-mode sliders, effect strength, Reset, and comfort noise for Muted
├── Hex Microphone setup
│   ├── Linux -> no driver: a PipeWire virtual source (Audio/Source "Hex Microphone",
│   │   node hex_microphone) that Hex plays into through `pw-cat` while the Voice Filter is on
│   │   ├── Status -> Starting… / Available ("Choose Hex Microphone as your input…") / Unavailable + reason
│   │   ├── pw-cat missing or exits -> Unavailable with its last error line; retries every 2 s
│   │   ├── --raw -> only where pw-cat's help lists it (PipeWire 1.3.81+);
│   │   │   1.2 and older reject it and already play raw on -
│   │   ├── Off or Hex quits -> the source disappears (pw-cat ends with its input); no root, nothing installed
│   │   └── Session priority 1 -> WirePlumber never picks it as the default input Hex records
│   ├── macOS -> select Hex Microphone as the input in the call app / In use (by name only when observed on that input) + Uninstall
│   │   ├── Version mismatch -> Update + Uninstall
│   │   ├── Pre-Anomaly com.kitlangton driver at the same path -> Update replaces it in place
│   │   ├── Not installed -> Install -> macOS admin prompt -> copies bundled HexMicrophone.driver + restarts coreaudiod
│   │   └── Inline state -> Installing… / Uninstalling… / Cancelled / Failed with reason
├── Live microphone capture -> Raw audio to Hex transcription (unmodified)
│   ├── Source -> Hex's dictation microphone (System default follows the OS default, never Hex Microphone itself), never a meeting's second stream
│   │   └── macOS: with Release when idle off, locking the screen or sleeping the
│   │       display releases that microphone, so Hex Microphone pauses until wake or unlock
│   │       (dictate.warm-away)
│   ├── Lock-free ring -> 48 kHz virtual sink (macOS IOProc; Linux 11 ms blocks into a one-page pipe) -> Raised-cosine fade on intentional hold
│   │   └── Not dictating -> bit-exact pass-through; non-48 kHz inputs resample continuously
│   └── Ring underflows/overflows -> process.log each minute while an app reads it, and on sink stop
└── Offline CLI -> hex dev voice-filter render <in.wav> --mode <m> --dictation <start>-<end> -o <out.wav>
```

Sources: [audio/voice_filter.rs](../../src/audio/voice_filter.rs),
[audio/hex_microphone.rs](../../src/audio/hex_microphone.rs),
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
`try_it_records_and_plays_through_the_host`,
`preview_each_effect_before_install_without_applying_it`,
`voice_filter_setup_distinguishes_an_available_input_from_a_call`,
`rapid_auditions_replace_pending_audio_and_invalidate_work_in_flight`, and
`voice_filter_needs_labs_or_hex_microphone`.
Scenes: `mac-voice-filter`, `mac-voice-filter-muted`,
`mac-voice-filter-driver-missing`, `voice-filter`, `voice-filter-unavailable`
(Linux).
Linux observed (apollo, PipeWire 1.6): the source lists in `pactl list sources`
and `pw-record --target hex_microphone` records the fed audio at 48 kHz; no
automated check covers it, and no call app was tried.
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
├── Menu (macOS), top to bottom: now, things to do, the app
│   ├── Labs > Upcoming meetings in menu bar (off) -> an Upcoming section first:
│   │   up to five of the reminder poll's meetings, soonest first, as local
│   │   start–end time and title ("Now · " in progress, "Tomorrow " or the short
│   │   weekday for a later day, ended left out), "No upcoming meetings" when
│   │   none; choosing a row with a call link joins it through its own path
│   │   (opens the link, records that occurrence so no reminder pops, closes a
│   │   showing reminder; the click doesn't record), a row without one is
│   │   informational (labs.md, meetings.md)
│   ├── Meetings on or a meeting under way: Recording · <title> · 12:34 (opens
│   │   the Meetings pane), Stop & Transcribe, Copy Section ▸; idle: Record Meeting
│   ├── Pause Hex (Labs › Pause from menu bar, off, #82) -> every capture
│   │   shortcut and the secure-input hotkeys go inert, the tap passes keys
│   │   through, and the row reads Resume Hex (labs.md); Resume waits for a
│   │   neutral keyboard, and a restart starts unpaused
│   ├── Paste Last Dictation ⌥⇧V (saved binding), Copy Recent Dictation ▸ (Labs),
│   │   Copy Last Meeting Transcript (Meetings on), Restore Last Capture (Labs ›
│   │   Screenshots on; dimmed until a capture card was closed), Speech Model ▸,
│   │   Vocabulary ▸ (Labs › Vocabulary menu, off: Add term… opens Settings on
│   │   the Vocabulary sheet with a new focused row, then the last five saved
│   │   terms newest first as informational rows, then Open vocabulary…, which
│   │   opens the sheet alone; off, the row is hidden and the menu is unchanged,
│   │   labs.md)
│   ├── Settings… ⌘, · Check for Updates… · Quit Hex ⌘Q
│   │   └── Labs > ⌘Q closes Settings (off, #1473) -> ⌘Q closes the Settings
│   │       window instead; dictation keeps running, the menu bar icon quits (labs.md)
│   ├── No Meetings… (Settings… opens the same window) and no Show Hex in
│   │   Screen Recordings (Annotate's pane has it)
│   ├── In the interface language: macOS title case in English, else the
│   │   window's translated labels; fixed rows retitle as the menu opens, state
│   │   rows (meeting, pause, update) with each update; meeting titles never
│   │   translate. Check: the_menu_follows_the_interface_language
│   └── check: the_menu_reads_upcoming_then_meetings_then_actions_then_the_app,
│       paste_last_shows_the_saved_binding_or_nothing,
│       the_pause_row_reads_pause_or_resume,
│       vocabulary_lists_the_last_five_terms_newest_first (status_item.rs;
│       the row order and key equivalents only, no native menu is drawn)
├── Icon -> 18 pt monochrome Hex mark template (recording variant + elapsed time)
│   └── Labs > Hide menu bar icon (off, #1064) -> hidden unless a meeting is under
│       way; shortcuts keep working; Finder or Spotlight reopen -> Settings (labs.md)
├── Settings / Paste Last -> Reachable with Dock icon hidden
├── Launch at Login -> macOS owns registration state
├── Check for Updates -> The updater (maintain.updates); opens the window, whose
│   footer then says "Checking for updates…", "Hex is up to date", or
│   "Couldn't check for updates" (#1120)
└── Quit -> Orderly shutdown (⌘Q closes Settings instead while its Lab is on)
```

```ts
Launch Hex                               // maintain.startup
├── `hex app --hidden` + usable menu-bar item -> No startup window, even before setup
│   └── check: login_item_launch_starts_without_the_window (the decision only); not run on a Mac
├── Setup incomplete / permissions or model missing -> Open setup or Settings
├── Show Dock icon on -> Open Settings
│   └── Started by Launch at login -> No startup window (#1321)
│       ├── macOS marks the open-application Apple event as a login-item launch
│       ├── The activation that comes with that launch doesn't reopen the window (#1298)
│       └── check: login_item_launch_starts_without_the_window (the decision only); not run on a Mac
└── Show Dock icon off + usable menu-bar item -> No startup window
    ├── Dictation starts independently of the Settings window
    └── Menu-bar Settings / Finder or Spotlight reopen -> Open or focus the window
        └── On the active Space, never switching back to where it last showed

Menu-bar installation fails -> Show Dock icon and open the window // recovery access

Show Dock icon off                       // maintain.dock-policy
├── Settings window open -> Dock icon stays // hiding it would deactivate Hex
└── Settings window closes -> Menu-bar only

⌘Tab to Hex while it's in the Dock and its window is closed   // maintain.app-switcher (#1298)
└── Opens the window in front, on the pane it last showed, as the Dock icon does
    ├── No reopen Apple event comes with ⌘Tab; Hex watches its own activation
    └── Menu-bar only, an overlay that took the keyboard, or the window already open -> nothing
    // check: switching_to_hex_opens_its_closed_window (the decision only); not run on a Mac
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

[desktop/login_item.rs](../../src/desktop/login_item.rs) serializes ServiceManagement calls on one
window-owned worker, with bounded request and response channels and an autorelease
pool per operation. Settings reads responses without blocking on its 250 ms tick;
refreshes do not accumulate while macOS is busy. The toggle is unavailable until
the first successful status lookup. A click is held in memory
(`PlatformState::login_item_requested`) only until the worker is idle again, so
the switch need not wait for `SMAppService` (about 120 ms per toggle, measured
October 24, 2026, plus up to one tick). macOS owns registration state; no login
Boolean is persisted. Scenes never start the worker or call macOS.

The worker tests in `desktop/login_item.rs` exercise thread reuse, nonblocking polling,
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
from computer-use observation reopening the window. Cmd+Q and Quit Hex each
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

Closing the window only orders it out, and AppKit keeps an ordered-out window
on the Space it last showed on, so reopening it from the menu bar on another
Space switched back to that one. `macos_rune` gives the window
`NSWindowCollectionBehaviorMoveToActiveSpace`, so it opens on the active
Space instead. No automated check covers Spaces; it needs a native check with
two desktops.

Checks in [meeting/watcher.rs](../../src/meeting/watcher.rs):
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
│   ├── User-Agent: hex/<build channel>/<version> (hex/dev/0.0.0-dev-171, hex/stable/2.3.1); nothing else about the install
│   ├── Failed check -> backs off, never past hourly
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
│   │   └── .lnk files in Start Menu, Desktop, and taskbar pins -> retargeted in one PowerShell run
│   │       // target is versions\<version>\hex.exe; arguments and name stay, an old-version icon is repointed
│   │       // a Start Menu Hex.lnk that reads as empty (zero-filled) is recreated, but never when it is gone
│   │       // saves flush updates\shortcut.tmp.lnk; a failed replace saves the retarget in place, the repair copies
│   ├── Linux -> SHA-256 only -> new version directory, `current` switched
│   ├── Staged -> checks continue; Restart to update stays offered meanwhile
│   │   ├── Another release this install takes (update, required) -> download -> replaces the staged one
│   │   │   └── macOS -> its unpacked .Hex-update-<version> folder goes with it
│   │   └── Same release, rollout deferred, unconfirmed switch, rollback, failure -> keep the staged one
│   └── Sidebar Update, menu bar / tray: Restart to update; a dev build lists its
│       first changes and What's new (https://anoma.ly/hex/dev) under it (updater/notes.rs)
│       ├── A meeting is recording -> refused with that reason; the update stays
│       │   staged and Ready, so the next click after it ends restarts. The macOS
│       │   status item dims Restart to update while it would be refused
│       ├── One more check first (manifest within 15 s) -> Checking / Downloading -> newest staged
│       │   └── A release published after the relaunch -> the next launch's check
│       ├── macOS -> renamex_np swap (also at quit) -> relaunch -> old bundle removed at launch
│       │   // one .Hex-update-<version> folder at a time: staging a newer build removes the one it replaces
│       └── Labs › Release notes link -> a release build's footer links to what changed since this build:
│           anoma.ly/hex/dev?since=<version> for a dev build, github.com/anomalyco/hex/releases/tag/v<version> for a stable one;
│           local and test builds show no link (by build kind, not version string)
├── macOS location not replaceable (read-only, translocated, root-owned) -> Download update (DMG)
├── Package manager (pacman, Nix) -> its command, once it serves another version
├── Source build, test build -> never checks
├── `HEX_X86_BASELINE=ivybridge` source build (2012 floor) -> source build: never checks; official artifacts target x86-64-v3
└── Failure -> keep the running version, back off, Check for updates retries

Check for updates pressed (footer, menu bar, app menu)   // maintain.update-feedback (#1120)
├── The menu bar's and app menu's open the window at General
└── Footer -> "Checking for updates…" -> "Hex is up to date" | "Couldn't check for updates"
    // the hourly check alone keeps "Check for updates"; check:
    // an_asked_update_check_says_how_it_went (fixture; not run on a Mac)

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

Every quit Hex starts ends through its own drain: Rune is told the app has work
after its loop (`rune::orderly_exit`, called by `macos_rune::run`,
`windows::rune::open`, and `rune_ui::run_app`), so Quit Hex, `hex quit`, and
Restart to update stop and join the meeting controller. The staged bundle is
installed before that join (the swap is atomic and touches no meeting files),
and a quit that begins during Restart to update's final check never swaps a
second time. On macOS the AppKit terminate notification drains the same way,
with 5 s before `_exit` (`DEADLINE`), extended up to 30 s from the quit's
start while a recording or recovery is in flight (`MEETING_DEADLINE`), plus up
to 5 s more after it ends (still within the 30 s bound) for the rest of the
drain, so a finalising meeting is never cut off. Linux's service stop has its
own 20 s bound (`STOP_DEADLINE`) under the unit's 30 s `TimeoutStopSec`;
Windows marks quitting and arms 20 s around its post-loop teardown, extended to
30 s while meeting audio is in flight and polled rather than read once
(`arm_windows_deadline`), exiting 1 when the window failed. Restart to update
refuses while meeting audio is in flight (`updater::restart_hold`), re-checks
after its final manifest check, and keeps the release staged.

[updater.rs](../../src/updater.rs) owns the worker and the shared status the
window, menu bar, and tray show; `updater/protocol.rs` passes the release
service's shared test vectors (`tests/fixtures/release-protocol`), and the
per-OS modules have tests for activation, the macOS bundle swap and
signature checks (ad-hoc fixtures against the real requirement), the rollout
bucket, receipts, the channel choice, never comparing versions (a legacy
2.3.2-dev.29 follows 0.0.0-dev-N; dev to stable is a switch), the switch
waiting for confirmation, only a different self-taken release replacing a staged
update (`only_a_different_release_this_install_takes_replaces_a_staged_update`),
Restart to update's final check and one-time report
(`restart_to_update_checks_once_more_and_reports_once`), Linux re-staging keeping
the running version (`replacing_a_staged_update_keeps_the_running_version`),
macOS keeping one unpacked folder however many builds it stages
(`a_newer_staged_release_removes_the_folder_of_the_one_it_replaces`,
`launch_cleanup_removes_every_unpacked_update_but_the_running_bundle`,
`the_folder_to_keep_is_recognized_through_a_non_canonical_spelling`), the quit
race (a release staged after a quit installed one is dropped:
`a_release_that_finishes_staging_after_a_quit_installed_one_is_discarded`, the
decision only; a restart never swaps twice after that:
`a_restart_after_a_quit_installed_the_update_does_not_swap_again`), and
Windows Authenticode (a Microsoft-signed system file). No test drives the worker
through a real re-stage and restart; a channel rolled back while an update is
staged still restarts into the staged release. The
Update channel row shows only while this install updates itself (not test,
source, managed, or package-manager installs); the `mac-settings-dev-channel`
scene renders it, and `rune_ui` tests cover saving and hiding it.
Not yet observed: a published dev channel release, a switch between channels on
a real install, a real update on any OS from the release service, the macOS
swap and relaunch of a Developer ID build, a Windows update of a signed build.
Manifests are unsigned; on Linux a compromised HTTPS origin could serve any
executable with a matching manifest, which Linux code signing cannot catch.

**Fixed after 0.0.0-dev-1046 (Dev channel):** through that build, every staged
release unpacked its own `.Hex-update-<version>` folder beside `Hex.app`
(`stage` in [macos.rs](../../src/updater/macos.rs)) and the only removal was
`finish_on_launch`, so a Hex that ran for days while the Dev channel (a check
every ten minutes) published builds left one ~80 MB copy of the app per build in
`/Applications`. Staging a newer release now removes the folder of the one it
replaces once the replacement is the staged update, and a release whose download
finishes after a quit installed an update is dropped instead of replacing it
(that folder holds the bundle the installed update replaced), so a restart never
swaps twice. A quit that has not installed one keeps the older staged update
Ready, so the quit still installs it. The launch sweep still takes whatever a
swap or an interrupted stage left behind. Observed on macOS 26
with the published 0.0.0-dev-1046 build: the launch sweep removed a folder
holding an older signed build and one holding a swapped-in peer beside
`/Applications/Hex.app`, and a folder owned by another user stayed with
`could not remove an earlier Hex update … Permission denied (os error 13)`,
which this does not repair. No test drives a live quit through the race: the
guard is a decision test plus a restart-state test. Neither the staging sweep nor
the swap was exercised against a real release service on a real install.

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
[desktop/login_item.rs](../../src/desktop/login_item.rs), [macos_rune/status_item.rs](../../src/macos_rune/status_item.rs),
[updater.rs](../../src/updater.rs), the
[identity test](../../scripts/test-app-identity.sh), and
[app validation](../../scripts/validate-app.sh) are maintenance evidence starting
points. Preview badges do not prove installed-app login registration or a signed
update from an older supported build.

### Managed feature locks (macOS)

```ts
Configuration profile forces a key in anomaly.hex     // maintain.managed-locks
├── DisableMeetings -> no detection, recording, transcription, or notes;
│   hex meetings and hex mcp still read existing meetings
├── DisableAnnotate -> Annotate off
├── DisableOpenCodeProcessing -> no mode rewrites, meeting notes, or capture sends
├── DisableTransforms -> no transforms; the hex.config.ts host never starts
├── RestrictTransformsToLoopback -> transforms on; fetch reaches loopback only
│   (resolved addresses, every redirect, no proxy); exec/execFile refused
├── DisableLabs -> every Labs feature off
└── Window: setting off, disabled, "Managed by your organization";
    hex settings set / hex labs on / local API refuse with code "managed"
```

Only values a profile forces count (`CFPreferencesAppValueIsForced`), read once
per process; a `defaults write` value locks nothing. Windows and Linux read
none of these keys. [policy.rs](../../src/policy.rs) owns the keys:
`AppSettings` turns locked switches off as it loads and writes the saved values
back on save, so the person's own settings return when the profile goes away.
Work that follows a list rather than a switch checks the policy where it
starts: `Profile::configured` (rewrites, transformations), OpenCode generation
in `dictation/processor.rs`, `capture_host/routing.rs` sends,
`PersonalCommands::start`, and `personal_commands/script.rs` with
`net::loopback_agent`. Admin docs: `site/public/docs/admin.md`
(https://anoma.ly/hex/docs/admin).

Checks: `policy::tests`, `a_managed_lock_reads_off_and_never_rewrites_the_saved_switch`,
`a_managed_setting_is_refused`, `managed_locks_drop_the_rewrite_and_transformations`,
`switches_hold_every_feature`, and
`restricted_to_loopback_fetch_stays_on_this_computer_and_runs_no_programs` (a
redirect off the computer and a non-loopback address both fail before
connecting). Not yet observed: a real configuration profile on a managed Mac,
or the managed rows in the window.

## Other Platforms And Consumers

```ts
Linux beta                               // not macOS feature parity
├── hex start -> One per-user Hex -> Alt-Space capture -> Stream (Vulkan GPU) -> Paste
├── Labs › Live typing (labs.live_typing, off, wlroots-family Wayland) -> zwp_input_method_v2 preedit in the focused field
│   ├── Final text committed through the input method; no active input, another input method, multi-line, or no answer -> Paste  // labs.live-typing
│   ├── CPU engine or non-streaming model -> No words while speaking; the final text still commits there
│   └── Hex's first-run practice field on Linux takes the preedit and commit too while on
├── Speech engine (General › Advanced › Dictation; transcription_device) -> Auto | GPU | CPU (#856)
│   ├── Auto -> best GPU unless gpu-failures.json lists it under its current driver
│   │   (kernel + Vulkan driver libraries), then another GPU, then the CPU;
│   │   the row says which GPU stopped responding
│   ├── A GPU load, prewarm or run that fails with a device error (hang,
│   │   reset, lost device) -> reloads on the CPU; once the CPU works, that
│   │   GPU is recorded
│   ├── A run that fails out of memory (status 7) -> reloads on the CPU and is
│   │   not recorded (another copy can fill the GPU, #1165); the dictation
│   │   model says so in the row until a GPU load works or Hex restarts (#1984)
│   ├── An integrated GPU whose warm 1.5 s run takes over 300 ms -> Auto times
│   │   the CPU too and keeps the faster; a slower GPU is recorded (slow), so
│   │   later loads go straight to the CPU (#484: UHD 620, Iris Plus 655)
│   ├── GPU -> tries the GPU anyway; working, it clears that GPU's record
│   └── CPU -> CPU only; launched on CPU, Hex loads no GPU driver for speech
│       and switching to Auto or GPU says it turns on when Hex restarts (#1253)
│   // checked on apollo with a planted record (RADV); a real i915 hang not reproduced
│   └── CPU or a model that cannot stream -> Transcribe the whole clip after release
├── hex / hex app -> Window in the same process; closing it leaves Hex running
│   └── Ctrl+Q in the window or Quit Hex in the sidebar -> quits Hex (#808)
│       // check: ctrl_q_quits_hex (fixture; not run on a GNOME session)
├── Recording sounds -> Shortcut press / capture stop
├── Shortcuts -> One state machine, three sources        // linux.shortcuts
│   ├── X11 -> Root-window key grabs; General records the chord
│   ├── Wayland + GlobalShortcuts portal -> Desktop grants the triggers
│   │   ├── General > Change shortcut -> Press chord (Esc cancels)
│   │   │   └── Saved -> New portal session asks for it as preferred_trigger
│   │   │       under an id named for it (`dictation-CTRL-ALT-space`), so a desktop
│   │   │       that keeps an id's stored trigger (GNOME, KDE) binds the new one (#575);
│   │   │       an unchanged shortcut keeps its id and any trigger set in the desktop
│   │   │       // observed on the niri fork: a new session gets it, a rebind in the same one fails
│   │   │       // #575 fix checked by a unit test only, not on a GNOME session
│   │   ├── Each row: "Your desktop granted <trigger>" (it may assign another);
│   │   │   an empty trigger (KDE's for one it keeps unbound) counts as refused
│   │   │   └── Hyprland (XDG_CURRENT_DESKTOP) -> its portal names no trigger, since
│   │   │       its config binds them: muted "Your desktop assigns this key itself,
│   │   │       so Hex can't confirm it", no refusal note, info log (#635)
│   │   │       // check: a_missing_trigger_is_unconfirmed_where_the_desktop_assigns_keys
│   │   │       // (the row's data only); not run on a Hyprland session
│   │   ├── Portal version 2 -> Desktop shortcuts > Open -> ConfigureShortcuts,
│   │   │   the desktop's own dialog; ShortcutsChanged updates each row
│   │   │   ├── The call fails (the frontend says version 2 but the desktop's backend,
│   │   │   │   such as GNOME's, lacks it) -> the row goes away instead of a dead
│   │   │   │   Open button, and process.log warns (#723)
│   │   │   │   // no check; not run on a GNOME session
│   │   │   // a desktop keeps a stored binding over a new preferred_trigger for the same id
│   │   ├── Hold whose release never comes (GNOME sends none when a modifier
│   │   │   comes up first) -> once the desktop's repeats of the held press stop
│   │   │   for 1 s, it counts as released and the capture finishes; that inferred
│   │   │   release never starts a double tap. A hold too short to repeat still
│   │   │   finishes on the next press (#1227)
│   │   │   // check: a_hold_whose_repeats_stop_without_a_release_finishes_by_itself
│   │   │   // (the state machine only); not run on a GNOME session
│   │   ├── Refused shortcut -> Danger row + how to free it (niri: config.kdl)
│   │   │   under that shortcut's own row (Dictation, Output);
│   │   │   the rest keep working
│   │   └── The portal's backend fails the bind (Response 2 or a bus error, not
│   │       a dismissal; niri with the GNOME backend, #1014) -> as without the
│   │       portal below; this listener run doesn't bind again, a restart
│   │       (Enable, a settings change) tries once more
│   │       // check: only_backend_failures_count_as_a_missing_portal (the
│   │       // classification only); not run on niri
│   ├── Wayland without the portal -> Bind `hex` commands in the desktop
│   │   ├── The commands reach only the service listener (`hex start`/`hex service`);
│   │   │   a foreground `hex dictate` receives none and says so at startup (#1506)
│   │   ├── General hides shortcut rows and lists the commands
│   │   └── GNOME: Add GNOME keyboard shortcuts -> custom shortcuts running
│   │       `hex dictation toggle` (#781); written only while the listener has no
│   │       portal, and removed once the portal (or X11) binds the shortcuts, so a
│   │       press never both starts a portal hold and toggles it off (#931)
│   │       // check: entries_exist_only_without_the_portal (the rule only)
│   └── Listener starting or stopped -> `hex status` reports Unregistered,
│       never the X11 mode
├── hex dictation start|stop|toggle, hex paste-last
│   -> Same events as a shortcut                     // linux.commands
│   ├── start|toggle --raw -> Dictate without processing's events
│   ├── Idempotent: start while recording / stop while idle -> Nothing
│   └── Hex missing or not listening -> One-line error, non-zero exit
│       // as for hex quit|restart|status
├── hex dictation cancel -> Discards whatever records, any mode
├── Toggle's cancel shortcut (Escape by default), only while recording
│   ├── X11 -> Grabbed only for the recording, and Escape while Annotate draws
│   └── Portal -> Never bound: bind `hex dictation cancel` in the desktop
│       └── KDE startup -> before the bind, the pre-#579 `hex` set (with the
│           pre-#492 `cancel` on Escape), and every action under `anomaly.hex`
│           this bind doesn't ask for (`cancel`, `cancel-dictation`, and the ids
│           of shortcuts changed since, which kept their keys so the new id could
│           never take them, #898) are removed from kglobalaccel (`unregister`);
│           other apps untouched
│           // check: only_requested_shortcuts_are_shown (a stale grant stays
│           // out), kde_actions_hex_no_longer_binds_are_removed (the list only);
│           // the KDE removal is not run on a Plasma session
│       └── KDE's approval dialog -> Hex waits for it up to 30 min, not 60 s:
│           giving up closed the session, and KDE then kept every shortcut
│           with no key and never asked again (#898). A bind left unapproved
│           (dismissed, canceled, abandoned) is noted in
│           `kde-unapproved-shortcuts`; the next bind removes those ids while
│           they still have no key, so the dialog asks again; a key set in
│           System Settings since stays
│           // checked in a headless KWin 6.8 session (xdg-desktop-portal-kde
│           // 6.8.2): the dialog stays up with Hex "Enabling…"; with the note,
│           // keyless ids are removed and the dialog returns; without it they
│           // stay; Hex is listed under Applications in System Settings ›
│           // Shortcuts. Approving with OK was not exercised (no input there)
├── Recording HUD -> Dot scope, as on macOS, on the
│   │   top or bottom edge, recording with the voice's level, then transcribing
│   │   until accepted output finishes; Hidden position -> none   // linux.hud
│   ├── Display -> picked at each capture start (#486)
│   │   ├── X11 -> the monitor under the pointer (XRandR monitors); while shown,
│   │   │   checked every 250 ms; moves once two checks agree (#807)
│   │   ├── Layer-shell (niri, Sway, Hyprland) -> no output pinned: the
│   │   │   compositor's focused output
│   │   └── GNOME, KDE (XDG_CURRENT_DESKTOP) -> the primary display
│   └── Rune overlay in Hex's process (rune_hud, rune_hud.wgsl on the GPU)
│       ├── Wayland -> layer-shell overlay layer, no keyboard, empty input region
│       ├── X11 -> override-redirect window, empty input shape
│       ├── Wayland without layer-shell (zwlr_layer_shell_v1; GNOME) and XWayland
│       │   (`DISPLAY`) -> only the HUD opens on XWayland: override-redirect ARGB
│       │   window, notification type, empty input shape, primary monitor's edge,
│       │   scale from Xft.dpi; the rest of Hex (window, portal shortcuts) stays
│       │   on Wayland (#465)
│       │   // seen in headless mutter 51 + Xwayland on apollo: HUD at the top
│       │   // centre, animating, focus unchanged, the pointer over it hits the
│       │   // window below; not yet on a real GNOME Shell session
│       │   ├── Meeting capsules take clicks, so they open there only with Labs ›
│       │   │   Call recording controls on GNOME on and Xwayland already running
│       │   │   (meetings.md, labs.md; #1274)
│       │   ├── Annotate's capsule and ink take clicks too, so they open there only
│       │   │   with Labs › Annotate on GNOME and Xwayland already running; the ink
│       │   │   takes pointer regions and no keys, Esc can't stop drawing there, and
│       │   │   the capsule's pen button does (capture.md, labs.md)
│       │   ├── Drawn on the compositor's GPU only, opaque when its driver offers
│       │   │   no translucent alpha (NVIDIA), never moved to an iGPU or lavapipe
│       │   │   that can't make its swapchain (#979, #982)
│       │   └── Surface refuses its configuration -> that overlay alone fails with
│       │       wgpu's reason (Live text or the HUD); the window and other overlays
│       │       keep running; the next show tries again
│       │       // checked with a noop-backend Rune test; not on NVIDIA hardware
│       ├── Window's Rune app panics anyway, or Rune returns a startup error
│       │   -> `hex window show` errors and `hex status` reports the window
│       │   and HUD stopped, with the reason (#979)
│       ├── Neither (Rune refused it, or no layer-shell and no XWayland)
│       │   -> process.log warns once; `hex status` reports `hud_limitation`
│       │   and the listener's notice or warning (a missing wl-copy outranks it)
│       └── No window loop (no GPU or display, `hex dictate`) -> no HUD
├── Tray host (StatusNotifierWatcher) -> Tray icon: click opens the window; menu as Windows' (tray_menu, linux/tray, linux/icons, desktop/tray_icon, rune::Tray)
│   ├── Symbolic theme icon -> `anomaly.hex-symbolic.svg` written to hicolor/scalable/apps at startup, its name served beside the pixmap
│   │   ├── Plasma 6, GNOME's AppIndicator extension, Waybar -> draw the mark in the panel's text color
│   │   │   // check: the_tray_icon_is_a_recolorable_symbolic_svg,
│   │   │   // the_tray_icon_lands_in_the_users_scalable_hicolor_theme,
│   │   │   // a_dangling_tray_icon_link_is_not_in_place
│   │   │   // not checked against a real KDE or GNOME panel
│   │   ├── Plasma 6 -> recolors only with icon themes that follow the color scheme (Breeze, Papirus); else the SVG's default color
│   │   ├── Plasma 5 -> also prefers IconName but has no panel palette, so the SVG's color there is untested
│   │   ├── A running Plasma session -> told to reload icons after Hex writes the file, since KIconLoader lists hicolor folders at startup
│   │   ├── A user's own file at that name -> kept and named; a dangling link or failed write -> the name is unset
│   │   └── Default color -> the same light/dark guess as the pixmap
│   ├── Mark (pixmap) -> white; black when the desktop's color scheme prefers light (portal Settings color-scheme)
│   │   ├── Read once when the tray is created; a later switch applies at the next launch
│   │   ├── // check: only_an_explicit_light_color_scheme_gets_the_dark_mark
│   │   │   // decodes the portal value
│   │   └── Shown by hosts that don't use theme names, or can't find the name (Waybar falls back)
│   └── No tray host -> No icon; status and sounds remain available
├── Microphone -> System default or a saved device; `--device` overrides
│   └── System default -> ALSA's default, resolved at each stream open // no live follow
├── General > Launch at login -> writes or removes ~/.config/autostart/anomaly.hex.desktop (X-Hex-Managed; linux/login_item)
│   ├── Background portal (GNOME, KDE; xdg-desktop-portal 1.20+) -> the portal writes it (X-XDP-Autostart=anomaly.hex; linux/background)
│   ├── Installed by mise or asdf -> starts the version manager's shim, if it runs Hex from home
│   ├── An entry without the mark -> error beside the row; Hex never replaces it
│   └── A user target wants hex.service (~/.config/systemd/user/*.wants/hex.service:
│       Home Manager's programs.hex.autostart, `systemctl --user enable`) -> on;
│       turning it off errors, naming where to disable it; no entry is written (#1164)
│       // check: an_enabled_systemd_unit_starts_hex_at_login (the link scan only)
├── Microphone failure -> Same recovery as macOS, except an interrupted dictation is never kept (RecoveringAudioInput)
│   ├── Stream fails -> Cancel an unfinished capture (macOS transcribes and copies a held one), "reopening" failure, retry with backoff
│   ├── Startup open fails -> Retry with backoff; the listener keeps running
│   └── Released while idle, on-demand open fails -> Failed entry; stays released
│   └── Overload (Xrun) glitch -> Counted; the polling thread logs one line per burst; not a failure
├── Paste -> Restore the previous clipboard 500 ms after the shortcut  // linux.clipboard-restore
│   ├── Labs > Keep dictation on clipboard (off) -> no snapshot, no restore; an
│   │   earlier paste's pending restore stands down and the dictation stays (labs.md)
│   ├── Before the transcript -> read every offered format (32 MiB, 1 s shared)
│   │   text and image/png first, then the rest, and offer them back in the
│   │   offer's own order; empty offer -> cleared again; nothing is read once
│   │   the deadline has passed
│   ├── One format unreadable, or over the remaining cap -> that format is
│   │   skipped and the rest still come back; an INFO line says how many came
│   │   back and names the first eight skipped
│   ├── No format could be read, or the clipboard's owner changed while it was
│   │   being read -> transcript stays; the all-failed error names the first
│   │   eight skipped formats and how many more there were
│   ├── Later paste, copy, or another client replaced the transcript -> left alone
│   ├── Paste only reached the clipboard -> transcript stays for Ctrl+V
│   └── Wayland without ext- or wlr-data-control -> transcript stays (KWin offers ext, #719)
│       ├── Settings › Output -> note pointing to Labs › Restore clipboard on GNOME while it is off
│       └── Labs `gnome-clipboard-restore` on and this user's Xwayland already running
│           -> snapshot, check, and restore through XWayland's CLIPBOARD (#1111)
│           // never connects first, so never starts XWayland; checks: xwayland_* in
│           // linux/clipboard.rs, gnome_desktops_point_to_the_clipboard_lab_until_it_is_on;
│           // not run on GNOME
│   └── Wayland: wl-copy, then the shortcut, chosen at the first paste  // linux.wayland-paste
│       ├── wl-copy not on PATH (direct installs add no packages) -> Settings and
│       │   `hex status` warn while the listener runs; capture and transcription
│       │   still run, and the paste fails naming wl-clipboard before a helper
│       │   is spawned
│       ├── Asked by a modifier shortcut -> wait for its portal release + 250 ms
│       │   (2 s cap without a release); after a hex command -> 250 ms
│       │   // Hyprland merges held physical modifiers into virtual-keyboard keys (#353)
│       ├── Shortcut fails partway -> releases the keys it still holds
│       │   // portal path unit-tested; X11 untested
│       ├── Compositor offers zwp_virtual_keyboard_manager_v1 and wtype is installed -> wtype
│       │   └── V sent on evdev KEY_V (placeholder releases first), so Hyprland's
│       │       binds read Ctrl(+Shift)+V, not Ctrl(+Shift)+Escape (#444)
│       ├── Otherwise -> RemoteDesktop portal: keyboard-only session, approved once
│       │   (onboarding or Settings can ask first, `setup.paste-access`)
│       │   (restore token, persist_mode 2) -> keymap-resolved evdev keycodes
│       │   // wl_seat keymap: a letter needs one code across every group that
│       │   // has it (Dvorak v = KEY_DOT, AZERTY a = KEY_Q), while
│       │   // Ctrl/Shift/Enter must be on 29/42/28 in every group; a letter
│       │   // no group has still goes out on its pc105 key (a lone ru: KEY_V),
│       │   // since the keysym path searches only the active group; else the
│       │   // whole chord as keysyms; KWin 6.8's fake input and Mutter lose a
│       │   // keysym outside the active group
│       │   ├── 15 s after the last paste -> session closes when its grant restores, so
│       │   │   GNOME's remote-control indicator shows only around pastes (#718); a grant
│       │   │   that can't restore keeps its session; KDE keeps it too, for the
│       │   │   paster's lifetime, since Plasma notifies on every session start (#2508)
│       │   ├── Session showed the dialog -> 250 ms for it to hand focus back
│       │   └── Restored or open session -> 150 ms after wl-copy (minus the reopen), so the target holds the new
│       │       selection before Ctrl+V (GNOME pasted nothing or the old clipboard, #578)
│       │       // fixed delay, no acknowledgement; not observed on a real GNOME session
│       └── Neither works -> Failed: "Copied to the clipboard; press Ctrl+V…" naming
│           the RemoteDesktop portal; the clipboard keeps the transcript
└── Portal identity -> Register as `anomaly.hex` (reverse-DNS: GNOME 49/50's
    shortcuts provider refused the bare `hex`, so binding failed, #579)
    ├── Installs ship anomaly.hex.desktop (StartupWMClass=anomaly.hex); windows
    │   report app_id / WM_CLASS anomaly.hex -> one launcher entry
    ├── Icon=anomaly.hex -> packages install hicolor 16–512 px PNGs (deb/rpm refresh
    │   the icon cache); otherwise Hex writes them to ~/.local/share/icons/hicolor
    │   at startup and fixes Icon= in managed entries it wrote (#652)
    ├── Startup -> Hex's own managed hex.desktop (launcher, autostart) renamed
    │   to anomaly.hex.desktop; the installer does the same; unmanaged ones kept
    └── No anomaly.hex.desktop in XDG data dirs -> Hex writes
        ~/.local/share/applications/anomaly.hex.desktop (X-Hex-Managed),
        NoDisplay when an older package's hex.desktop already shows Hex,
        removed once a package ships anomaly.hex.desktop; never rewritten while
        its program is there; pointed at this Hex (or a version manager's
        shim) once its program is gone or superseded by that shim (#810)
        // KDE bindings and RemoteDesktop grants stored under `hex` are asked again once
        // checked by unit tests only, not on a GNOME 49 session
GNOME portal shortcuts (#723)            // linux.gnome-shortcuts
├── New GNOME or KDE Plasma setup -> dictation default Ctrl+Alt+Space (GNOME's
│   window menu and Plasma's KRunner keep Alt+Space; #898)
│   // checked in a headless KWin 6.8 session: the portal's dialog flags Alt+Space
├── Granted key GNOME keeps (Alt+Space, Super+Space, Alt+Tab, Print, …) -> red note under the row
├── Keys changed in GNOME Settings -> ShortcutsChanged -> pressing them dictates at once;
│   the row note, the Status hint and `hex status --json` listener.shortcuts show them
└── Annotate's shortcut -> its own row under dictation's in General
    // observed on apollo: gnome-control-center 51.1 provider, portal-gnome 50, stand-in Shell
Enable / Disable (General, tray)         // linux.listener-persist
├── Saves enable_on_launch -> a disabled Hex stays disabled after a restart (#575)
└── GNOME (XDG_CURRENT_DESKTOP) Disable -> after the listener's session closes, a
    fresh session binds no shortcuts: GNOME Settings stores that empty set, so
    Hex's entries leave its list; Enable shows GNOME's approval dialog again
    // never on KDE: an empty bind drops the user's keys (#482)
GNOME startup -> pre-#579 `hex` set listed in GNOME Settings -> removed through
    gsettings (reset its shortcuts, drop it from `applications`); other apps untouched
    // observed on apollo with gnome-control-center 51.1's provider, portal-gnome 50,
    // a stand-in org.gnome.Shell, not a full GNOME session
```

Clipboard restoration lives in [linux/paste.rs](../../src/linux/paste.rs)
(generation and timing) and [linux/clipboard.rs](../../src/linux/clipboard.rs)
(snapshot and serving). Observed on apollo (2026-10-29), in nested headless Sway:
`native_wayland_clipboard_shortcut` restores a two-format clipboard and keeps an
empty one empty; a full dictation from a file-fed ALSA microphone pasted into a
GTK target, then Ctrl+V pasted the original text; an X11 round trip on Xwayland
restored a 3 MB image through INCR. Not observed on X11 dictation, KDE, or
Hyprland. `collect_formats` (a skipped format, all lost, an empty offer, an
empty read, read order, and the deadline cutoff) is unit-tested against fake
reads only. The owner-change guards are not covered by any test: X11 compares
the selection's owner window, so the same window replacing its contents mid-read
is not seen, and Wayland compares the selection offer after one roundtrip.
The Wayland paste choice lives in [linux/paste.rs](../../src/linux/paste.rs) and
[linux/remote_desktop.rs](../../src/linux/remote_desktop.rs), whose
[keymap.rs](../../src/linux/remote_desktop/keymap.rs) reads the desktop's
keymap. Observed: the wtype path in the headless-Sway smoke and the
virtual-keyboard probe on niri (apollo). niri's portal frontend advertises
RemoteDesktop but its GNOME backend fails CreateSession, so wtype stays first
there. The RemoteDesktop paste itself (KWin, Mutter) has unit tests for its key
sequences and its keymap resolution only and is not yet observed on a real
desktop: it sends evdev keycodes where the keymap confirms each letter and
Ctrl, Shift, and Enter (`ctrl:swapcaps` and the like fall back to keysyms), and
sends a letter no group has on its pc105 key, rather than keysyms, which fixes
KWin 6.8's fake input, where a Latin `v` under a Russian layout arrived as a
bare `v` with Ctrl stuck down (#1939), and Mutter, which drops a keysym outside
the active group. Neither fix is observed on a real KDE or GNOME session
either. The KDE session exemption (`closes_idle_remote_desktop` in
[linux/session.rs](../../src/linux/session.rs)) is unit-tested only, not
observed on Plasma. The modifier wait (`ShortcutKeys` in
[linux/input.rs](../../src/linux/input.rs)) has unit tests for its timing only;
it is not yet observed on Hyprland, and a modifier held past the wait still
changes the paste. wtype numbers its keymap from evdev code 1 in order of first
use, and Hyprland matches binds through its configured layout, so a bare
`-k v` read as Escape and a Ctrl+Shift+Escape bind swallowed Terminal paste
([#444](https://github.com/anomalyco/hex/issues/444)). Hex now releases 46
never-pressed placeholder keysyms first so V lands on `KEY_V`;
`wtype_sends_v_on_the_v_key` checks the numbering against wtype's rule, not a
real Hyprland session.

```ts
Windows beta                             // x64 (Vulkan GPU) and ARM64 (CPU)
├── Install -> Setup.exe or install-windows.ps1 -> hex.exe + install.json receipt
├── Launch -> DLLs load from System32 and Hex's folder only, never PATH (#587)
│   └── DX12 compiles shaders with FXC (System32), never a dxcompiler.dll on PATH
├── GPU -> Rune draws with DX12 only (Vulkan only without a DX12 adapter) on
│   the lowest-power adapter that drives a display (an unused integrated GPU
│   beside the monitors' card would present every surface across adapters);
│   a GPU whose device fails hands the window to the next adapter (#662)
│   ├── Hidden startup -> no Direct3D device and no DX12 adapter enumeration:
│   │   Rune makes its DX12-or-Vulkan choice on the first real GPU use (rune
│   │   53c7f60, rune#200), so a tray launch no longer initialises Direct3D
│   │   on every GPU, which NVIDIA's App can take as a game launch (#1308;
│   │   not yet observed with NVIDIA's App, and the x64 speech model's Vulkan
│   │   device on the same GPU may still trigger it)
│   ├── Tray at rest -> no Rune GPU device until something shows: the capture
│   │   capsule, picker, ink, and live text hold no window or GPU surface
│   │   while hidden, and make both at each show (Rune release_gpu_when_hidden);
│   │   the HUD gives its surface back at each hide too (release_gpu_after, a
│   │   zero idle on Windows; Linux keeps it 5 minutes), then the device goes
│   │   ├── Dictation shortcut's modifiers down (the left Alt) -> Rune
│   │   │   makes the device and the HUD's pipeline on a thread (warm_gpu_with,
│   │   │   unclaimed for 30 s they go), so the HUD shows without them; this
│   │   │   first call also makes the DX12-or-Vulkan choice
│   │   └── A show with no device and no warm-up waits for one (about 200 ms
│   │       plus the HUD pipeline's 75 to 115 ms on an RTX 5070 Ti; Rune's
│   │       overlay_gpu_lifecycle measures the device part); a cold first show
│   │       pays the adapter check too, roughly 150 ms
│   └── A launch that crashed drawing (gpu-draw.pending, written when Rune
│       starts the launch's first GPU device, left in the data folder, same
│       version; cleared by the first frame on any surface, a clean quit, or
│       an orderly session end - Rune's quit hook runs on WM_QUERYENDSESSION
│       and WM_ENDSESSION before the process may end, also for a windowless
│       launch; a tray launch that never draws writes none) -> high-performance
│       GPU (WGPU_POWER_PREF=high), warned in process.log, until Hex updates
│       or the file is deleted
├── Present cadence -> after any change the window presents every refresh for
│   1 s, then rests: windowed G-SYNC/FreeSync sees one rate change per
│   interaction, not a swing per hover (#689; Rune e2ea18e4; not run on Windows)
├── Transcription GPU -> ggml's Vulkan, discrete before integrated, CPU last;
│   a failed GPU load retries on the CPU; the GPU record and Settings ›
│   Speech engine work as on Linux (#856)
│   ├── A run that fails out of memory -> CPU for this process, said in
│   │   Speech engine until Hex restarts (#1984; not run on Windows)
│   ├── A launch that crashed starting the GPU (gpu-start.pending, a version
│   │   and crashed attempts) -> the next launch retries once; a second crash
│   │   keeps the CPU, warned in process.log, and Settings › Speech engine
│   │   says "Choose Auto or GPU", which clears the marker and turns the GPU
│   │   on at restart; an update deletes a marker from another build
│   │   (#662; not run on Windows)
│   └── hex status -> Speech names the loaded device ("Vulkan — …"), or "CPU
│       (GPU start crashed twice)" after the marker forced the CPU
├── Hex app -> General, tray, HUD; Enabled = shortcut works in every app
├── HUD display -> the monitor under the pointer (GetCursorPos) at each capture
│   start, inside that monitor's work area, at its own DPI (#486); while
│   shown, checked every 250 ms; moves once two checks agree (#807)
├── HUD start -> a fresh first-frame attempt (own baseline, clock, and one
│   recovery hide): no frame in 1.5 s -> hidden at once (not the exit fade,
│   which needs a frame), so Rune releases the window and GPU surface. A
│   frame-less attempt also backs the HUD off, so a GPU that can't present
│   one is not asked to make a window, swapchain and device on every
│   dictation: no show for 1 min, doubled per failed attempt in a row to
│   30 min. Only a first frame its own attempt's resolve sees resets it; a
│   late one keeps the backoff but leaves the HUD up for that recording,
│   and a recording whose own attempt failed gets no further show either
│   (one warning with the cooldown). A blocked start places and hides once,
│   building nothing. A second start restarts a pending attempt, a previous
│   attempt's reason never hides the new one, and each attempt hides at most
│   once. Rune's overlay failure is the Settings/log reason only, still
│   shown while the HUD backs off. Logged once per reason; Settings'
│   listener warning (dictation.md, rune_hud::FirstFrame)
├── Launch again -> Running app shows its window; hidden (login) launch -> no-op
├── Tray icon -> White Hex mark; light taskbar (SystemUsesLightTheme) -> black mark
│   └── Theme read when the tray is created; a later switch applies at the next launch
├── Tray menu (tray_menu, shared with Linux)    // maintain.tray
│   ├── Recording / Transcribing… status line and tooltip while the listener works
│   ├── Show Hex · Paste last dictation (refocuses the app the menu covered)
│   ├── Speech model: downloaded models, ✓ on the selected one; none while one prepares
│   └── Update item · Enable / Disable Hex · Quit Hex
├── Single-modifier shortcut -> Left / Either / Right side picker, as macOS
├── Recordable keys -> letters, digits, F1–F24, named keys, mouse buttons (#794)
│   ├── Punctuation -> saved by virtual key (`oem 1`…`oem 102`), so a layout
│   │   switch keeps it; every keycap (Settings chips, recorder, tray) shows
│   │   what it types on the layout now (`Ù`, `HotkeyKey::keycap`, #815)
│   ├── Numpad -> `Num 1`, `Num *`, `Num Enter`, apart from the top row and
│   │   matched by scan code + extended flag, so Num Lock off still matches;
│   │   the recorder runs its own keyboard hook for them (`NumpadWatch`), so
│   │   Num Lock off records Num 8, not Up (#815)
│   ├── Menu, Pause, Scroll Lock -> may stand alone
│   ├── Insert, Home, End, Page Up, Page Down alone -> Labs › Special keys
│   │   alone; off, a saved one goes back to its default
│   └── Any other key -> the recorder says "Unsupported key" and starts over
├── Left Alt -> Hold or Toggle -> Capture -> Stream (GPU) -> Paste
├── Toggle's cancel shortcut (Escape by default) -> Discards a toggled recording;
│   the hook takes it only while one records. Otherwise Escape -> Nothing (#786)
├── One modifier alone (left Alt by default, #2278) -> Shortcut; Alt/Win release masked (no menu, Start)
│   ├── AltGr (a synthesized Left Ctrl plus Right Alt) -> Never presses the default;
│   │   an either-side Alt would press on every AltGr character
│   ├── Another key joining the hold -> Its chord; the press cancels at once
│   │   (Labs › Hold through other keys keeps a hold a second or more old)
│   └── Another modifier the chord doesn't name -> Its chord too: cancels, and
│       Toggle's press locks nothing; Alt's release goes unmasked, unless Hex
│       swallowed a key of that chord (Paste Last's V re-arms the mask)
├── Microphone switched off in Settings › Privacy -> Onboarding Microphone row -> Allow opens it
├── Microphone -> System default or a saved device
│   ├── Windows System default -> The default endpoint's listed cpal device, never
│   │   cpal's default handle: its monitor leaves thread-pool threads STA, which
│   │   then fails ActivateAudioInterfaceAsync (RPC_E_CHANGED_MODE)
│   │   └── No default endpoint resolves -> a listed stand-in microphone (built-in
│   │       first); the next route change resolves the default; nothing usable -> error
│   ├── Windows default input changes -> Switch once capture is idle // endpoint
│   │   notification client bumps the input route; Windows never opens a default
│   │   handle, so cpal's default-change report never applies
│   └── Windows default input reports no default format -> Hex's own WASAPI paths
│       (the endpoint by id, as when picked by name), then other formats it lists;
│       the read error keeps its cause (#1845, unverified on real Windows)
├── Paste -> Waits for Shift, Alt, and Win; a held Ctrl is kept and harmless
│   └── Ctrl+V carries scan codes from the foreground layout, so Chromium apps
│       see KeyboardEvent.code: VS Code's terminal takes it as Paste instead of
│       sending ^V to a TUI inside it (#618; from VS Code's source, not run)
├── Update staged (managed install) -> Tray, sidebar footer: Restart to update
├── Another release (unmanaged install) -> Tray, sidebar footer: Download update
├── Another channel chosen -> Tray, sidebar footer: Switch to Stable 2.3.1 (confirm)
├── Otherwise -> Sidebar footer: Check for updates
└── Modes, History, Activity, transformations -> as on macOS
```

Windows updates follow `maintain.updates` ([updater.rs](../../src/updater.rs),
`updater/windows.rs`). CI test builds never check. The tray item and tooltip
([windows/rune.rs](../../src/windows/rune.rs)) follow the published update
state. They pass the cross-compiled Clippy check, but no one has clicked them
on a real Windows PC. The white tray mark vanished on a light taskbar
([#460](https://github.com/anomalyco/hex/issues/460)); a light taskbar now gets
the same mark in black (`a_light_tray_gets_a_dark_mark_with_the_same_shape`
checks the pixels, not a real taskbar). Releases are Authenticode-signed once Azure Artifact
Signing is configured ([windows-signing.md](../windows-signing.md)).

**Known defect, outside Hex:** Discord's game detection matches the running
`hex.exe` to *HEX: Shards of Fate*
([#445](https://github.com/anomalyco/hex/issues/445)). The executable name stays
as it is because the managed install layout, updater, shortcut, and login entry
use it. The workaround is to hide the entry in Discord's Registered Games
([Windows guide](../windows.md#known-issues)). No check covers it.

No Windows Meetings or local transcription API. See the
[Windows guide](../windows.md) for the parity table, streaming notes, and
release flow; [check-windows](../../.github/workflows/check-windows.yml) builds,
lints, and tests x64 and builds and tests ARM64 on every pull request. Physical
smoke tests on Windows 10 and on ARM64 hardware have not been run.

```ts
Linux updates                            // maintain.linux-updates
├── Direct install (versions/ + install.json receipt) -> release service -> staged -> sidebar footer: Restart to update
├── Source install (source-build marker) -> Sidebar footer: Updated from your source checkout
├── Local build (HEX_LOCAL_BUILD=1, every OS) -> Sidebar footer: 3.4.0 · Local build
│   └── Hover or read its accessible description, or run `hex --version` -> full commit and updates off; no checks or channel row
├── pacman (AUR hex-bin: /usr/bin, root-owned) or Nix store -> Sidebar footer: Updated by your package manager
└── Anything else (a downloaded executable) -> Sidebar footer: Download update
```

The install kind comes from `updater/linux.rs`. The 2026-10-23 apollo
observation (a staged update and the sidebar button) predates the release
service and is not evidence for it.

No Linux meetings. Wayland shortcuts need either the desktop's GlobalShortcuts portal
or the user's own keybindings for the `hex` commands; Hex reads no input
devices. Click-through and a physical portal or keybinding press still need
native evidence. Its [smoke](../../scripts/test-wayland-paste.sh) explicitly
isolates `HEX_APPLICATION_SUPPORT_DIR`, pastes while another keyboard holds
Ctrl and Alt, opens the window through a second launch, and checks that
closing it leaves the same process answering.

Shortcut rules are pure and tested once for every source in
[linux/input.rs](../../src/linux/input.rs): hold, double-tap lock, toggle,
double-tap only, Escape ending nothing, Paste Last, and command idempotence,
plus X11 auto-repeat filtering and a portal release that never arrives
(Hyprland's Lua binds drop it when the modifier comes up first; see
[dictation.md](dictation.md)). The X11 grab tests run on an isolated display in
CI; `commands_and_grabs_share_the_x11_listener` drives grabs and commands
through one listener. [scripts/test-linux-service.py](../../scripts/test-linux-service.py)
checks that commands fail without a listening Hex.
On 2026-10-22 the portal path was observed on niri with xdg-desktop-portal-gnome
using synthetic uinput keys against an isolated service: granted triggers,
hold and Escape while held (Escape no longer cancels since #786), double-tap lock, Paste Last, and commands. A
physical keyboard press has not been recorded.

`linux.hud`: [linux/rune_hud.rs](../../src/linux/rune_hud.rs) keeps the
listener's generation slot (an old listener cannot hide a restarted HUD) and
turns its reports into [rune_hud.rs](../../src/rune_hud.rs)'s events;
reports before the window's app exists are dropped. The shaders render
offscreen with `cargo test --bin hex rune_hud_frames -- --ignored`
(2026-10-28 on apollo: Dot scope frames match the Metal look; 2026-10-29 on
apollo: recording, processing, and done after Dot scope became the default).
The GTK HUD it replaced is deleted. The text pill was observed on
2026-10-22 in a headless sway on apollo (isolated data root, real microphone,
`hex service`): `hex dictation start` showed the pill at the top edge and it
was gone after `hex dictation stop` and transcription. The Rune overlay itself
was also observed passing a virtual-pointer click through to the window below
and leaving keyboard focus where it was (Rune `examples/background`). Not
observed: X11 with Hex, niri, KWin, Mutter.

See the [Linux guide](../linux.md), [linux/app.rs](../../src/linux/app.rs),
[linux/portal.rs](../../src/linux/portal.rs), and
[Linux CI](../../.github/workflows/check-linux.yml) for existing checks.
The [direct installer/updater](../../src/updater/linux.rs) and
[installer tests](../../scripts/test-install-linux-release.sh) do not establish
a published, supported-host-validated signed release. [Nix](../nix.md) owns its
package updates; Hex updates only managed direct installs. Its Home Manager
module merges `programs.hex.settings` into Hex on every activation through
`hex settings apply` ([control-cli.md](control-cli.md)); `nix/checks.nix`
evaluates the module, not a live activation.

```ts
Linux process                            // maintain.linux-service
├── hex / hex app -> Hex running? -> Activate{activation token} over the socket -> exit 0
│   ├── No Hex, hex.service loaded -> session.env -> systemctl start (Type=notify) -> Activate
│   ├── No Hex, no unit or custom data dir -> This process becomes Hex
│   │   └── Custom data dir -> Keep the supplied desktop environment; no user-manager query
│   │       or Wayland socket discovery (unless HEX_SYSTEMD_UNIT explicitly opts in)
│   └── Two racing launches -> One Hex; the other forwards and exits 0
├── hex start / desktop autostart / hex service -> Same, without the window
│   └── No WAYLAND_DISPLAY (none at all, or only Xwayland's DISPLAY)
│       -> Adopt the manager's live WAYLAND_DISPLAY, else the newest answering socket
│       (#756); behind DISPLAY only for an explicit wayland session type, or an unset
│       one whose manager names a live display. An explicit x11 or tty login keeps its
│       X display. Launchers adopt first, so session.env agrees. The shortcut backend
│       follows the adopted Wayland session: the portal, or the `hex` commands where
│       it has none
├── hex dictation|paste-last -> Socket worker -> Listener queue (never the UI thread)
├── hex dictate -> Foreground listener; adopts the session's Wayland display like hex service;
│   `hex` commands reach only the service listener, never it (#1506)
├── hex status -> Published status; hex quit / Quit Hex / launcher action -> Drain, exit 0
├── Any instance stop still draining after 20 s -> _exit before the unit's 30 s TimeoutStopSec
│   └── A transcription or paste cut by the deadline is lost; checked by unit tests only,
│       no observed systemd stop
├── Stop / SIGTERM after a full window queue -> The window's quit is retried until taken
├── Open the window -> Maps at 980×760 (min = max), tiling WMs float it -> Resizable from 860×560
├── Close the window -> Keep microphone, accepted jobs, model preparation, and HUD
│   └── Uncommitted shortcut capture -> Cancel and restore the prior listening state
├── Crash -> systemd Restart=on-failure -> Hex without a window
├── Update -> Restart the unit -> Hex reopens the window
├── hex restart without a display (SSH) -> Keep the saved session.env
├── Saved XAUTHORITY file gone (a reboot: Xwayland's cookie is new each login)
│   -> Take the user manager's current XAUTHORITY, else drop it (#465)
├── Window fails (no display, no GPU) -> Logged; Hex keeps running
└── Startup -> transcribe.cpp's Vulkan instance first, on the main thread
    └── then the model load and the window's wgpu instance never create one at once
```

Sources: the process, host, and window seam in [linux/app.rs](../../src/linux/app.rs),
the single-instance socket and systemd lifecycle in
[linux/service.rs](../../src/linux/service.rs), the CLI in
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
`hex dictate` command loads its own snapshot once. A settings file that isn't
valid JSON, or isn't a JSON object, starts Hex with defaults and shows the parse
error, but no longer blocks dictation; saving over it is refused, even after it
is fixed, until Hex restarts. Its two-second poll keeps the running settings
when the file isn't a JSON object, and adopting a fixed or removed file also
clears that refusal, so the window's next save works without a restart. A load
that fails outright still blocks capture until a successful save or a restart.
Dismissing that failed-load error does not authorize recording with defaults.
Feedback is initialized at these entry points, so a delayed listener start
cannot reset a newer live volume selection.

Checks: [test-linux-service.py](../../scripts/test-linux-service.py) exercises
the real socket and process lifetime without a display, installed model, or
audio device; it passed on apollo (Arch, niri) on 2026-10-22.
It also checks custom-root isolation with a stale Xauthority path and an
answering Wayland socket nearby: no `systemctl` call or adopted display.
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
[linux/app.rs](../../src/linux/app.rs) checks the failure gate and recovery after a
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
Linux General > Sounds > Sound volume     // dictate.feedback.linux; X11 + Wayland
├── Default / older settings -> 50%
├── Slider at 0 -> No recording tones; the last level is kept
└── Release above 0 -> Save and apply live; preview the start sound
Linux General > Sounds > a set            // dictate.sound-theme
└── Classic keys (default), Kalimba, and five more sets -> Press to save and play; Play only previews

Capture                                  // hex app and hex dictate
├── Shortcut starts capture -> Queue start sound immediately, before audio arrives
├── Hold passes 300 ms -> While dictating mute/pause/lower begins, never at the press
├── Finish retained recording -> Stop sound at the release, while the 200 ms
│   tail is still captured; a muted output unmutes first   // dictate.stop-tone
│   └── Clip turns out near-silent or speechless -> Stop sound already played,
│       nothing pasted, no error sound or note; the log has its level
├── Brief discarded tap -> Start sound, but no stop sound or transcription
└── Dictation pasted -> Done sound; failures play the error sound
```

The shared [feedback player](../../src/feedback.rs) uses Hex's bundled sounds
([Feedback](dictation.md#feedback)) on every platform; only macOS wake/sleep
use `afplay`.
Decoding happens before the Linux capture loop; the output device opens for
the first tone and is released shortly after the last one (about ten seconds
on macOS, five minutes on Windows, one second on Linux), as described in
[macOS feedback](dictation.md#feedback). A fatal stream error (the output became
unavailable or invalid) drops the output, so the next tone reopens it. Playback
admission is bounded and nonblocking. Output preparation failures are logged
without blocking dictation. The Settings window saves the volume to
`settings.json` and the service reloads it without restarting the listener. The start sound does not wait
for the 300 ms capture-retention threshold; input delivery and audio-device
latency still apply. **While dictating** mute, pause, or lower does: the audio loop
requests it when the capture's audio reaches 300 ms past the press, and not
during a release tail. Until 2026-10-22 the service muted at the press. This
ordering is checked by reading the loop, not by a test or a muted device.

Checks: `bundled_sounds_decode_without_an_audio_device` and
`feedback_admission_never_waits_for_playback` in the player.
`start_sound_precedes_audio_and_does_not_change_short_tap_discard` in
[linux/dictation.rs](../../src/linux/dictation.rs) exercises the production start
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
TypeScript SDK + local service             // macOS, Linux, Windows
├── Caller records audio -> Explicit helper -> Raw transcription
│   └── Linux, Windows -> No packaged helper; command: hex service --embedded
├── Caller streams audio -> Running app's /streams -> Partials, segments, final
│   └── Dictation first; uses the loaded model only (docs/streams.md)
├── Running app's /transcriptions -> Listener's engine when it holds the model
│   ├── Otherwise the service's own copy of the requested model
│   ├── Whole-clip run -> A Whisper clip's silent edges dropped, and a window
│   │   the detector hears no speech in skipped (see dictation)
│   ├── ?process=global -> Global mode's output processing (Labs › Process API transcripts)
│   └── ?timestamps=segment|word -> The model's own segment or word times
│       └── 422 unsupported-timestamps when it has no such level (Labs › Timestamps in API transcripts)
├── A web page's fetch -> Never allowed: no CORS, OPTIONS is 401
├── Running desktop capture API -> Separate microphone ownership protocol
│   ├── No capture silence gate           // model output, unlike hotkey dictation;
│   │   └── its whole-clip run still drops a Whisper clip's silent edges,
│   │       skips a window with no speech, and drops hint echoes
│   ├── Linux, Windows -> The listener records it (dictation::service owner)
│   │   ├── With the microphone chosen in Settings, as the shortcut does
│   │   ├── Shortcut or `hex dictation` records -> 409 dictation-busy
│   │   ├── Client's dictation records -> shortcut starts and ends nothing
│   │   ├── Lapsed lease or owner's cancel -> only that dictation is dropped
│   │   ├── Microphone lost -> finish answers microphone-lost (no kept clip)
│   │   └── Start with "partials": true -> GET /dictations/{id}/partials
│   │       └── SSE {stable, tentative} while the model streams (GPU)
│   ├── macOS -> No partials: dictationPartials false; the SSE ends at once
│   └── hex service -> serviceCapture false, 404 service-capture-unavailable
└── Distribution
    ├── SDK 0.3.0 -> Published
    └── Turnkey signed helper -> Unfinished
```

Linux and Windows serve the same local API from the running app
([linux/app.rs](../../src/linux/app.rs), [windows/rune.rs](../../src/windows/rune.rs))
and from `hex service [--embedded]`; a failed start only logs, and Hex runs on
without it. On Linux, the embedded service answered `/health`,
`/capabilities`, `/models`, and transcribed a WAV with Parakeet Unified
English on the Vulkan iGPU (October 30). The running app's shared-engine path,
`/streams` on a live Linux or Windows listener, and anything on Windows beyond
`cargo xwin clippy` (x64) are unverified.

Linux `/dictations` (#981) shares the macOS HTTP contract and the owner,
lease, and level logic in [dictation/service.rs](../../src/dictation/service.rs);
the listener ([linux/dictation.rs](../../src/linux/dictation.rs)) records it
without a hold threshold or release tail. Unit tests cover ownership, lease
expiry, busy refusal beside a shortcut capture, and the HTTP round trip
(`local_api` tests now run on Linux). A live Linux microphone run through the
SDK is unverified.

Windows `/dictations` uses the same owner in the listener
([windows/dictation.rs](../../src/windows/dictation.rs)), so a client gets
Hex's microphone choice, pre-roll, and While dictating behavior, and the HUD
shows the recording. Live text is opt-in per dictation: `POST /dictations
{"source": "…", "partials": true}` holds a live-text demand for the
recording's stream, and `GET /dictations/{id}/partials` (owner token, once)
sends `data: {"stable": "…", "tentative": "…"}` after each change until the
dictation ends; it is provisional, and `/finish` returns the transcript.
`/capabilities` reports `dictationPartials`. Checked live on Windows x64
(start, partials subscription, finish, a silent room's empty transcript);
not run on Linux.

`process=global` (Labs › Process API transcripts, off by default) lets a host
ask the running app for the text after its Global mode's output processing, on
`POST /transcriptions` and `POST /streams/{id}/finish`
([streams.md](../streams.md#post-streamsidfinish),
[labs.md](labs.md)). The response keeps its fields and adds `processed`; off,
or without the parameter, responses are unchanged raw transcripts, and the
embedded helper installs no processor (`409 processing-unavailable`, even
with the lab off). Processing runs one at a time (`429 queue-full`).
`/capabilities` always includes `outputProcessing`.

`timestamps=segment|word` (Labs › Timestamps in API transcripts, off by
default) makes `POST /transcriptions` return the speech model's own
`segments`, and for `word` its `words`, with `timestampGranularity`; times
are milliseconds from the submitted audio's start. Absent and `none` keep the
response unchanged; off, a timed request is `403 timestamps-disabled`, and a
model without that native level is `422 unsupported-timestamps`. Nothing
estimates the times, and `/streams` and `/dictations` are unchanged
([labs.md](labs.md), [service contract](../specs/local-transcription-service.md)).

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
engines only: `transcription/stream` tests cover windows, partials, the
backlog during dictation, preemption and offline fallback, the lease, cancel,
and busy; `local_api::streams` tests cover routes, auth, and bounds; and
`transcription/engine` tests cover yielding streams closed by dictation. No
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
└── HUD Lab in release -> Only in the playground (Labs › Playground)

`hex app -- --snap OUT --scene NAME` -> Rendered fixtures    // not proof capture/download/update happened
Linux `hex` -> the Linux window on Rune, in the Hex process // dev.rune-ui
├── Calls the host directly: saves settings through it; woken on host changes
├── Wayland frame: where winit draws the client-side frame (GNOME), a floating
│   decorated window's bottom corners round with the frame's headerbar, and are
│   square when maximized, tiled or fullscreen, as the frame is; a compositor that
│   draws the decorations itself (KDE, sway, Hyprland) keeps its square content
│   // Rune #1188, checked in a nested weston and under sway, not on a real GNOME session
├── Sidebar: General, Modes, History
├── Transcription picker -> ChooseModel / CancelModel; progress from the snapshot
├── First run -> the onboarding story; Microphone only while the privacy switch is off
├── Modes: "Choose a file…" through file_dialog (FileChooser portal); drag to reorder
├── Modes: app icons (hicolor theme or pixmaps, PNG or SVG) in the picker,
│   chips, and mode rows; monogram while loading or without an icon
├── Model and application searches: Up/Down highlight, Enter picks it
├── Not yet: right-click delete
└── macOS/Windows surfaces as scenes too (Capabilities + preview data):
    Meetings (Recordings | Settings, opt-in card), Voice Filter,
    HUD Lab, Activity, permission rows, Launch at login, Dock icon, shortcut
    sides, the permissions page
macOS `hex app` -> one menu-bar process, Rune window // dev.rune-ui-macos
├── rune::App on the main thread: accessory, windowless unless setup/onboarding/Dock icon open it
├── Same AppKit loop: status item, updater, Hex/File/Window menu, Dock reopen event, the HUD pump (16 ms timer; the HUD itself is a Rune overlay)
├── Runtime thread: model preparation (menu bar: installed only), starts the listener once setup is ready
├── Listener, meeting controller, local API: their own threads
├── macos_host: AppKit event monitor records chords (Option, Fn); NativeHost does permissions,
│   login item, Hex Microphone, meeting start/stop/rename/trash/reveal, relaunch
├── Meetings pane lists the store through meeting::query (1 s rescan, sent only when changed);
│   a controller failure shows in the pane; rune::FileDialogs (file_dialog) chooses the
│   meetings folder and Modes' "Choose a file…" .app
├── First run: practice dictations from the listener's event log; a listener that failed to start
├── Sidebar and pane header move the window (Rune El::window_drag; the title bar is transparent)
├── While the window shows, its poll re-checks timed state every 250 ms (Rune Tx::wake_after); close hides it (Dock policy restored)
├── ⌘Q / menu-bar Quit / Ctrl-C quit and join
├── Meeting capsules: Rune interactive overlays (offer, call ended, ready, recording
│   with Copy agent prompt / Copy all of the in-call transcript; compact until hovered,
│   dragged to any corner)
└── Modes app icons: Finder's icon (NSWorkspace) in the picker, chips, and mode rows
Windows `hex app` -> one tray process, Rune window // dev.rune-ui-windows
├── Runtime thread owns WindowsDesktopHost (listener, model preparation, OpenCode, transformations, updater)
├── Named event: a second launch shows the window; tray Show / Enable-Disable / Quit
├── rune::App: --hidden starts windowless and makes no Direct3D device (on x64
│   the speech model still loads on the GPU through Vulkan); closing releases
│   the window and its swapchain too (release_gpu_when_closed), so a tray Hex
│   holds neither and capture tools such as NVIDIA's Instant Replay don't take
│   it for the game (#1308); the next open makes them again where the window
│   was and at its size; Quit or no tray ends it
├── General adds Launch at login; Terminal paste in Labs; chord capture polls
├── HUD: shared indicator model + CPU shader in a Rune overlay
├── Modes: "Choose a file…" opens rune::FileDialogs (file_dialog) on its own thread
├── Modes app icons: the executable's first icon (PrivateExtractIconsW)
└── Not yet: custom caption
iOS prototype (`HEXMobile`) -> Record/transcribe/copy and keyboard handoff
├── Speech model preparation: launch dictates with Apple Speech on iOS 26 (an OS
│   asset, no app download) or Parakeet V2; a preparation that says nothing for 30 s, or
│   nothing new for 120 s, ends as "The speech model isn't ready. Try again." (time iOS
│   has Hex suspended doesn't count), so "Checking the speech model" cannot stay at 0%
│   forever. Apple's reservation and asset status are watched at 60 s the same way, a
│   reservation or release that never answers is forgotten, and a keyboard check that
│   hangs counts no strike toward Hex's handoff. The keyboard's own check has its own 30 s
│   limit, retries after a real background trip, and a late answer can't reopen it. Unit
│   tests only (`DictationControllerTests`, `SpeechModelInstallTests`,
│   `AppleSpeechAssetReservationsTests`); no physical iPhone run checks either deadline.
├── Experiments: Full keyboard, Apple Intelligence cleanup, Custom vocabulary,
│   Hinglish, Greeklish, Two-language dictation, Adaptive touch, German spatial autocorrect,
│   Swipe typing in more languages, Korean keyboard (두벌식), Dvorak punctuation keys, Comma
│   and period keys, Quick accents, Emoji shortcodes, ABC under the emoji key, Keyboard
│   themes (Black, White, Navy, Forest, Ember), Two-thumb selection, Delete swiped words
│   letter by letter, Hide word suggestions, Join the keyboard when unfolded, Learn each
│   language separately
├── Full keyboard's languages (Settings › Keyboard › Languages, with Full chosen): one or
│   more layouts, the space bar names the current one and a slide switches letters, accents
│   and autocorrect, and the input mode reports the language typing
│   (`UIInputViewController.primaryLanguage`, which Apple documents as superseding the
│   Info.plist's en-US; unit-tested, whether the host's spelling follows it is unverified on
│   a device); Italian restores unambiguous unaccented spellings on space ("perche" →
│   "perché", "citta" → "città", `KeyboardLanguage.accentRestorations`) and offers "è" and
│   "c'è" in the strip for "e" and "ce", which are words of their own and are never changed
│   by space. Hungarian is a normal language (no experiment): its own QWERTZ rows and QWERTY,
│   its nine accented vowels held on a e i o u (á é í ó ö ő ú ü ű), the iPhone's `hu`
│   dictionary behind its autocorrect (a missing accent is the cheapest fix, "koszonom" →
│   "köszönöm", while a word of its own such as "meg" keeps its spelling), English as its
│   bilingual partner through the Latin-script rule, and no typing (spatial) model
├── Korean keyboard (두벌식) (off by default, `experiments.koreanKeyboard`): Settings ›
│   Experiments adds 한국어 to Settings › Keyboard › Languages, typed on the standard two-set
│   rows (ㅂㅈㄷㄱㅅㅛㅕㅑㅐㅔ / ㅁㄴㅇㄹㅎㅗㅓㅏㅣ / ㅋㅌㅊㅍㅠㅜㅡ, shift's ㅃㅉㄸㄲㅆ and ㅒㅖ, no
│   qwerty or azerty choice). `HangulComposer` composes each syllable in the field (한 → 하 →
│   ㅎ → nothing on delete, ㅗ then ㅏ → ㅘ and back, 닭 then ㅗ → 달고, 한 then ㅏ → 하나) by
│   deleting only the part of the run that changed and inserting what it became; space,
│   return, punctuation, a page or language switch, emoji, dictation, paste, undo, a selection
│   action, the delete-word swipe, a cursor move or the keyboard hiding commits the run. A
│   cursor move is recognized by the text before the run (a 32-character tail, so a host that
│   trims the front of its window still matches if the window holds at least those 32
│   characters plus the run), the text after the cursor, the field, and a reported selection;
│   a host that trims more than the tail or reports no selection is best-effort, and a host
│   that reports no text before the cursor joins syllables only while jamo come within about
│   half a second (a cursor move followed by a jamo inside that window can't be detected). A
│   field whose text after the cursor changes while you type (inline autocomplete) commits
│   each jamo, so syllables don't join there. Korean has no accent holds, spelling
│   dictionary, autocorrect, word suggestions or typing model, and while the
│   experiment is off it is not listed, Automatic never picks it, and a stored "ko" is
│   ignored.
│   Unit tests only (`HangulComposerTests`, `HangulKeyboardTests`,
│   `KoreanKeyboardGatingTests`): no CI compiles the iOS targets, and no physical iPhone run
│   checks it
├── German spatial autocorrect (off by default, `experiments.germanSpatialAutocorrect`):
│   with Spatial autocorrect on too, the Deutsch layout reads taps with the German
│   model Android ships (`ios/HEXKeyboard/TypingModel/de.hextm`, the same v3 file
│   and hash as `android/app/src/main/assets/typing/de.hextm`) and the capitalisation
│   sidecar beside it (`de.capitals`, built by `ios/scripts/build-german-capitals.py`,
│   each word's share written with a capital): a noun takes its capital anywhere from
│   a 95% share and after an article or determiner from 50% ("das haus" -> "das Haus",
│   "zu leben" stays), whether the decoder corrected it or it was typed right, and a
│   word that is also a common verb form or stem people type alone ("frage", "reise",
│   "kauf", "schlaf") is capped at 94 so "ich frage" and "schlaf gut" stay, as is every
│   two-letter word (a chat abbreviation like "gl", never a noun); "in" counts as a
│   determiner ("in Ruhe") but "ohne" does not. A 50-94% word with no determiner, and
│   any word the list lacks, keeps the dictionary's rule ("heute abend" -> "heute
│   Abend"), and Auto-capitalization off adds none.
│   Off, German keeps the dictionary engine and has no word list, so no letter odds,
│   model completions or bilingual partner model
├── Two-thumb selection (off by default, `experiments.twoThumbSelection`): while
│   the space bar is the trackpad, a second finger starts a selection from the cursor;
│   the finger further left moves the range's start and the other its end, each with its
│   own `TrackpadCursor.State` (the trackpad's gain, acceleration and rows). Settling the
│   trackpad at the start can move the host cursor once; after that it doesn't move while
│   selecting. `UITextDocumentProxy` can read a selection but
│   not set one, so the strip shows the text around the range with the selected part
│   highlighted, within the text the host shares (`documentContextBeforeInput`/
│   `AfterInput`). Both thumbs up shows Delete · Cut · Copy · Dictate · Cancel: Delete
│   moves to the range's end with `adjustTextPosition` and deletes its characters one by
│   one (an emoji is one), Cut copies then deletes, Copy copies and puts the cursor at the
│   end, Cancel changes nothing, and any other key dismisses them without typing. Cut and
│   Copy appear only with Full Access (the strip's clipboard rule). The range is clamped to
│   the text the host shares around the cursor when the selection starts; if that context
│   hasn't caught up yet the whole span is kept. An action runs only while the host still
│   shares the whole range (a smaller or different text dismisses it, with a reason in the
│   log). Dictate leaves the range in place and starts the keyboard's warm or in-keyboard
│   dictation there; when its words are about to insert and the field still shows the range
│   unchanged (same document and surrounding text), they replace it, otherwise they insert
│   at the cursor as any dictation would and the range stays. Cancelling or hearing nothing
│   leaves the range untouched. Where dictation would open Hex instead (no Full Access, or
│   the keyboard can't dictate here), tapping Dictate dismisses the actions and opens Hex or
│   shows the Full Access hint, leaving the text. An empty range ends like a
│   normal trackpad lift. Delete and Cut call the host's delete once per Character, so a
│   host that removes one code point per call (some Devanagari and Thai clusters) can leave
│   fragments. Off, a second finger is ignored exactly as before.
│   Unit tests only: no CI compiles the iOS targets, and no physical iPhone run checks it
├── Swipe typing in more languages (off by default, `experiments.swipeMoreLanguages`): with
│   Settings › Keyboard › Swipe typing on, a swipe reads the layout language's own word list
│   wherever one is bundled, not only English and Spanish: also French (France), Polish, and
│   German while Spatial autocorrect and German spatial autocorrect are on. A language with
│   no typing model keeps a drag as a slide to another key. Off, only English and Spanish
│   swipe, exactly as before.
│   Gaps:
│   - A word with ß (German) or œ/æ (French) has no key, so a swipe drops it from its
│     candidates ("groß", "Straße", "cœur").
│   - A swiped German noun goes in lowercase: the noun capitalisation applies to tapped
│     words only.
│   - French and Polish have no language-specific never-suggest rows (only the shared
│     ones), so a swipe's top reading isn't filtered for language-specific offensive words
│     there.
│   - The decoder's parameters are tuned on English and Spanish, so French, Polish and
│     German accuracy is unmeasured.
│   Unit tests only: no CI compiles the iOS targets, and no physical iPhone run checks it
├── Delete swiped words letter by letter (off by default, `experiments.swipeDeleteLetterByLetter`):
│   right after a swipe, delete acts as anywhere else and takes one character, instead of the
│   whole swiped word and the space the swipe added before it (#2321). The swipe's record
│   clears with that delete, as with any delete, so what remains types like ordinary text.
│   Off, delete takes the whole word back as before.
│   Unit tests only: no CI compiles the iOS targets, and no physical iPhone run checks it
├── Hide word suggestions (off by default, `experiments.hideWordSuggestions`): the full
│   keyboard's strip keeps its height, the Hex key, Undo, status, notices and the Paste chip,
│   but stops suggesting words: no typing or next-word suggestions, swipe readings, finished
│   word emoji, or the chip that keeps what you typed while a correction is still ahead.
│   Autocorrect still corrects on space, and after Delete the chip that puts back what it
│   replaced (`.revert`) stays. With nothing suggested above it, holding a strip word to add it
│   to Custom vocabulary is gone. Off, the strip suggests exactly as it did.
│   Unit tests only: no CI compiles the iOS targets, and no physical iPhone run checks it
├── Join the keyboard when unfolded (off by default, `experiments.joinKeyboardWhenUnfolded`):
│   the Duo unfolded and held upright splits as Apple's does there; on, it draws one joined
│   keyboard instead, at unfoldedWide's own 39 pt rows and 51.7 pt row pitch, 19 pt in from
│   the keyboard's own side as unfoldedTall's keys sit from its screen's, with the iPhone's
│   portrait bottom row (123 and the globe to z's right edge, the emoji key on x's column
│   when the globe shows, space after them, return from m). Apple's Split Keyboard
│   preference can't be read by a keyboard extension. Off, the split keyboard is unchanged.
│   Unit tests only: no CI compiles the iOS targets, and no physical iPhone run checks it
├── Dvorak punctuation keys (off by default, `experiments.dvorakPunctuation`): English's Dvorak
│   letters gain comma and period ahead of P on row 1, whose nine keys narrow to start at the
│   left margin and end at the same delete key, and a semicolon just left of the space bar,
│   a letter wide (`referenceW`), only where the bar still holds a letter-wide space; on the
│   Duo's split keyboard only the left half takes the two punctuation keys, on a pitch narrowed
│   so the half keeps its right edge (its top-row keys are narrower than the right half's), and
│   the semicolon takes from the left space bar, which keeps ending where it did. They type
│   < > : with a shift you press or caps lock; auto-capitalization leaves the comma. A URL or
│   email field keeps its own `.` key rather than a second key of the same kind. With the
│   experiment off, every layout is key for key the one it was; a docked iPad keeps Apple's own
│   rows. Adaptive touch learns the narrower key plane apart from Apple's Dvorak.
│   Unit tests only (`KeyboardLayoutTests`, `FullKeyboardEngineTests`): no CI compiles the iOS
│   targets, and no physical iPhone run checks it
├── Comma and period keys (off by default, `experiments.commaPeriodKeys`): a comma just left
│   of the space bar and a period just right of it, both a letter wide, on an iPhone's letters
│   page in a plain text field. On the joined keyboard the comma starts where the bar used to
│   start and the period ends where it used to end, the bar moving inward at both ends; on the
│   Duo's split keyboard the comma takes from the left half's bar and the period from the right
│   half's, each bar keeping its inner edge. Neither is added unless the bar still holds a
│   letter-wide space after them (on an upright iPhone showing both the globe and emoji keys
│   with the Hex key beside space there isn't room, so they're skipped). They type `,` and
│   `.` with shift on too. The 123 and #+= pages, a URL or email field (which keeps its own
│   `.`), an iPad, and English's Dvorak letters while Dvorak punctuation already puts both on
│   row 1 are unchanged; off, every layout is key for key the one it was.
│   Unit tests only (`KeyboardLayoutTests`, `FullKeyboardEngineTests`): no CI compiles the iOS
│   targets, and no physical iPhone run checks it
├── Quick accents (off by default, `experiments.quickAccents`): a held letter's callout opens
│   on the language's preferred accent (`KeyboardLanguage.quickAccentPrimaries`: Spanish o →
│   ó, English e → é, Italian a → à, capitals via the callout's own mapping), else the first
│   variant after the key's own where it has no entry, found where the arrangement put it, so
│   letting go in place types it. The cell under the finger at opening (`accentOpenCell`) is
│   remembered, so the preselection stands until the finger moves into another cell, where
│   sliding picks that one as before; letting go far below still cancels, and another key
│   going down while the callout is held commits the preselection. Letters only: the 123 and
│   #+= pages' held symbols, and the mark keys ˇ and ʼ, keep the key's own selected. Off, the
│   callout opens on the key's own, as before.
│   Unit tests only (`FullKeyboardEngineTests`): no CI compiles the iOS targets, and no
│   physical iPhone run checks it
├── Emoji shortcodes (off by default, `experiments.emojiShortcodes`): in a plain text field
│   that takes emoji (the emoji key shows), with autocorrect on and secure entry off, while
│   the full keyboard's text before the cursor ends in a colon and a name after whitespace or
│   at the start of the field (":pizza": the first character a letter or digit, at least two
│   letters or digits, then only a-z, 0-9, "_", "+" and "-", case-insensitive), the strip
│   shows up to three emoji the bundled index names (`EmojiShortcode`, over `EmojiLibrary`
│   and scripts/build-emoji-data.py's data) instead of its words: an exact name or alias
│   first, then the names and keywords the token starts, one per family; your emoji use breaks
│   exact ties and moves prefix matches up. A tap takes the whole token back and types the
│   emoji with its skin tone, counted like one from the panel; typing the closing ":" of a
│   name typed in full (":pizza:") types the emoji instead. The emoji search, fields with
│   autocorrect off and secure fields get neither. No match leaves the strip and typing as
│   they were, a colon inside a word ("abc:pizza") is not a token, and off, nothing changes.
│   The setting is in the App Group, and without Full Access it reads off. Only names the
│   index weighs as names or aliases complete: ":heart:", ":joy:" and ":tada:" are keywords
│   in the data and stay text, and ":smile:" completes to 😊, its first alias, not 😄; adding
│   them needs more aliases and a regenerated index, which scripts/build-emoji-data.py can't
│   make from the repo alone (Unicode's emoji-test.txt 18.0 and CLDR 49 are not in the repo).
│   Unit tests only (`EmojiShortcodeTests`): no CI compiles the iOS targets, and no physical
│   iPhone run checks it
├── Learn each language separately (off by default, `experiments.learnPerLanguage`):
│   every typing correction and dictation fix is tagged with the language it was typed
│   or dictated in; on, the keys learn a language's typos and rejections from that
│   language's corrections and the untagged ones, and a dictation applies only the fixes
│   made in its own language and the untagged ones (a fix learned in en-US applies in
│   en-GB); learned words are one list per keyboard language
│   (`keyboard.learnedWords.<code>`, seeded from the shared `keyboard.learnedWords`
│   list), with bilingual autocorrect's partner keeping its own list and fixes; Delete
│   what Hex learned erases the shared list and every language's; off, everything is the
│   one shared list and learning it was before. Unit tests only: no CI compiles the iOS
│   targets, and no physical iPhone run checks it
├── Two-language dictation (off by default, `experiments.twoLanguageDictation`, iOS 26+):
│   while Apple Speech dictates with `SpeechTranscriber`, one recording is read twice, in
│   the language dictation resolved and in the first other keyboard language
│   (`LanguageSettings.keyboardLanguages`, mapped with `dictationLanguage(for:)`) whose
│   `SpeechTranscriber` model is installed and not set aside as failing; nothing downloads
│   for it. Each reading keeps its timed runs (characters, timing, transcription
│   confidence), and `TwoLanguageMerger` cuts both at the earlier of two word ends within
│   150 ms, scores a reading by its confidence-weighted seconds clipped to each segment, and
│   chooses a reading per segment with a 0.35 s cost per language change (ties to the
│   primary). A seam word is dropped only when its span overlaps the last kept word's by half
│   the shorter one. A switch keeps the runs' own characters and adds a space only when
│   neither side has whitespace; when the second language wins nothing, the primary's
│   original text is used unchanged, so CJK and Thai spacing, punctuation and untimed runs
│   stay as Apple gave them. The text runs through the same pipeline with the primary
│   language. No installed second language, a failed second analyzer, or no timed words in
│   either reading dictates as before; a primary reading that finished silent reports empty
│   so the caller throws its no-speech error without transcribing again; off, the path is
│   unchanged.
│   The app, keyboard, widgets, and tests built on a macOS 26 GitHub runner with 33 tests
│   passing (run 45005571072, the latest observed verification); nothing is measured on an
│   iPhone.
├── Custom vocabulary (off by default): a local list of up to 100 terms; every
│   engine spells matched words the listed way before cleanup and again after
│   formatting
├── Recognition hint: Apple Speech (iOS 26) contextual strings, best effort;
│   Apple documents them only for DictationTranscriber, and Parakeet's batch
│   path receives none
└── Proof limits: unit tests only; no CI compiles the iOS targets
Android keyboard -> English, Spanish, German, Bokmål, French and Italian typing assistance
├── Hands-free dictation keeps the screen on -> the keyboard's window and Home's add Android's
│   `FLAG_KEEP_SCREEN_ON` (no permission) while a dictation is opening, listening or transcribing,
│   and clear it once idle, so the display timing out can't hide the IME, whose leave cancels the
│   recording, or take Home out of sight, which cancels it too. Idle again, the screen sleeps
│   normally. Checks: it compiles and the existing unit tests pass (`./gradlew :app:compileDebugKotlin
│   :app:testDebugUnitTest -x prepareSpeechModel` on OpenJDK 21 and Android SDK 36), but no automated
│   test covers the flag, and nothing was tried on a device or emulator. The repo has no Android CI.
├── Symbol pages -> Gboard's ?123 and =< pages (Pixel 8 emulator, API 36), the same on every
│   layout: ?123 draws 1–0, then @ # $ _ & - + ( ) /, then * " ' : ; ! ? between the page switch
│   and delete, and =< draws ~ ` | • √ π ÷ × § Δ, then £ ¢ € ¥ ^ ° = { } \, then % © ® ™ ✓ [ ]
│   the same way, so ` types again, and #, _, +, *, = and every other printable ASCII character
│   types from the letters, one of the two pages, or a hold. The bottom row is ABC , 1234 space .
│   enter under ?123 and Gboard's ABC < 1234 space > enter under =<, whose own rows have no < or >;
│   a held < or > keeps the marks it had there. Spanish's ¿ and ¡ are the holds on ? and ! (the
│   rows have no keys of their own for them). Checks: `SymbolPagesTest` in android/app/src/test
│   (both pages' keys, every printable ASCII character for each layout and local currency, no key
│   drawn twice on a page) and `./gradlew :app:testDebugUnitTest :app:compileDebugKotlin` passed
│   175 tests on 2026-11-24 on OpenJDK 21 and Android SDK 36 (offline, `-x prepareSpeechModel`);
│   the repo has no Android CI, and nothing was tried on a device or emulator.
├── Português, Nederlands, Polski or Русский with no pinned layout -> English's keys and
│   accents (DictationLanguage.layout is null), with English's word list only standing in for
│   the keys' geometry: the strip keeps what was typed and your own dictionary words as
│   completions, and text shortcuts, double-space period, capitals and the literal chip stay.
│   No English completions, next words, autocorrection, tap correction, swipe typing or emoji
│   keywords. A layout pinned in Keyboard > Keyboard layout brings that layout's model.
│   Checks: `aDictationLanguageWithoutItsOwnLayoutHasNoTypingModel` in InsertionTest.kt and
│   `standInOffersYourWordsAndShortcutsButNoEnglishCompletionsOrNextWords` /
│   `standInTurnsOffAutocorrectButKeepsYourTextReplacements` in StandInTypingTest.kt; not
│   tried on a device.
├── Keyboard > Suggest swear words (off) -> opt in to profanity; listed slurs never offered
├── Type a listed word in full -> keep its spelling on space or tap its literal chip
├── An Italian or French elision typed without its apostrophe -> while Autocorrect is on, in a word
│   field with the it or fr model, and with no whole-word fix already reading the token ("diro" ->
│   "dirò", so no "d'iro"; a fix that splits the token counts only when it is the elision's own two
│   words, "l ho" for "l'ho", never "non c'è" for "nonce" or "la e" for "lae"), the strip offers the
│   article and the word after it joined up: "laltro" -> "l'altro", "dellanno" -> "dell'anno",
│   "unaltra" -> "un'altra", "lho" -> "l'ho", "lheure" -> "l'heure", "daccord" -> "d'accord". The
│   longest elision the language has wins ("dell'anno", never "d'ellanno"), and the word after it
│   must be one of the model's own starting with a vowel (accents and ligatures included) or an h,
│   with a unigram cost of at most 98 eighths of a nat ("insalata" 98 and "uva" 93 fit; "om" 99,
│   "ae" 100, "ol" 104 and the rest of the reported junk 114 to 120 do not). Italian elides the h
│   of ho, hai, ha and hanno alone (l'ho, l'hai, l'ha, l'hanno, never l'hotel); French elides its h
│   generally (l'homme, l'heure). A word of the model's own list is no elision ("lago", "della",
│   "dove", "sanno"), and a spelling only the books have as typed ("laltro", "lanno", "larte",
│   "lheure") is read as one only when the word after it is likelier by at least 8.2 nats (about
│   1,450 times, since a books cost already includes a 0.92-nat backoff): "allodio" ("all'odio"),
│   "sensei", "couvres", "terrance", "quantico", "noh" and "dok" are not, while a run-together
│   spelling the books lack ("lestate", "lospedale") is read as one by the commonness rule alone.
│   A suggestion only, never a fix of its own: space leaves the word as typed, and a decoder fix of
│   its own ("dell'anno", "un'altra") is unchanged. With Labs > Keyboard > Smart quotes on, the
│   strip shows the ’ and a pick inserts it.
│   Checks: `ElisionSuggestionTest` (10: both languages' elisions, the longest prefix, the model's
│   own words, the books' words, an accentless word the decoder fixes, junk remainders, a split fix
│   that is not the elision, the Italian h, English unchanged and a picked elision left as it is)
│   and `./gradlew :app:testDebugUnitTest -x prepareSpeechModel`: 205 tests pass on 2026-11-24 on
│   OpenJDK 21 and Android SDK 36 (platform 36, build-tools 35.0.0, NDK 27.2.12479018). A throwaway
│   scan of the shipped lists reads 63 of the 80,990 Italian and 127 of the 108,519 French
│   .valid-only words as an elision, where the first cut read 1,809 and 2,869; the rest are mostly
│   the books' run-together spellings of real elisions ("larte", "l'opera", "d'histoire"). The repo
│   has no Android CI, and nothing was tried on a device or emulator.
├── Fields that ask for no suggestions, completions or filtering (a search box; the Google app's
│   TEXT|MULTI_LINE|NO_SUGGESTIONS is 0xa0001) -> suggestions, next words, emoji suggestions, swipe
│   typing, tap correction, text replacements and double-space period stay; only an automatic
│   autocorrection on space or punctuation is off, and no autocorrection is highlighted in the strip,
│   since the field asked for none of its own suggestions. Your text replacements still apply there
│   and still show their highlighted chip.
│   Checks: `FieldTypingTest` (7/7) in android/app/src/test covers the field's classification;
│   the strip's chip and the space and punctuation gates are read from `KeyboardEngine` and `TypingWord`.
├── Fields marked private or no-learning (a browser's incognito tab) -> normal suggestions,
│   next words, autocorrection, swipe typing and double-space period, because Android's
│   `IME_FLAG_NO_PERSONALIZED_LEARNING` only says that nothing typed there is learned. Undoing
│   an autocorrection there still restores what was typed, but the rejection is not remembered
│   for later sessions, and a picked emoji does not join the picker's recents. Checks:
│   `FieldTypingTest` in android/app/src/test (7/7) and
│   `./gradlew :app:testDebugUnitTest :app:compileDebugKotlin` passed on 2026-11-24 (169
│   tests in all, on OpenJDK 21 and Android SDK 36); the repo has no Android CI, and
│   nothing was tried on a device or emulator.
├── A TYPE_NULL field (a terminal) -> its text stays on the editor's side, so Hex reads and edits
│   none of it. Delete, and a held Delete, send one KEYCODE_DEL key press per press instead of the
│   surrounding-text delete, which reports success but deletes nothing there; Delete's slide is off,
│   the space bar's cursor slide steps with arrow keys, and Enter sends KEYCODE_ENTER when the
│   editor has no action. Suggestions, autocorrection, swipes and smart quotes were already off in
│   such a field.
│   Checks: `editorsThatKeepTheirOwnTextTakeKeyEvents` in `FieldTypingTest`
│   (android/app/src/test) covers the field's classification; the key presses are read from
│   `KeyboardEngine` and were not tried on a device.
├── Labs > Keyboard > Symbols on letter keys (off) -> hold a letter from any of QWERTY's three
│   rows for its symbol first, with the letter's accents after it; the marks are
│   Gboard's own (the ?123 page draws the same ones): a s d f g h j k l for
│   @ # $ _ & - + ( ) and z x c v b n m for * " ' : ; ! ?, so ! and ? are reachable and
│   no mark sits on two letter keys. Symbols follow the letter, so AZERTY q/w and QWERTZ y get
│   none where their layouts draw them below row 1. With Local currency on, d holds the
│   phone's symbol in $'s place. Row 1 keeps its digit while it shows one: with Number
│   row off it hints and holds the digit first. With it on, the top row's letters show and
│   hold Gboard's symbols in the hidden digit's place (q for %, p for }, r for =), while a
│   letter that already has a row 2 or 3 symbol keeps that one wherever the layout draws it
│   (AZERTY a and z keep @ and `*`, QWERTZ z keeps `*`). The top row's symbols
│   stay on row 1, so a layout that draws q, w or y lower is unchanged there. Off, the keys
│   are unchanged. `heldLettersOfferSymbolsWhenTheLabIsOn` in InsertionTest.kt covers the
│   holds; `theHomeAndBottomRowsShowGboardsSymbols`, `noSymbolIsDrawnOnTwoLetterKeys` and
│   `theLocalCurrencyTakesDsSymbol` in KeyboardPageTest.kt cover the two rows, the currency
│   and that no mark repeats, beside `withBothLabsTheTopRowShowsGboardsSymbols` and
│   `theTopRowSymbolsStayOnTheTopRow` for the top row; `./gradlew :app:testDebugUnitTest
│   :app:compileDebugKotlin -x prepareSpeechModel` passed 198 tests in
│   https://github.com/anomalyco/hex/actions/runs/45450568457 (Temurin JDK 21 and Android SDK 36,
│   GitHub-hosted); not tried on a device or emulator. The repo has no Android CI.
├── Labs > Output > Signature (off) -> one editable line (up to 200 characters; empty adds
│   nothing, default "Sent from Hex") on its own line at the end of each keyboard dictation,
│   added once; the next dictation starts on a new line below it, and a signed dictation
│   never continues the signature line. Home's preview and Copy stay unsigned, and Hex's
│   own fields are never signed.
│   Unit tests plus a passing `./gradlew :app:testDebugUnitTest :app:compileDebugKotlin`
│   on a GitHub-hosted runner (https://github.com/anomalyco/hex/actions/runs/45268305325, the
│   latest observed verification); the repo has no Android CI.
├── Labs > Keyboard > Number row (off) -> 1 2 3 4 5 6 7 8 9 0 above the letter rows on
│   every layout (en, es, nb, de, fr, it), typed like normal keys. The first letter row's
│   corner digits and digit-first hold are hidden while it shows; accents on hold stay, and
│   the Symbols on letter keys hints and holds still work: row 1's first ten letters show a
│   symbol instead of the digit, Gboard's such as q for % and p for }, or its own home- or
│   bottom-row symbol where it has one, like AZERTY a for @. The keyboard is one row taller
│   and the Hex menu matches that height; ?123, the symbols pages and the pads keep their keys
│   and are a row shorter than the letters while it is on. Off, the page is unchanged.
│   KeyboardPageTest covers it, including the top row's symbols, and passed in
│   https://github.com/anomalyco/hex/actions/runs/45450568457 (testDebugUnitTest + compileDebugKotlin,
│   GitHub-hosted); not tried on a device. The repo has no Android CI.
├── Labs > Keyboard > Local currency (off) -> the phone's region picks the ?123 page's
│   currency symbol in $'s place from a fixed set of single-character symbols: GBP £, EUR €,
│   JPY and CNY ¥, INR ₹, PHP ₱, KRW ₩, RUB ₽ and UAH ₴. USD, a locale with no country, a
│   region with no currency (Antarctica), a first language whose region is not a country
│   (Latin American Spanish, es-419) and every other currency keep $. Only the first
│   system locale with a country is read, and its `Currency.currencyCode` rather than a
│   localized symbol chooses the character, so the same phone shows the same one in any
│   language. Holding the key offers $ first, then $'s held symbols without a second copy
│   of the local one, in their order. The =< page's money row (£ ¢ € ¥) shows $ in the slot of a
│   local £, € or ¥, and gives only ¥'s slot to $ for ₹, ₱, ₩, ₽ and ₴, so £, ¢ and € stay drawn,
│   no page draws a symbol twice, and $ is always on one of them; $'s holds drop the local symbol
│   itself, whichever it is, so their list never repeats it.
│   The letters page reads it too, for d's held symbol and corner hint in place of $;
│   otherwise only the symbol pages read it, and a number, PIN, phone or date pad, including
│   the symbols page's own 1234 pad, draws none and reads none. Off, both pages, d's hold
│   and every other hold are unchanged.
│   Checks: `LocalCurrencyTest` (rows, holds, ten keys a row and region cases, 7/7),
│   `SymbolPagesTest` (5/5) and `theLocalCurrencyTakesDsSymbol` in KeyboardPageTest.kt in
│   android/app/src/test, and `./gradlew :app:testDebugUnitTest :app:compileDebugKotlin`
│   passed 198 tests in https://github.com/anomalyco/hex/actions/runs/45450568457 (Temurin JDK 21
│   and Android SDK 36, GitHub-hosted); not tried on a device or emulator, and the repo has no
│   Android CI.
├── Labs > Keyboard > Emoji suggestions (off) -> on the English layout, while a word
│   that names an emoji is being typed (smile -> 😊, joy/haha -> 😂, heart -> ❤️), the
│   strip's last slot offers it beside the word suggestions, padded to that slot even when
│   it is the only one. Another layout's model never offers one. A tap replaces the word
│   and commits it with the same space rules as a word suggestion; continuing to type
│   keeps the word. Nothing matching leaves the strip unchanged. The list keeps the same
│   0.85 strong cut-off as the iPhone strip, plus a reviewed joy -> 😂 and a reviewed
│   exclusion list (number words that name clock faces, and ordinary words that name an
│   emoji's object in another sense), generated from the iOS keyboard's
│   emoji-index-en.txt (Unicode emoji-test.txt 18.0 and CLDR 49 annotations, Unicode
│   License v3, plus Hex's chat words). It is a compact word-to-emoji asset read lazily
│   only while the toggle is on; the loader keeps only emoji the device's `Paint.hasGlyph`
│   can draw (minSdk 31 draws up to Emoji 13.1), so the strip never shows tofu. Matching is
│   case-insensitive and whole-word.
│   Checks: `./gradlew :app:testDebugUnitTest :app:compileDebugKotlin` (includes
│   `EmojiKeywordsTest`) and `python3 scripts/code-shape.py --check`; the repo has no
│   Android CI.
├── Labs > Keyboard > Keyboard tools (off) -> while the strip has nothing else to show (no
    suggestion chips, Undo dictation, notice or dictation) and the field is not a password,
    PIN or other secure field, the "Hold Hex to talk" hint becomes up to four icon buttons
    without making the keyboard taller: Emoji (the bottom row's emoji key, so only on the
    letters, never on a symbol page or a pad), Paste (the editor's own
    `performContextMenuAction(android.R.id.paste)`, so Hex never reads the clipboard; shown
    only while `clipboard.hasPrimaryClip()`), Settings (reopens Hex on its Keyboard page
    through an intent extra MainActivity reads, NEW_TASK | CLEAR_TOP, replacing whatever Hex
    screen was open) and Undo (the last autocorrection reverted exactly as the Delete right
    after it does, shared through `KeyboardEngine.revert`; shown only while that correction is
    still right before the cursor, and only when no next-word suggestions fill the strip, though
    the Delete right after an autocorrection still undoes it when they do). A field with no text
    before the cursor and none after it shows them in the sentence starters' place, so they are
    there when an empty field opens; typing anything fills the strip with predictions as before.
    Changing the toggle redraws a visible strip at once, starters and tools swapping in an open
    empty field; a word being typed, a swipe's readings and a word Delete took back keep their
    line, and off, the strip is unchanged.
    Checks: `./gradlew :app:testDebugUnitTest :app:compileDebugKotlin` (includes
    `KeyboardToolsTest`) and `python3 scripts/code-shape.py --check`; the repo has no
    Android CI. Not tried on a device or emulator.
├── Labs > Keyboard > Smart quotes (off) -> in a plain-text field (not a password, email or URL
    field, or a TYPE_NULL field such as a terminal), the ' and " keys type ’ ‘ “ ”: an opening
    quote — ‘ or “ — at the start, after whitespace, or after an opening bracket or quote, and
    the closing quote or apostrophe — ’ or ” — after anything else. A word a suggestion,
    autocorrection or swipe commits goes in with ’ for its straight apostrophe ("don't" becomes
    "don’t"), your own text replacements get ’ for each straight apostrophe they contain (a " in
    one stays straight), and a dictation's straight quotes are placed by the same rules, after its
    spacing is decided, in the same plain-text fields as the keys. The strip shows the ’ a
    suggestion will insert. The Italian and French elisions the typing part above describes
    ("laltro" -> "l’altro") show and go in with ’ too. A quote key's held alternates still type
    exactly the character they show. Off, keys, picks and dictation insert the straight characters
    they did before, and existing ’ and ” are treated like ' and " for word boundaries, spacing and
    lookups. Checks: `./gradlew :app:compileDebugKotlin :app:testDebugUnitTest` passed with 205
    tests (including `SmartQuotesTest`'s 12) on 2026-11-24, on OpenJDK 21 and Android SDK 36
    (platform 36, build-tools 35/36, NDK 27.2.12479018); the repo has no Android CI, and nothing was
    tried on a device or emulator. `python3 scripts/code-shape.py --check` passes.
├── Labs > Keyboard > Password manager suggestions (off) -> while Android's autofill service —
    Bitwarden, 1Password or Google Password Manager, which must itself support Android 11's inline
    suggestions — offers logins, one-time codes or cards for the focused field, its own chips take
    the word slots' place in the strip: up to six, in the service's order, the pinned ones held at
    the edge outside the scroll. A tap lets Android fill the field; Hex lays the service's views out
    and never reads a login, a code or a card, and nothing is logged or sent. A new field, the
    keyboard hiding or the toggle turning off clears them. Off, Hex makes no inline request, so
    Android's normal autofill dropdown is exactly as before.
    Checks: `./gradlew --no-daemon :app:testDebugUnitTest :app:compileDebugKotlin -x
    prepareSpeechModel` passed 178 tests, `InlineAutofillTest`'s 3 among them (the pinned split and
    the chip sizes), on 2026-11-24 on OpenJDK 21 (Arch Linux 21.0.13) and
    Android SDK 36 (platform 36, build-tools 35.0.0, NDK 27.2.12479018) with compileSdk 36 and
    minSdk 31; the repo has no Android CI, and nothing was tried on a device or emulator.
└── Labs > Keyboard > Voice-only mode (off) -> a "Voice only" subtype of Hex, registered at runtime
    with `InputMethodManager.setAdditionalInputMethodSubtypes` (never in input_method.xml), that
    replaces the keyboard with one compact dictation bar: the strip's dot waveform and a status
    line, ✓ to finish (or to start again after a blocked or failed start) and ✕ to cancel. Showing
    it starts dictation at once, with the keyboard's guards: a password field, no text field, a
    microphone another dictation holds, or a missing microphone permission shows the reason in the
    bar, whose ✕ goes back. ✓ transcribes through the same MicCapture queue and model as the
    keyboard and inserts the text at the cursor with the same signature and continuation rules
    (commitSpeech). After the insertion, after ✕, or after a failed dictation (its note shows for
    1.2 s) Hex calls `switchToPreviousInputMethod()`, opening Android's keyboard picker when that
    returns false, at most once per dictation; an input restart right after a leave shows the bar
    idle rather than recording again. The subtype is auxiliary, so Hex's own globe-key rotation
    stays as it was. Enabling it: turning the toggle on has Android 14 and later enable the
    voice subtype itself with `setExplicitlyEnabledInputMethodSubtypes` (the full keyboard and voice
    subtypes); Android 12 and 13 have no such API, so there the toggle opens Android's Hex subtype
    screen to tick "Voice only". Turning it off clears the subtype and returns the explicit set to
    the implicit default. Reach it from another keyboard: Android gives a voice-input key one voice
    input method, preferring the system's, so HeliBoard's and other AOSP-based keyboards' mic keys
    reach Hex only when no system voice input (such as Google voice typing) is enabled; otherwise
    use the keyboard switcher or Android's picker. The IME registers the subtype again when it
    starts, which matters after a settings restore. Off, no subtype or explicit set is touched and
    Hex is exactly the keyboard it was; a voice subtype that is somehow current shows the full
    keyboard. Checks:
    `./gradlew :app:compileDebugKotlin :app:testDebugUnitTest` (with `-x prepareSpeechModel` and a
    build-tools 36.0.0 override, since AGP's default 35.0.0 is not installed) passed 166 tests,
    `VoiceOnlyTest`'s 4 among them, on 2026-11-24 on Temurin JDK 21 and Android SDK 36 (platform 36,
    build-tools 36.0.0, NDK 27.2.12479018); `VoiceOnlyTest` covers the bar-or-keyboard and restart
    decisions, `python3 scripts/code-shape.py --check` and `git diff --check` pass, and the repo has
    no Android CI; nothing was tried on a device or emulator.
Unmerged Windows / deferred features -> Not part of this map
```

Sources: [meeting.rs](../../src/meeting.rs),
[meeting/watcher.rs](../../src/meeting/watcher.rs),
[control/developer_control.rs](../../src/control/developer_control.rs),
`SCENES` in [rune_ui.rs](../../src/rune_ui.rs), [iOS guide](../../ios/README.md),
[Android typing and checks](../../android/benchmark/typing/README.md).
The iOS custom vocabulary experiment stays off by default and keeps its list in
the App Group. Apple Speech also receives it as a best-effort recognition hint,
unverified on a physical iPhone; Apple documents contextual strings only for
`DictationTranscriber`, and Parakeet's batch path receives none. iOS
physical-device proof and temporary-WAV failure cleanup remain gaps;
best-effort deletion is not guaranteed deletion.

`dev.rune-ui` lives in [rune_ui.rs](../../src/rune_ui.rs) and its pane modules;
its tests drive the Rune runtime headlessly (navigation; General rows per
shortcut source, fitting the default window with Advanced collapsed, Shortcut mode's modal, saves through the host, Quit Hex, microphone-release
confirmation, shortcut recording and Escape, conflicts; History select, two-step
delete and clear, copy; Modes add/delete, corrections, transformations (drag reorder saves once
on release), application conflicts, a chosen file joining the mode, arrow
keys in the model and application searches; the picker's ChooseModel/CancelModel; setup's download; draft
saves; and, from macOS/Windows preview data, each OS's panes and rows, Launch
at login, permission warnings, the
Meetings opt-in card and Settings view,
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
service saved a new mode and a correction. See
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

`dev.rune-ui-windows` lives in [windows/rune.rs](../../src/windows/rune.rs) and
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
and settings projections: Paste Last remains available in both build profiles.
`option_control_v_is_an_ordinary_chord` in [suppression.rs](../../src/suppression.rs)
checks that Option-Control-V, the removed meeting paste, is an ordinary chord
that discards a capture. These replace an
unconditional debug-only expectation and a test-only copy of the reservation rule;
they do not establish native keyboard delivery or meeting insertion.

`structural_event_equality_preserves_serialized_identity` in
[desktop/events.rs](../../src/desktop/events.rs) covers Activity's full-event equality across
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
