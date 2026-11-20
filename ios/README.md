# Hex for iPhone

**Status:** Internal TestFlight app (`HEXMobile`), custom keyboard extension
(`HEXKeyboard`), and Live Activity + Control Center widget extension
(`HEXWidgets`). The containing app records a generation-scoped temporary WAV,
transcribes on-device with Apple Speech on iOS 26+ (`SpeechTranscriber`, or
`DictationTranscriber` for the languages it lacks, such as Norwegian, Danish,
Swedish and Hebrew) or FluidAudio Parakeet V2 (iOS 18+, English), and removes
each capture's file after inference, cancellation, or failure. During a warm session the keyboard
extension sends commands and inserts completed text over the shared App Group
(`group.anomaly.hex.mobile`). Otherwise it dictates by itself (in-keyboard
dictation, iOS 26+): iOS refuses to record from a keyboard extension
directly, so it captures through `getUserMedia` in a hidden `WKWebView`
under Hex's microphone permission, writes a temporary WAV, and transcribes
it on-device with Apple Speech, downloading the speech assets on first use
(`HEXKeyboard/KeyboardLocalDictation.swift`). iOS ends a keyboard that passes
its memory limit (about 60–90 MB depending on the model), so while less is left
than starting needs (`os_proc_available_memory`: 32 MB for the first
dictation, 24 MB once the capture web view is loaded) Dictate and the Hex key
open Hex instead, as before iOS 26. Audio never leaves the iPhone.

## Capabilities

- **One-tap keyboard dictation (`HEXKeyboard`):**
  - **First run (`HEXMobile/Onboarding.swift`):** Three pages. **Welcome**
    says what Hex is. **Set up Hex** is a checklist that updates every second:
    Hex Keyboard, Allow Full Access, the microphone (iOS asks for it when the
    Keyboard row's **Open Settings** or **Continue** is tapped, never from a
    button of Hex's that says Allow, per App Review 5.1.1(iv); **Open Settings**
    once it's denied), and the speech model's download, with one **Open
    Settings** button (Hex's page, with Keyboards inside) and the steps to take
    there. Continue needs the keyboard added (or unverifiable), not the
    microphone, so declining it never blocks the app; Full Access still reads
    Not checked, and the model keeps downloading. **Try the keyboard** focuses a field: once the keyboard
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
    setup** button (`hex-dictation://keyboard/setup`), which opens the app's
    setup sheet. Home's status line counts the setup steps left ("Finish
    setup · 1 step ›", else "Ready") and opens the same sheet: live status for
    the keyboard, Full Access, the microphone ("For dictation" with
    **Continue**, which iOS's prompt follows, until it's asked; never a button
    that says Allow, per App Review 5.1.1(iv)), and the speech model, a field to
    switch to Hex in while Full Access is unconfirmed, and **Open Settings**
    (Hex's page in Settings) while the keyboard isn't ready. Full Access shows
    as on once the keyboard has run with it (`keyboard.fullAccessSeenAt`): iOS
    reports Full Access only inside the keyboard, so the app can't tell on from
    off until then, and the sheet calls it "Not confirmed yet".
  - **Opening Hex:** SwiftUI `Link`, `openURL`, and `extensionContext.open`
    do nothing in a keyboard extension, so the keyboard opens Hex through
    the `UIApplication` at the end of its responder chain
    (`ContainingAppOpener`).
  - **Cold start (before iOS 26):** Tapping **Dictate** opens the containing app
    (`hex-dictation://keyboard/record?...`) with a 10-second
    handoff window and starts recording. Hex never opens another app: the
    handoff screen shows a quiet "Swipe bar to return" cue over the home
    indicator, and you swipe back to the app you were typing in.
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
  - **Mid-sentence dictation:** when the text before the cursor doesn't end a
    sentence (`.` `!` `?` `…` or a line break), the keyboard lowercases the
    dictation's first word, except "I" and its contractions, acronyms, words
    with inner capitals, and words the dictation language's `UITextChecker`
    dictionary doesn't know in lowercase (likely names). German keeps the case,
    since its dictionary takes nouns in lowercase; so does a language with no
    dictionary on the iPhone (`TranscriptFormatter.insertionText`).
  - **Full keyboard Hex key (Settings › Keyboard › Full):** A tap, or a
    still hold of 0.35 s, starts dictation: in a warm session, else inside
    the keyboard (in-keyboard dictation, iOS 26+), else by opening Hex. Moving
    more than 10 pt first opens the style menu; releasing over a row picks it, and releasing elsewhere leaves
    the menu open for taps (`HexKeyGesture`). VoiceOver activation dictates,
    and the **Dictation styles** action opens the menu.
  - **Push to talk:** Holding still for 0.35 s on the full keyboard's Hex key
    or the dictation bar's **Dictate** starts dictation at once with a light
    haptic (Settings › Keyboard › Haptics), and lifting the finger finishes it as
    **Done** does; a quick tap still starts it and **Done** finishes it. On
    the Hex key, sliding more than 10 pt before the hold fires opens the menu
    instead, and once dictation has started, moving the finger changes
    nothing. Push to talk needs dictation in the keyboard (a warm session or
    in-keyboard dictation); a hold that opens Hex is only a tap.
  - **Apple Intelligence cleanup (Settings › Experiments › Clean up with
    Apple Intelligence, iOS 26+):** Off by default. For every style but `Verbatim`,
    one on-device FoundationModels pass before formatting fixes clearly
    misheard words and spoken self-corrections, within a 2 s budget. A
    timeout, error, empty or implausible output keeps the raw transcript, and
    the toggle is disabled with a reason when Apple Intelligence is unusable.
  - **Clean up what I say (Settings › Dictation › Clean up what I say):** Off by
    default. For every style but `Verbatim`, Hex's dictation cleanup tagger
    labels each spoken word KEEP or DELETE on the iPhone and drops ums,
    repeats, and the words a spoken correction replaces ("3pm, actually 4pm"
    becomes "4pm"); a word goes only above P(DELETE) 0.98.
    `CleanupReconstruction`, a port of the tagger's `reconstruct.js`, then
    repairs the commas and capitals next to a deletion. It never adds,
    reorders, or changes words. The tagger only knows English (run on
    Spanish it dropped negations: "Te digo que no puedo ir" became "Te
    puedo ir"), so Spanish, German and Norwegian get word rules instead
    (`SpokenCleanupRules`): the language's hesitation sounds go (the
    styles' list below, so in-keyboard dictation, which no style formats,
    loses them too), a filler between commas goes ("este", "o sea"),
    and a correction marker set off by commas ("digo", "perdón", "no",
    "mejor dicho"; "nein", "ich meine"; "nei", "nei vent", "jeg mener")
    drops the words it replaces, counted back to the nearest earlier word
    that matches the repair's first word or is of its kind (a number,
    weekday, month, or today and tomorrow): "a las 3, digo, a las 4" becomes
    "a las 4". With no such word the sentence stays as said, and other
    languages stay as heard. It runs after transcription and learned fixes
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
  - **Custom vocabulary (Settings › Dictation › Custom vocabulary):** Off by default.
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
  - **Learn how I type (Settings › Privacy › Learn how I type):** Off by default, and
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
  - **Spatial autocorrect (Settings › Experiments › Spatial autocorrect):**
    Off by default; it needs the full keyboard with Autocorrect on. A noisy-channel decoder
    (`TypingDecoder`) replaces `UITextChecker`'s one-edit guesses on space and in the
    strip for English and Spanish: each tap is scored against every letter with a 2D
    Gaussian around the key (shifted by Learn how I type's tap offsets when it has 20
    or more taps on a letter), words are found with a beam search over a memory-mapped
    word list and over your own words (Contacts names and other `UILexicon` terms,
    one-word text replacements, words you kept, fixes Hex learned), a word-pair language
    model weighs them by the word before, a missed space is tried by splitting the taps,
    accented letters read from their base key (é from e), and what you typed is replaced
    only when the best reading beats it by a margin (more when what you typed is a word).
    Shortcuts, learned fixes, contractions, and words you kept still come first. The
    models (`HEXKeyboard/TypingModel/en.hextm` and `es.hextm`, about 3.6 MB each) are
    built from Google Books Ngram v3 by `scripts/build-typing-model.py` (see
    Acknowledgements below).
  - **Record typing logs (Settings › Privacy › Advanced › Record typing logs):** Off by default,
    and apart from Learn how I type. Its description says what it records, and
    turning it on asks first in a dialog (App Review 2.5.14): each tap and the
    text you type or dictate with Hex, including the text around the cursor,
    for 14 days, on this iPhone, to export or delete, never password, code or
    card fields. **Turn On** turns it on; **Cancel** leaves it off. Turning it
    off doesn't ask. For testing and training prediction and
    per-user touch models: the keyboard writes every touch on Hex's keys (raw
    position, radius, `UITouch` timestamp, the key it resolved to), every layout
    with each key's frame and touch target, what each key did to the text
    (inserts, deletes, autocorrections and undos, suggestion picks, double-space
    periods, returns), the suggestions shown, the host's text around the cursor
    after each word and when the host changes it, and each dictation's steps
    (heard, learned fixes, cleanup, inserted) with its outcome. One NDJSON file a
    day in the App Group (`TypingLogs/`), kept 14 days, at most 64 MB a day;
    Privacy › Advanced › Typing logs shares a day or all of them (AirDrop to a Mac) and
    deletes them. Password, one-time code, and card fields write one `skipped`
    line and nothing else. Events are buffered on the main thread and encoded and
    appended on a utility queue every 2 s. Nothing is sent. The schema is in
    [`docs/typing-logs.md`](docs/typing-logs.md).
- **Settings (`ContentView`, `SettingsPages.swift`):** Home fits one screen: the
  Hex header, one status line ("Ready" with a check, or "Finish setup · 1 step ›"),
  Dictate, and four rows that each open a short page, then **Keyboard settings**
  (Hex's page in iOS Settings) and **Run setup again**, and last **Privacy
  policy**, **Support** and **Acknowledgements**. Privacy policy (App Review
  5.1.1(i)) and Support open in Safari inside Hex (`HexLinks`, one base URL:
  `https://dev.anoma.ly/hex` until the production site serves
  `anoma.ly/hex/privacy`). Each row has a one-line description at most. The pages:
  - **Dictation:** Style, Dictation language, Translate what I say, Clean up
    what I say, Custom vocabulary (its list below the switch), Keep mic warm,
    and Always use iPhone microphone.
  - **Keyboard:** Keyboard, **Full** or **Dictation bar** (stored as
    `FullKeyboardSettings.isEnabled`), then Keyboard layout, Autocorrect and Key
    pop-ups only while Full is chosen, Haptics (the dictation bar uses it too),
    and a field to try the keyboard in.
  - **Privacy:** "Everything stays on this iPhone", Learn how I type (what it
    learned below the switch), and under **Advanced**, Record typing logs (off
    by default) with its Typing logs viewer while it's on.
  - **Experiments:** Emoji (it says it needs the full keyboard while the
    dictation bar is chosen; Clear emoji history while it's on), and Clean up
    with Apple Intelligence.

  Simulator scenes: `--snap-state settings-home` (ready), `setup` (keyboard
  added, Full Access not confirmed yet), `setup-sheet`, `setup-sheet-microphone`
  (before iOS has asked for the microphone), and
  `settings-<dictation|keyboard|privacy|experiments>[-on][-end]`. Each writes
  its page's switches to the App Group: off by default, on with `-on`.
  `-end` starts the page scrolled to the bottom. `settings-privacy-confirm`
  opens Record typing logs' consent dialog.
- **Dictation language (Settings › Dictation › Dictation language, iOS 26+):** Automatic by
  default: the first of the iPhone's preferred languages (then its region's
  locale, then US English) that Apple Speech transcribes, so a Norwegian iPhone
  dictates Norwegian with no setup; Nynorsk tries Bokmål, the only Norwegian
  Apple has. The menu also lists every locale `SpeechTranscriber` or
  `DictationTranscriber` supports, by name. `SpeechTranscriber` is used wherever
  it has the language (as before); `DictationTranscriber` (preset
  `longDictation`) covers the rest (`SpeechLanguagePicker`,
  `AppleSpeechLanguages`, `AppleSpeechModule`). The setting lives in the App Group
  (`settings.dictationLanguage`, nil for Automatic) so in-keyboard dictation uses
  the same rule and the same downloaded assets; both record the language they
  resolved (`settings.dictationLanguage.resolved`). Picking a language prepares
  (and if needed downloads) its model at once, or after the dictation in flight.
  An iPhone without `SpeechTranscriber` (A13: iPhone 11, SE 2nd generation, all
  3–4 GB) dictates English with `DictationTranscriber` on iOS 26+: loading
  Parakeet there sat at 50% (Core ML's compile) until iOS ended Hex, at every
  launch. Parakeet stays where it fits (6 GB) and before iOS 26. After Hex ended
  while loading Parakeet, the next launch waits for Set up instead of loading it
  again (`ParakeetLoadGuard`), and a Parakeet download that sits at one fraction
  for 90 s ends with a message. **When a model won't install**
  (`SpeechModelInstall`, shared by the app and in-keyboard dictation): Apple lists
  some languages whose `SpeechTranscriber` asset never arrives ("transcription.ru
  asset not found after attempted download"), so Hex then installs the same
  language on `DictationTranscriber` and goes straight to it for three days. A
  download whose progress doesn't move for 60 s is given up on. When neither
  module installs while the iPhone is online, Automatic sets the language aside
  for three days and dictates the next one ("Russian isn't available on this
  iPhone yet. Using English."); a language picked by hand says so and offers that
  one ("Try English", with **Use English** in the app). Offline or stalled, it
  says the download failed and tries again next time. Reservations make room
  under `AssetInventory.maximumReservedLocales` by releasing others first. Only
  Hex's own error text reaches the app or keyboard (`HexDescribedError`); Apple's
  and other packages' descriptions go to logs, however readable.
  The setup sheet's Speech model row names the language ("Norwegian Bokmål ·
  Apple"). Steps that only know English follow the language: Clean up what I
  say (an English tagger) uses word rules in Spanish, German and Norwegian and
  skips other languages; Clean up with Apple
  Intelligence runs only when the on-device model supports the language; the
  styles trim English fillers (um, uh, er, ah) only in English, Nordic ones (eh,
  ehm, øh, hm) in Norwegian, Danish and Swedish, Spanish ones (eh, em, mmm) in
  Spanish, German ones (äh, ähm, öhm) in German, and none elsewhere (Norwegian
  "er" is "is", Portuguese "um" is "a"); translation's source is the dictation
  language rather than a guess. Custom vocabulary works in any language.
- **Switching the dictation language in the keyboard (iOS 26+, Full Access):**
  the dictation bar shows the language beside the style ("Norsk ⌄") and opens a
  tray of chips; the full keyboard's Hex key menu has a Language row with its
  own page, and the space bar names the language faintly ("English", "Norsk").
  Both offer Automatic plus up to five languages: the one set, the ones picked
  lately (`settings.dictationLanguage.recent`), then the iPhone's preferred
  languages, each matched to one Apple Speech has (`DictationLanguageSwitch`;
  the keyboard lists Apple's languages once per process and caches them in
  `settings.dictationLanguage.supported`). Names are each language's own
  ("Español", "Deutsch", "עברית"), with a region code only where two share a
  language. A pick writes the same App Group setting as Hex's menu, so
  in-keyboard dictation uses it at once; a warm Hex session follows it within
  100 ms and prepares the language between dictations, and a handoff or the
  Action Button picks it up before recording.
- **Translate what I say (Settings › Dictation, iOS 26+):** Off by default. Translates
  each dictation on the iPhone with Apple's Translation framework into the
  language picked under Insert in, once its pack is downloaded; without the
  pack the transcript goes in as spoken. Where you dictate, a switch picks
  translated or as spoken for the next dictations and keeps the last choice
  (`experiments.translateDictation.pastesTranslation`, translated by
  default; turning the experiment on resets it to translated): two rows in
  the full keyboard's Hex key menu ("Paste in English", "Paste in Spanish",
  named in English, "Paste as spoken" when the dictation language is
  unknown), a "Paste in English" chip in the dictation bar's language tray,
  and a "Paste in English" toggle under Insert in. It shows only while the
  experiment is on and you dictate in another language than the target.
  While dictations paste translated, the space bar and the dictation bar's
  language label read "Español → English". The App Group carries the
  switch, so in-keyboard dictation, a warm session, and the Action Button
  all follow it.
- **Keyboard layouts (Settings › Keyboard › Keyboard layout, with Full chosen):** English,
  Norsk (`å` ending row 1, `ø æ` row 2), Español (`ñ` ending row 2) and
  Deutsch (QWERTZ with `ü`, `ö ä`), as Apple's iOS keyboards lay them out
  (`KeyboardLanguage`: letter rows, long-press accents, spelling dictionary).
  Automatic follows the dictation language (Hebrew and other languages without a
  layout get English); before any dictation language is known, the first iPhone
  keyboard, then preferred language, that has a layout. Rows of 11 keys narrow
  only the letters: shift, delete and the bottom row keep their English sizes,
  and row 3's letters sit centred between shift and delete. Long-press accents
  drop letters that have their own key (Norwegian A has no å or æ; Æ and Ø offer
  Ä and Ö), put the language's own first (Spanish á, é, í, ó, ú, ñ, ª, º), and
  German keeps ß on a held S. Autocorrect and suggestions use the layout's
  `UITextChecker` dictionary (`nb`, `es`, `de`); where the iPhone has none
  they stay off except your own text replacements, and the English contractions
  and typo list (`i` → `I`) apply only to English. Neighbouring-key costs follow
  the layout (on QWERTZ, Z sits beside T), and an accent or tilde you left out
  is the cheapest fix of all ("aqui" → "aquí", "manana" → "mañana"; a word the
  dictionary knows, such as "esta", is never changed). A two-letter non-word one
  neighbouring key (or one swap) from a very common two-letter word is fixed too
  ("tk" → "to", Spanish "nl" → "no"; `KeyboardLanguage.commonTwoLetterWords`, English
  and Spanish only). Auto-capitalization is unchanged.
  The 123 and #+= pages are English's in every language, and the accent lists
  come from Apple's layouts as remembered, not checked against a device.
- **Emoji (Settings › Experiments › Emoji, for the full keyboard):** Off by default. An emoji key
  right of 123 (after the globe) in plain text fields, not web address, email or
  ASCII-only ones. It opens a panel in place of the strip and the keys: a row of
  emoji for the text before the cursor, then a grid that scrolls sideways through
  Frequently used and the eight categories, with ABC, the categories and delete
  below. The row reads the last eight words (the newest counts most), their plurals
  and endings, letters held down ("lmaooo"), phrases ("new year") and the word still
  being typed, matched against Unicode's CLDR names and keywords for the layout's
  language and English and against Hex's own chat words ("congrats", "bday"), one
  emoji per family ("woman running", "man running"); your emoji break ties and fill
  the rest (`EmojiSuggester`). Search (the magnifier) types with the letter keys into
  a field in the strip, results above it, by name first. Holding an emoji with skin
  tones offers five; the one you pick is typed and kept for that emoji. After a space,
  an empty strip offers the finished word's own emoji ("pizza " → 🍕), never in place
  of word suggestions. The App Group keeps a use count per emoji that halves every 10
  days and each emoji's skin tone, no words (`EmojiUsageStore`); Clear emoji history
  empties it. The index is `HEXShared/FullKeyboard/Emoji/emoji-index-<lang>.txt`,
  built by `scripts/build-emoji-data.py` from Unicode's emoji-test.txt 18.0 and CLDR
  49 (Unicode License v3, `LICENSE-Unicode.txt` beside it, which permits commercial
  use with the notice) and `scripts/emoji-aliases-en.txt`; the keyboard maps it and
  binary-searches it in place (about 0.7 MB on the heap), loading it on the first
  emoji key press or finished word. Emoji newer than 15.1 are listed only where the
  emoji font draws them. Nothing leaves the iPhone.
- **Screen stays on while dictating in Hex:** recording and transcribing set
  `isIdleTimerDisabled`, and every way out (done, cancel, failure) hands the
  screen back to Auto-Lock. iOS honors it only while Hex is in front: the
  handoff screen and in-app recording, not a warm session or in-keyboard
  dictation, where another app is in front and a keyboard extension has no
  public way to keep the screen on.
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
  - Only these dictations get a Live Activity; the keyboard and the app show
    their own recording UI. It shows Copied for 2 s, and a cancel, failure, or
    empty capture ends it at once, under a background task so a suspending app
    can't leave it behind (`LiveActivityCoordinator`). Every end also ends any
    other Hex activity, and launch, returning to Hex, and ending the warm
    session (Keep mic warm › Off) end any no dictation owns.

## Acknowledgements

The app shows these under Settings › Acknowledgements (`HEXMobile/AcknowledgementsView.swift`).

- **Google Books Ngram data:** Spatial autocorrect's word and word-pair frequencies come
  from the Google Books Ngram Viewer data, version 3 (2020), by Google, licensed under
  [Creative Commons Attribution 3.0](https://creativecommons.org/licenses/by/3.0/). Counts
  are summed over 1980–2019 and kept only for words in the lists below.
- **SCOWL:** English words are checked against SCOWL (Spell Checker Oriented Word Lists)
  and VarCon by Kevin Atkinson, sizes up to 70 (<http://wordlist.aspell.net/>). Copyright
  2000-2018 by Kevin Atkinson. Permission to use, copy, modify, distribute and sell these
  word lists, the associated scripts, the output created from the scripts, and its
  documentation for any purpose is hereby granted without fee, provided that the above
  copyright notice appears in all copies and that both that copyright notice and this
  permission notice appear in supporting documentation. Kevin Atkinson makes no
  representations about the suitability of this array for any purpose. It is provided
  "as is" without express or implied warranty. SCOWL's inflections come from WordNet 1.6,
  Copyright 1997 by Princeton University, all rights reserved, provided "as is" without
  warranties (the full notice is in the app); its other sources are public domain.
- **RLA-ES:** Spanish words are checked against RLA-ES, the Spanish spelling dictionaries
  for LibreOffice by Santiago Bosio and contributors (Spain and Mexico), used under the
  [Mozilla Public License 1.1](https://www.mozilla.org/en-US/MPL/1.1/)
  (<https://github.com/sbosio/rla-es>).

## Build & Test

Prerequisites: Xcode 26+ and [XcodeGen](https://github.com/yonaskolb/XcodeGen) 2.46.0+.

```sh
cd ios
scripts/fetch-cleanup-model.sh   # the cleanup tagger's Core ML model, kept out of git
xcodegen generate
xcodebuild test -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=<SIM_UDID>"
```

The emoji index files are generated and committed: after changing
`scripts/emoji-aliases-en.txt` or the sources, run `scripts/build-emoji-data.py`
(it downloads the pinned Unicode and CLDR files, or reads them from `--sources`).

The cleanup tagger's model (`HEXKeyboard/CleanupTagger/HexCleanupTagger.mlpackage`,
25 MB) is a GitHub release asset (`ios-model-cleanup-tagger-1`), not a git file.
`fetch-cleanup-model.sh` downloads it and checks its SHA-256; Xcode Cloud runs it
from `ci_scripts/ci_post_clone.sh`. A new model gets a new release tag and hash.

## Simulator vs. Physical iPhone Proof Limits

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
- **Apple Speech hint:** Custom vocabulary's contextual strings are best effort
  and unverified on a physical iPhone. Apple documents contextual strings only
  for `DictationTranscriber`; Hex sets them on both modules, and Parakeet's
  batch path gets no hint.
- **Dictation languages:** `SpeechLanguagePickerTests` cover the picking rule
  and `DictationControllerTests` a language change during a dictation. The
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
  Unverified on a physical iPhone.
- **Speech model fallback:** `SpeechModelInstallTests` and `StallWatchTests` run
  the fallback, set-aside, offline, and stall rules against fakes of Apple
  Speech's calls (the simulator has no speech models), and `--snap-state
  full-keyboard-language-unavailable`, `keyboard-language-unavailable`,
  `language-unavailable` and `language-switched` render the messages. Which
  module fails for which language on iOS 26.x, whether a stalled Apple download
  ever moves again, the reservation limit on a full iPhone, and Parakeet's
  memory on 4 GB iPhones are unverified on hardware.
- **Keyboard layouts:** `KeyboardLayoutTests` check every language's rows,
  widths, hit frames (tiling the key area with no gaps or overlaps), balloons
  and accent menus at 320–440 pt and in landscape; `AutocorrectEngineTests` and
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
  iPhone 13, which enforces no limit and leaves out GPU memory): the first
  dictation adds 7–8 MB on the dictation bar and 11–13 MB on the full keyboard
  within 3 s of the tap (to 29–32 MB), and later ones 1–5 MB. The 32 and 24 MB
  thresholds rest on that plus the Metal scope's GPU memory, not on an iPhone's;
  `keyboard-capture-probe.log` records each step's footprint, peak and headroom.
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
  extension (Release, iPhone 17 simulator, iOS 27.2, no GPU memory, no headroom
  reading) the footprint was 19.0 MB with the emoji key, 28.7 MB with the panel open,
  22.7 MB after picking suggestions and closing it, 41.6 MB (peak 48.4) after
  scrolling every section and 36.5 MB once closed: about 350 emoji glyph images the
  system's text rendering caches process-wide (18.7 MB), which no public API frees
  and neither a smaller size nor a font instance of the panel's own changed. Whether
  iOS purges them under memory pressure, and so whether browsing everything leaves
  in-keyboard dictation short of its 32 MB, is unverified on an iPhone. The panel
  opened in 22–26 ms the first time and 12–14 ms after (about 21 ms until on screen).
- **No iOS CI:** Custom vocabulary is covered by unit tests only. No CI job
  compiles or tests the iOS targets, so these checks run only in a local
  `xcodebuild test`.

## Internal TestFlight releases

[Hex Mobile in App Store Connect](https://appstoreconnect.apple.com/apps/6836837368/testflight/ios)
uses Anomaly's signing team. The GitHub **Release Hex Mobile** workflow checks
`dev` at noon and 6 p.m. Chicago time, or on manual dispatch, and cuts an
`ios-beta-*` prerelease when iOS inputs have changed. These tags are separate
from desktop releases. The marketing version stays fixed across beta builds.
`MARKETING_VERSION` stays 0.2.0 for TestFlight; App Store submissions use their own
higher version (the first is 0.2.1). To build one, push the tag
`ios-beta-appstore-<version>` (e.g. `ios-beta-appstore-0.2.1`) on the chosen `dev` commit:
it matches the Xcode Cloud start condition, and `ci_scripts/ci_post_clone.sh` stamps that
version into `project.yml` before generating the project. Once Apple approves a
version, later uploads must be higher than it. Hex Mobile is iPhone only (`TARGETED_DEVICE_FAMILY` 1).

The Xcode Cloud workflow needs a tag start condition with prefix `ios-beta-`,
container `ios/HEXMobile.xcodeproj`, shared scheme `HEXMobile`, an iOS Archive
action, and a TestFlight post-action for **Hex Internal**. Cloud owns the build
number. Verify the run's source tag and the processed build in that group.