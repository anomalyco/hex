# Hex for iPhone

**Status:** Internal TestFlight app (`HEXMobile`), custom keyboard extension
(`HEXKeyboard`), and Live Activity + Control Center widget extension
(`HEXWidgets`). The containing app records a generation-scoped temporary WAV,
transcribes on-device with Apple Speech (`SpeechTranscriber` on iOS 26+) or
FluidAudio Parakeet V2 (iOS 18+), and removes each capture's file after
inference, cancellation, or failure. During a warm session the keyboard
extension sends commands and inserts completed text over the shared App Group
(`group.anomaly.hex.mobile`). Otherwise it dictates by itself (in-keyboard
dictation, iOS 26+): iOS refuses to record from a keyboard extension
directly, so it captures through `getUserMedia` in a hidden `WKWebView`
under Hex's microphone permission, writes a temporary WAV, and transcribes
it on-device with Apple Speech, downloading the speech assets on first use
(`HEXKeyboard/KeyboardLocalDictation.swift`). Audio never leaves the iPhone.

## Capabilities

- **One-tap keyboard dictation (`HEXKeyboard`):**
  - **First run (`HEXMobile/Onboarding.swift`):** Three pages. **Welcome**
    says what Hex is. **Set up Hex** is a checklist that updates every second:
    Hex Keyboard, Allow Full Access, the microphone (**Allow** asks for it;
    **Open Settings** once it's denied), and the speech model's download, with
    one **Open Settings** button (Hex's page, with Keyboards inside) and the
    steps to take there. Continue needs the keyboard added (or unverifiable)
    and the microphone; Full Access still reads Not checked, and the model
    keeps downloading. **Try the keyboard** focuses a field: once the keyboard
    has reported Full Access and text is in the field, it shows success and
    **Done** enters the app. If Full Access hasn't appeared 4 s after a
    keyboard switch or new text, it says Full Access looks off, with **Open
    Settings**; **Skip for now** enters the app. It shows once:
    `onboarding.completed` in the app's UserDefaults records Done or Skip,
    `onboarding.page` resumes a quit part-way on the same page, and a launch
    that finds the keyboard ready and the microphone allowed (an existing
    user) records completion without showing it. **Run setup again** in the
    app's settings replays it from page 1. It never shows under
    `--snap-state` except its own scenes: `onboarding-welcome`,
    `onboarding-allow`, `onboarding-try`, `onboarding-try-done`,
    `onboarding-try-off`, and `onboarding-tour`, which walks every page by
    itself for a screen recording.
  - **Setup:** The keyboard needs Full Access to reach Hex through the App
    Group. Without it, the keyboard shows the Settings path and an **Open Hex
    setup** button (`hex-dictation://keyboard/setup`). The app's setup card
    shows live status for the keyboard, Full Access, the microphone, and the
    speech model until all four are ready (after onboarding too), and opens
    Hex's page in Settings. Full Access shows as on once the keyboard has run
    with it (`keyboard.fullAccessSeenAt`): iOS reports Full Access only
    inside the keyboard, so the app can't tell on from off until then.
  - **Opening Hex:** SwiftUI `Link`, `openURL`, and `extensionContext.open`
    do nothing in a keyboard extension, so the keyboard opens Hex through
    the `UIApplication` at the end of its responder chain
    (`ContainingAppOpener`).
  - **Cold start (before iOS 26):** Tapping **Dictate** opens the containing app
    (`hex-dictation://keyboard/record?...`) with a 10-second
    handoff window, starts recording, and opens the selected host app's URL
    scheme (`messages://`, `slack://`, `claude://`, `chatgpt://`,
    `mobilenotes://`, `discord://`, `mail://`, `x-web-search://`, etc.) using
    public `UIApplication.shared.open(_:)`. Because iOS keyboard extensions
    cannot inspect the host app's bundle ID without private APIs, you pick your
    usual return app once on the `↩` chip (or jump bar), and apps without a
    registered URL scheme fall back cleanly within ~1s to the bottom-edge swipe
    bar and quick-jump chips. **Edit apps…** in the app's return menu picks and
    orders the apps the keyboard tray and jump bar offer.
  - **GPU Dot Scope (`HEXShared/DotScope.swift`, `DotScopeShader.metal`):**
    Both `HEXMobile` and `HEXKeyboard` render Hex's desktop menu-bar braided
    oscilloscope (`src/dot_scope.rs`, `src/rune_hud.wgsl`) at native display
    refresh via `CAMetalLayer` + `CADisplayLink`, pausing at 0% CPU/GPU once
    settled in idle. The keyboard extension mounts it only while a capture or
    transcription is active and shows a static `Canvas` dot row at rest, so a
    fresh keyboard process builds no Metal device, pipeline, layer, display
    link, or idle pre-roll before its first frame; the app's docked keyboard
    preview keeps the live scope. The home-indicator swipe cue's pipeline is
    compiled on first use, which only the app shows. Lock Screen / Dynamic
    Island Live Activities render a
    matching `Canvas` dot-matrix badge (`HexDotMatrixBadge`) because WidgetKit
    archives static view snapshots rather than hosting live `CAMetalLayer`s.
    Meter polling stays alive through a temporary non-recording state of the
    same capture; it resumes app levels and keyboard snapshots when the recorder
    is ready again. Capture completion, interruption, and Stop end polling.
  - **Warm session (`Off`, `5m`, `15m`, `1h`):** Picking a duration on **Keep
    mic warm** starts an `AVAudioEngine` input session immediately (default
    `15m`) so subsequent taps on **Dictate** record directly inside the
    keyboard with zero app switches; picking `Off` or tapping the header timer
    ends it.
  - **Styles:** `Polished` (sentence case + punctuation + filler trim),
    `Casual` (lowercase chat without trailing period), `Notes` (spoken
    `"bullet"` / `"next line"` markers), and `Verbatim`, with cursor-aware
    insertion spacing and repeating backspace.
  - **Full keyboard Hex key (Experiments › Full keyboard):** A tap, or a
    still hold of 0.35 s, starts dictation: in a warm session, else inside
    the keyboard (in-keyboard dictation, iOS 26+), else by opening Hex. Moving
    more than 10 pt first opens the style and **Return to** menu; releasing over a row picks it, and releasing elsewhere leaves
    the menu open for taps (`HexKeyGesture`). VoiceOver activation dictates,
    and the **Dictation styles** action opens the menu.
  - **Apple Intelligence cleanup (Experiments › Clean up with Apple
    Intelligence, iOS 26+):** Off by default. For every style but `Verbatim`,
    one on-device FoundationModels pass before formatting fixes clearly
    misheard words and spoken self-corrections, within a 2 s budget. A
    timeout, error, empty or implausible output keeps the raw transcript, and
    the toggle is disabled with a reason when Apple Intelligence is unusable.
  - **Clean up what I say (Experiments › Clean up what I say):** Off by
    default. For every style but `Verbatim`, Hex's dictation cleanup tagger
    labels each spoken word KEEP or DELETE on the iPhone and drops ums,
    repeats, and the words a spoken correction replaces ("3pm, actually 4pm"
    becomes "4pm"); a word goes only above P(DELETE) 0.98.
    `CleanupReconstruction`, a port of the tagger's `reconstruct.js`, then
    repairs the commas and capitals next to a deletion. It never adds,
    reorders, or changes words. It runs after transcription and learned fixes
    (in the app also after custom vocabulary), before Apple Intelligence
    cleanup, translation, and formatting, in in-keyboard dictation and in the
    app. The tagger is a BERT L-4/H-512 token classifier fine-tuned from
    Google's Apache-2.0 `bert_uncased_L-4_H-512_A-8` on generated examples,
    shipped as an int8 Core ML model (28.5 MB) with its WordPiece vocabulary
    once, in `HEXKeyboard.appex` (`HEXKeyboard/CleanupTagger/`). The app loads
    it from there. `CleanupTokenizer` matches Hugging Face's `BertTokenizer`,
    including its Unicode 8.0 character classes. The model loads on the Neural
    Engine or CPU when a dictation starts recording with the experiment on,
    stays loaded, and is freed on a memory warning. Cleanup gets 150 ms; a
    timeout, an error, or a result with no words left keeps the transcript as
    heard.
  - **Custom vocabulary (Experiments › Custom vocabulary):** Off by default.
    A local list of up to 100 names and phrases, applied to every engine. After
    transcription, matching words are rewritten to the listed spelling across
    case, spaces, tabs, a single hyphen, and possessive apostrophes, before
    cleanup and again after formatting, so a style cannot undo a spelling. The
    listed spelling is kept exactly, even at a sentence start (an all-lowercase
    term stays lowercase), and a term that spells its own boundaries ("iPhone 7",
    "X-ray") matches those boundaries exactly. A word that carries its own
    apostrophe only matches a term that has one, several transcript words are
    joined only when the term reads as a compound, and compound terms like
    "WeChat" can also match the plain phrase ("we chat"). On iOS 26 Apple Speech
    the list is also sent as a best-effort recognition hint; Parakeet's batch
    path gets no hint.
  - **Learn how I type (Experiments › Learn how I type):** Off by default, and
    shown whether or not the full keyboard is on. A private journal in the App
    Group (`typing-journal.json`) that never stores what you write: per-key tap
    offsets, and word-level pairs with counts and at most one word of context on
    each side. Only plain text fields that allow autocorrect count
    (`TypingJournal.allows`). Nothing leaves the iPhone.
    - **Typing:** words you erase and retype, autocorrect fixes you undo with one
      backspace, suggestions you pick, and autocorrections you turn down by
      tapping what you typed. A typo you fixed the same way twice (never a
      completion, never fixed two ways) becomes a fix on space, only a suggestion
      when what you typed is a real word; an autocorrection you turned down twice
      never applies again (`TypingLearning`, read by `AutocorrectEngine`).
    - **Dictation:** from the full keyboard and the dictation bar alike, the
      keyboard remembers each dictation by the text around it
      (`DictationEditTracker`), finds it again after edits through Hex's keys and
      on `textDidChange`, and diffs it word by word against what Hex inserted and
      the raw transcript the app or in-keyboard dictation sends with it
      (`DictationEditInference.swift`). It settles after 60 s, when the keyboard
      goes away or the field changes, on a Send return key, or when the host
      clears the field (a send). Each dictation gets an outcome (kept, edited,
      rewritten when more than half changed, undone, deleted, or redictated when
      deleted and dictated again within 20 s) and its pairs: substitutions,
      insertions, deletions, words cleanup dropped that you added back, and
      fillers Hex left that you deleted, each as heard, inserted, and final words.
      New writing before or after a dictation, changes that aren't plausibly the
      same words, edges the host's cut-short context can't show, and translated
      dictations teach nothing.
    - **Applied:** a substitution seen twice and never contradicted (you kept the
      heard words in a dictation you corrected, changed the fix back, or fixed
      the same words another way) is applied right after transcription, in
      in-keyboard dictation and in the app before custom vocabulary and cleanup,
      whole words only (never inside a longer, hyphenated, dotted, or slashed
      word) and following the original's case unless the fix has capitals. Its
      meant words join the Apple Speech hints after custom vocabulary's.
      Typing insights lists these fixes with a switch each and the share of
      dictations kept and edited; Delete what Hex learned erases everything,
      including what a running keyboard still holds.
  - **Record typing logs (Experiments › Record typing logs):** Off by default,
    and apart from Learn how I type. For testing and training prediction and
    per-user touch models: the keyboard writes every touch on Hex's keys (raw
    position, radius, `UITouch` timestamp, the key it resolved to), every layout
    with each key's frame and touch target, what each key did to the text
    (inserts, deletes, autocorrections and undos, suggestion picks, double-space
    periods, returns), the suggestions shown, the host's text around the cursor
    after each word and when the host changes it, and each dictation's steps
    (heard, learned fixes, cleanup, inserted) with its outcome. One NDJSON file a
    day in the App Group (`TypingLogs/`), kept 14 days, at most 64 MB a day;
    Experiments › Typing logs shares a day or all of them (AirDrop to a Mac) and
    deletes them. Password, one-time code, and card fields write one `skipped`
    line and nothing else. Events are buffered on the main thread and encoded and
    appended on a utility queue every 2 s. Nothing is sent. The schema is in
    [`docs/typing-logs.md`](docs/typing-logs.md).
- **Resilient capture & expiry contract (`DictationController`):**
  - Monotonic `jobGeneration`, retained `transcriptionTask`, and unique
    per-capture WAV filenames ensure an expiring warm session never drops an
    already-accepted transcription (deliverable for 30s), while a stale
    winding-down inference can never overwrite a newer capture or delete its
    audio file.
  - Phone/FaceTime/Siri audio interruptions finalize captures `>= 300 ms` so
    spoken words are not lost, cancel shorter taps cleanly out of `Listening`,
    and resume warm keepalive when iOS signals `.shouldResume`. **Always use
    iPhone microphone** (on by default) records from the built-in microphone in
    `playAndRecord` with `mixWithOthers` and A2DP output, in the app and in
    in-keyboard dictation, so music in a car or headphones keeps playing and the
    route never drops into call audio. Turning it off allows Bluetooth HFP
    microphones, which pause other audio.
- **Action Button, Control Center, Back Tap & Live Activity:**
  - `DictateWithHexIntent` (`AudioRecordingIntent` + `LiveActivityIntent`) and
    `HexDictationControl` (`ControlWidget`) start recording on first press with
    a Dynamic Island / Lock Screen Live Activity (`Stop` and `Cancel` buttons)
    and transcribe + copy to the clipboard on second press.

## Build & Test

Prerequisites: Xcode 26+ and [XcodeGen](https://github.com/yonaskolb/XcodeGen) 2.46.0+.

```sh
cd ios
scripts/fetch-cleanup-model.sh   # the cleanup tagger's Core ML model, kept out of git
xcodegen generate
xcodebuild test -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=<SIM_UDID>"
```

The cleanup tagger's model (`HEXKeyboard/CleanupTagger/HexCleanupTagger.mlpackage`,
25 MB) is a GitHub release asset (`ios-model-cleanup-tagger-1`), not a git file.
`fetch-cleanup-model.sh` downloads it and checks its SHA-256; Xcode Cloud runs it
from `ci_scripts/ci_post_clone.sh`. A new model gets a new release tag and hash.

## Simulator vs. Physical iPhone Proof Limits

- **Verified in Simulator (`HEXMobileTests` + `--snap-state` scenes):** Compilation
  across `HEXMobile`, `HEXKeyboard`, and `HEXWidgets`; App Group command/snapshot
  framing; the `DictationController` generation/expiry race and 30s post-close
  transcript delivery; 10s handoff window + URL-scheme auto-return fallback;
  interruption handling; meter recovery after injected startup and mid-capture
  recorder unavailability (including keyboard levels and heartbeat); deterministic
  style formatting; UI layouts; and first-run onboarding's launch decision (a
  fresh install shows it, a saved page resumes, a ready keyboard and
  microphone skip it).
- **Requires a physical iPhone:** Real `AVAudioSession` background keepalive
  survival across lock/app switches, live orange indicator behavior, on-device
  `SpeechTranscriber` / Neural Engine Parakeet latency, URL-scheme return into
  installed third-party apps (Messages, Slack, Claude, ChatGPT), Dynamic Island
  Live Activity interaction, and hardware Action Button invocation. Transient
  `AVAudioRecorder.isRecording` changes during warm handoff or Bluetooth route
  changes have not been reproduced on hardware; the meter recovery tests inject
  that condition through `ActiveAudioCapture`.
- **Apple Speech hint:** Custom vocabulary's contextual strings are best effort
  and unverified on a physical iPhone. Apple documents contextual strings only
  for `DictationTranscriber`, Hex sets them on `SpeechTranscriber`, and
  Parakeet's batch path gets no hint.
- **Learn how I type:** Dictation edit inference (span location, word diff,
  outcomes, the learning threshold, and applying fixes) is covered by unit
  tests only. How much `documentContextBeforeInput` and
  `documentContextAfterInput` each host app shows, and whether it calls
  `textDidChange` after the keyboard's own edits, varies by app and is
  unverified on a physical iPhone; when the context is cut short or missing,
  the dictation is settled on where it was last seen or not learned from.
- **Record typing logs:** `TypingLogTests` cover the line format, day files,
  the cap, retention, the skip rule, and a real `FullKeyboardView` typing
  through the logger, whose edits replay to the field's text. Touches there are
  synthetic samples fed to the same path `UITouch`es take; real touch radii,
  sampling rates, how often hosts call `textDidChange`, and the extension's
  write cost on an iPhone are unmeasured.
- **Clean up what I say:** `CleanupTaggerTests` check, in the simulator
  (CPU only), the tokenizer against Hugging Face on 1,891 words,
  reconstruction against `reconstruct.js` on 670 cases, and the model on the
  137 hand-written sentences at 0.98. Its labels match the fp32 checkpoint's
  on all 1,128 words, and 136 of 137 outputs match the ONNX int8 pipeline.
  The other sentence differs on two words within 0.01 of the threshold,
  which ONNX's int8 activations keep and the checkpoint deletes. In the
  simulator the model loads in about 150 ms, cleans a sentence in about
  3 ms (p95 under 8 ms), and adds about 3 MB to the process footprint,
  because the CPU leaves the int8 weights memory-mapped from the bundle. The
  simulator's GPU path took 0.8 s to load and added 30 MB, so Hex doesn't use
  the GPU. Neural Engine load time, latency, and the keyboard extension's
  footprint next to Apple Speech are unmeasured on an iPhone;
  `keyboard-capture-probe.log` records the footprint around each in-keyboard
  dictation.
- **First run:** Reading Full Access from the keyboard on page 3, the
  keyboard-switch signal behind "Full Access looks off"
  (`UITextInputMode.currentInputModeDidChangeNotification`), and Settings
  opening on Hex's page are unverified on a physical iPhone. Before iOS 26
  without a warm session, dictating on page 3 takes the cold-start path, whose
  handoff screen replaces the page.
- **No iOS CI:** Custom vocabulary is covered by unit tests only. No CI job
  compiles or tests the iOS targets, so these checks run only in a local
  `xcodebuild test`.

## Internal TestFlight releases

[Hex Mobile in App Store Connect](https://appstoreconnect.apple.com/apps/6836837368/testflight/ios)
uses Anomaly's signing team. The GitHub **Release Hex Mobile** workflow checks
`dev` at noon and 6 p.m. Chicago time, or on manual dispatch, and cuts an
`ios-beta-*` prerelease when iOS inputs have changed. These tags are separate
from desktop releases. The marketing version stays fixed across beta builds.

The Xcode Cloud workflow needs a tag start condition with prefix `ios-beta-`,
container `ios/HEXMobile.xcodeproj`, shared scheme `HEXMobile`, an iOS Archive
action, and a TestFlight post-action for **Hex Internal**. Cloud owns the build
number. Verify the run's source tag and the processed build in that group.
