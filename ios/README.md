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
(`HEXKeyboard/KeyboardLocalDictation.swift`). With Full Access, the web view
and its page load as the keyboard shows (unless memory is already short), without
the microphone, so no orange dot shows until you dictate; the full keyboard's
Hex key then starts capture as the finger lands, before the touch is a tap or
a hold, and a slide to the style menu or a cancelled touch drops it with
nothing transcribed (its orange dot shows for that moment). iOS ends a keyboard that passes
its memory limit (about 60–90 MB depending on the model), so while less is left
than starting needs (`os_proc_available_memory`: 32 MB for the first
dictation, 24 MB once the capture web view is loaded) Dictate and the Hex key
open Hex instead, as before iOS 26. Audio never leaves the iPhone. Hex never asks
for Speech Recognition access: that permission covers `SFSpeechRecognizer`, which Hex
doesn't use, and `SpeechAnalyzer` runs without it in the app and the keyboard alike
(a TestFlight tester who turned it off dictated, iOS 27.0.1, build 51).

## Capabilities

- **One-tap keyboard dictation (`HEXKeyboard`):**
  - **First run (`HEXMobile/Onboarding.swift`):** The desktop's pages and
    order. **Setting up Hex** starts the speech model downloading (the engine
    `transcriberFactory` picks; a launch that found Parakeet ended mid-load
    stays failed until Try again) and moves on after a beat. **With Hex**, a
    short demo, moves on by itself. **Let Hex hear you** has two rows, the
    microphone (its pill and the page's Continue say Continue before iOS's
    prompt, never Allow, per App Review 5.1.1(iv)) and the Hex keyboard with
    Full Access (Open Settings, Hex's page); a done row shows only its check,
    and the page moves on once both are done. Continue never needs the
    microphone. **Almost ready** shows only while the model downloads, with
    Continue without a model after a failure or 30 s without progress.
    **Your turn** keeps the keyboard up and counts once a dictation arrives or
    three words are typed with Full Access; an empty dictation says Hex heard
    nothing (the keyboard notes it in the App Group). **Hex is ready** shows
    only once every step is done, else Almost ready with what's left. The
    model step counts the keyboard's own model too
    (`KeyboardSpeechPreparation`): on iOS 26 the keyboard dictates with
    Apple Speech even when Hex uses Parakeet, so Hex installs that language's
    model as the keyboard would; where Apple has none on the iPhone, it marks
    `keyboardNeedsHex` so the Hex key opens Hex from the first tap. Home's
    Ready counts it the same way. The
    model's progress pill sits in the top bar until it's ready; Skip shows on
    the demo and the practice only. It shows once: `onboarding.completed` in
    the app's UserDefaults records Start using Hex or Continue without a
    model, `onboarding.page` resumes a quit part-way on the same page, and a
    launch that finds the keyboard ready and the microphone allowed (an
    existing user) records completion without showing it. **Run setup again**
    replays it from page 1. Its `--snap-state` scenes are `onboarding-setup`,
    `-demo`, `-permissions`, `-permissions-done`, `-waiting`, `-failed`,
    `-try`, `-try-done`, `-try-empty`, `-try-off`, `-ready`, and
    `onboarding-tour`, which walks every page by itself for a recording.
  - **Without Full Access (App Review 4.4.1):** Hex is a keyboard first. The
    full keyboard types without Full Access or network: letters, shift and caps
    lock, numbers and symbols, accents, delete, return, the globe key, and
    suggestions and autocorrect (`UITextChecker`, the supplementary lexicon,
    and, in English and Spanish, Spatial autocorrect's bundled model). Words
    it learns may last only while the keyboard runs, as an iPhone may not let
    it write the App Group; haptics, Paste and dictation need Full Access. Without Full
    Access the keyboard always mounts the full keyboard, whichever keyboard is
    chosen (the dictation bar types only spaces and line breaks). Dictation needs
    Full Access (Hex's App Group and the microphone): tapping the Hex key, or
    sliding for its menu, shows for 8 s in the suggestion strip "Allow Full
    Access to dictate" and "Settings › Apps › Hex › Keyboards", with an **Open
    Hex** chip (`hex-dictation://keyboard/setup`) that opens the app's setup
    sheet. Nothing else opens Hex without Full Access, and the strip shows no
    notice until the Hex key is tapped.
  - **Setup:** Home's status line counts the setup steps left ("Finish
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
  - **Styles (`TranscriptFormatter`, run by `DictationPipeline` wherever Hex
    dictates: the app, the keyboard's warm session and cold start, the Action
    Button, Control Center and in-keyboard dictation):** `Polished` (full
    sentences: fillers trimmed, each sentence capitalized, "I" too, and ended,
    in Chinese and Japanese with "。"), `Casual` (texting: sentences may start
    lowercase, though never an acronym or a word with inner capitals; "i" for
    "I"; no comma after an opening "yeah" or "ok"; no final full stop on a
    message of one line and up to three sentences), `Notes` (spoken `"bullet"`
    / `"new line"` markers, else several sentences become a bulleted list and
    "Shopping list: eggs, milk and bread" bullets its items; no opening "so",
    "yeah" or "and then", no final full stops), and `Verbatim` (as the speech
    model wrote it), with cursor-aware insertion spacing and repeating
    backspace. On an already punctuated sentence ("Sounds good, see you at
    5.") Polished and Verbatim read the same; they differ on fillers, missing
    capitals or endings, and with Clean up what I say on.
  - **Lowercase (Settings › Dictation › Lowercase):** Off by default. After the
    style and custom vocabulary's spellings, everything is lowercased, custom
    vocabulary included, as desktop's Lowercase step (`str::to_lowercase` on the
    final text) does, on every path (`DictationLowercaseSettings`, App Group key
    `settings.dictationLowercase`).
  - **Format lists (Settings › Dictation › Format lists):** On by default
    (`DictationListSettings`, App Group key `settings.dictationFormatLists`), in
    English and Spanish, every style but Verbatim (`SpokenLists`, inside the
    style after its fillers, so each style ends the lines its own way). "first …
    second …" ("1st … 2nd …", "primero … segundo …", "en primer lugar …"),
    "number one … number two …" (also "step", "item", "point", "número", "paso")
    and "one, … two, …" become `1. …` lines; "bullet", "bullet point", "next
    item", "dash" ("viñeta", "guion") said twice or more become `- …` lines; "new
    line" and "new paragraph" ("nueva línea", "nuevo párrafo", "salto de línea")
    break lines. Text before the list gets its own line (with a colon unless it
    ends a sentence), and what follows the last item's sentence its own line.
    Guards keep prose: markers count up from one, at least two non-empty items,
    each marker at a clause start (the start, after a mark or line break, or
    after "and"/"then"/"y"/"luego" that are), a bare number needs its comma, and
    words like "the first time", "second guess", "a new line of", "bullet
    points" stay as said. `TranscriptCleanupTests` holds the list cases and
    62 prose sentences that must not change.
  - **Spoken "@" (English, every style but Verbatim):** "at the rate" (India's
    name for @) and "at sign" become "@", and an address said around it joins
    up: "John at the rate Gmail dot com" becomes "john@gmail.com", "dot" turning
    into "." only inside the address (`SpokenCleanupRules.spokenSymbols`, run by
    `CleanupTagger.cleanIfEnabled` with Clean up what I say on or off, in the app
    and in-keyboard dictation). "at the rate" needs a domain after it, so "at the
    rate of 5%" and "at the rate we planned" stay; "at" alone is never touched.
    Apple's `SpeechTranscriber` glues the domain to the word before ("at the
    rategmail.com"), which is split again; `DictationTranscriber` already writes
    "@" for "at sign" and for "john at gmail dot com". Polished leaves an address's
    case alone, starts no sentence at a full stop inside a word ("gmail.com"),
    and ends no lone address with a full stop.
  - **Mid-sentence dictation:** when the text before the cursor doesn't end a
    sentence (`.` `!` `?` `…` or a line break), the keyboard lowercases the
    dictation's first word, except "I" and its contractions, acronyms, words
    with inner capitals, and words the dictation language's `UITextChecker`
    dictionary doesn't know in lowercase (likely names). German keeps the case,
    since its dictionary takes nouns in lowercase; so does a language with no
    dictionary on the iPhone (`TranscriptFormatter.insertionText`).
  - **Full keyboard Hex key (Settings › Keyboard › Full, the default):** A tap, or a
    still hold of 0.35 s, starts dictation: in a warm session, else inside
    the keyboard (in-keyboard dictation, iOS 26+), else by opening Hex.
    In-keyboard dictation listens from touch-down, so a tap's or hold's first
    words are kept; moving
    more than 10 pt first opens the style menu, dropping that capture; releasing over a row picks it, and releasing elsewhere leaves
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
    styles' list below), a filler between commas goes ("este", "o sea"),
    and a correction marker set off by commas ("digo", "perdón", "no",
    "mejor dicho"; "nein", "ich meine"; "nei", "nei vent", "jeg mener")
    drops the words it replaces, counted back to the nearest earlier word
    that matches the repair's first word or is of its kind (a number,
    weekday, month, or today and tomorrow): "a las 3, digo, a las 4" becomes
    "a las 4". With no such word the sentence stays as said, and other
    languages stay as heard. It runs after transcription and learned fixes
    and custom vocabulary, before Apple Intelligence
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
    A local list of up to 100 names and phrases, applied to every engine, in the
    app and in-keyboard dictation. After
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
  - **Auto-capitalization (Settings › Keyboard › Auto-capitalization):** On by default,
    as Apple's (`FullKeyboardSettings.autoCapitalization`, App Group key
    `settings.keyboardAutoCapitalization`). Off, shift never turns itself on, whatever the
    field's `autocapitalizationType` asks (a field's or sentence's start, after "." "!" "?"
    or the double-space period, which still types ". "), and autocorrect and swipe typing add no
    capital you didn't type: "i" stays "i", "im" becomes "i'm", "mary" stays "mary" and
    "mondya" becomes "monday", as Apple's keyboard does with Auto-Capitalization off (seen on
    the iOS 27.2 simulator). Words you typed with a capital, and your own spellings (text
    replacements, Contacts names, words you kept, fixes Hex learned), keep theirs; shift and
    caps lock work as before, and the strip may still offer a capitalised word to pick.
    Dictation has its own switch (Settings › Dictation › Lowercase).
  - **Spatial autocorrect (Settings › Keyboard › Spatial autocorrect):**
    On by default, shown under Autocorrect while it's on (it needs the full keyboard
    with Autocorrect on). Stored as `experiments.spatialAutocorrect` from when it was an
    experiment off by default: unset or true is on, false off, so testers who turned it
    off keep it off. A noisy-channel decoder
    (`TypingDecoder`) replaces `UITextChecker`'s one-edit guesses on space and in the
    strip for English and Spanish: each tap is scored against every letter with a 2D
    Gaussian around the key (shifted by Learn how I type's tap offsets when it has 20
    or more taps on a letter), words are found with a beam search over a memory-mapped
    word list and over your own words (Contacts names and other `UILexicon` terms,
    one-word text replacements, words you kept, fixes Hex learned), a word-pair language
    model weighs them by the word before, a missed space is tried by splitting the taps,
    accented letters read from their base key (é from e), and what you typed is replaced
    only when the best reading beats it by a margin (more when what you typed is a word,
    including a name, day or place typed in lowercase that the dictionary knows only
    capitalised, which is capitalised as the dictionary path does: "mary" -> "Mary").
    Shortcuts, learned fixes, contractions, and words you kept still come first. Other
    layouts (Norsk, Deutsch, Italiano, Français, Português, Polski, Русский, Українська) keep the dictionary
    engine in their own language, and Srpski (no iOS dictionary) none, never the English
    model, except as bilingual autocorrect's second language (below). The
    models (`HEXKeyboard/TypingModel/en.hextm` and `es.hextm`, about 3.6 MB each) are
    built from Google Books Ngram v3 by `scripts/build-typing-model.py` (see
    Acknowledgements below).
  - **Offensive words (Settings › Keyboard › Suggest swear words):** like Apple's
    keyboard, the strip and autocorrect never offer slurs, and offer swear words only
    with Suggest swear words on (off by default; App Store rating 4+). The list
    (`HEXShared/FullKeyboard/never-suggest.tsv`: layout, `slur` or `swear`, spelling or
    two-word expression; `*` lines apply to every layout) is LDNOOBW as curated for Hex
    for Android, entry for entry the same as `android/app/src/main/assets/typing/never-suggest.tsv`
    (see Acknowledgements below); change both together. `OffensiveWords` is checked when a
    suggestion is looked up: `UITextChecker`'s guesses and completions, learned fixes, and
    Spatial autocorrect's readings (`TypingDecoder.NeverSuggest`, so the models still score
    them as words). An expression is blocked only as the pair ("blow job"), never its words
    alone. A word it may not offer, typed in full, is never corrected (as on Android); your own `UILexicon` words
    (Contacts names, text replacements) are always offered.
  - **Swipe typing (Settings › Keyboard › Swipe typing):** On by default (stored as
    `experiments.swipeTyping` from when it was an experiment off by default: unset or true
    is on, false off, so testers who turned it off keep it off), for the
    English and Spanish layouts only (every other layout keeps a drag as a slide to
    another key) and off under VoiceOver. A touch that goes down on a letter and travels
    more than a key's pitch (width plus gap) is a swipe (`SwipeTyping`): its key no longer
    types, a light trail (a shape layer, the last 0.45 s of the path) follows the finger,
    and the strip shows what the path reads as so far, decoded off the main thread at most
    every 70 ms. Lifting types the best reading after a space, unless the text before
    ends in a space, a line break or opening punctuation (`( [ { « “ ‘ ¿ ¡ @ # /`, or a
    straight quote after a space), as Apple's QuickPath does; shift capitalises its first
    letter (caps lock all of it), the space it adds can start a sentence ("Hi." then a
    swipe types " There"), and a name the dictionary only knows capitalised is
    capitalised. The strip then offers the next three readings, the likeliest in the
    centre; tapping one puts it in the word's place. Until anything else changes the
    text, delete takes the whole word back (with the space it added) and space never
    autocorrects it; a tap after it adds to the word. A second finger going down ends the
    swipe first, so its word goes before that key's letter. On the iPhone Duo's split
    keyboard a swipe reads across the middle between the halves: its points are read with
    the right half pushed against the left and the middle shrunk to one key's gap
    (`KeyboardLayout.joinedForSwipe`), so either half's G and V is the one letter, as for
    Spatial autocorrect's taps. Taps, holds for accents, slides
    shorter than a key, delete's word slide, the space bar's trackpad, 123 and the Hex key
    work as before: no swipe starts on them, and none once an accent menu is open.
    `SwipeDecoder` is a shape-matching decoder after SHARK2: the path is resampled to 32
    points in key pitches; words come from the language's typing model by the keys the
    path starts and ends near (each start letter is one range of the sorted word list,
    read in place from the memory-mapped model, so nothing is built or kept between
    swipes), kept when every key lies near the path and their ideal path (the polyline
    through their keys' centres) is about as long; each is scored on location (point by
    point, then dynamic time warping for the best 80), shape, and how near its first and
    last keys the finger went down and lifted, and weighed with the model's word-pair
    frequency after the word before. English contractions, which the Google Books model
    barely counts, and your own words (Contacts names and other `UILexicon` terms, one-word
    text replacements, words you kept) are read beside it. It never reads a slur, and reads
    swear words only with Suggest swear words on (`TypingDecoder.NeverSuggest` and
    `mayOffer`, as the strip). Its weights are tuned on synthetic swipes of the
    autocorrect benchmark's held-out dev sentences; Record typing logs keeps a swipe's
    touch moves and a `swipe_end` line (points, decode time, readings) for fitting it to
    real fingers.
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
    appended on a utility queue every 2 s. Nothing is sent. While a field is
    logged, both keyboards show a small "● Logging" mark the whole time (App
    Review 2.5.14): in the full keyboard's strip corner, or at the start of its
    recording bar while dictating, and beside the Hex mark in the dictation bar.
    The schema is in
    [`docs/typing-logs.md`](docs/typing-logs.md).
- **Settings (`ContentView`, `SettingsPages.swift`):** Home fits one screen: the
  Hex header, one status line ("Ready" with a check, or "Finish setup · 1 step ›"),
  Dictate, and four rows that each open a short page, then **Keyboard settings**
  (Hex's page in iOS Settings) and **Run setup again**, and last **Privacy
  policy**, **Support** and **Acknowledgements**. Privacy policy (App Review
  5.1.1(i)) and Support open in Safari inside Hex (`HexLinks`, one base URL:
  `https://dev.anoma.ly/hex` until the production site serves
  `anoma.ly/hex/privacy`). Each row has a one-line description at most. The pages:
  - **Dictation:** Style, Lowercase, Format lists, Dictation language, Translate what I say, Clean up
    what I say, Custom vocabulary (its list below the switch), Keep mic warm,
    and Always use iPhone microphone.
  - **Keyboard:** Keyboard, **Full** (the default) or **Dictation bar** (stored
    as `FullKeyboardSettings.isEnabled`, App Group key `experiments.fullKeyboard`
    from when the full keyboard was an experiment: unset or true is Full, false
    the dictation bar, so testers who turned that experiment off keep the bar),
    then Languages, Auto-capitalization, Autocorrect (with Spatial autocorrect below it while it's
    on), Suggest swear words and Key pop-ups only while Full is chosen, Haptics (the dictation bar uses it too),
    then Swipe typing and Emoji (Clear emoji history while it's on), which say they need the
    full keyboard while the dictation bar is chosen, and a field to try the keyboard in.
  - **Privacy:** "Everything stays on this iPhone", Learn how I type (what it
    learned below the switch), and under **Advanced**, Record typing logs (off
    by default) with its Typing logs viewer while it's on.
  - **Experiments:** Hinglish (Hindi in Latin letters) and Clean up with Apple
    Intelligence.

  Simulator scenes: `--snap-state settings-home` (ready), `setup` (keyboard
  added, Full Access not confirmed yet), `setup-sheet`, `setup-sheet-microphone`
  (before iOS has asked for the microphone), and
  `settings-<dictation|keyboard|privacy|experiments>[-on][-end]`. Each writes
  its page's switches to the App Group: off by default, on with `-on`; switches that are
  on by default (Spatial autocorrect, Swipe typing, Emoji) stay on.
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
  one ("Try English", with **Use English** in the app). With nothing else to move
  to (an English-only iPhone), Automatic tries the set-aside language again
  rather than none, so "Apple Speech doesn't have your language" means neither
  module lists any of the iPhone's languages. Offline or stalled, it
  says the download failed and tries again next time. In the keyboard, a model it
  can't get itself (each process holds its own reservation, and one released or
  removed since Hex downloaded it reads as not installed there) says "Open Hex to
  download English." and Dictate and the Hex key open Hex, which downloads it,
  until a model installs, a language is chosen, or a day passes
  (`SpeechModelNeedsHex`, `settings.dictationLanguage.keyboardNeedsHex`); a move
  to another language keeps its own sentence. Reservations make room
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
  own page, and the space bar names the language faintly ("English", "Norsk"),
  unless the keyboard switches between two or more languages: then it names the keyboard's.
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
  pack the transcript goes in as spoken. Where you dictate, a "Translate to
  English" switch turns it on or off without opening Hex (testers asked for
  a button, not a setting): a row in the full keyboard's Hex key menu, lit
  with a checkmark while on, and a chip in the dictation bar's language tray.
  It writes Settings' own switch (`experiments.translateDictation`; on also
  sets `experiments.translateDictation.pastesTranslation` to translated), and
  Settings re-reads both when it shows. Below it, Translation language lists
  up to six targets (the current one, English, the dictation and preferred
  languages, then common ones, only those Apple translates into); picking one
  sets `experiments.translateDictation.target` and turns translation on.
  Settings keeps the full language list, the pack download and its "Paste in
  English" toggle under Insert in. While dictations paste translated, the Hex
  key wears the target's code ("EN") in its corner, the dictation bar shows
  "→ EN" beside the Hex mark, and the space bar reads "Español → English".
  The App Group carries the switch, so in-keyboard dictation, a warm
  session, and the Action Button all follow it. The keyboard shares the
  app's String Catalog, so the Hex key menu is in Chinese on a Chinese iPhone.
- **Keyboard languages (Settings › Keyboard › Languages, with Full chosen):** Automatic (the
  default, as before: the dictation language's layout, below), or one or more layouts ticked
  and dragged into order (`KeyboardLanguagesPage`, App Group `settings.keyboardLanguages`,
  which reads the one-layout `settings.keyboardLayout` until first written and writes its
  first language there). The layouts of the iPhone's own keyboards and preferred languages
  come first ("On this iPhone"); ticking one from Automatic keeps Automatic's language beside
  it, and unticking the last goes back to Automatic. With two or more, the space bar names
  the current one ("Français", with the region only beside another French layout) and a
  quick swipe along it switches (left the next, right the one before): letters, accents
  and autocorrect. Apple and Gboard switch with a long press on the globe, but a Face ID
  iPhone shows no globe in a third-party keyboard and Hex's space-bar slide is its cursor
  trackpad, so this follows SwiftKey: with two or more languages the cursor slides once the
  space bar was held still for 0.4 s (the hold that starts Apple's trackpad, below), a slide
  before that is a swipe, and one shorter than 24 pt types a space
  (`FullKeyboardEngine.trackpadHoldDelay`, `languageSwipeDistance`).
  The globe key is unchanged. The keyboard comes back in the language it was left in
  (`settings.keyboardLanguages.current`; without Full Access an iPhone may keep it only
  while the keyboard runs). **Switch dictation too** (off by default, shown with two or more
  on iOS 26+) sets the dictation language to the keyboard's on each switch, in the
  iPhone's region where Apple Speech has it (`KeyboardLanguage.dictationLanguage`; none for
  Srpski). **Bilingual autocorrect:** the first other chosen language with Latin letters and
  an iOS dictionary (`KeyboardLanguageList.partner`: English beside French, French beside
  English; none for Русский, Українська, Srpski, or two French layouts) is checked on the same keys,
  with its Spatial model where it has one, as Apple's "Français et English" keyboard does: a
  word either language knows as typed is never changed, a typo only one can fix gets that
  fix ("teh" → "the" while French types), and when both can, the language of the last words
  only one dictionary knows wins, else the clearly cheaper slip, else the keyboard's own;
  the strip shows both languages' fixes and completions. In the simulator the English
  keyboard alone changed 34 of 77 common French words and the French keyboard alone 27 of
  80 English words; with both chosen, none.
- **Keyboard layouts (each language's letters):** English,
  Norsk (`å` ending row 1, `ø æ` row 2), Español (`ñ` ending row 2),
  Deutsch (QWERTZ with `ü`, `ö ä`), Italiano and Português on the English letters
  (Apple's keyboards for them are QWERTY), Français (France) on Apple's AZERTY (rows of
  10, 10 and 6 letters, W to N under E to I with a gap beside shift and delete, as on
  Apple's French and English keyboard; Apple's French-only one adds a ´ dead key that Hex
  leaves out), Français (Canada) on QWERTY and Français (Suisse) on QWERTZ, as on Apple's
  (automatic picks them for `fr-CA` and `fr-CH`, AZERTY for any other French, Belgium
  included), Русский (Apple's ЙЦУКЕН: 11, 11 and 9 letters, ё on a
  held е, ъ on a held ь), Українська (the standard Ukrainian ЙЦУКЕН, ДСТУ 4862:2007, in the
  rows the iOS 27.2 simulator's own Ukrainian keyboard shows: 12, 12 and 10 keys,
  `йцукенгшщзхї`, `фівапролджєʼ`, `ячсмитьбюґ`, with ʼ the U+02BC apostrophe Ukrainian
  spells with; as the system keyboard's long-press options were observed, only ʼ offers
  anything, the other apostrophes (' ’ ‘ and the backtick), and ₴, which the system keyboard
  offers on its held currency key, is held on $; no ы, э, ъ or ё), Polski (QWERTY, as the
  standard Polish layout is, on the English letters; it matches the system keyboard's
  observed long-press options: Polish's own letter right after the base on A, C, E, L, N, O,
  S and Z, so ą ć ę ł ń ó ś ż, with ź after ż, and nothing on U, I or Y; its currency key
  is €, held for € $ £ ¥ ₩ ₽ and no zł, which Hex holds on the #+= page's €) and Srpski (Latin: Apple's QWERTZ option for it; Apple's iPhone
  Serbian Latin keyboard has no keys of its own for č ć đ š ž, they are held on c, d, s
  and z), as Apple's iOS keyboards
  lay them out (`KeyboardLanguage`: letter rows, long-press accents, spelling dictionary).
  Automatic follows the dictation language (Hebrew, Greek, Cyrillic Serbian and other
  languages without a layout get English; Serbian only in Latin script, `sr-Latn`);
  before any dictation language is known, the first iPhone
  keyboard, then preferred language, that has a layout. It never follows Hex's own
  input mode, whose `primaryLanguage` is always the Info.plist's en-US. Rows of 11 keys narrow
  only the letters: shift, delete and the bottom row keep their English sizes,
  and row 3's letters sit centred between shift and delete. Russian's nine-letter row 3
  wouldn't fit beside them, so there shift and delete narrow instead, as on Apple's;
  Ukrainian's twelve-key rows leave them one letter wide, as on Apple's. Long-press options
  match each language's system keyboard and its observed long-press options (iOS 27.2
  simulator: every letter, 123 and #+= key held for about 3 s with a real touch, the
  callout read from the accessibility tree; `KeyboardLanguage.letterHolds` and
  `symbolHolds`, `AccentCatalog`): a key the system keyboard holds nothing on holds
  nothing (Spanish Z, Serbian E), letters with their own key drop out (Norwegian A has no
  å or æ, and Æ and Ø offer Ä and Ö; Spanish N offers only ń), German keeps ß on a held
  S, and Hex's callout shows the options in the system's left-to-right order with the base
  letter at the end under the thumb, in one row where the system wraps a long list
  (English A) into two. Autocorrect and suggestions use the layout's
  `UITextChecker` dictionary (`nb`, `es`, `de`, `it`, `fr`, `pt_BR` then
  `pt_PT`, `pl`, `ru`, `uk`; Serbian would use `sr_Latn`, which iOS 27.2 doesn't have, nor a
  Croatian one); where the iPhone has none
  they stay off except your own text replacements, and the English contractions
  and typo list (`i` → `I`) apply only to English. Neighbouring-key costs follow
  the layout (on QWERTZ, Z sits beside T), and an accent or tilde you left out
  is the cheapest fix of all ("aqui" → "aquí", "manana" → "mañana"; a word the
  dictionary knows, such as "esta", is never changed). A two-letter non-word one
  neighbouring key (or one swap) from a very common two-letter word is fixed too
  ("tk" → "to", Spanish "nl" → "no"; `KeyboardLanguage.commonTwoLetterWords`, English,
  Spanish and Italian only). Auto-capitalization is unchanged.
  The 123 and #+= pages are English's in every language; their held keys follow each
  language's system keyboard where it differs (# holds № in Russian and Ukrainian, the
  digits º and ª in Spanish and Italian, . holds · in French, $ holds ₴ in Ukrainian and
  kr in Norwegian).
- **Size (full keyboard):** The suggestion strip is Apple's 44 pt predictive bar in
  every state (suggestions, Undo dictation, the Full Access hint, the recording bar,
  the emoji panel's top row, the Logging mark) and both orientations, with 8 pt under
  it, so row 1's keycaps start 52 pt down, as Apple's do (53 pt on the iPhone 17 Pro,
  17 Pro Max and SE, iOS 27.2 simulator): 270 pt in portrait, 189 pt in landscape.
  Dictating no longer grows the keyboard. On the iPhone Duo (`KeyboardPosture`,
  model `iPhone19,4`) iOS sets third-party keyboards in an inset platter with a band
  of its own above them, so the strip has no gap there, and the keys follow Apple's
  for each screen: 56 pt rows folded, 59.7 pt unfolded and turned, 51.7 pt unfolded
  upright, starting as far in from the screen's side as Apple's. Measured in the
  simulator; unverified on a Duo.
- **Split keyboard (iPhone Duo):** Unfolded and held upright (the inner screen wider than
  tall), the full keyboard splits as Apple's does there (`SplitGeometry`): each half at
  the keyboard's side with Apple's 46.3 pt keys on a 57 pt pitch, row 2 half a key in
  and row 3's letters a key and a half, an odd row's middle key on both halves (G and V
  in English), a space bar on each half, and nothing in the middle, where a tap types
  nothing. The strip sits over the right half, where Apple's predictions are; the
  recording bar still spans the top. Every layout splits by the same rule (measured
  against Apple's English split only; AZERTY, QWERTZ, Russian and the rest follow the
  rule unchecked), and every page (123, #+=). Spatial autocorrect reads taps as on the
  unsplit keyboard (`KeyboardLayout.unsplitX`), so either G is one key. There's no
  setting: Apple's split follows its own Split Keyboard preference, which a keyboard
  extension can't read, so Hex splits whenever the Duo is in that posture (the iOS
  27.2 simulator has Apple's off until `KeyboardShouldSplitInDefaultContext` is set).
- **Touch targets (full keyboard):** Every point of the key area types a key: gaps
  split down the middle, the outer keys reach the screen's edges, and row 1 also
  takes the 8 pt gap under the strip (on the Duo, the strip's bottom 4 pt below its
  pills, `KeyboardLayout.stripTouchBand`), so a tap just above a keycap types it
  rather than picking a suggestion (Apple's keys reach about 8 pt above theirs,
  measured in the simulator). While you type (a
  key is down, or one went down or came up in the last 0.25 s) the strip's whole
  lower half goes to the row-1 key below the finger
  (`FullKeyboardEngine.takesStripTouch`): Record typing logs showed every strip
  pick that soon after a key was an e aimed high landing on the left slot, which
  kept "th" and typed a space where "the" was meant; nobody reads a new suggestion
  and taps it that fast. Taps are not
  shifted: moving every logged tap up by 2 to 8 pt landed no more of them on the
  meant key, and Apple's row borders sit within about 2 pt of the gaps' middles
  when nothing is typed. While autocorrect is on in a text field that allows it,
  the English and Spanish word lists (Spatial autocorrect's models, whether or not
  it is on) let the likelier next letter claim up to 5 pt of a
  neighbouring letter's target, 1.25 pt for each e-fold it is likelier
  (`FullKeyboardEngine.letterOdds`): after "th" a tap just inside d types e, as
  Apple's does (there e reaches about 10 pt into d). A finger that rolls within
  that reach keeps the letter. Other layouts keep fixed targets. The keyboard's
  window no longer holds back touches near the screen's side edges for system
  gestures: Record typing logs showed them reaching the keys about 80 ms later
  than others on an iPhone (whether this removes it is unverified on hardware;
  the simulator never delayed them).
- **Spaces (full keyboard):** A suggestion (a word, what you typed, or an emoji)
  types its own space, so a space pressed right after it is swallowed, as on Apple's
  keyboard; it still counts towards double-space for a period. Holding the space bar
  still for 0.4 s starts the trackpad, as Apple's does, and letting go then types no
  space (`FullKeyboardEngine.trackpadHoldDelay`); sliding 10 pt starts it at once, unless
  the keyboard switches between two or more languages (Keyboard languages, above).
- **Slides from 123, #+= and shift (full keyboard):** As on Apple's keyboard, holding 123
  (or ABC, or #+=) shows its page, and sliding shows the pop-up of the key under the
  finger (or darkens it with Key pop-ups off); letting go types that key and goes back to
  the page it came from. Letting go back on the page key types nothing and goes back too;
  off the keys it types nothing and the page stays. Holding shift and sliding to a letter
  shows and types its capital. None of them is a swipe, and a slid-to key opens no
  accent menu (Apple's does after a hold).
- **Emoji (Settings › Keyboard › Emoji, for the full keyboard):** On by default (stored as
  `experiments.emoji` from when it was an experiment off by default: unset or true is on,
  false off, so testers who turned it off keep it off). An emoji key
  right of 123 (after the globe) in plain text fields, not web address, email or
  ASCII-only ones. It opens a panel in place of the strip and the keys: a row of
  emoji for the text before the cursor, then a grid that scrolls sideways through
  Frequently used (eight columns: your emoji, then common ones) and the eight
  categories, at the system emoji keyboard's 46 pt pitch, with ABC, the categories and
  delete below, drawn without keycaps as the system's are; a tap anywhere under the
  grid goes to the key under it. The row reads the last eight words (the newest counts most), their plurals
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
- **LDNOOBW:** The keyboard's suggestion filter uses the
  [List of Dirty, Naughty, Obscene, and Otherwise Bad Words](https://github.com/LDNOOBW/List-of-Dirty-Naughty-Obscene-and-Otherwise-Bad-Words/tree/5faf2ba42d7b1c0977169ec3611df25a3c08eb13)
  (LDNOOBW), © 2012–2020 Shutterstock, Inc., and contributors: its English, Spanish,
  German and Norwegian lists, licensed under
  [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/).
  Hex lowercased and classified the entries, omitted ordinary, medical and search-only
  terms, and added common slurs, inflections and spelling variants.
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
- **In-keyboard dictation's start time:** The simulator's WebKit captures from a
  mock microphone, and on a Mac without a microphone the warm session's
  recorder can't start, so how long a real microphone takes to open
  (getUserMedia, the audio session, the recorder) shows only on an iPhone, in
  the probe log's `first audio` and `warm start` lines.
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
  Ukrainian is `DictationTranscriber`'s only (macOS 27.2 lists `uk-UA` there and
  not for `SpeechTranscriber`): a `say -v Lesya` clip came back word for word in
  Cyrillic, without punctuation, after an 18 s model download.
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
  space-bar label, pairing, migration, the dictation pick), `FullKeyboardEngineTests` the
  swipe against the hold-then-slide trackpad and switching in a `FullKeyboardView`, and
  `AutocorrectEngineTests` bilingual autocorrect on stub dictionaries and iOS's own
  (`testABilingualKeyboardKeepsBothLanguagesWords` prints the break counts). Checked live in
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
  and accent menus at 320–440 pt, in landscape and in the iPhone Duo's postures; `AutocorrectEngineTests` and
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
  full|dictation-bar` picks the keyboard and `--typing-log-mark` shows the
  Logging mark in the keyboard scenes. The simulator's keyboard reads the App
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
  an in-keyboard dictation after it no higher. `EmojiSuggesterTests` check that a cell
  lands where a label drew its emoji and that drawing every emoji stays in the cache's
  limit; `--snap-state full-keyboard-emoji-probe` prints the app's footprint, the
  renderer's counts and each frame's work scrolling every emoji at 40 pt a frame
  (p50 1.6–1.8 ms, p95 2.5–6 ms against 0.6–1.0 ms drawn as text). The panel opens in
  24–29 ms the first time and 15–17 ms after (8–9 ms drawn as text, which reused the
  shared font's images).
- **No iOS CI:** Custom vocabulary is covered by unit tests only. No CI job
  compiles or tests the iOS targets, so these checks run only in a local
  `xcodebuild test`.

## Internal TestFlight releases

[Hex Mobile in App Store Connect](https://appstoreconnect.apple.com/apps/6836837368/testflight/ios)
uses Anomaly's signing team. The GitHub **Release Hex Mobile** workflow checks
`dev` at noon and 6 p.m. Chicago time, or on manual dispatch, and cuts an
`ios-beta-*` prerelease when iOS inputs have changed. These tags are separate
from desktop releases. The marketing version stays fixed across beta builds.
`MARKETING_VERSION` is 0.2.2 for TestFlight, above the last approved App Store version
(0.2.1); App Store submissions use their own version. To build one, push the tag
`ios-beta-appstore-<version>` (e.g. `ios-beta-appstore-0.2.1`) on the chosen `dev` commit:
it matches the Xcode Cloud start condition, and `ci_scripts/ci_post_clone.sh` stamps that
version into `project.yml` before generating the project. Once Apple approves a
version, later uploads must be higher than it. Hex Mobile is iPhone only (`TARGETED_DEVICE_FAMILY` 1).

The Xcode Cloud workflow needs a tag start condition with prefix `ios-beta-`,
container `ios/HEXMobile.xcodeproj`, shared scheme `HEXMobile`, an iOS Archive
action, and a TestFlight post-action for **Hex Internal**. Cloud owns the build
number. Verify the run's source tag and the processed build in that group.