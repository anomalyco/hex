# Simulator vs. physical iPhone

Part of [Hex for iPhone](../README.md).

What the simulator proves and what only a physical iPhone can.

- **Verified in Simulator (`HEXMobileTests` + `--snap-state` scenes):** Compilation
  across `HEXMobile`, `HEXKeyboard`, and `HEXWidgets`; App Group command/snapshot
  framing; the `DictationController` generation/expiry race and 30s post-close
  transcript delivery; 10s handoff window; interruption handling; meter recovery after injected startup and mid-capture
  recorder unavailability (including keyboard levels and heartbeat); deterministic
  style formatting; UI layouts; and first-run onboarding's launch decision (a
  fresh install shows it, a saved page resumes, a ready keyboard and
  microphone skip it).
- **Requires a physical iPhone:** Real `AVAudioSession` background keepalive
  survival across lock/app switches, live orange indicator behavior, on-device
  `SpeechTranscriber` / Neural Engine Parakeet latency, Dynamic Island
  Live Activity interaction, and hardware Action Button invocation. Transient
  `AVAudioRecorder.isRecording` changes during warm handoff or Bluetooth route
  changes have not been reproduced on hardware; the meter recovery tests inject
  that condition through `ActiveAudioCapture`.
- **Screen recordings with the microphone:** The simulator can't screen-record,
  so whether one picks up noise while Hex records shows only on an iPhone:
  record with the microphone on with Hex closed, Hex open and idle, a warm
  session on, and during in-keyboard dictation (WebKit's `videoChat` session,
  which Hex can't change), and compare.
- **Live words (In bar and Into field):** `LiveDictationTests` cover how results become the
  words shown, the defaults, the end of a long dictation, and Into field's
  typing, revising, taking back, stopping when the field changes or an app reports no text,
  private fields, an app's Send (three in a row, Undo after one, an edit that isn't one), and
  the words shown over the keys where Into field can't type
  (`testIntoFieldShowsTheWordsOverTheKeysWhereItCantType`). The simulator has no speech models, so there the preview and typing ran on a
  replay of Apple's results recorded on a Mac (`LiveTranscriptReplay`), in a host app, light and
  dark, upright and turned, on an iPhone SE's globe key and with no model at all (no preview).
  On an iPhone 17 Pro (iOS 27.2): the preview, Into field, a 60 s dictation, memory with
  and without, and the keyboard hidden mid-dictation with Into field (Home, then back: the
  held text pasted over its words). Unchecked: VoiceOver on a device (the test runner there has
  no VoiceOver control), hosts that lag or show part of the text (web views, Notes) and real
  speech in a room rather than a clip from a speaker.
- **In-keyboard dictation's start time:** The simulator's WebKit captures from a
  mock microphone, and on a Mac without a microphone the warm session's
  recorder can't start, so how long a real microphone takes to open
  (getUserMedia, the audio session, the recorder) shows only on an iPhone, in
  the probe log's `first audio` and `warm start` lines.
- **Apple Speech hint:** Custom vocabulary's contextual strings are best effort
  and unverified on a physical iPhone. Apple documents contextual strings only
  for `DictationTranscriber`; Hex sets them on both modules, and Parakeet's
  batch path gets no hint. On macOS 27.2.1 `SpeechTranscriber`'s text didn't
  change with them (0 of 126 clips of names and terms), and
  `DictationTranscriber` wrote the listed spelling in 28 of 132 places against
  4 without them, so for English on a recent iPhone `SoundAlikeSpelling` is
  what writes a name the listed way.
- **Dictation languages:** `SpeechLanguagePickerTests` cover the picking rule
  and `DictationControllerTests` a language change during a dictation, Automatic
  following the keyboard's language, and Spanish never reaching Parakeet. The
  simulator can't transcribe: `SpeechTranscriber.isAvailable` is false there
  and `DictationTranscriber` fails with "Model not available", so
  `AppleSpeechClipTests` (skipped unless `HEX_SPEECH_CLIPS` names a folder of
  `<language>.wav` clips) reports that. The same code (`AppleSpeechTranscriber`,
  `AppleSpeechModule`, the picker) built for macOS 26 transcribed `say` clips
  word for word: German and Spanish through `SpeechTranscriber`, Norwegian
  (`Nora`) and Hebrew (`Carmit`) through `DictationTranscriber`, which
  downloaded each model in 11–16 s. `DictationTranscriber` returned Norwegian
  and Hebrew without punctuation under every preset (the short-form presets
  returned nothing), so Polished only capitalizes and ends the sentence.
  Ukrainian is `DictationTranscriber`'s only (macOS 27.2 lists `uk-UA` there and
  not for `SpeechTranscriber`): a `say -v Lesya` clip came back word for word in
  Cyrillic, without punctuation, after an 18 s model download.
  Polish is too (macOS 27.2.1 lists `pl-PL` for `DictationTranscriber` only):
  `say -v Zosia` clips came back with every diacritic (ą ć ę ł ń ó ś ź ż), numbers as
  digits ("10 min", "19:30"), and no punctuation.
  Czech is too (macOS 27.2.1 lists `cs-CZ` for `DictationTranscriber` only, a 12 s model
  download): `say -v Zuzana` clips came back with every háček and čárka ("Příliš žluťoučký kůň
  úpěl"), numbers and times as digits ("10 min.", "19:30", "8:15") and no punctuation. It
  never goes to Parakeet, which is English-only.
  Greek is too (macOS 27.2 lists `el-GR` for `DictationTranscriber` only): `say -v Melina`
  clips came back with every tonos, final ς, numbers as digits ("10 λεπτά", "7:30"), and no
  punctuation. Lowercase lowers with Unicode's context-aware mapping, so a word-final Σ
  becomes ς ("ΟΔΟΣ" → "οδος"), as desktop's `to_lowercase` does.
  **Spanish variants** have unit tests only: a region Apple has keeps it (es-CL on a
  Chilean phone), es-419 and es-CO resolve to es-US then es-MX, before iOS 26
  `SFSpeechRecognizer` keeps es-CO and es-419 as themselves and falls to es-US, then
  es-MX, then Spain's for an unlisted region, plain `es` by the iPhone's Region,
  a picked variant survives a swipe away and back, the switch offers one variant per
  language with Chinese's variants kept apart, and the Hex key menu names the region.
  No iPhone or simulator ran the Spanish variants for this change, and the Settings
  line and menu labels weren't captured.
  Unverified on a physical iPhone.
- **Speech model fallback:** `SpeechModelInstallTests` and `StallWatchTests` run
  the fallback, set-aside, offline, stall and keyboard-handoff rules against fakes
  of Apple Speech's calls (the simulator has no speech models),
  `SpeechLanguagePickerTests` the set-aside and no-`SpeechTranscriber` picks, and `--snap-state
  full-keyboard-language-unavailable`, `keyboard-language-unavailable`,
  `language-unavailable` and `language-switched` render the messages. Which
  module fails for which language on iOS 26.x, whether a stalled Apple download
  ever moves again, the reservation limit on a full iPhone, and Parakeet's
  memory on 4 GB iPhones are unverified on hardware.
- **Keyboard languages:** `KeyboardLayoutTests` cover the list (resolution, switching,
  space-bar label, pairing, migration, the dictation pick, English & Spanish's keys,
  dictation following the keyboard, Spanish's Americas variants and the iPhone's Region),
  `HangulComposerTests` the two-set composer and the keyboard typing through it,
  `FullKeyboardEngineTests` the swipe against the
  hold-then-slide trackpad and switching in a `FullKeyboardView` (English & Spanish too,
  and a picked Spanish variant surviving the swipe away and back), and
  `AutocorrectEngineTests` bilingual autocorrect on stub dictionaries and iOS's own
  (`testABilingualKeyboardKeepsBothLanguagesWords` prints the break counts). Hungarian's
  QWERTZ rows and held vowels, typing through them in a `FullKeyboardView`, and its accent
  fixes are unit tests too (`testHungariansRowsAndHoldMatchItsLayout`,
  `testHungarianTypesAndSwitches`, `testHungarianRestoresMissingDiacritics`); no
  simulator or iPhone typed on its keys for this change. Dutch's rows, Apple's holds
  (from the runtime's Keyboard-nl.plist), typing ë ï and j́ through a `FullKeyboardView`
  and its diaeresis fixes are unit tests too (`testLetterRowsMatchApplesLayouts`,
  `testDutchTypesAndSwitches`, `testDutchRestoresAMissingDiaeresis`). Checked live in
  Messages in the simulator (English, Français, Русский). Memory (Release, iPhone 17 Pro
  simulator, iOS 27.2, Full Access, three runs, `footprint` after typing a sentence): Français
  alone 28.7–28.8 MB, with English beside it 29.7–29.9 MB; English alone 28.9–29.9 MB, with
  Français 29.5–29.9 MB; resting 18.7–19.1 MB and peaks 37–41 MB either way, so the second
  language costs about 1 MB (the simulator counts no GPU memory). A real iPhone, the strip's
  latency with two Spatial models (English and Spanish), and whether a keyboard without Full
  Access keeps the current language are unverified.
- **Keyboard layouts:** `KeyboardLayoutTests` check every language's rows,
  widths, hit frames (tiling the key area and row 1's band above it with no gaps
  or overlaps), balloons
  and accent menus at 320–440 pt, in landscape and in the iPhone Duo's postures, and the
  iPad's (`testPad…`: each size docked either way up against Apple's rows, floating, the
  hide-keyboard key, targets); `AutocorrectEngineTests` and
  `FullKeyboardEngineTests` check typing, dictionaries and English-only fixes.
  Whether the simulator or an iPhone has a `UITextChecker` dictionary for
  `nb` decides Norwegian autocorrect; how good those corrections are is
  unmeasured.
- **Learn how I type:** Dictation edit inference (span location, word diff,
  outcomes, the learning threshold, and applying fixes) is covered by unit
  tests only. How much `documentContextBeforeInput` and
  `documentContextAfterInput` each host app shows, and whether it calls
  `textDidChange` after the keyboard's own edits, varies by app and is
  unverified on a physical iPhone; when the context is cut short or missing,
  the dictation is settled on where it was last seen or not learned from.
- **Adaptive touch:** `FullKeyboardEngineTests` and `AutocorrectEngineTests` cover what
  teaches it (words that stood, picks, undone fixes, retypes; never autocorrections,
  swipes, claimed letters, strip touches or sensitive fields), contexts, the evidence
  floor, the grip fallback, the file (size, backup exclusion, damage, reset) and the guard
  (no new rewrite, no new join, names and your words kept). The autocorrect benchmark's
  held-out sentences (Tatoeba text, synthetic taps; iPhone 17 simulator, iOS 27.2) with the
  Experiment off, or on without evidence, match the keyboard without it item for item
  (2,000 of 2,000 outputs and strip hits, English and Spanish). With a profile learned from
  the 300 dev sentences, English WER goes from 5.24% to 5.16% (16 words fixed, 5 broken,
  none typed right) and Spanish from 8.34% to 8.26% (12 fixed, 3 broken); the strip's hit
  after two letters rises 0.8 and 0.3 points. With the personal centres deciding alone it
  would be 5.04% and 8.00%: the guard holds back most of the gain. Learn how I type's
  journal alone does better (5.00% and 8.02% in the same replay, offline): with both on,
  5.04% and 8.04%. In an offline replay of a grip change (learned on taps 6 pt right,
  typed 6 pt left), the profile without the fallback broke 38 English words stock got
  right; the fallback stops at the stock centres and breaks none, and then learns the new
  grip. Per-letter engine p95 changed by -0.09 to +0.04 ms against the Experiment off
  (simulator noise), peak footprint by at most +0.25 MB. Real iPhones, real grips and
  hands, host apps' text reports, Full Access off on a device (whether the switch is
  readable, the extension's own container), and how often people retype or undo are
  unverified.
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
- **In-keyboard dictation memory:** Measured only in the simulator (Release,
  iPhone 13, iOS 27.2, which enforces no limit and leaves out GPU memory; three
  runs each, the same before and after the full keyboard became the default):
  the full keyboard rests at 18.4–18.6 MB with Full Access and 18.0–18.3 MB
  without, and its first dictation reaches 30.8–31.8 MB within 3 s of the tap
  (peak 32.1–33.0 MB, so about 13 MB); the dictation bar rests at 20.7–20.8 MB
  and its first dictation reaches 27.3–27.9 MB (about 7 MB). Later dictations
  add 1–5 MB. The 32 and 24 MB thresholds rest on that plus the Metal scope's
  GPU memory, not on an iPhone's; `keyboard-capture-probe.log` records each
  step's footprint, peak and headroom, and keeps only its newest 256 KB once it
  passes 512 KB.
- **Without Full Access:** Verified in the simulator with the keyboard in a
  real hosted field (`--snap-state hosted-keyboard`) and Full Access off: a
  fresh install mounts the full keyboard and offers no in-keyboard dictation;
  keys typed through the engine autocorrect ("thw" became "the", "tk" "to",
  and with Spatial autocorrect, on by default, "ofthw" "of the"); the Hex key shows the
  hint. Scenes: `full-keyboard-no-access[-hint][-dark]`; `--keyboard
  full|dictation-bar` picks the keyboard. The simulator's keyboard reads the App
  Group without Full Access; whether an iPhone's does is unverified, which is
  why the keyboard mounts the full keyboard without Full Access whatever was
  chosen. Full Access in a simulator: a `kTCCServiceKeyboardNetwork` row for
  the containing app (`anomaly.hex.mobile`, auth 2) in the shut-down
  simulator's `data/Library/TCC/TCC.db`. Unverified on a physical iPhone.
- **First run:** Reading Full Access from the keyboard on page 3, the
  keyboard-switch signal behind "Full Access looks off"
  (`UITextInputMode.currentInputModeDidChangeNotification`), and Settings
  opening on Hex's page are unverified on a physical iPhone. Before iOS 26
  without a warm session, dictating on page 3 takes the cold-start path, whose
  handoff screen replaces the page.
- **Emoji:** `EmojiSuggesterTests` cover the index, ranking, search, the strip and
  the history, and score 100 sentences
  (`HEXMobileTests/Fixtures/emoji-context-sentences.json`) for a sensible emoji in
  the top five: 30 hand-written to tune (30/30, top-1 27), 30 hand-written and held
  out (30/30, top-1 27; written by the same hand as the chat words, so optimistic),
  and 40 written by a separate agent that saw no code or word lists (37/40, top-1 28
  before any change; 38/40, top-1 29, now). CLDR's words alone scored 26, 22 and 30.
  Apple's on-device word embedding (`EmojiSemanticIndex`, app and tests only) changed
  none of these, cost 5.3 MB that stayed after release and 100–120 ms to build, so the
  keyboard doesn't use it; Apple's sentence embedding scored 18/40. In the keyboard
  extension (Release, iPhone 17 Pro simulator, iOS 27.2, Full Access, a hosted field, no
  GPU memory, no headroom reading, `footprint` resident and peak): drawn as text, each
  emoji's image (about 48 KB) stayed in a cache of the shared emoji font that nothing
  public empties, so browsing every category reached 62.7 MB (peak 67.5) and scrolling
  past every emoji 137.7 MB (peak 142.3), still held once closed. The panel now draws
  its emoji itself (`EmojiGlyphRenderer`): a private copy of the emoji font, from the same
  memory-mapped file, replaced every 32 emoji and once drawing pauses for 0.3 s, and a
  2 MB least-recently-used cache of the bitmaps, freed with the panel and on a memory
  warning. Two runs: typing 30.6–34.6 MB, the panel open 36.4–40.0 MB, every category
  39.4–43.0 MB, every emoji 40.0–42.8 MB (peak 44.3–47.9), 31.9–35.3 MB once closed, and
  an in-keyboard dictation after it no higher. Measured again with the bar's scrubbing
  (one run, same setup): tapping every category peaked at 44.5 MB and scrolling past
  every emoji at 44.4; one slide across five categories peaked at 46.0, three slides
  back and forth across the bar at 47.5 and ten at 48.3, each back to 42–43 MB resident
  after. `EmojiSuggesterTests` check that a cell
  lands where a label drew its emoji and that drawing every emoji stays in the cache's
  limit; `--snap-state full-keyboard-emoji-probe` prints the app's footprint, the
  renderer's counts and each frame's work scrolling every emoji at 40 pt a frame
  (p50 1.6–1.8 ms, p95 2.5–6 ms against 0.6–1.0 ms drawn as text). The panel opens in
  24–29 ms the first time and 15–17 ms after (8–9 ms drawn as text, which reused the
  shared font's images).
- **No iOS CI:** Custom vocabulary is covered by unit tests only. No CI job
  compiles or tests the iOS targets, so these checks run only in a local
  `xcodebuild test`.
