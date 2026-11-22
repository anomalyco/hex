# Hex for iPhone

**Status:** Internal TestFlight app (`HEXMobile`), custom keyboard extension
(`HEXKeyboard`), and Live Activity + Control Center widget extension
(`HEXWidgets`). The containing app records a generation-scoped temporary WAV,
transcribes on-device with Apple Speech on iOS 26+ (`SpeechTranscriber`, or
`DictationTranscriber` for the languages it lacks, such as Norwegian, Danish,
Swedish and Hebrew) or FluidAudio Parakeet V2 (iOS 18+, English). Before iOS 26,
every other language, and English on 3 GB iPhones (iPhone XR, SE 2nd generation)
or after Parakeet couldn't load, goes to Apple's `SFSpeechRecognizer` on device
only (`OnDeviceSpeechTranscriber`, chosen by `SpeechEngineChoice`). Hex removes
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
open Hex instead, as before iOS 26. Audio never leaves the iPhone. From iOS 26 Hex
never asks for Speech Recognition access: that permission covers `SFSpeechRecognizer`,
which Hex uses only before iOS 26, and `SpeechAnalyzer` runs without it in the app and
the keyboard alike (a TestFlight tester who turned it off dictated, iOS 27.0.1, build 51).
Before iOS 26 Hex asks for it as it gets `SFSpeechRecognizer` ready (setup, or Set up),
never at a launch by itself.

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
    nothing (the keyboard notes it in the App Group). The keyboard also notes
    when it comes on screen and leaves (`KeyboardPresence`): when Hex's
    keyboard isn't up 2 s after a keyboard switch, the page says to tap the
    globe until it is, and only words that arrive while it's up count as
    typed. iOS's dictation and other keyboards take it off screen, so their
    text never finishes the practice; the page says to tap the Hex key
    instead. **Hex is ready** shows
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
    and, where a typing model is bundled for the language, Spatial autocorrect's
    model: English and Spanish, and German with its experiment on). Words
    it learns may last only while the keyboard runs, as an iPhone may not let
    it write the App Group; haptics, key clicks (Apple's keyboard guide), Paste and
    dictation need Full Access. Without Full
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
    (Hex's page in Settings) while the keyboard isn't ready. iOS reports Full
    Access only inside the keyboard, so the keyboard reports it to the App Group
    as it loads, each time it appears and every 5 s while shown
    (`keyboard.fullAccessOn`/`Off`, with when its process started, and
    `keyboard.fullAccessSeenAt` for an on), and posts a Darwin notification the
    app refreshes on. Full Access shows as on once a keyboard has run with it,
    and as off only when a keyboard said so since Hex last came to the
    foreground (it changes only in Settings), from a keyboard process no older
    than the one that last saw it on. Otherwise the sheet says "Not confirmed
    yet" with "Switch to Hex below to confirm." under it, and focuses the field.
  - **Opening Hex:** SwiftUI `Link`, `openURL`, and `extensionContext.open`
    do nothing in a keyboard extension, so the keyboard opens Hex through
    the `UIApplication` at the end of its responder chain
    (`ContainingAppOpener`).
  - **Cold start (before iOS 26):** Tapping **Dictate** opens the containing app
    (`hex-dictation://keyboard/record?...`) with a 10-second
    handoff window and starts recording. Hex never opens another app: the
    handoff screen shows a quiet "Swipe bar to return" cue over the home
    indicator, and you swipe back to the app you were typing in. The text goes in
    once the keyboard is on screen there again: iOS often starts a new keyboard on
    the way back, and one that inserted while it loaded, before its field was
    connected, lost the text (iPad mini, iPadOS 18, TestFlight build 115).
  - **GPU Dot Scope (`HEXShared/DotScope.swift`, `DotScopeShader.metal`):**
    Both `HEXMobile` and `HEXKeyboard` render Hex's desktop menu-bar braided
    oscilloscope (`src/dot_scope.rs`, `src/rune_hud.wgsl`) at native display
    refresh via `CAMetalLayer` + `CADisplayLink`, pausing at 0% CPU/GPU once
    settled in idle. The keyboard extension mounts it only while a capture or
    transcription is active and shows a static `Canvas` dot row at rest, so a
    fresh keyboard process builds no Metal device, pipeline, layer, display
    link, or idle pre-roll before its first frame; the app's docked keyboard
    preview keeps the live scope. The home-indicator swipe cue's pipeline is
    compiled on first use, which only the app shows. Both layers turn off the Metal
    Performance HUD (Settings › Developer › Graphics HUD), which would draw its frame
    rate under the home well's Dictate pill, unless launched with `MTL_HUD_ENABLED=1`. Lock Screen / Dynamic
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
  - **French spacing (every style but Verbatim):** dictation in French, or translated
    into it, gets the spaces French puts around punctuation, last in the pipeline
    (`FrenchSpacing`): a narrow no-break space (U+202F) before `? ! ;` and a no-break
    space (U+00A0) before `:` and inside `« »`; Canadian French none before `? ! ;`
    (OQLF), Swiss French narrow ones inside `« »` too. Whatever was there becomes
    exactly that one space, and only a mark that ends a word is spaced, so 10:30, URLs,
    addresses, emoticons (":)", ";-)"), code in backticks and a mark at a line's start
    stay as they are. Apple Speech writes a plain space before "?" in fr-FR and fr-CH and
    none in fr-CA's `DictationTranscriber` (macOS 27.2.1, `say` voices), and every
    style but Verbatim took the space out ("ça va?"). Apple's French keyboards type
    no space before a mark (iOS 27.2 simulator, black box), so the full keyboard
    doesn't either.
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
  - **Spoken punctuation (Settings › Dictation › Spoken punctuation):** On by
    default (`DictationPunctuationSettings`, App Group key
    `settings.dictationSpokenPunctuation`), in English, Spanish, French, German,
    Italian and Portuguese, every style but Verbatim (`SpokenPunctuation`, inside
    the style after Format lists, so "dash … dash …" stays a list). Punctuation
    said by name becomes the mark: "comma", "period"/"full stop", "question
    mark", "exclamation point", "colon", "semicolon", "ellipsis", "dash",
    "hyphen", "apostrophe", "hashtag", "dollar sign", "open quote … close
    quote" (or "quote … end quote"/"unquote"), "open paren … close paren" and
    "smiley" (":-)"), and each language's own words ("coma", "punto", "signo de
    interrogación", "abrir comillas", "punto y aparte"; "virgule", "point
    d'interrogation", "ouvrez les guillemets"; "Komma", "Fragezeichen",
    "Anführungszeichen unten … oben", "Klammer auf … zu"; "virgola", "punto
    interrogativo", "apri virgolette"; "vírgula", "ponto final", "abre aspas").
    Quotes are “ ” (« » in French, „ “ in German), French spacing follows
    (`FrenchSpacing`), and a word after "." "?" "!" gets a capital. Black box
    (macOS 27.2.1, `say` voices): `DictationTranscriber` already writes every
    mark, so its text has no names left; `SpeechTranscriber` writes "comma",
    "question mark" and some French, German and Italian names, leaving the
    rest as words, often between commas; Parakeet writes every name as words
    ("noon, comma, see you soon, period", "tonight question mark?"). A mark the
    model already put beside a name is replaced, never doubled. Guards keep
    prose: never after an article, possessive, preposition or a word that
    makes the name a noun ("a period of time", "the comma is", "Oxford comma",
    "trial period", "ein wichtiger Punkt"), before one that continues it
    ("question mark tattoo", "colon cancer", "punto de vista"), in a compound
    ("comma-separated"), or between numbers ("two comma five"); the names that
    are everyday nouns ("period", "point"/"Punkt"/"punto"/"ponto", "question
    mark", "dash", "smiley") also need the line's end, a mark after them or a
    clause start after them; a colon needs words after it; a closing quote or
    bracket closes only an open one. `TranscriptCleanupTests` holds the
    models' transcripts and 141 prose sentences (84 English) that must not
    change. Polished no longer ends a line that ends in "," or ";" ("Hi
    Sarah," before a line break) with a period, and keeps the space before an
    emoticon (":-)").
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
  - **Spoken counts (English, every style but Verbatim):** a count of three
    numbers or more from one, in words or digits with only commas or spaces
    between them, is written in digits: Apple's `SpeechTranscriber` writes
    "Test one, 2, 3." where Parakeet writes "Test 1, 2, 3", and both become "Test
    1, 2, 3." "123" right after "test" or "testing" and ending its clause, which
    Parakeet and `DictationTranscriber` write for a quick "testing one two
    three", becomes "1, 2, 3" too (`SpokenCleanupRules.spokenCounts`, run by
    `CleanupTagger.cleanIfEnabled` before the tagger, with Clean up what I say on
    or off). Other numbers stay ("Chapter one, two and three", "room 123", "test
    123 passed"). The tagger alone took a count in words for a self-correction
    ("Test one two three" became "Test three."); `CleanupGuard` keeps it.
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
  - **Auto-start:** Settings › Keyboard › Auto-start (`FullKeyboardSettings.dictationBarAutoStart`)
    lets the dictation bar start dictating by itself as it shows, as a tap on **Dictate**
    would, 0.8 s after it appears (after the first frame and the launch's optional work),
    only while idle, outside number pads and the emoji panel, and only where it dictates
    without leaving the field (a warm session or in-keyboard dictation, never by opening
    Hex). **Never**, **On switch** (the default) or **Always**. iOS doesn't tell a
    keyboard whether you switched to it or a field opened with it, so On switch guesses
    (`KeyboardSwitchGuess`): the keyboard comes back in the field it last hid in
    (`documentIdentifier`), and the field's app didn't go to the background
    (`NSExtensionHostDidEnterBackground`) from 2 s before that hide on. Switching to
    another keyboard and back in one field starts; a field that opens with Hex already
    chosen doesn't. Known gap: the first switch to Hex in a field another keyboard opened
    is another document, so it doesn't start. Unproven on a device: whether
    `documentIdentifier` stays the same across a keyboard switch and changes when a field
    is focused again, which the guess relies on.
  - **Push to talk:** Holding still for 0.35 s on the full keyboard's Hex key
    or the dictation bar's **Dictate** starts dictation at once with a light
    haptic (Settings › Keyboard › Haptics), and lifting the finger finishes it as
    **Done** does; a quick tap still starts it and **Done** finishes it. On
    the Hex key, sliding more than 10 pt before the hold fires opens the menu
    instead, and once dictation has started, moving the finger changes
    nothing. Push to talk needs dictation in the keyboard (a warm session or
    in-keyboard dictation); a hold that opens Hex is only a tap. The full
    keyboard's bar takes the same hold (0.4 s, Apple's long press) anywhere no
    word is: an empty slot, beside the Hex key, or a bar with nothing in it,
    through the Hex key's own path (`HexKeyGesture`, `hexKeyHeld`), but not
    while the bar shows the Full Access hint or a notice. A tap there does nothing,
    a slide cancels it, and a word's slot keeps its tap and its hold (Add to
    vocabulary). The bar's Paste is plain text on the words' line, and Paste and
    Next keyboard centre on the keyboard's middle, not the strip's. Paste shows
    only while the field is empty, as Apple's keyboard offers a copy, and Code
    from Messages only in an empty one-time-code field with a number pad or with
    autocorrection set either way (as WebKit sets it on Safari's
    `autocomplete="one-time-code"`): a chat box that marks itself one-time-code
    keeps its suggestions.
  - **Apple Intelligence cleanup (Settings › Experiments › Clean up with
    Apple Intelligence, iOS 26+):** Off by default. For every style but `Verbatim`,
    one on-device FoundationModels pass before formatting fixes clearly
    misheard words and spoken self-corrections, within a 2 s budget. A
    timeout, error, empty or implausible output keeps the raw transcript, and
    the toggle is disabled with a reason when Apple Intelligence is unusable.
  - **Multilingual model (Settings › Experiments › Multilingual model (Parakeet), iOS
    18+):** Off by default, and disabled on iPhones under 6 GB
    (`MultilingualParakeetSettings`, in the App Group). While on, Hex dictates
    Parakeet Ultra's 25 languages (Moondream's post-training of NVIDIA Parakeet TDT
    0.6B v3) with it, which hears them mixed within a sentence (Dutch with English
    words, a TestFlight request); any other dictation language keeps the engine Hex
    picks without it. The language Ultra takes is the one set, else the one Automatic
    follows on the keyboard, else the iPhone's first. Turning it on downloads
    FluidInference's Core ML build (632 MB, pinned to revision `95eaa59`, each file
    checked against its SHA-256 and resumed from its `.partial`) into Application
    Support, out of backups, through the usual preparation bar ("Downloading 632.2 MB ·
    Wi-Fi recommended", then Core ML's compile); turning it off deletes it, as does a
    launch that finds a download left with it off. FluidAudio 0.15.5 has no
    `.ultra`, so `ParakeetUltraModel` loads the four models itself and hands
    FluidAudio v3 `AsrModels` (its own loader would replace a folder it can't load
    with v3's download). It is chosen only with 1.5 GB left to load it
    (`os_proc_available_memory`), and never again after a load that ended Hex
    (`ParakeetLoadGuard`, its own keys) until the switch is turned off and on. The
    keyboard can't load it, so while it takes the dictation language, Dictate and the
    Hex key open Hex, as before iOS 26 (a warm session still dictates without
    leaving the app). Measured on a Mac (`~/dev/experiments/2026-11-21-hex-multilingual-parakeet`):
    mixed Dutch-English test sentences 7.5% WER against Apple Speech nl-NL's 26.9%,
    pure Dutch 0.9% (0.4%), pure English 0% (0%). Load memory, compile time and speed
    on an iPhone are unmeasured.
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
    app and in-keyboard dictation. The full keyboard uses the same list whether or not the
    switch is on (Settings › Keyboard › Vocabulary edits it too; TestFlight build 56:
    "custom dictionary for uncommon programmer slang"): its words, and each word of a
    phrase, are never autocorrected away, a different case is put right on space ("github"
    → "GitHub"), the strip completes them before the dictionary's words ("kub" →
    "kubectl") with no spelling guesses for them, Swipe typing reads them as your own words,
    and a bilingual partner shares them. Holding the word you typed in the strip's left slot
    offers **Add to vocabulary** there (`FullKeyboardView+Vocabulary`; a capital only a
    sentence's start gave the word is left out); without Full Access
    the keyboard can't write the App Group, so the word is kept for that session only.
    Before this the keys knew only iOS's text replacements and Contacts names
    (`requestSupplementaryLexicon`), never Hex's list. After
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
  - **Learn how I type (Settings › Privacy › Learn how I type):** On by default (an
    unset switch reads on; turning it off is saved and stays off), and
    shown whether or not the full keyboard is on. It learns only from typing and
    dictating with Hex, on this iPhone, never from what you write elsewhere. A
    private journal in the App Group (`typing-journal.json`) that never stores
    what you write: per-key tap offsets, and word-level pairs with counts and at
    most one word of context on each side. Only plain text fields that allow
    autocorrect count (`TypingJournal.allows`). Nothing leaves the iPhone.
    - **Typing:** words you erase and retype, autocorrect fixes you undo from the
      strip, suggestions you pick, and autocorrections you turn down by
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
    - **Per language (Settings › Experiments › Learn each language separately):**
      Off by default (`experiments.learnPerLanguage`, App Group). Every correction and
      dictation fix is tagged with the language it was typed or dictated in either way,
      so turning it on needs nothing learned again; a fix made in two languages is
      tagged with neither and counts for every one. On, the keys learn a language's typos
      and rejections from that language's corrections and the untagged ones, and a
      dictation applies only the fixes made in its language and the untagged ones (a fix
      learned in en-US applies in en-GB). Learned words are one list per keyboard
      language (`keyboard.learnedWords.<code>`), started from the shared
      `keyboard.learnedWords` list the first time that language is used; bilingual
      autocorrect's partner keeps its own list and its own fixes. Where fingers land and
      Adaptive touch stay shared. Delete what Hex learned erases the shared list and every
      language's. Two limits: a dictation fix you kept as heard in one language still
      counts against that fix in every language, and retyped words and picked suggestions
      are tagged with the keyboard's main language, never the bilingual partner's.
      Off, everything is the one shared list and learning it was before.
  - **Auto-capitalization (Settings › Keyboard › Auto-capitalization):** On by default,
    as Apple's (`FullKeyboardSettings.autoCapitalization`, App Group key
    `settings.keyboardAutoCapitalization`). Off, shift never turns itself on, whatever the
    field's `autocapitalizationType` asks (a field's or sentence's start, after "." "!" "?"
    or the double-space period, which still types ". "), and autocorrect and swipe typing add no
    capital you didn't type: "i" stays "i", "im" becomes "i'm", "mary" stays "mary" and
    "mondya" becomes "monday", as Apple's keyboard does with Auto-Capitalization off (seen on
    the iOS 27.2 simulator). Words you typed with a capital, and your own spellings (text
    replacements, Contacts names, words you kept, fixes Hex learned, your vocabulary), keep
    theirs; shift and caps lock work as before. The strip offers no capital you didn't type
    either (TestFlight build 56: "even with auto-capitalization off, many suggested words are
    capitalized"): a spelling guess or completion iOS knows capitalised ("Mandy", "Monday")
    and the contraction "I'd" for "id" come lowercase there; sentence and clause starters
    keep "I" and "I'm" (#2135), and next words and swipe readings already had no capital.
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
    A run that is no word and no dictionary's at all is also read as a word plus one slip
    in the next ("specialchsracter" -> "special character") or as three to six words typed
    together ("seeyoutomorrow" -> "see you tomorrow"), when the words without their missed
    spaces are no less likely than the run; those can replace a run the whole-word margin
    would keep. Deutsch keeps the ordinary readings: its lowercase model cannot give a
    split's nouns their capitals.
    Shortcuts, learned fixes, contractions, and words you kept still come first. Other
    layouts (Norsk, Deutsch, Italiano, Français, Português, Polski, Română, Русский, Українська, Ελληνικά) keep the dictionary
    engine in their own language, and Srpski (no iOS dictionary) none, never the English
    model, except as bilingual autocorrect's second language (below). The
    models (`HEXKeyboard/TypingModel/en.hextm` and `es.hextm`, about 3.6 MB each) are
    built from Google Books Ngram v3 by `scripts/build-typing-model.py` (see
    Acknowledgements below); `de.hextm` is the German model Android builds and ships
    (`android/benchmark/typing/build-layout-model.py`), the same v3 format and, on iOS,
    off until the experiment below is turned on.
  - **German spatial autocorrect (Settings › Experiments › German spatial autocorrect):**
    Off by default (`experiments.germanSpatialAutocorrect`, App Group), and only while
    Spatial autocorrect is on too. With both on, the Deutsch layout reads taps with
    `HEXKeyboard/TypingModel/de.hextm`, byte-identical to
    `android/app/src/main/assets/typing/de.hextm` and the hash its `SHA256SUMS` lists: the
    word list feeds the decoder, the letter odds and the strip, and a bilingual partner may
    read German taps too. German's model is lowercase throughout, so a noun takes its capital
    from the list beside it, whether the decoder corrected it or it was typed right ("hsus"
    reads "haus" and comes out "Haus"; "das haus" becomes "das Haus"): 95% and up anywhere,
    and 50-94% after an article or determiner, while Auto-capitalization is on. A 50-94%
    word with no determiner before it, and any word the list lacks, keeps the dictionary's
    rule: a verb stays lowercase ("zu leben", "ich frage", "wollt ihr essen") and a word the
    dictionary knows only capitalised still takes its capital ("heute abend" becomes "heute
    Abend"). The builder caps at 94, so a capital arrives only after a determiner: a word that
    is also a common first-person or imperative verb form ("frage", "reise", "drucke") or the
    stem people type alone ("kauf", "schlaf", "sitz": "ich kauf noch brot", "schlaf gut"); the
    separable prefixes "teil" and "preis", the adjective "klasse", and the verb forms the
    counts miss ("filme", "rolle", "presse", "pumpe", "campe", "pflanze", "beichte", "fische",
    "taufe"); and every two-letter "word", a chat abbreviation ("gl", "rn") rather than a
    noun. The determiner list includes "in" ("in Ruhe", "in Frage") but not "ohne" ("ohne
    essen" is a verb). The list is
    `HEXKeyboard/TypingModel/de.capitals`, built by `scripts/build-german-capitals.py`
    from the same Google Books shards: for each model word it compares the 1980–2019 counts
    of the all-lowercase and first-letter-capitalised spellings and lists
    `word<TAB>percent` for the words at least 50% capitalised and seen at least 40 times,
    leaving out pronouns and possessives capitalised for other reasons (`FUNCTION_WORDS`:
    "sie", "ihr", "mein", and the rest). Rebuild it from `ios/` with
    `python3 scripts/build-german-capitals.py HEXKeyboard/TypingModel/de.hextm HEXKeyboard/TypingModel/de.capitals`;
    it streams the eight shards through curl and gzip four at a time, caches each shard's
    counts under `scripts/.build-german-capitals`, keeps their merged per-token counts there
    so a rule change renders without streaming again, and applies the caps after counting.
    Off, Deutsch keeps the dictionary engine exactly as before: no decoder, no word list for
    the letter odds or model completions, and no model for a bilingual partner. Swipe typing
    stays English and Spanish only.
  - **Adaptive touch (Settings › Experiments › Adaptive touch):** Off by default
    (`experiments.adaptiveTouch`). The keyboard learns where you land on each physical
    letter key, on this device, and Spatial autocorrect's decoder and the strip read your
    taps with those personal centres (`AdaptiveTouch.swift`). Keycaps, hit rectangles,
    the next-letter claims and Swipe typing are unchanged, and it never makes an automatic
    correction the keyboard wouldn't make with it off: space puts in a personal reading
    only when the centres read without it also clear it by the margin
    (`AutocorrectEngine.decoderBaselineKeys`, `TypingDecoder.replaces`), else their own
    decision, and joining two words at the space bar's edge is theirs alone to decide;
    what only the personal centres read better is offered in the strip. While
    on, Learn how I type's per-letter offsets stop moving the decoder's centres (they stay
    that baseline), and its word-level learning carries on.
    - **What teaches it (`AdaptiveTouchLabels.swift`):** only taps with evidence of the
      letter meant, and only once the next word shows the word stood: a word left as typed
      (letters the key under the finger typed, not ones the next-letter odds claimed), a
      picked suggestion of the same length (at most two letters changed), a word kept over
      an autocorrection by reverting it from the strip or tapping it, and a letter erased and retyped as a
      neighbour (the erased tap is labelled with the letter that replaced it). Never an
      autocorrection left in place (or a word before it revised, or two joined), a swipe, a
      slide, a strip touch, an accent pick, or a password, code, email, URL, secure or
      no-autocorrect field. Taps and words wait in
      memory for at most two words and 30 s, and are dropped when the cursor moves, the
      host changes the text, the field or the keyboard's size changes.
    - **What it keeps:** sums per physical key in four rotating bins of 200 labelled taps,
      for each context (letters, posture, orientation, split, Hex key side, width) and one
      of two grip profiles: offsets in keycaps (so a new key plane of the same size keeps
      them), shrunk toward the row and the keyboard, at most 0.3 keycap and 0.05 per
      update, fixed spread. No letters, words, times or sequences: 23,588 bytes, in a file
      in the keyboard's own container with Full Access (protected, excluded from backup,
      never in the App Group or its exports, written in batches off the main thread), in
      memory only without it. Settings shows Session only, Learning (n of 400 taps) or
      Active from counts the keyboard reports; Reset touch learning, and Delete what Hex
      learned, bump a reset generation that drops it from memory and disk (the file is
      the keyboard's, so it goes the next time the keyboard runs) and that a write
      queued before can't undo.
    - **When the decoder uses it:** a profile is used only after 400 labelled taps over
      three sessions, while it fits your recent taps better than the centres read without
      it (log-likelihood per tap, each axis, over 32 labels). Once it fits worse (a new
      grip, another hand), the keyboard goes back to them at once and labels teach the
      second profile; a grip that comes back gets its profile back. Before the first 400
      taps nothing is in use, and a new grip just mixes into what is learned.
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
    autocorrects it. As on Apple's keyboard (watched on iOS 27.2), a space stays pending
    after the word: a tapped letter, digit, opening mark or symbol goes in after a space
    ("hello" then w-o-r-l-d types "hello world", "(" types "hello ("), while
    `, . ! ? ; : ) ] } ' - /` go against the word ("hello,") and take that space; shift you
    set stays, and delete then takes the letters and the space one at a time. A second
    finger going down ends the swipe first, so its word goes a space before that key's
    letter. On the iPhone Duo's split
    keyboard a swipe reads across the middle between the halves: its points are read with
    the right half pushed against the left and the middle shrunk to one key's gap
    (`KeyboardLayout.joinedForSwipe`), so either half's G and V is the one letter, as for
    Spatial autocorrect's taps. Taps, holds for accents, slides
    shorter than a key, delete's word slide, the space bar's trackpad, 123 and the Hex key
    work as before: no swipe starts on them, and none once an accent menu is open. A swipe
    that lifts on the key it went down on, never more than 1.6 key pitches from it, is a
    tap that slid: its letter goes in as a tap's would ("a" dragged a little too far and
    back), unless it read as a word that starts and ends on that key ("lol").
    `SwipeDecoder` is a shape-matching decoder after SHARK2: the path is resampled to 32
    points in key pitches; words come from the language's typing model by the keys the
    path starts and ends near (each start letter is one range of the sorted word list,
    read in place from the memory-mapped model, so nothing is built or kept between
    swipes), kept when every key lies near the path and their ideal path (the polyline
    through their keys' centres) is about as long; each is scored on location (point by
    point, then dynamic time warping for the best 80), shape, and how near its first and
    last keys the finger went down and lifted, the best 80 also on how their keys account
    for where the finger turned and, from the touches' times, where it slowed (an edit
    distance: a turn or pause no key of the word explains costs, and so does a key the word
    goes straight through without a pause on it), and weighed with the model's word-pair
    frequency after the word before. A straight stroke runs over the letters between its
    ends, so the path alone can't tell "tired" from "tied" or "wire" from "write" (X
    tester: guesses didn't account for the letters under the swipe); a pause on r can.
    English contractions, which the Google Books model barely counts, and your own words
    (Contacts names and other `UILexicon` terms, one-word
    text replacements, words you kept) are read beside it. It never reads a slur, and reads
    swear words only with Suggest swear words on (`TypingDecoder.NeverSuggest` and
    `mayOffer`, as the strip). Its weights are tuned on synthetic swipes of the
    autocorrect benchmark's held-out dev sentences and on mined sets of common words that
    share a path; on the held-out test sentences (typical fingers, 390 pt) top-1 is 97.1%
    in English and 94.2% in Spanish (94.9 and 90.9 without the turns and pauses), and on
    held-out words that share a path 96.0% and 94.5% (85.1 and 81.2); a decode takes
    0.91 ms at the median and 2.85 ms at p99 on an M5 Max (experiment folder
    2026-11-20-hex-swipe-typing, results/2026-11-21). Record typing logs keeps a swipe's
    touch moves and a `swipe_end` line (points, decode time, readings) for fitting it to
    real fingers.
  - **Record typing logs (Settings › Privacy › Advanced › Record typing logs):** Off by default,
    and apart from Learn how I type. A one-time migration (`migrations.typingLogsOff.v1` in the
    App Group, `TypingLogSettings.turnOffOnce`, run by the app and the keyboard at launch)
    turned it off for everyone who had it on, keeping the logs already recorded.
    Its description says what it records, and
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
  - **Dictation:** Style, Lowercase, Format lists, Spoken punctuation, End with a
    period, Dictation language, Translate what I say, Clean up what I say,
    Custom vocabulary (its list below the switch), Keep mic warm,
    and Microphone.
  - **Keyboard:** Keyboard, **Full** (the default) or **Dictation bar** (stored
    as `FullKeyboardSettings.isEnabled`, App Group key `experiments.fullKeyboard`
    from when the full keyboard was an experiment: unset or true is Full, false
    the dictation bar, so testers who turned that experiment off keep the bar;
    on an iPad, where the keyboard always shows the dictation bar, the row says so:
    "On iPad, Hex shows the dictation bar"), Auto-start (Never, On switch by default, or
    Always) while the dictation bar is chosen or on an iPad, then Languages, Auto-capitalization, Autocorrect (with Spatial autocorrect below it while it's
    on), Suggest swear words, Vocabulary (Hex's custom vocabulary list, how many words it
    holds) and Key pop-ups only while Full is chosen, Haptics (the dictation bar uses it too),
    then Swipe typing and Emoji (Clear emoji history while it's on), which say they need the
    full keyboard while the dictation bar is chosen, and a field to try the keyboard in.
  - **Privacy:** "Everything stays on this iPhone", Learn how I type (what it
    learned below the switch), and under **Advanced**, Record typing logs (off
    by default) with its Typing logs viewer while it's on.
  - **Experiments:** Hinglish (Hindi in Latin letters), Clean up with Apple
    Intelligence, Adaptive touch (its status and Reset touch learning below the switch),
    German spatial autocorrect, ABC under the emoji key, and Multilingual model
    (Parakeet).

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
  Apple has. Apple Speech's Spanish includes es-ES, es-MX and es-US (and es-CL on
  recent systems), and Spanish follows the iPhone rather than always Spain: a region
  Apple has keeps it (es-CL on a Chilean phone), es-419, es-CO, es-AR and any other
  Latin American, United States or unlisted region is the Americas' Spanish (es-US,
  then es-MX), plain `es` is the iPhone's Region's (es-MX in Mexico, es-US in the
  United States, Puerto Rico and Latin America, es-ES elsewhere), and only Spain and
  Equatorial Guinea stay on es-ES (`DictationLanguageSwitch.spanishCandidates`,
  `spanish(forRegion:)`). Before iOS 26 `SFSpeechRecognizer` lists es-CO and es-419
  itself, so the on-device pick keeps them, then falls through es-US, es-MX and
  Spain's. While
  the keyboard switches between two or more languages, Automatic follows the one
  it shows (Keyboard languages, below). The menu also lists every locale
  `SpeechTranscriber` or
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
  launch. Parakeet stays where it fits (6 GB) and before iOS 26, for English only: when
  Apple Speech doesn't answer, any other language stays on Apple Speech, which says it can't
  dictate it, rather than going to Parakeet as English
  (`DictationController.appleSpeechLanguage`). After Hex ended
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
  `settings.dictationLanguage.supported`). One variant per language, the first
  of the current, the recents and the preferred winning, so Spain's Spanish and
  Mexico's never both take a chip; picking one drops its siblings from the
  recents (Norwegian's nb/nn/no aliases stay one language). Chinese stays
  apart, so Mandarin's zh-CN, Taiwan's zh-TW and Cantonese's zh-HK each keep a
  chip and a pick keeps the others in the recents. Names are each language's
  own ("Español", "Deutsch", "עברית"), with the region where Apple Speech has
  more than one of that language ("Español (US)" or "Español (ES)", "English
  (GB)"; Norwegian stays "Norsk"), so the menu and the dictation bar's chips
  say which variant dictation actually uses. Under Automatic, Settings ›
  Dictation language says the same itself while the keyboard follows: "Follows
  keyboard: English (US) · Spanish (US)" names each keyboard language's
  dictation language, and is refreshed when Hex comes back to the foreground,
  not only when the row
  appears. A long press on a language (a chip, or a row on the Hex key menu's Language
  page) shows **Remove**, which takes it off (`settings.dictationLanguage.removed`, so one
  of the iPhone's own languages stays off too) and puts the one set back on Automatic;
  Settings › Dictation language's **Remove from keyboard** does the same (TestFlight build
  56: a language couldn't be removed, and one could only be added in Hex). **More
  languages…** lists every other language Apple Speech has, the iPhone's own first, and
  picking one there adds it (in the dictation bar the list takes the keys' place). Removing
  or adding one never changes Automatic, which still follows the keyboard's language.
  A pick writes the same App Group setting as Hex's menu, so
  in-keyboard dictation uses it at once; a warm Hex session follows it within
  100 ms and prepares the language between dictations, and a handoff or the
  Action Button picks it up before recording.
- **Translate what I say (Settings › Dictation, iOS 26+):** Off by default. Translates
  each dictation on the iPhone with Apple's Translation framework into the
  language picked under Insert in, once its pack is downloaded. When it
  can't (no pack, a pair Apple can't translate, or a translation that fails
  twice, each in a new `TranslationSession`, never one kept), the
  transcript goes in as spoken and the keyboard says so for 8 s:
  "Couldn't translate, pasted as spoken" over the fix ("Download Chinese in
  Settings"), whose Open Hex (the dictation bar: a tap) opens Settings ›
  Dictation (`hex-dictation://keyboard/translation`). Settings keeps the
  last failure (`experiments.translateDictation.lastFailure`, with Apple's
  error code, never the text) beside the download until a dictation
  translates. With Hinglish on, Hindi going into another language is
  translated from its Devanagari, and romanized only if it goes in as
  spoken. Where you dictate, a "Translate to
  English" switch turns it on or off without opening Hex (testers asked for
  a button, not a setting): a row in the full keyboard's Hex key menu, lit
  with a checkmark while on, and a chip in the dictation bar's language tray.
  It writes Settings' own switch (`experiments.translateDictation`), the
  only switch: nothing else turns translation off (build 56's "Paste in
  Hindi" left `experiments.translateDictation.pastesTranslation` false
  with Settings still on; that reads as off, and the next switch removes
  it), and Settings re-reads it when it shows. Below it, Translation language lists
  up to six targets (the current one, English, the dictation and preferred
  languages, then common ones, only those Apple translates into); picking one
  sets `experiments.translateDictation.target` and turns translation on.
  Settings keeps the full language list and the pack download under Insert in. While dictations paste translated, the Hex
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
  and autocorrect. English and Español chosen together are Apple's bilingual "English &
  Spanish" keyboard (`KeyboardLanguageList.layout`): both type on the first one's keys and
  long presses, so English first is QWERTY with ñ on a held n in both, Español first keeps
  its ñ key in both, and the swipe switches the language they autocorrect and dictate in
  (black box, iOS 27.2 simulator: Spanish added to Apple's English keyboard has no ñ key and
  English's long presses; English added to its Spanish keyboard keeps the ñ key and
  Spanish's). Apple and Gboard switch with a long press on the globe, but a Face ID
  iPhone shows no globe in a third-party keyboard and Hex's space-bar slide is its cursor
  trackpad, so this follows SwiftKey: with two or more languages the cursor slides once the
  space bar was held still for 0.35 s (the hold that starts Apple's trackpad, below), a slide
  before that is a swipe, and one shorter than 24 pt types a space
  (`FullKeyboardEngine.trackpadHoldDelay`, `languageSwipeDistance`).
  The globe key is unchanged. The keyboard comes back in the language it was left in, in
  every app and field, as Apple's keyboards do (black box: a switch in Messages held in
  Safari and back; `settings.keyboardLanguages.current`; without Full Access an iPhone may
  keep it only while the keyboard runs). The input mode says what it types: Hex sets
  `UIInputViewController.primaryLanguage` to the language's BCP 47 identifier
  (`KeyboardLanguage.inputModeLanguage`) as the language changes (unit-tested), meant to
  make the host spell and autocorrect in the language typing; Apple documents it as
  superseding the extension's Info.plist en-US, but whether the host follows it (Notes's red
  underlines, for one) is not verified on a device. Switching from the full keyboard to the
  dictation bar leaves the last reported language in place. A field gives a third-party
  keyboard nothing else to go on: before this change `documentInputMode` was observed as nil
  in Messages and the plist's en-US in a field that asks for English; what it reports with
  the runtime language is unverified. **Dictation follows the keyboard:** with
  two or more languages and Dictation language on Automatic, dictation is in the language
  the space bar shows, in the iPhone's region where Apple Speech has it
  (`LanguageSettings.keyboardDictationLanguage`,
  `KeyboardLanguage.dictationLanguage`; none for Srpski), in the keyboard and in Hex,
  which prepares the new language between dictations (TestFlight build 56: with the
  keyboard on Español, dictation stayed English and Spanish came out "incomprehensible").
  Spanish is the iPhone's Spanish, not always Spain's: a region Apple has keeps it
  (es-CL on a Chilean phone), es-419, es-CO and any other Latin American or United
  States region is es-US, then es-MX, plain `es` and no Spanish preferred language
  follow the iPhone's Region (es-MX in Mexico, es-US in the United States, Puerto Rico
  and Latin America, es-ES elsewhere), and Spain and Equatorial Guinea stay on es-ES.
  A Spanish variant picked by hand for that language is used again
  when the space bar comes back to it: the recent pick goes ahead of the preferred
  languages, so es-US or es-MX survives the switch away and back. It doesn't where the
  iPhone's own language has no layout (Hebrew), as dictating is how that language gets
  written. **Switch dictation too** (off by default, shown with two or
  more on iOS 26+) follows there too, and puts a dictation language picked by hand back on
  Automatic at each switch (it used to write the keyboard's language as a pick, which
  stayed after Spanish was unticked or the switch turned off). **Bilingual autocorrect:**
  the first other chosen language with Latin letters and
  an iOS dictionary (`KeyboardLanguageList.partner`: English beside French, French beside
  English; none for Русский, Українська, Ελληνικά, Srpski, or two French layouts) is checked on the same keys,
  with its Spatial model where it has one, as Apple's "Français et English" keyboard does: a
  word either language knows as typed is never changed, a typo only one can fix gets that
  fix ("teh" → "the" while French types), and when both can, the language of the last words
  only one dictionary knows wins, else the clearly cheaper slip, else the keyboard's own;
  the strip shows fixes and completions in the language the words before are in, never the
  other's, even where that language has none (TestFlight build 56, Čeština and English:
  "autocomplete suggests English words in Czech too"; the iOS 27.2 simulator's Czech
  dictionary completes nothing, and English filled the strip), and with nothing before to
  tell, both languages', in turn. In the simulator the English
  keyboard alone changed 34 of 77 common French words and the French keyboard alone 27 of
  80 English words; with both chosen, none. Any two Latin-script languages with an iOS
  dictionary pair this way (English with Français, Deutsch, Italiano, Português, Polski,
  Čeština, Norsk, Română, Español, or two of those), whatever their order; Polski's simulator
  dictionary takes every word, so the any-word guard (#2143) leaves it out there. The words before
  count for a language when only its dictionary knows them, or, from three letters, when
  both do but only its typing model has them among its commonest (iOS's French and German
  dictionaries take "the", "you", "your" and "please"). Once they are the keyboard's own
  language, a word the other dictionary knows still gets its sure fixes (a missing
  apostrophe or common slip from `unambiguousFixes`, a two-letter slip); a word without
  a vowel never counts as the other's ("dk", "tkm" are abbreviations there); the other's
  name guesses ("Zuri" for "Ziri") are never fixes; Spatial autocorrect may still read the
  taps as another word, counting the other's word as one it changes only on its valid-word
  margin; and at a sentence's start, "Im" and "Dont" are revised to "I'm" and "Don't" once
  the next word leans English (`bilingualRevision`; German "Im Haus" stays). A word kept
  because only one language knows it as typed takes the other's accents once the next word
  leans that one, as Apple's keyboard puts "nao sei" right at "sei" (TestFlight build 56,
  English and Português): "voce esta" becomes "você esta" ("voce" is an English word), only
  for a lowercase guess of that dictionary's that adds accents alone, and not when the words
  before lean the other way (`accentRevision`). "nao" alone was already "não" beside
  Português. Held-out
  English benchmark (2,000 Tatoeba sentences' synthetic taps, Spatial autocorrect on,
  iPhone 17 simulator, iOS 27.2, keys as of #2180): English alone 5.75% WER; with Français
  beside it 7.56% before, 5.79% now (99 correct words broken against 125 before and 113
  alone); with Deutsch 7.53% before, 5.79% now; with the German Spatial experiment on,
  7.18% before, 5.70% now. Typed at a key's centre after "we said that", 1 of 80 common
  French words changes ("dont" to "don't") and 3 of 72 German ones ("auch", "ist", "habe"),
  none before; none at a message's start or after French or German words, and a slip in a
  French or German word at a message's start is fixed as often as before (26 of 50, 18 of
  40). Per-letter engine p95 with a partner 4.1-5.4 ms (3.9-5.1 ms before), 2.2 ms alone.
  Black box (iOS 27.2 simulator): Settings › Keyboards offers "Add to English (US)
  Keyboard" for French (France, Canada, Switzerland), "Type in English and French on the
  same keyboard", which makes one "English & French" bilingual keyboard on English's QWERTY
  with no language switch; Portuguese, Polish and Turkish offer "Add to Existing
  Keyboard". There it fixed "I dont know" and "Dont do it" to "don't", kept "est" and
  "merci" after English words, and wrote "Im" as "I m"; the simulator's French word data is
  thin ("je pense" became "he pence"), so how it weighs French there is unverified.
- **Layout per language (Settings › Keyboard › Languages › Layout):** each language the keys
  show (every chosen one, Automatic's while none is, and only the first of English and
  Español together) gets a menu of the layouts Apple's Keyboard Layout list offers for it
  (iOS 27.0 simulator, black box), its own (today's) first: English QWERTY, AZERTY, QWERTZ,
  Dvorak; Deutsch "QWERTZ – Deutsch" (with ü ö ä), QWERTZ, QWERTY, AZERTY; Español
  "QWERTY – Español" (with ñ), QWERTY, AZERTY, QWERTZ; Français (France) AZERTY, QWERTY,
  QWERTZ, (Canada) QWERTY, AZERTY, QWERTZ, (Suisse) QWERTZ, AZERTY, QWERTY (Apple's
  "– French" variants add only the ´ key Hex leaves out); Italiano and Português QWERTY,
  AZERTY, QWERTZ; Polski and Română QWERTY, QWERTZ, AZERTY; Čeština "QWERTZ – Čeština" and
  "QWERTY – Čeština", Apple's "QWERTZ – Czech/Slovak" and "QWERTY – Czech/Slovak" (iOS 27.2
  simulator), both its own rows with the ´ and ˇ keys, Z and Y swapped on the second; Srpski
  QWERTZ (Hex's, though Apple now adds it as QWERTY), QWERTY, AZERTY. Norsk, Русский, Українська and Ελληνικά keep their
  one layout (Apple also offers a phonetic "Russian – QWERTY", not built). The plain
  layouts are the same 26 letters in every language, with the language's bottom row and
  123 pages; only the letters move (`KeyboardLetterLayout`, `KeyboardLanguage.letterRows(_:)`).
  Holds and autocorrect stay the language's: English on QWERTZ (the TestFlight request,
  build 56) holds what English QWERTY does, and a plain layout without the language's own
  letter keys holds them right after their base letter, as Apple's does (German QWERTZ:
  A Ä À…, O Ö…, U Ü…; Spanish QWERTY: N Ñ Ń). Apple's Dvorak is P Y F G C R L and delete,
  A O E U I D H T N S from the edge, shift and Q J K X B M W V Z, on keys narrowed so that
  row fits (38.3 pt pitch on a 402 pt iPhone); split on the Duo it keeps delete on row 3
  (Apple's split Dvorak unobserved). Kept per language in the App Group
  (`settings.keyboardLanguages.layouts`, `LanguageSettings.letterLayout(for:)`); unset,
  not offered, or unreadable (an iPhone that keeps the App Group from a keyboard without
  Full Access), a language types on its own layout. Spatial autocorrect, the touch decoder,
  swipe typing, next-key growth and the neighbouring-key cost read the keys as drawn, and
  Adaptive touch learns each arrangement as its own key plane; typing logs' `layout` lines
  name it (`letters`).
- **Keyboard layouts (each language's letters):** English,
  Norsk (`å` ending row 1, `ø æ` row 2), Español (`ñ` ending row 2; beside English, the
  first of the two's keys, above),
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
  is €, held for € $ £ ¥ ₩ ₽ and no zł, which Hex holds on the #+= page's €), Română (QWERTY
  on the English letters, as Apple's iPhone Romanian keyboard is: ă and â held on A, î on I,
  ș on S and ț on T right after the base, the comma-below ș and ț (U+0219, U+021B), never the
  cedilla ş ţ, and nothing on the other letters; Apple's Romanian layout with keys of its own
  for them is the iPad's; its currency key is €, held for € $ £ ¥ ₩ ₽ as Polish's is, and its
  held " offers " „ ” » « “), Ελληνικά (it matches the standard Greek layout and the system
  keyboard's observed long-press options, in the rows the iOS 27.2 simulator's own Greek
  keyboard shows: `ερτυθιοπ´`, `ασδφγηξκλ`, `ζχψωβνμ`, nine, nine and seven keys, with no ;
  or ς key; the tonos key ´ (U+00B4) accents the vowel right before it, α´ → ά, Α´ → Ά,
  ϊ´ → ΐ, and after anything else, an accented vowel included, types ´ itself, as the system
  keyboard was observed to with real touches in Messages (no dead key, no ¨ key); held, each
  vowel offers its tonos form, ι and υ also ϊ ΐ and ϋ ΰ, σ offers ς and κ the kai sign ϗ, and
  in capitals Σ offers nothing and Ι, Υ only Ί Ϊ, Ύ Ϋ, each callout laid out as the system's,
  the key's own on the left (`AccentCatalog.arrangement`, as for English and Spanish); σ types ς while it ends the word and
  turns back into σ when another letter follows, as the system keyboard does; its 123 page
  has the ano teleia · in ;'s place and ; (the Greek question mark, which also starts the next
  sentence with a capital) in ?'s, its #+= page ? in |'s, its currency key is $ held as
  English's, and its held " offers French's order), Čeština (it matches the standard Czech
  layout and the system keyboard's observed long-press options, in the rows the iOS 27.2
  simulator's own Czech keyboard, "QWERTZ – Czech/Slovak", shows: `qwertzuiop`, `asdfghjkl´`,
  `yxcvbnmˇ`, ten keys edge to edge on rows 1 and 2 and shift and delete a letter wide on row
  3; the ´ (U+00B4) and ˇ (U+02C7) keys are no dead keys: with real touches in Messages, each
  accents the letter right before it, ´ giving á ć é í ĺ ń ó ŕ ś ú ý ź and ˇ č ď ě ľ ň ř š ť ž
  and ů after u, capitals too (A´ → Á, Uˇ → Ů), and after anything else, an accented letter
  included, types itself ("aˇ", "á´"); delete then takes the accented letter whole
  (`CzechTyping`); held, E offers ě é ê è ę ë ē ė, R ř ŕ, T ť, Z ž ź ż, U ú ů û ü ù ū, I í î ï
  ì į ī, O ó ô ö ò õ œ ø ō, A á à ą â ä æ ã å ā, S š ß ś, D ď, L ł ľ ĺ, Y ý ÿ, C č ç ć, N ň ñ ń
  and the ˇ key the ring ˚ (which makes ů and å), each callout laid out as the system's
  (`AccentCatalog.arrangement`), and every other key, ´ included, nothing; its 123 and #+=
  pages and their held keys are English's) and Srpski (Latin: Apple's QWERTZ option for it; Apple's iPhone
  Serbian Latin keyboard has no keys of its own for č ć đ š ž, they are held on c, d, s
  and z), as Apple's iOS keyboards
  lay them out (`KeyboardLanguage`: letter rows, long-press accents, spelling dictionary).
  Automatic follows the dictation language (Hebrew, Cyrillic Serbian and other
  languages without a layout get English; Serbian only in Latin script, `sr-Latn`);
  before any dictation language is known, the first iPhone
  keyboard, then preferred language, that has a layout. It doesn't read back the input mode
  Hex reports (`primaryLanguage`), which is derived from this choice. Rows of 11 keys narrow
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
  `pt_PT`, `pl`, `cs`, `ro`, `ru`, `uk`, `el`; Serbian would use `sr_Latn`, which iOS 27.2 doesn't have, nor a
  Croatian one); where the iPhone has none
  they stay off except your own text replacements, and the English contractions
  and typo list (`i` → `I`) apply only to English. Neighbouring-key costs follow
  the layout (on QWERTZ, Z sits beside T), and an accent or tilde you left out
  is the cheapest fix of all ("aqui" → "aquí", "manana" → "mañana"; a word the
  dictionary knows, such as "esta", is never changed). Italian also restores common
  accented spellings independently of the checker's guesses and a bilingual partner's
  dictionary, where the plain spelling is no word of its own ("perche" → "perché", "citta" →
  "città", the days, the -ità words; `KeyboardLanguage.accentRestorations`); "e"/"è" and
  "ce"/"c'è" stay as typed, because "e" is "and" and "ce" is "to us": the strip offers "è"
  and "c'è", and space never applies them (`KeyboardLanguage.accentSuggestions`). In Polish
  every missing diacritic in a word counts as one such fix
  ("zolw" → "żółw", "maly" → "mały"). The
  iOS 27.2 simulator lists `pl_PL` but its checker flags no word, so Polish fixes are
  checked against a stub dictionary there. Greek likewise: every tonos or diaeresis left out
  counts as one such fix, and so does σ where the word ends in ς ("καλημερα" → "καλημέρα",
  "λογοσ" → "λόγος"); Greek fixes are checked against a stub dictionary too. Czech likewise:
  every háček or čárka left out counts as one such fix ("cesky" → "český", "prilis" →
  "příliš"), checked against a stub dictionary. A two-letter non-word one
  neighbouring key (or one swap) from a very common two-letter word is fixed too
  ("tk" → "to", Spanish "nl" → "no"; `KeyboardLanguage.commonTwoLetterWords`, English,
  Spanish and Italian only). Auto-capitalization is unchanged.
  The 123 and #+= pages are English's in every language but Greek (above); their held keys follow each
  language's system keyboard where it differs (# holds № in Russian and Ukrainian, the
  digits º and ª in Spanish and Italian, . holds · in French, $ holds ₴ in Ukrainian and
  kr in Norwegian).
- **Size (full keyboard):** On an iPhone held upright the suggestion strip is Apple's
  predictive bar from the top of its suggestion pill down: 38 pt in every state
  (suggestions, Undo dictation, the Full Access hint, the recording bar, the emoji
  panel's top row, the Logging mark), with Apple's 8 pt under it, so row 1's keycaps
  start 46 pt down and the keys stay on Apple's rows (Apple's keys overlaid on Hex's,
  iPhone 17 Pro, iOS 27.2 simulator): 247 pt on an iPhone 17 or 17 Pro, 254 pt on an SE,
  266 pt on an Air or Pro Max. The words are on Apple's line, 26 pt above the keys and
  20 pt below the keyboard's top edge; an autocorrection is a small 18 pt chip around
  its word, and the pills (the Hex key, the recording bar) are 28 pt, 6 pt down, so
  nothing touches the top edge. iOS draws about 17 pt of its own panel above a
  third-party keyboard in some fields and nothing in others (on an iPhone 17 Pro, iOS
  27.2: Notes has it, a Safari form's first focus doesn't; Gboard and SwiftKey get the
  same), and a keyboard can't tell which, so the strip carries its own room: the 27 pt
  strip it replaces left the words 9 pt under the edge whenever iOS added nothing.
  Turned, the strip is 42 pt and the keyboard 204 pt.
  Where iOS gives the keyboard a shorter view than it asks for (Spotlight on an iPhone
  17 Pro Max, iOS 27.3, TestFlight: about 20 pt short), the rows fit the height it has:
  the strip keeps its size and the keycaps and the gaps between rows shrink alike
  (`KeyboardMetrics.fitted`), instead of row 4 falling under iOS's globe band.
  Dictating no longer grows the keyboard. On the iPhone Duo (`KeyboardPosture`,
  model `iPhone19,4`) iOS sets third-party keyboards in an inset platter with a band
  of its own above them, so the strip has no gap there, and the keys follow Apple's
  for each screen: 56 pt rows folded, 59.7 pt unfolded and turned, 51.7 pt unfolded
  upright, starting as far in from the screen's side as Apple's. Measured in the
  simulator; unverified on a Duo.
- **Background (full keyboard and dictation bar):** Under its keys Hex draws a thin
  blurred material (`KeyboardBackdropView`: the system's ultra-thin material, shaded 22%
  in dark mode, with the panel's 26 pt top corners) instead of a fixed colour, so the
  keyboard takes most of the colour of iOS's glass panel behind it. On an iPhone 17 Pro
  (iOS 27.2, dark mode) the glass reads 31 of 255 over Notes and 54 over a light web
  page; Hex's area reads 36 and 46, where the opaque 34,34,35 panel it replaces was a
  slab 20 darker than the glass over light pages, with a hard top edge and square corners,
  which read as a clipped top bar. It isn't clear, as SwiftKey's is, because iOS can leave
  Apple's keyboard under a third-party one: in a field that takes focus as its screen
  appears and has an input accessory view, switching from Hex to Apple's keyboard and back
  keeps Apple's key plane in the app's view tree under Hex (every switch in the iOS 27.0
  and 27.2 simulators, under a bare clear keyboard extension too; one TestFlight report,
  build 56, iPhone 17, iOS 27.0.1; not in Notes or Safari on an iPhone 17 Pro on 27.2).
  Through the material's blur those keys are an even, slightly lighter tone with no shapes.
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
  still for 0.35 s starts the trackpad, as Apple's does, and letting go then types no
  space (`FullKeyboardEngine.trackpadHoldDelay`). A tap that slides before then is still a
  space, however far it goes, as on Apple's (black box, iOS 27.2: taps of 60 to 150 ms
  sliding 8 to 24 pt, and slides of 40 pt in 150 ms, all typed one); a slide of 10 pt used
  to start the trackpad at once and type nothing (TestFlight build 56: "missing the space
  key"). With two or more languages such a slide is a language swipe (Keyboard languages,
  above). In a rollover space keeps its place: a key going down while the space bar is
  still held types the space first, as a held letter's does. The
  full keyboard's trackpad moves as Apple's does (black box, iOS 27.2): the finger drags a
  point through the text at the text's own scale, in two dimensions, and the cursor goes to
  the character nearest it. Slowly a point of finger is a point of text, and faster it goes
  further (1.4 times at 400 pt/s, twice at 1,000, 2.6 times at 2,000, `TrackpadCursor.gain`);
  a line takes the font's line height (20 pt), the first half of one; the x carries across
  short lines; it stops at a line's start and end and at the text's first and last line. A
  keyboard can only move the cursor by UTF-16 offsets, so `TrackpadCursor` lays out the text
  the field shares (the body font's own widths, rows wrapped at the keyboard's width less
  40 pt) and moves by the offset to the nearest position, a 👍🏽 in one step. The field shares
  only the cursor's sentence or line and a little before it, so at that text's edge the cursor
  waits for the host to show more, and past its first or last line it steps just past it in
  one move, which the host makes only if there's text there. The keys blank to keycaps 60%
  of the way to the backdrop, as Apple's, with no haptic while it moves. Apple's
  second-finger tap to select text has no counterpart: `UITextDocumentProxy` (iOS 27.2 SDK)
  can read the selection but not make one, so a second finger does nothing. The dictation bar's
  space bar is the same trackpad along the line (the same still hold or a 10 pt slide, a
  character per 7.5 pt, the keys' glyphs blanking, a selection tick per step; TestFlight
  build 56 asked for it), and a tap still types a space.
- **Smart punctuation (full keyboard):** As on Apple's keyboard (black box, iOS 27.2), the
  ' and " keys type opening quotes at the start, after a space, a bracket or an opening
  quote, and closing ones elsewhere (‘Hi’ it’s “yes” 5’9”); a second hyphen makes an em dash
  (a--b is a—b); autocorrect's and the strip's apostrophes are ’ (don’t, I’m). Each language
  types its own quotes (`SmartQuotes`): „ “ ‚ ‘ in German, « » with no-break spaces in French,
  „ ” ‚ ’ in Polish, « » in Norwegian, Russian and Ukrainian, “ ” ‘ ’ elsewhere. A field's
  `smartQuotesType` or `smartDashesType` of `.no` turns each off (Messages' composer has
  both off: Apple's keyboard types straight quotes and -- there). The keys reason about the
  text with straight quotes (`SmartPunctuation.plain`). A mark that ends a clause (, . ! ? ;
  : )) right after a picked suggestion takes the space the suggestion typed ("Hello,"), as
  Apple's does with `smartInsertDeleteType` on or off.
- **Delete after an autocorrection (full keyboard):** As on Apple's keyboard (black box,
  iOS 27.2), delete takes only the space or mark after the corrected words, and the strip's
  left slot offers what you typed, highlighted with an undo arrow (Apple's text view shows
  it in a bubble under the word, which a keyboard can't draw); tapping it puts that back,
  without the space, and turns the fix down for the session. Space keeps the fix. The offer
  comes back whenever the cursor is at the end of the latest autocorrection again, revisions
  ("well see" to "we'll see") and joins included. Text replacements offer none, as on Apple's.
- **Key clicks:** Every key of the full keyboard (delete, shift, 123 and return included, and
  each delete repeat), its strip, and the dictation bar's keys ask for the system click on
  touch-down (`UIDevice.playInputClick`), which plays only because the keyboard's root view
  (`KeyboardInputView`) adopts `UIInputViewAudioFeedback`: the full keyboard view adopted it
  before, as a subview, and never clicked. It follows Settings › Sounds & Haptics › Keyboard
  Feedback › Sound, with no setting of Hex's. Apple's keyboard has other sounds for delete and
  the modifier keys, but the click is the only keyboard sound a keyboard may play, so every key
  gets it. In the iOS 27.2 simulator each press reaches the system click (system sound 1104,
  in the keyboard's log) with Full Access or without, and none with the Sound switch off; on
  an iPhone, Apple's guide says it needs Allow Full Access.
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
  grid goes to the key under it, each category the full width of its slot. With
  Settings › Experiments › ABC under the emoji key on (off by default,
  `experiments.emojiLettersUnderEmojiKey`, App Group), ABC sits exactly where the emoji
  key that opened the panel was, its x and width, so switching back to the letters needs
  no thumb move; a tap left of it still goes to ABC, and the categories take the room
  between it and delete. Off, or in a panel with no emoji key behind it (the dictation
  bar's), ABC keeps its leading-end place. As on the
  system emoji keyboard (iOS 27.2 simulator), a tap on a category jumps the grid there as
  the finger lifts, and a finger sliding along the bar scrubs the grid live, each icon's
  slot running through its own category from start to end, with the chip on the category
  under the finger and a light tick for each new one (`EmojiCategoryBar`); scrolling the
  grid moves the chip. The row reads the last eight words (the newest counts most), their plurals
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
  - A keyboard's dictation never leaves the keyboard stuck. Hex reads the
    keyboard's Done and Cancel and heartbeats the job's snapshot until its
    text is in, in a warm session and in One shot alike (One shot used to
    ignore them, so a handed-off recording ran to its 60 s limit), and asks
    iOS for background time to finish the text. The keyboard takes a
    recording or transcription whose heartbeat is more than 6 s old as over
    (`KeyboardSnapshot.isOrphanedJob`): a Hex that ended, crashed or was
    suspended mid-job used to hold every keyboard in recording for good. A
    result that missed its 30 s to go in by itself stays offered as **Paste
    dictation** for 10 minutes, in the full keyboard's strip and in the dictation
    bar's top row (the only keyboard on an iPad, which offered nothing). A failure or note
    from Hex ("Another app is using the microphone. Try again.") shows for 8 s
    after Hex wrote it (`KeyboardSnapshot.messageAt`), not for as long as it
    stays in the App Group; meanwhile both keyboards dictate in the keyboard, and
    after it a warm session's next start is Hex's Try again.
  - An in-keyboard dictation the keyboard is dismissed from (or another app
    comes forward) stops its capture at once, keeps what was said, and
    finishes without the keyboard; its text is held in the App Group
    (`keyboard.heldDictation`), goes into the same field if the keyboard
    comes back there within 30 s, and is offered as Paste dictation
    anywhere else or later. A capture that goes quiet for 6 s finishes with
    what it heard, and a transcription stuck for 2 minutes is given up.
  - Phone/FaceTime/Siri audio interruptions finalize captures `>= 300 ms` so
    spoken words are not lost, cancel shorter taps cleanly out of `Listening`,
    and resume warm keepalive when iOS signals `.shouldResume`. Hex's own
    recording (the app, a warm session's keepalive and captures) is
    `playAndRecord` with `mixWithOthers` and A2DP output, never `duckOthers`,
    so it doesn't interrupt another app's video or music, and ending a session
    deactivates with `notifyOthersOnDeactivation`. It sets no preferred sample
    rate: `playAndRecord` runs the microphone and the output at one rate, and
    the 16 kHz it used to ask for moved everything the iPhone played and
    recorded (a screen recording's audio too) to 16 kHz while Hex held the
    session (48 kHz to 16 kHz in the simulator). The recorder writes 16 kHz
    itself, and the keepalive is input only (an `AVAudioEngine` input tap, no
    output node: its output element is off). A keepalive that keeps stopping by
    itself is restarted at once, then after 1 s, 2 s, … up to 30 s, not every
    100 ms. **Microphone** (`MicrophoneChoice`, `MicrophoneRoute`) picks each
    dictation's input: **Automatic** (the default) a wired or USB headset's
    microphone, the car's in CarPlay (the driver's microphone Siri uses, where
    the iPhone's hears the speakers and the road), else the iPhone's, so AirPods
    and other Bluetooth headphones keep playing at full quality and nothing
    waits for call audio; **iPhone** always the built-in one; **Bluetooth** also
    Bluetooth hands-free microphones (AirPods, Bluetooth car kits, which iOS
    can't tell from headphones), in call audio (8–16 kHz) and still mixing. Only
    Bluetooth adds `allowBluetoothHFP`, and the input is always set
    explicitly. The warm keepalive stays on the iPhone's microphone whatever the
    choice, so a warm session never holds a car or headset in call audio between
    dictations; a dictation moves to its input as it starts (a Bluetooth
    headset's call link can take 0.5–2 s, unmeasured here). The setting replaced
    Always use iPhone microphone: left on it reads as Automatic, turned off as
    Bluetooth. In-keyboard dictation asks WebKit for the same input by name
    (`deviceId`), read from the keyboard's own session; left to itself WebKit
    takes the input of the route already playing, the iPhone's while a car or
    headphones play over A2DP. Its capture session (`playAndRecord`,
    `videoChat`, no `mixWithOthers`) no app can change: other apps' video and
    music pause while it records and WebKit ends it without telling them, so
    once it has ended the keyboard activates and deactivates its own session with
    `notifyOthersOnDeactivation` (`InterruptedAudio`), for players that resume
    on `.shouldResume`. Unverified on an iPhone: the simulator has no audio
    interruptions.
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

- **Google Books Ngram data:** Spatial autocorrect's word and word-pair frequencies, and
  German's capitalisation shares, come from the Google Books Ngram Viewer data, version 3
  (2020), by Google, licensed under
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
- **Parakeet V2:** Hex dictates English with NVIDIA's
  [Parakeet TDT 0.6B v2](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2), licensed under
  [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/),
  downloaded as FluidInference's
  [Core ML conversion](https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v2-coreml)
  (CC BY 4.0, encoder weights at reduced precision), used unchanged.
- **Parakeet Ultra:** Experiments › Multilingual model downloads
  [Parakeet Ultra](https://huggingface.co/moondream/parakeet-ultra) by Moondream, a
  post-training of NVIDIA's
  [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3), both licensed
  under [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/),
  as FluidInference's [Core ML conversion](https://huggingface.co/FluidInference/parakeet-ultra-coreml)
  (revision `95eaa59`: 8-bit integer encoder weights, decoder and joint re-exported), used
  unchanged.
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

### Replaying real typing

`TypingReplayTests` runs recordings of real fingers through the keyboard as it runs: the
key plane, Swipe typing or taps with Spatial autocorrect, then the engine's spacing. It is
opt-in, one test per mode, and the datasets below are not vendored, so convert a recording
to one of the two JSON shapes first. `xcodebuild` passes environment variables to the test
runner only when they are prefixed with `TEST_RUNNER_`, and strips the prefix, so
`TEST_RUNNER_HEX_SWIPE_REPLAY` becomes `HEX_SWIPE_REPLAY`:

```sh
TEST_RUNNER_HEX_SWIPE_REPLAY=/path/swipes.json xcodebuild test \
  -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=<SIM_UDID>" \
  -only-testing:HEXMobileTests/TypingReplayTests
```

Swipe recordings are `{"sentences":[{"source":String,"session":String,"words":[String],
"gestures":[{"x":[Double],"y":[Double],"t":[Double],"start":Double?}]}]}`: one gesture per
word, x 0…1 across the ten columns of the top letter row, from q's left edge to p's right
edge (their centres at 0.05 and 0.95), y 0…1 from the top of the first letter row to the
bottom of the third (row centres at 1/6, 1/2 and 5/6), t milliseconds since the finger
landed, and the optional `start` milliseconds from the sentence's first landing to that
one, so gestures keep the person's gaps; without it each begins 0.5 s after the previous
lift, longer than the two windows the replay's clock drives that a lift leaves open (the
double-space period and the strip's touch guard). A start that would put a finger down before
the previous one came up is moved to that lift, in both modes, and `clamped` counts those. A
gesture recorded as one point is a tap. Hex's second and third letter rows are offset half a
column and one and a half columns, as the format assumes. Tap recordings replace `gestures`
with `taps`, one `{x, y, t}` per touch, each array holding that touch's samples (a tap is
usually one) on the sentence's clock; y > 1 is the space bar, x 0.25…0.75 across it.
`TEST_RUNNER_HEX_TAP_REPLAY=/path/taps.json` does the same for taps, with Spatial
autocorrect reading each word on its space.

Both report a line per source inside `TYPING-REPLAY … TYPING-REPLAY-END`: total words,
words outside the dictionary and their share (a word is in the dictionary when the decoder's
model has it or English's contractions in `AutocorrectEngine.englishContractions` do; the
model lookup gets only the word's ASCII a–z letters and apostrophes, so accented, hyphenated
and numeric tokens normally count as outside), and accuracy among the in-dictionary words.
Scoring lowercases and straightens curly `’`: a tap compares letters, digits and apostrophes,
a swipe only letters. Taps report that as `accuracy` over every in-dictionary word and also
`all-words-accuracy` over every word, so the gap between the two is what the out-of-dictionary
words contribute. Swipes report `accuracy` and `typed-or-offered` over the swipe attempts
(`attempts`): gestures that became swipes, ones whose path left the key by a
swipe's pitch and weren't promoted, and ones that read nothing. They also report
`in-dictionary-accuracy` over every in-dictionary word — taps included, a skipped gesture
counted wrong — and `all-words-accuracy` over every word, which is the end-to-end number: what
the person reading the field saw, with an out-of-dictionary word now able to count as right
when the field has it. `taps` and `taps-right` are the recordings whose path never leaves its
key by a swipe's pitch: the keyboard types that key's letter, and the field keeps it, as for a
person typing a one-letter word. Tap mode keeps Swipe typing attached as the keyboard runs it,
so a recorded tap whose samples drift can be promoted to a swipe; `read-as-swipes` counts the
recorded taps the plane took that way. `not-swipes` counts swipe attempts the plane never took
as swipes or that read nothing; `skipped` counts gestures that began off the letter keys, and
`clamped` the touches moved forward to the previous lift; unlike `taps` and `taps-right`,
which count in-dictionary words, those three count every recorded touch, and
`read-as-swipes` also counts every delivered tap, whatever its word. A tap's words are
compared with their apostrophes; a swipe's by letters only, because a swipe's path runs over
letter keys and cannot draw one. The decoder's own median and p90 milliseconds close the
line, though a swipe's preview decode can overlap its timed final decode, so a `decode-ms`
sample may include that overlap; the words are unaffected. `HEX_REPLAY_LIMIT` caps sentences per source;
`HEX_REPLAY_OUT` writes each sentence's intended and typed words (prefix both with
`TEST_RUNNER_` too).

The test with no recording checks Hex's key geometry against the grid and runs clean
gestures through the same mapping to type the right words; it cannot catch a converter that
emits wrong coordinates.

Numbers can move between simulator iOS versions because autocorrect also consults the system
text checker for word validity and names, and its dictionary changes with iOS.

Public datasets of real typing it is meant for:

- [FUTO swipe dataset](https://huggingface.co/datasets/futo-org/swipe.futo.org) (MIT).
- [How We Swipe](https://osf.io/sj67f) (Leiva et al., MobileHCI 2021; MIT).
- [Noisy Typing on QWERTY Keyboards](https://zenodo.org/records/24225302) (Vertanen and
  Kristensson, IUI 2023; CC-BY-4.0).
- [Tap Typing with Touch Sensing Images](https://github.com/google-research-datasets/tap-typing-with-touch-sensing-images)
  (CC-BY-4.0).

See [`benchmark/typing-replay/README.md`](benchmark/typing-replay/README.md) for each
dataset's download and its `convert.py` command.

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
- **Screen recordings with the microphone:** The simulator can't screen-record,
  so whether one picks up noise while Hex records shows only on an iPhone:
  record with the microphone on with Hex closed, Hex open and idle, a warm
  session on, and during in-keyboard dictation (WebKit's `videoChat` session,
  which Hex can't change), and compare.
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
  `FullKeyboardEngineTests` the swipe against the
  hold-then-slide trackpad and switching in a `FullKeyboardView` (English & Spanish too,
  and a picked Spanish variant surviving the swipe away and back), and
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

## Internal TestFlight releases

[Hex Mobile in App Store Connect](https://appstoreconnect.apple.com/apps/6836837368/testflight/ios)
uses Anomaly's signing team. The GitHub **Release Hex Mobile** workflow checks
`dev` at noon and 6 p.m. Chicago time, or on manual dispatch, and cuts an
`ios-beta-*` prerelease when iOS inputs have changed. These tags are separate
from desktop releases. The marketing version stays fixed across beta builds.
`MARKETING_VERSION` stays 9.0.0 for TestFlight, so every `dev` build reaches testers
without a new beta review; it's never submitted to the App Store. App Store releases are
1.0.x and use their own version. Apple rejects any upload below the highest approved App
Store version, so TestFlight's version must stay above every App Store release. To build one, run `scripts/marketing-release.sh <version> [commit]`, which pushes
the tag `ios-beta-appstore-<version>` (e.g. `ios-beta-appstore-1.0.0`) on that `dev` commit:
it matches the Xcode Cloud start condition, and `ci_scripts/ci_post_clone.sh` stamps that
version into `project.yml` before generating the project. Once Apple approves a
version, later uploads must be higher than it. Hex Mobile is iPhone only (`TARGETED_DEVICE_FAMILY` 1).
An iPad still installs it (as an iPhone app) and runs its keyboard, which there is always the
dictation bar (the full keyboard is iPhone only). Docked, the bar keeps its top 44 pt clear:
iPadOS adds its shortcut bar (undo, redo, paste) to a custom keyboard, usually above it but
in some apps over its top (Notes, iPadOS 26.6), where it covered the bar's top row. A
floating keyboard (iPhone-wide) has no shortcut bar and no strip. The bar's top row is
40 pt tall around its 26 pt pills, and every control in it takes taps across the row's
full height, at least 44 pt wide, with the Hex mark and each tray's close reaching the
screen's edges.

The Xcode Cloud workflow needs a tag start condition with prefix `ios-beta-`,
container `ios/HEXMobile.xcodeproj`, shared scheme `HEXMobile`, an iOS Archive
action, and a TestFlight post-action for **Hex Internal**. Cloud owns the build
number. Verify the run's source tag and the processed build in that group.