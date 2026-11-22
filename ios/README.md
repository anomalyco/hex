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
(`HEXKeyboard/KeyboardLocalDictation.swift`), or, with Experiments › Speech model in
keyboard on, with Parakeet on the Neural Engine in the keyboard's own process
(`HEXKeyboard/KeyboardParakeet.swift`, below). With Full Access, the web view
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
    model: English, Spanish and French, and German with its experiment on). Words
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
    mic warm** starts an `AVAudioEngine` input session immediately so
    subsequent taps on **Dictate** record directly inside the keyboard with zero
    app switches; picking `Off` or tapping the header timer ends it. Unless
    chosen, it is `Off`, with the row hidden, where the keyboard dictates by
    itself (`KeyboardSelfDictation`: iOS 26, and the keyboard hasn't opened Hex
    for a dictation in the last 7 days, for any reason: the keyboard records its
    own (memory, a missing speech model, the Multilingual model) and Hex records
    every keyboard launch, since TestFlight build 129 showed keyboards that opened
    Hex for reasons they left no record of; a dictation the keyboard finishes by
    itself clears it), and `15m` elsewhere; worked out on each read, never
    stored. The row shows whenever a duration is in effect, and stays on the
    page once picked. Once on iOS 26, a stored `15m` (which tapping the old
    default to warm the mic also stored) was taken for not chosen where the
    keyboard dictates by itself. A Dictate that opens Hex there brings the `15m`
    default back from that launch, so it warms as before 129.
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
  - **Numbers as digits (Settings › Dictation, on by default; English, every style but
    Verbatim):** numbers said in words are written in digits, as Apple's
    `SpeechTranscriber` writes them, so every engine reads the same
    (`SpokenNumbers`, run by `CleanupTagger.cleanIfEnabled` after Spoken counts and before
    the tagger, so the tagger never removes a number in digits outside a self-correction).
    Parakeet V2 now and then writes a whole sentence in words ("The ratio is five point
    five.", "november twenty second"), mostly in a real voice: 53 of 80 test sentences in
    Ryan's cloned voice against 11 of 80 in macOS voices (macOS 27.2.1,
    `experiments/2026-11-22-hex-numbers`). It writes decimals ("5.5", "3.1.4", "1.2
    million"), percentages ("20%"), numbers from ten up and compounds ("25", "312",
    "15,000"; commas from 10,000, as Apple writes "8000"), years ("1999", "2026"), money
    with "dollars" ("$5", "$15.40"), two to nine before a plural noun or unit ("2 dogs", "6
    foot"), ranges ending in one ("5 to 10 minutes"), times with am, pm or o'clock ("6 am",
    "7:15 pm"), ordinals in dates ("November 22nd", "the 4th of July"; "May" and "March"
    only with a capital), three or more spoken digits ("4729", "555-123-4567"), and minus or
    negative before a number ("-5", "-$40"). Text already in digits keeps its form ("2,400",
    "$4.99"). Not yet: "one" alone ("one of them", "no one", "one more"), times and prices
    without a marker ("three thirty", "four ninety nine", "twelve fifty"), ranges and scores
    without a unit ("eight to twelve", "three to one"), fractions, ordinals outside dates
    ("finished first", "twenty first floor", "a second"), small numbers before anything else
    ("at eight", "GPT five"), and any run of number words that reads two ways ("one thirty").
    Other languages keep their words: each needs its own number words, compounds (German
    "fünfundzwanzig", French "quatre-vingt-dix") and separators ("5,5" and "15.000" in
    German, French, Spanish, Italian and Dutch). `SpokenCleanupRulesTests` holds the 80
    sentences as both engines wrote them in both voices.
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
  - **While the keyboard records: Show what I say and Type as I speak (Settings ›
    Dictation, iOS 26+):** While the keyboard records by itself (in-keyboard dictation),
    the capture's chunks also go to Apple's `SpeechAnalyzer` as they arrive
    (`AppleLiveTranscriber`: `SpeechTranscriber` with volatile and fast results, or
    `DictationTranscriber`'s progressive long dictation for its languages), only when the
    language's model is already on the iPhone: it never downloads one. **Show what I say**
    (on by default): the full keyboard's keys go and the words so far take their place
    under the recording bar (`LiveDictationPreviewView`), the settled words in the keys'
    ink and the tail the recogniser may still change muted, the newest line in view with
    whole lines above it. It takes every touch where the keys were, so nothing types while
    it shows; the recording bar's ✕ and ✓ work as before, and an iPhone with a Home button
    keeps its globe key where it was. VoiceOver reads it as one element, never announced as
    words change. When the dictation goes in, the preview fades with the words it had while
    the keys come back; the field gets the dictation's own text, with its style, clean-up,
    punctuation, held results and Undo dictation as before, so it can differ from the
    preview: with the speech model in keyboard (Parakeet, English's default) the preview's
    words are Apple's and the text Parakeet's ("10" can go in as "ten"). **Type as I speak**
    (off by default): the words are typed into the field as they come
    (`LiveFieldTyping`), only the changed tail retyped. It keeps exactly what it typed and
    edits only while the text before the cursor still ends with it; when the dictation's
    text goes in, its words come out first, so the text goes in where and as it would without
    them, in one step. ✕, a failure or nothing heard take them out. The keyboard hiding
    mid-dictation takes them out where the app still takes edits; one gone to the background
    doesn't, so they are noted (`LiveDictationSettings.leftover`) and the held text, inserted by
    itself or with Paste dictation, replaces them where the field still ends with them.
    Once the field changes under it (a key, a cursor move, a selection, or an app that
    reports no text), it stops and leaves its words, and the text goes in at the cursor as
    without it. Never in secure, password, one-time code or card number fields, over a
    selection or a two-thumb Dictate's range, or with Translate what I say on. Neither runs
    before iOS 26, for dictations recorded through Hex (Keep mic warm), when less than 32 MB
    is left once the capture runs (`KeyboardLiveDictation`: the recogniser's share in the
    keyboard with margin, and the speech model's transcription after it), on an error, or
    when the recogniser isn't running 4–6 s into the audio; under 24 MB, or on a memory warning, the recogniser goes
    and the dictation goes on. The dictation bar shows no preview (Type as I speak works
    there, unverified on an iPhone). On an iPhone 17 Pro (iOS 27.2, keyboard limit 177 MB,
    Parakeet loaded, a clip played from a Mac's speaker): the first words show about 1.2–1.3 s
    after speech starts (2 s after the capture's first audio; 2.1 s and 3 s on the first
    dictation after the keyboard starts), then about a second behind; over a 60 s dictation
    the keyboard ended at 60.1 MB with the preview against 58.1 MB without (peak while
    recording 66.5 against 60.1, at Parakeet's transcription 78.7 against 76.6), and two-sentence
    dictations 0.4–0.8 MB higher. A scrolling text view holding the whole text had peaked
    27 MB higher there, so the preview draws only about its box's worth. Measured on the Mac
    Studio (macOS 27.2.1) with the keyboard's own code fed a clip in the page's
    4,096-sample chunks in real time: first words 0.5–1.0 s after the first word ends, then
    0.6–0.8 s behind (90% within 1.2 s), in bursts about once a second; 2% of words shown
    were rewritten (7 of 336, "730." to "7.30."); words settle only at a pause. Without fast
    results words came 2.7–3.5 s late, and `DictationTranscriber` rewrote 15% of them. The simulator has no speech models: in
    simulator builds only, `LiveTranscriptReplay` plays a recorded run (`live-replay.json` in
    the App Group, or the keyboard's own Library for an unsigned build) from the first audio.
    `ParakeetLiveTranscriber` (the audio so far again about once a second, a 20 s window) is
    written for when the speech model's own words should show, and not used yet.
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
    keeps its suggestions. On a number pad, or with only Apple's keyboards
    beside Hex, it switches to the next keyboard; elsewhere (Emoji or another
    third-party keyboard on, where that switch lands on them about half the
    time) a tap opens iOS's keyboard list, as the globe's hold does, to pick
    Apple's. It needs no Full Access, which only Paste does.
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
    follows on the keyboard, else the iPhone's first. The experiment never takes
    dictation away: until Ultra is ready, Hex and the keyboard dictate as they would
    with it off, and Hex switches to Ultra once it has loaded (a dictation in flight
    isn't interrupted). Turning it on downloads
    FluidInference's Core ML build (632 MB, pinned to revision `95eaa59`, each file
    checked against its SHA-256 and resumed from its `.partial`) into Application
    Support, out of backups, beside that engine (`ParakeetUltraDownload`): home and the
    switch say "Downloading the multilingual model: 123.4 MB of 632.2 MB" with a bar. A
    dropped connection, or bytes that stop for 20 s, tries again by itself after 2, 4, 8,
    16, then 30 s, starting over whenever it got further; in the background it waits for
    Hex to come back to the front, which picks it up at once (and starts a fresh
    connection after 25 s away). After six tries in a row that got nothing further, or a
    refused request or a full iPhone, it says what happened in plain words with Try again
    and Turn off. Once downloaded, Ultra's first load (Core ML's compile, a minute or
    more) runs beside the usual engine too, except where Parakeet V2 dictates and leaves
    no room for it (English before iOS 26), where Ultra takes V2's place for that load;
    later launches load it directly when Core ML has compiled it for this iOS. Turning it off stops the download, then deletes it;
    a launch deletes a leftover only when the switch reads off as stored, never when the
    App Group can't be read (a launch before the first unlock). FluidAudio 0.15.5 has no
    `.ultra`, so `ParakeetUltraModel` loads the four models itself and hands
    FluidAudio v3 `AsrModels` (its own loader would replace a folder it can't load
    with v3's download). It is chosen only with 1.5 GB left to load it
    (`os_proc_available_memory`), and never again after a load that ended Hex
    (`ParakeetLoadGuard`, its own keys) until Try again beside the status, or the switch
    turned off and on. Unless Experiments › Speech model in keyboard has it loaded in the
    keyboard (below), the keyboard doesn't use it, so once Hex has it ready
    (`MultilingualParakeetSettings.isModelReady`, set after a load, cleared while it's
    missing, set aside or off) and it takes the dictation language, Dictate and the Hex
    key open Hex, as before iOS 26 (a warm session still dictates without leaving the
    app); until then the keyboard dictates itself. Measured on a Mac (`~/dev/experiments/2026-11-21-hex-multilingual-parakeet`):
    mixed Dutch-English test sentences 7.5% WER against Apple Speech nl-NL's 26.9%,
    pure Dutch 0.9% (0.4%), pure English 0% (0%). Load memory, compile time and speed
    on an iPhone are unmeasured.
  - **Speech model in keyboard (Settings › Experiments › Speech model in keyboard, iOS
    26+):** On unless the person turns it off, and disabled (shown off) on iPhones under
    6 GB (`KeyboardParakeetSettings`, in the App Group; a value stored by hand wins, so a
    switch turned off stays off). Off, nothing of it loads and the keyboard dictates exactly
    as before. On, Hex downloads Parakeet V2 (464 MB, about 700 MB with the keyboard's
    compiled copy) into the App Group (`KeyboardParakeetDownload`; turning it off stops and
    deletes it unless Hex itself dictates with V2). By itself it downloads only where the
    keyboard would use it (`KeyboardParakeetDownload.hold`): English dictation without
    Two-language dictation or the Multilingual model, the keyboard's own memory limit
    measured on this iPhone (the keyboard writes footprint plus `os_proc_available_memory`
    to the App Group at each launch and tells Hex with a Darwin notification) and at least
    `KeyboardParakeetBudget.requiredLimit`, a connection that isn't expensive or constrained
    (`NWPathMonitor`: not cellular, a personal hotspot or Low Data Mode), and 2 GB free.
    It checks at launch, whenever Hex comes to the front, when such a connection returns and
    when the keyboard reports its limit. Turned on by hand (a stored true, including from
    the builds where it was off by default) it downloads at once on any connection. Until
    then the row says what it waits for: "Downloads after you first use the Hex keyboard",
    "Downloads on Wi-Fi", "Downloads once the iPhone has 2 GB free", "Only for English
    dictation". The keyboard's own dictation transcribes with Parakeet in
    the keyboard's process (`HEXKeyboard/KeyboardParakeet.swift`), with the app closed:
    V2 for English, and Parakeet Ultra for a language Experiments › Multilingual model
    dictates once its files are there, in which case Dictate and the Hex key stop opening
    Hex for it. Parakeet's files moved from the app's Application Support into the App
    Group (`SpeechModelStore`), renamed in place at launch (`moveLegacyDownloads`) rather
    than downloaded again; each folder is marked complete (`.hex-complete`) once every file
    is there, and the keyboard loads only a marked one. The keyboard loads it after its
    first frame and launch work, off the main thread (the preprocessor on the CPU, the rest
    on the CPU and Neural Engine, one chunk at a time, never the GPU, which iOS gives a
    keyboard only in the background role), and keeps it while its process lives, until a
    memory warning. Its first load compiles it for the Neural Engine in the keyboard's own
    Caches (17 s for V2, 12 s for Ultra on an iPhone 17 Pro); a dictation never waits for
    that, and uses Apple's speech recognition exactly as with the experiment off whenever
    the model isn't loaded, isn't on the Neural Engine (the keyboard's Neural Engine memory,
    else Core ML's compute plan before iOS 27), another transcription runs, the keyboard's
    memory limit (footprint plus `os_proc_available_memory`) is under 131 MB (V2 and Ultra
    alike: with the model loaded the keyboard records at up to 91 MB, and Ultra's
    transcription rose 9 MB in Hex's keyboard, V2's 16 MB), too little is
    left, the recording is longer than 90 s or there isn't room for it (FluidAudio holds the
    whole recording in memory, about 0.42 MB a second measured, counted at 0.5 MB on top of
    the 24 MB transcription headroom; Apple's speech recognition streams the file, so a long
    dictation goes there and arrives a little later), it hears nothing, or it fails
    (`KeyboardParakeetBudget` has the measured numbers). Core ML keys what it compiled to the files themselves: a model downloaded
    again, or moved, compiles again, so the log calls a load "cold" by its time (5 s or
    more), and moving a legacy folder forgets the app's compile record for it
    (`ParakeetCompileRecord.forget`), so V2 says it's optimizing for this iPhone and Ultra
    gets ready beside the engine that dictates. The Experiments row says what it's doing: downloading,
    downloaded, getting ready in the keyboard, ready, or that the keyboard can't use it now
    (`KeyboardParakeetSettings.keyboardState`, written by the keyboard). Each dictation logs
    its engine, the keyboard's limit, footprint before and at peak, load and transcription
    time to `keyboard-capture-probe.log`. Measured in Hex's keyboard on an iPhone 17 Pro
    (iOS 27.2, keyboard limit 177 MB) with the app force-quit: V2's first load 15.4 s, the
    keyboard recording at 56 MB and peaking at 72 MB while V2 transcribed 9.4 s of audio in
    0.28 s (Apple's speech recognition, same clip: 0.34 s), with 468 MB on the Neural Engine
    outside the footprint. Twenty dictations in one keyboard process stayed on V2 with a
    peak of 96 MB, ten hide-and-show cycles and a five-minute hide kept the process and the
    model, a new process reloaded it in 0.3 s, and Ultra peaked at 66 MB (607 MB on the
    Neural Engine). A probe keyboard (`~/dev/experiments/2026-11-22-hex-keyboard-asr/native`)
    put 60 s of audio at 0.25–0.5 s; on the CPU, four chunks at once passed the limit
    there, so the keyboard never runs that. Linking FluidAudio into the keyboard grows its binary from
    5.9 to 12.6 MB and, with the experiment off, its footprint at launch by about 0.2 MB
    (writable data dyld fixes up), with no change to its first frame.
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
    strip for English, Spanish and French (France): each tap is scored against every letter with a 2D
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
    layouts (Norsk, Deutsch, Italiano, Français (Canada), Français (Suisse), Português, Polski, Română, Русский, Українська, Ελληνικά) keep the dictionary
    engine in their own language, and Srpski (no iOS dictionary) none, never the English
    model, except as bilingual autocorrect's second language (below). The
    models (`HEXKeyboard/TypingModel/en.hextm` and `es.hextm`, about 3.6 MB each) are
    built from Google Books Ngram v3 by `scripts/build-typing-model.py` (see
    Acknowledgements below); English's contractions count as often as the words they stand
    for ("do not"), and it adds Hex's own chat and tech words (`scripts/en-extra-words.tsv`),
    byte-identical to Android's `en.hextm`; `de.hextm` is the German model Android builds and ships
    (`android/benchmark/typing/build-layout-model.py`), the same v3 format and, on iOS,
    off until the experiment below is turned on. `fr.hextm` is Android's French model, built
    the same way from Google Books v3's French shards (corpus spellings, with the books'
    elided forms such as "c'est") and byte-identical to
    `android/app/src/main/assets/typing/fr.hextm` (its `SHA256SUMS` hash). On the held-out
    French replay (2,000 Tatoeba sentences typed on AZERTY, scored as
    `android/benchmark/typing/quality-gate.py` does) it makes 76.4% fewer word errors than
    the raw taps (35.3% with `UITextChecker` alone), changes 0.10% of correctly typed words
    (0.56%) and restores 81.1% of the accents left out where the plain spelling is in neither
    the model nor `fr.valid` (78.7%), short of the gate's 85%. Canada's and Switzerland's French don't read it yet (`fr-CA`, `fr-CH`).
    Beside a model, a language with its own word list (`HEXKeyboard/TypingModel/<code>.valid`,
    Android's `android/benchmark/typing/build-valid-words.py` output: today French's) takes what
    is a word from the model and that list alone, not from `UITextChecker` (`HexWordList`,
    memory-mapped and searched in place; `SystemTextChecker.ownWords`), and a missing system
    dictionary no longer turns its model off (`AutocorrectEngine.hasWords`). `UITextChecker`
    still gives guesses and completions while it is in that language. Beside the list, the
    language's `<code>.capitals` (Android's `build-capitals.py` output, read in place the same way)
    says which words are names: one the books write with a capital at least 95% of the time is no
    word typed all lowercase, so it takes its capital as `UITextChecker` gives one ("paris" ->
    "Paris", "jean" -> "Jean"; "marie", 94%, stays), and a listed name outside the model is a word.
    German's `de.capitals` is read in place through the same reader (`GermanCapitals`). With `fr.valid` French
    restores 86.1% of the accents the gate counts (81.1% with `UITextChecker`). English, Spanish
    and German keep `UITextChecker`: their own lists moved a gate number the wrong way.
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
      memory only without it. Settings shows that it resets without Full Access, Learning (n of 400 taps) or
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
    ends in a space, a line break, an attachment (U+FFFC: a shared link's card in Messages,
    a picture in the text) or opening punctuation (`( [ { « “ ‘ ¿ ¡ @ # /`, or a
    straight quote after a space), as Apple's QuickPath does; shift capitalises its first
    letter (caps lock all of it), the space it adds can start a sentence ("Hi." then a
    swipe types " There"), and a name the dictionary only knows capitalised is
    capitalised. The strip then offers the next three readings, the likeliest in the
    centre; tapping one puts it in the word's place. Until anything else changes the
    text, delete takes the whole word back (with the space it added), unless Settings ›
    Experiments › Delete swiped words letter by letter is on
    (`experiments.swipeDeleteLetterByLetter`, off by default), which makes delete act as
    anywhere else and take one letter at a time; space never autocorrects it. As on Apple's
    keyboard (watched on iOS 27.2), a space stays pending
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
    appended on a utility queue every 2 s. Nothing is sent. The keyboards look
    the same with it on as with it off: it is only the switch. The schema is in
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
    period, Dictation language, Show what I say and Type as I speak (iOS 26 and later),
    Translate what I say, Clean up what I say,
    Custom vocabulary (its list below the switch), Keep mic warm,
    and Microphone.
  - **Keyboard:** Keyboard, **Full** (the default) or **Dictation bar** (stored
    as `FullKeyboardSettings.isEnabled`, App Group key `experiments.fullKeyboard`
    from when the full keyboard was an experiment: unset or true is Full, false
    the dictation bar, so testers who turned that experiment off keep the bar;
    an iPad follows the same choice, where until build 117 it always showed the bar), Auto-start (Never, On
    switch by default, or Always) while the dictation bar is chosen, then Languages, Number row (off by default), Auto-capitalization, Autocorrect (with Spatial autocorrect below it while it's
    on), Suggest swear words, Vocabulary (Hex's custom vocabulary list, how many words it
    holds) and Key pop-ups only while Full is chosen, Haptics (the dictation bar uses it too),
    then Swipe typing and Emoji (Clear emoji history while it's on), which say they need the
    full keyboard while the dictation bar is chosen, and a field to try the keyboard in.
  - **Privacy:** "Everything stays on this iPhone", Learn how I type (what it
    learned below the switch), and under **Advanced**, Record typing logs (off
    by default) with its Typing logs viewer while it's on.
  - **Experiments:** Hinglish (Hindi in Latin letters), Greeklish (Greek in Latin
    letters), Two-language dictation,
    Clean up with Apple Intelligence, Adaptive touch (its status and Reset touch learning
    below the switch), German spatial autocorrect, Korean keyboard (두벌식), Dvorak punctuation
    keys, Comma and period keys, Quick accents, ABC under the emoji key, Keyboard themes (its
    Theme menu below the switch while it's on), Two-thumb selection, Delete swiped words
    letter by letter, Hide word suggestions, Join the keyboard when unfolded, and Multilingual
    model (Parakeet).

  Simulator scenes: `--snap-state settings-home` (ready), `setup` (keyboard
  added, Full Access not confirmed yet), `setup-sheet`, `setup-sheet-microphone`
  (before iOS has asked for the microphone), and
  `settings-<dictation|keyboard|privacy|experiments>[-on][-end]`. Each writes
  its page's switches to the App Group: off by default, on with `-on`; switches that are
  on by default (Spatial autocorrect, Swipe typing, Emoji) stay on.
  `-end` starts the page scrolled to the bottom. `settings-privacy-confirm`
  opens Record typing logs' consent dialog. `--experiment-keyboard-themes` turns
  Keyboard themes on, and `--keyboard-theme automatic|black|white|navy|forest|ember`
  picks the theme the full keyboard draws;
  the dictation bar keeps the system look, and iOS draws its own strip above or below
  a third-party keyboard, which a theme can't color.
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
  launch. Parakeet stays where it fits (6 GB) and before iOS 26, for English only, on
  an iPhone without `SpeechTranscriber`: an iPhone that has it keeps English on Apple
  Speech, on `DictationTranscriber` too while `SpeechTranscriber`'s English is set
  aside, and goes to Parakeet only once neither Apple module's English installed twice
  in a row (TestFlight build 126: one failed install sent an iPhone 15 Pro Max to a
  Parakeet download that stalled, until a reinstall). When Apple Speech doesn't answer,
  English stays on it there too, and any other language stays on Apple Speech, which
  says it can't dictate it, rather than going to Parakeet as English
  (`DictationController.appleSpeechLanguage`). After Hex ended twice in a row while
  loading Parakeet, the next launches wait for Set up instead of loading it again, for
  a day and on that build only (`ParakeetLoadGuard`: iOS ending Hex in the background
  or a swipe away mid-load once is no evidence), and a Parakeet download that sits at
  one fraction for 90 s ends with a message. A preparation that says nothing for 30 s
  ("Checking the speech model": Speech or its asset store not answering), or nothing
  new for 120 s after, ends with "The speech model isn't ready. Try again."
  (`watchPreparation`), as does the keyboard model's check behind setup's last step,
  whose row then offers **Try again**. Apple's reservation and asset status are watched
  at 60 s the same way (`SpeechModelDeadline`), a reservation or release that never
  answers is forgotten so the next attempt asks again, and a keyboard check that hangs
  counts no strike toward Hex's handoff (`SpeechModelNeedsHex.keyboardError`).
  **When a model won't install**
  (`SpeechModelInstall`, shared by the app and in-keyboard dictation): Apple lists
  some languages whose `SpeechTranscriber` asset never arrives ("transcription.ru
  asset not found after attempted download"), so Hex then installs the same
  language on `DictationTranscriber`, and after the second such failure in a row goes
  straight to it for three days. A download whose progress doesn't move for 60 s is
  given up on. When neither module installs while the iPhone is online, twice in a row,
  Automatic sets the language aside for three days and dictates the next one ("Russian
  isn't available on this iPhone yet. Using English."); a language picked by hand says
  so and offers that one ("Try English", with **Use English** in the app). A model that
  installs clears the failures so far. Only Hex's own failures set anything aside:
  in-keyboard dictation's are often its own (each process holds its own reservation, one
  released, removed or over Apple's limit reads as not installed there, "Too many asset
  locales allocated"). With nothing else to move
  to (an English-only iPhone), Automatic tries the set-aside language again
  rather than none, so "Apple's speech recognition can't dictate this language" means neither
  module lists any of the iPhone's languages. Offline or stalled, it
  says the download failed and tries again next time. In the keyboard, a model it
  can't get itself says "Hex couldn't transcribe that. Try again." the first time, and the
  second time in a row (within 6 h, no model in place since) "Open Hex to download
  English.": Dictate and the Hex key then open Hex, which downloads it, until a model
  installs (in either process; Hex checks again each time it comes to the front), a
  language is chosen, or a day passes (`SpeechModelNeedsHex`,
  `settings.dictationLanguage.keyboardNeedsHex`); a move to another language keeps its
  own sentence. Coming back to Hex after it was in the background starts over a check
  that failed or never answered, and a check that gives up bumps its generation so a
  late answer can't reopen the row (`KeyboardSpeechPreparation`). Reservations make room
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
- **Two-language dictation (Settings › Experiments › Two-language dictation, iOS 26+):**
  Off by default (`experiments.twoLanguageDictation`, App Group). While on, and while
  dictation uses Apple Speech's `SpeechTranscriber`, one recording is read twice: in the
  language dictation resolved (unchanged) and in the first other language in Settings ›
  Keyboard › Languages whose base language differs from it, mapped by the same rule the
  keyboard-follow language uses (`LanguageSettings.dictationLanguage(for:)`). Both must
  resolve to `SpeechTranscriber`, and the second language's model must already be installed
  (`AssetInventory.status`): dictation never downloads for it, and a language whose
  `SpeechTranscriber` model was set aside as failing is skipped. Two `SpeechAnalyzer`s
  read the same WAV, each with its own `AVAudioFile`, prepared with
  `prepareToAnalyze(in: nil)` before starting, and a `SpeechTranscriber(locale:`,
  `transcriptionOptions: [], reportingOptions: [],`
  `attributeOptions: [.audioTimeRange, .transcriptionConfidence])`; both get custom
  vocabulary's contextual strings. Each final result's timed runs give the merge its words
  (the run's own characters, leading whitespace included, start, end, confidence), and a
  run without letters or without a time range keeps its characters on the word before it.
  `TwoLanguageMerger` cuts both readings at the earlier of two word ends within 150 ms,
  scores each reading in a segment by its confidence-weighted seconds clipped to the
  segment, and picks a reading per segment with a two-state Viterbi that charges 0.35 s of
  confident audio for each language change, ties going to the primary. A word the new
  reading starts with is dropped only when it repeats the last word kept, case and
  punctuation aside, and the two spans overlap by at least half of the shorter one. The
  winning runs are joined with their own characters, inserting one space at a language
  change only when neither side has whitespace there. Whenever the second language wins no
  piece, the primary's original text is used unchanged, so Chinese, Thai, punctuation and
  untimed runs are exactly as Apple gave them. The text then runs through the same pipeline
  with the primary language. No second installed language, a second analyzer that throws,
  or no timed words in either reading dictates exactly as before; a primary reading that
  finished without hearing anything reports empty so its caller throws its own no-speech
  error without transcribing again; with the switch off no two-language work starts.
  `TwoLanguageMergeTests` cover the merge and the run extraction. The app, keyboard,
  widgets, and tests built on a macOS 26 GitHub runner with 33 tests passing (run
  45005571072, the latest observed verification); nothing here has been measured on an
  iPhone, and the simulator can't transcribe.
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
  spoken; Greeklish does the same for Greek, translated from Greek letters.
  Where you dictate, a "Translate to
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
  Spanish's). English and Deutsch together are its "English & German" keyboard the same way
  (iOS 27.2 and 26.5 simulators): German added to the English keyboard is QWERTY with
  English's long presses, English added to the German keyboard keeps its layout, QWERTZ –
  Deutsch's ü ö ä keys or, on QWERTZ, ten keys a row with ä ö ü and ß held. Apple and
  Gboard switch with a long press on the globe, but a Face ID
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
  **한국어 (두벌식):** Settings › Experiments › Korean keyboard (off by
  default, `experiments.koreanKeyboard`) adds 한국어 to the list: the standard Dubeolsik rows
  with shift's ㅃㅉㄸㄲㅆ and ㅒㅖ, and no qwerty/azerty choice. `HangulComposer` composes each
  syllable in the field (한, then delete to 하, ㅎ, nothing; ㅘ splits to ㅗ; 닭 + ㅗ → 달고,
  한 + ㅏ → 하나), the keyboard replacing only the run's changed characters through
  `deleteBackward`/`insertText` (no marked text). Space, return, punctuation, a page or
  language switch, emoji, dictation, paste, undo, a selection action, the delete-word swipe, a
  cursor move or hiding the keyboard commits the run. The cursor check records the last 32
  characters before the run, the text after the cursor and the field, plus the host's selected
  text, so a move to another place whose text before it ends alike commits too while a host
  that trims the front of its window still matches if the window holds at least those 32
  characters plus the run; a host that trims more than the tail or reports no selection is
  best-effort, and a host that reports no text before the cursor joins syllables only while
  jamo come within about half a second (a cursor move followed by a jamo inside that window
  can't be detected). A field whose text after the cursor changes while you type (inline
  autocomplete) commits each jamo, so syllables don't join there. Korean has no accent
  holds, spelling dictionary, autocorrect, word suggestions or typing model, and with
  the experiment off the language isn't listed, Automatic never picks it, and a stored "ko" is
  ignored. Unit tests only (`HangulComposerTests`, `HangulKeyboardTests`,
  `KoreanKeyboardGatingTests`): no CI compiles the iOS targets and no device run checks it.
- **Layout per language (Settings › Keyboard › Languages › Layout):** each language the keys
  show (every chosen one, Automatic's while none is, and only the first of English and
  Español together) gets a menu of the layouts Apple's Keyboard Layout list offers for it
  (iOS 27.0 simulator, black box), its own (today's) first: English QWERTY, AZERTY, QWERTZ,
  Dvorak; Deutsch "QWERTZ – Deutsch" (with ü ö ä), QWERTZ, QWERTY, AZERTY; Español
  "QWERTY – Español" (with ñ), QWERTY, AZERTY, QWERTZ; Français (France) AZERTY, QWERTY,
  QWERTZ, (Canada) QWERTY, AZERTY, QWERTZ, (Suisse) QWERTZ, AZERTY, QWERTY (Français (France)'s
  AZERTY is Apple's "AZERTY – French", the first layout Apple lists for France and Belgium, with
  the apostrophe key; Canada's QWERTY and Switzerland's QWERTZ are Apple's "QWERTY – French" and
  "QWERTZ – French", the first each lists, with the key ending row 2; every other French layout
  is the plain one); Italiano and Português QWERTY,
  AZERTY, QWERTZ; Polski and Română QWERTY, QWERTZ, AZERTY; Čeština "QWERTZ – Čeština" and
  "QWERTY – Čeština", Apple's "QWERTZ – Czech/Slovak" and "QWERTY – Czech/Slovak" (iOS 27.2
  simulator), both its own rows with the ´ and ˇ keys, Z and Y swapped on the second; Magyar
  "QWERTZ – Magyar" and "QWERTY – Magyar", the standard Hungarian QWERTZ with every accented
  vowel on a held key, Z and Y swapped on the second (Apple's own Hungarian layout list wasn't
  read on a simulator for this change); Srpski
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
  (Apple's split Dvorak unobserved). Settings › Experiments › Dvorak punctuation keys (off by
  default, `experiments.dvorakPunctuation`) puts `,` and `.` ahead of P on row 1, whose nine keys
  narrow to start at the left margin and end at the same delete key, puts `;` just left of the
  space bar where it can give up a letter's width, and types `<`, `>` and `:` with a shift you
  press or caps lock (auto-capitalization leaves the comma); on the split Duo the two
  punctuation keys join only the left half, on a pitch narrowed so it keeps its right edge (its
  top-row keys are narrower), and `;` takes from the left space bar. A URL or email field keeps its
  own `.` key instead of row 1's, and Experiments › Comma and period keys adds no second pair
  beside the space bar where this experiment put them on row 1. Off, every layout is key for key
  the one it was. Kept per
  language in the App Group
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
  (Apple's keyboards for them are QWERTY), Français (France) on Apple's "AZERTY – French" (rows of
  10, 10 and 7 keys: W to N and the apostrophe key, a letter wide and centred between shift
  and delete at their English size, as on Apple's; Hex's keycaps fall on Apple's to the pixel on
  an iPhone 17 Pro and SE, upright and turned, iOS 27.2 simulator). The apostrophe key
  (`FrenchTyping`) shows ’ right after a letter and types the apostrophe, curled as the field
  curls quotes (l’homme, qu’il), and holds ' ’ ‘ `; anywhere else it shows and types ´ (never a
  dead key) and holds ´ ˋ ˆ ¨, as Apple's does. It is no letter, so the decoder, Adaptive touch,
  next-letter odds and swipe typing never read it, and French autocorrect fixes a slip on either
  side of an elision's apostrophe as a word of its own ("l'hpmme" → "l'homme", "k'homme" →
  "l'homme", `frenchElisionFix`). The iPad keeps its own ' and : key instead; Français (Canada) on QWERTY and Français (Suisse) on QWERTZ, as on Apple's
  "QWERTY – French" and "QWERTZ – French": the same apostrophe key ends row 2, ten keys a
  letter wide edge to edge under row 1's, its rules and holds France's (keycaps on Apple's to the
  pixel on an iPhone 17 Pro and SE, upright and turned)
  (automatic picks them for `fr-CA` and `fr-CH`, AZERTY for any other French, Belgium
  included), Русский (Apple's ЙЦУКЕН: 11, 11 and 9 letters, ё on a
  held е, ъ on a held ь), Українська (the standard Ukrainian ЙЦУКЕН, ДСТУ 4862:2007, in the
  rows the iPhone's own Ukrainian keyboard shows on the iOS it runs on, because Apple changed
  them in iOS 27: from iOS 27 (27.0 and 27.2 simulators) 12, 12 and 10 keys,
  `йцукенгшщзхї`, `фівапролджєʼ`, `ячсмитьбюґ`, with ʼ the U+02BC apostrophe Ukrainian
  spells with, and only ʼ offers anything when held, the other apostrophes (' ’ ‘ and the
  backtick); before iOS 27 (26.2 to 26.5 simulators) Russian's 11, 11 and 9, `йцукенгшщзх`,
  `фівапролджє`, `ячсмитьбю`, every letter the two share on Russian's key, ї on a held і and
  ґ on a held г. On both the 123 and #+= pages' apostrophe key is ʼ, held like the letter, and
  ₴, which the system keyboard offers on its held currency key, is held on $; no ы, э, ъ or ё), Polski (QWERTY, as the
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
  pages and their held keys are English's), Magyar (the standard Hungarian QWERTZ, so
  `qwertzuiop`, `asdfghjkl`, `yxcvbnm`, on the English keys' QWERTZ; its nine accented vowels
  are all held, á on A, é on E, í on I, ó ö ő on O and ú ü ű on U, the base first and then the
  neighbouring Latin accents, ő and ű the double-acute letters U+0151 and U+0171, and every
  consonant holds nothing; QWERTY is offered too) and Srpski (Latin: Apple's QWERTZ option for it; Apple's iPhone
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
  Ukrainian's rows (twelve keys from iOS 27, Russian's before it) leave them one letter wide, as on Apple's. Long-press options
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
  `pt_PT`, `pl`, `cs`, `hu`, `ro`, `ru`, `uk`, `el`; Serbian would use `sr_Latn`, which iOS 27.2 doesn't have, nor a
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
  "příliš"), checked against a stub dictionary, and Hungarian too ("koszonom" → "köszönöm",
  "kesobb" → "később"), while a word of its own keeps its spelling ("meg" and "kor" stay
  though "még" and "kör" exist). A two-letter non-word one
  neighbouring key (or one swap) from a very common two-letter word is fixed too
  ("tk" → "to", Spanish "nl" → "no", Hungarian "es" → "és"; `KeyboardLanguage.commonTwoLetterWords`,
  English, Spanish, Italian and Hungarian). Auto-capitalization is unchanged.
  The 123 and #+= pages are English's in every language but Greek (above); their held keys follow each
  language's system keyboard where it differs (# holds № in Russian and Ukrainian, the
  digits º and ª in Spanish and Italian, . holds · in French, $ holds ₴ in Ukrainian and
  kr in Norwegian).
- **Size (full keyboard):** On an iPhone the suggestion strip is Apple's prediction bar,
  as thick as Apple's (iOS 26.5 and 27.2 simulators, iPhone 17 Pro, Air, 17 Pro Max and SE,
  upright and turned, light and dark, beside Hex in the same field): 44 pt from the
  keyboard's top edge in every state (suggestions, Undo dictation, Paste, the Full Access
  hint, the recording bar, the emoji panel's top row), with Apple's 8 pt
  under it upright (6 pt turned), so row 1's keycaps start 52 pt down (50 turned), the keys
  stay on Apple's rows, and where iOS adds nothing above the keyboard Hex is exactly as tall
  as Apple's keyboard: 253 pt on an iPhone 17 or 17 Pro, 260 pt on an SE, 272 pt on an Air or
  Pro Max (`KeyboardMetrics.withAppleBar`). On an 844 to 874 pt iPhone from iOS 27 Apple's
  43 pt row 4 ends 4 pt below where iOS ends a third-party keyboard's view, so those 4 pt come
  out of all four rows alike: 42 pt keycaps with Apple's 11 pt gaps, row 1 on Apple's line and
  rows 2 to 4 1, 2 and 3 pt above Apple's, no row shorter than another; every bottom-row key
  takes taps down to the view's bottom edge. The gap between rows 3 and 4 is the space bar's
  across its width (row 3's keys whose middle is over it end at their keycaps), as on Apple's
  keyboard, whose space bar takes taps up to 2.5 pt into row 3's keycaps (iOS 27.2); over 123,
  emoji, return and the rest the gap splits, so a low m or delete above return never sends
  return (Hex's rows sit 1 to 3 pt above Apple's, so a finger trained on Apple's lands lower on
  Hex's row 3). Apple's space bar takes taps 13 pt below its
  keycap, down to its own key plane's end; the 17 pt under Hex's view belong to iOS's globe band
  and never reach a third-party keyboard. The 17 pt band iOS adds above a third-party
  keyboard in some fields (Notes, Spotlight, a re-tapped Safari field; on iOS 26 even a plain
  first-focused text view) is an exception Hex doesn't design for: there Hex is 17 pt taller
  than Apple's. Inside the bar Hex draws what Apple's does: the words, SF 17 pt, centred 25 pt
  down; a pending autocorrection in Apple's bubble, its whole cell across and a 38 pt capsule
  from 6 pt down to the bar's bottom; 1 × 24 pt dividers between filled cells, hidden beside the
  bubble; and the Hex key in a 59 pt cell of its own at its end of the bar, as Apple's writing
  tools icon has at the other (a TestFlight screenshot, iPhone 17 Pro Max, iOS 27.2.1), with a
  divider between it and the word beside it; the three cells share the rest. iOS 18 is
  unmeasured and keeps the 38 pt strip it had (247, 254 and 266 pt). Apple's keyboard draws other rows before iOS 27 (iOS 26.2 and
  26.5 simulators, iPhone 12, 16, 17 Pro, Air and 17 Pro Max, beside Hex in the same field):
  the same keycaps, but upright its panel is 7 pt taller under the keys on an 844 to 874 pt
  iPhone and 2 pt shorter on a 900 pt or taller one, so on iOS 26 Hex's rows sit 7 pt higher
  (row 4 whole, 260 pt) or 2 pt lower (270 pt) to stay on Apple's (`KeyboardMetrics.appleRowsAreIOS26`);
  the SE is the same on both. iOS 18 is unmeasured (the Studio has no iOS 18 simulator) and
  keeps the iOS 27 rows Hex drew there before.
  Turned, the bar is 44 pt with 6 pt to row 1 and the keyboard 204 pt (32 pt keycaps on a 40 pt pitch); on iOS
  26 Apple's turned keycaps are 27.33 pt on a 35.33 pt pitch ending 4 pt above the bottom, and
  Hex's are too, 187.33 pt in all (iOS 18, unmeasured, keeps 204 pt) (the SE keeps iOS 27's turned keys for now: Apple's turned SE
  on iOS 26 is its upright keyboard across the whole screen).
  Where iOS gives the keyboard a shorter view than it asks for (Spotlight on an iPhone
  17 Pro Max, iOS 27.3, TestFlight: about 20 pt short), the rows fit the height it has:
  Apple's bar gives up its 6 pt over the old 38 pt strip first, so a view up to 6 pt short
  keeps every key whole and in place, and beyond that the keycaps and the gaps between rows shrink alike
  (`KeyboardMetrics.fitted`), instead of row 4 falling under iOS's globe band.
  Dictating no longer grows the keyboard. On the iPhone Duo (`KeyboardPosture`,
  model `iPhone19,4`) iOS sets third-party keyboards in an inset platter with a band
  of its own above them, so the strip has no gap there, and the keys follow Apple's
  for each screen: 56 pt rows folded, 59.7 pt unfolded and turned, 51.7 pt unfolded
  upright, starting as far in from the screen's side as Apple's. Folded, the cover
  screen was measured beside Apple's keyboard on a Duo: its keycaps start 18.5 pt in
  (54.5 px of 1375 at 2.95 px per point), and row 4 stands on row 3's columns: 123
  from the edge to a gap before z, the globe on z (without it, space starts there),
  space up to return, and return from a gap after m to the edge. A Hex key right of
  space takes m's column, where Apple's dictation key is; left of space it follows the
  globe and emoji. Its labels are 0.875 of the iPhone's there, as Apple's are: its 123
  is 70 px wide folded against Hex's 80 at 2.95 px per point. The unfolded postures are
  measured in the simulator.
- **Number row (full keyboard, iPhone):** Settings › Keyboard › Number row (off by default,
  `settings.keyboardNumberRow`, App Group) puts 1 to 0 in a row of their own above row 1, in
  every language and posture, upright and turned. Off, nothing changes. On, the keyboard
  grows upward by the row and every row below stays on Apple's lines (iOS keeps a keyboard
  on the screen's bottom): Apple's iPhone keyboard has no number row, so it follows
  SwiftKey's (0.85 of a letter row's pitch, the same row gap; Gboard on iPhone has no row,
  only digits held on row 1): 34.9 pt keycaps and 45.9 pt more keyboard on a 17 Pro (293 pt,
  granted in a text view, a Safari form and Spotlight on the iOS 27.2 simulator), 47.6 pt on
  a Pro Max, 34 pt turned. The digits sit on the 123 page's columns, take the gap under the
  strip as row 1 did, and hold what the 123 page's digits hold (0 holds °). On the 123 page
  the row hides, since its row 1 is the same digits, but its band stays so nothing jumps, and
  that row 1 reaches up through it; the #+= page keeps the row. The digits never reach
  Spatial autocorrect, next-letter touch help or Swipe typing (a drag from a digit is no
  swipe), and a word holding digits is never corrected. Where iOS gives less height, the
  number row shrinks with the others (`KeyboardMetrics.fitted`). The iPad keeps Apple's rows
  for now: its docked keyboards would need Apple's own number row on the 13-inch and room
  above the strip on the others.
- **Comma and period keys (full keyboard, iPhone):** Settings › Experiments › Comma and
  period keys (off by default, `experiments.commaPeriodKeys`, App Group) puts a comma just
  left of the space bar and a period just right of it, both a letter wide, on the letters page
  in a plain text field. On the joined keyboard the comma starts where the bar used to start
  and the period ends where it used to end, the bar moving inward at both ends; on the Duo's
  split keyboard the comma takes from the left half's bar and the period from the right
  half's, each bar keeping its inner edge. Neither is added unless the bar still holds a
  letter-wide space after them (on an upright iPhone showing both the globe and emoji keys
  with the Hex key beside space there isn't room, so they're skipped). They type `,` and `.`
  with shift on too (they have no shifted symbols). The 123 and #+= pages, a URL or email
  field (which keeps its own `.`), an iPad, and English's Dvorak letters while Dvorak
  punctuation puts both on row 1 are unchanged. Off, every layout is key for key the one it
  was. Unit tests only (`KeyboardLayoutTests`, `FullKeyboardEngineTests`): no CI compiles the
  iOS targets, and no physical iPhone run checks it.
- **Word suggestions (full keyboard):** The strip above the keys shows the words the engine
  offers as you type, and after a space the finished word's emoji. Settings › Experiments ›
  Hide word suggestions (off by default, `experiments.hideWordSuggestions`, App Group) stops
  the suggestions: the strip keeps its height, the Hex key, Undo, status, notices and the Paste
  chip, but shows no typing or next-word suggestions, swipe readings, finished-word emoji, or
  the chip that keeps what you typed while a correction is still ahead (keep literal).
  Autocorrect still corrects on space, and the chip that puts back what it replaced (`.revert`)
  stays after Delete. With no words offered, tapping the left slot to keep what you typed
  mid-word and holding a word to add it to Custom vocabulary are gone. Off, the strip suggests
  exactly as it did.
- **Background (full keyboard and dictation bar):** Hex draws nothing behind its keys, so
  the keyboard's background is iOS's own glass panel: the same colour as the band iOS adds
  above a third-party keyboard and the globe row under it, over any app or wallpaper, with
  or without that band (no step at Hex's edges on an iPhone 17 Pro, iOS 27.2, in Notes,
  Safari on a light page and Spotlight over a wallpaper, and in the iOS 27.0 and 27.2
  simulators, light and dark). One case gets a cover, because iOS can leave Apple's keyboard
  under a third-party one. In a field that takes focus as its screen appears and has an
  input accessory view, opened with Hex, switching to Apple's keyboard never tells Hex it
  disappeared and keeps its view; switching back puts a new Hex over both, and Apple's keys
  show through Hex's translucent ones (every switch in the iOS 27.0 and 27.2 simulators,
  under a bare clear keyboard extension too; one TestFlight report, build 56, iPhone 17,
  iOS 27.0.1; never in Notes, Safari or Spotlight on an iPhone 17 Pro). Hex can't see the
  app's views, but it can see its own earlier keyboard still on screen in the same field
  (`KeyboardsOnScreen`), and only then draws a thin blurred material under itself
  (`KeyboardBackdropView`), through which Apple's keys are an even, slightly lighter tone
  with no shapes. Opened with Apple's keyboard, or focused by a tap, the same field frees
  Hex as usual and nothing is left behind. Until build 117 the material was always on: 5 to
  9 off the glass in dark mode and neutral grey where the glass takes a wallpaper's tint, so
  Hex showed as a panel of another colour inside iOS's panel, with a seam under iOS's band.
- **Split keyboard (iPhone Duo):** Unfolded and held upright (the inner screen wider than
  tall), the full keyboard splits as Apple's does there (`SplitGeometry`): each half at
  the keyboard's side with Apple's 46.3 pt keys on a 57 pt pitch, row 2 half a key in
  and row 3's letters a key and a half, an odd row's middle key on both halves (G and V
  in English), a space bar on each half, and nothing in the middle, where a tap types
  nothing. The strip sits over the right half, where Apple's predictions are; the
  recording bar still spans the top. Every layout splits by the same rule (measured
  against Apple's English split only; AZERTY, QWERTZ, Russian and the rest follow the
  rule unchecked), and every page (123, #+=). Spatial autocorrect reads taps as on the
  unsplit keyboard (`KeyboardLayout.unsplitX`), so either G is one key. Apple's split
  follows its own Split Keyboard preference, which a keyboard extension can't read, so
  Hex splits whenever the Duo is in that posture (the iOS 27.2 simulator has Apple's off
  until `KeyboardShouldSplitInDefaultContext` is set). Settings › Experiments › Join the
  keyboard when unfolded (off by default, `experiments.joinKeyboardWhenUnfolded`, App
  Group) draws one joined keyboard there instead: unfoldedWide's own 39 pt rows on a
  51.7 pt row pitch, 19 pt in from the keyboard's own side as unfoldedTall's keys sit
  from its screen's, with the iPhone's portrait bottom row (123 and the globe to z's
  right edge, the emoji key on x's column when the globe shows, space after them,
  return from m). Off, nothing changes.
- **iPad:** The app is iPhone-only (`TARGETED_DEVICE_FAMILY` 1), but iPadOS runs its keyboard
  extension as an iPad keyboard across the screen, so the full keyboard shows there too
  (Settings › Keyboard picks it or the dictation bar, as on an iPhone). Docked, it has
  Apple's iPad mini rows on every iPad (`KeyboardPadRows.swift`; the 11- and 13-inch's own,
  with tab, caps lock and the 13-inch's number row, are a later step): delete ends row 1 and
  return row 2, a shift key opens and closes row 3, after the letters Apple's two-symbol keys
  (, and ! then . and ?; AZERTY , and ?, . and !, ' and :), the shifted symbol drawn over the
  one a tap types and typed with shift (`KeyboardKey.shifted`), and the bottom row the globe,
  123, the Hex key where Apple's dictation key is (whatever Settings › Hex key says: the strip
  is all suggestions), the space bar, emoji (or 123 when Emoji is off) where Apple's second
  123 is, and the hide-keyboard key. The letter keys are as wide as the longest row allows
  with its delete or return at least 1.236 and 1.24 letters wide (English 54.5 pt, French
  53.25 pt on the mini upright), row 2 is set in 0.4 of a key pitch (0.2 where it holds as
  many letters as row 1, as AZERTY's does), the left shift is a letter wide (1.061 on AZERTY),
  and delete, return and the right shift take what's left; the bottom row's keys are the same
  in every language. The 123 and #+= pages follow Apple's: digits and delete, @ # $ & * ( ) '
  " (€ £ ¥ _ ^ [ ] { } on #+=) and return, a page key either side of % - + = / ; : and the two
  two-symbol keys (§ | ~ … \ < > ! ? on #+=), and row 4 as the letters' with ABC; Hex has no
  undo or redo key there, so ABC takes the room of Apple's undo and second ABC.
  German is QWERTZ without ü, ö and ä as Apple's iPad German is, holding them on u, o and a.
  Measured against Apple's iPad mini in English, German and French (every key within a point);
  the other languages' rows follow the same rule unmeasured. Docked, a held key's accents open
  in Apple's iPad cells (below). The sizes are Apple's iPad key sizes (`KeyboardPadSize`,
  `KeyboardPadPlacement`, `KeyboardMetrics.pad`), measured beside Apple's keyboard in the iPadOS 27.2 simulator:
  docked, Apple's key heights, row pitch, gaps and side inset for the iPad mini (under 800 pt
  across), 11-inch and 13-inch (1,000 pt and up), upright or turned, so its four rows sit
  exactly on Apple's bottom four (letters and the bottom row; the 13-inch's number row and
  the 11- and 13-inch's tab, caps lock and extra keys are a later step), counted from the
  screen's bottom, where iPadOS leaves 20 pt empty under a third-party keyboard (3 pt under
  Apple's). Which way up comes from the keyboard's width against the screen, not the size
  classes, which an iPad keeps regular either way. Above the rows the strip is Apple's 44 pt
  bar with the words centred, and above it 44 pt stay clear, as the dictation bar keeps them:
  iPadOS always adds its own 55 pt bar above a docked keyboard (undo, redo, paste; empty in
  Windowed Apps), and in some apps it draws that bar over the keyboard's top instead (Notes on
  an iPad mini, iPadOS 26.6, TestFlight). So Hex is taller than Apple's (353 against 282 pt
  on the mini and 11-inch upright). Docked, the labels keep their upright sizes turned, and
  keys show no balloons, as Apple's don't.
  Floating (iPadOS makes every keyboard 320 pt wide, with no bar above and its own globe and
  dictation band below), Hex is Apple's floating keyboard: 26 × 38.25 pt keys on a 54.3 pt
  pitch, 35.5 pt shift and delete, 123, space and return on row 3's columns, and the strip
  where Apple's bar is, 271 pt in all as Apple's; it shows no globe of its own there.
  iPadOS grants a custom keyboard any height it asks for (up to 900 pt tried); one that asks
  for none gets 258 pt (343 turned) on the 11-inch. It always lays a third-party keyboard out
  at that default first and switches to the height asked for about 15 to 20 ms after
  `viewDidAppear`, even when the height was set before the view had a window (iPhone: 216 to
  247 pt), and a tap while the keyboard still moves lands where the key was a frame earlier.
  Measured with a scratch build logging every touch and layout (iPadOS 27.2 simulators, three
  at once): taps 100, 200, 400 and 800 ms after the keyboard appeared, 100 launches each, all
  went in on an 11-inch docked (400 of 400) and floating (399; one second tap never reached the
  keyboard), as on an iPhone 17 Pro (399; one first tap at 100 ms typed the key above). Launched
  into a turn, at the same moment after launch Hex takes taps sooner than Apple's keyboard
  (1.4 s: 75 of 80 against 2 of 80; from 1.6 s both every one); earlier the keyboard is still
  sliding in. Docked, a held key's accents show in Apple's iPad cells, half a key wide and
  53 pt tall, two rows once there are more than five (`AccentMenu.padArrangement`,
  `KeyboardMetrics.accentCellWidth`); floating, as on an iPhone. In the floating keyboard's
  emoji panel the category icons shrink to fit its 18.6 pt slots (17 pt in 22 pt and wider).
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
  of the way to the backdrop, as Apple's, with no haptic while it moves. Apple's second-finger
  tap to select text has no host counterpart: `UITextDocumentProxy` (iOS 27.2 SDK) can read the
  selection but not make one. Off (the default), a second finger does nothing. Settings ›
  Experiments › Two-thumb selection (off by default, `experiments.twoThumbSelection`, App Group)
  gives it one: while the space bar is the trackpad, a second finger starts a selection from
  the cursor, and the finger further left moves the start of the range, the other the end, each
  with its own `TrackpadCursor.State` (the trackpad's gain, acceleration and rows). Settling the
  trackpad at the start can move the host cursor once; after that, no host cursor moves. The
  strip shows the text around the range with the selected part highlighted,
  within the text the host shares: the range is clamped to what the field shares around the
  cursor when the selection starts (the field shares only the cursor's sentence or line). If
  the host's context hasn't caught up at the start the whole span is kept; either way an
  action runs only while the host still shares the whole range, and otherwise dismisses
  without acting. Both thumbs up shows Delete · Cut · Copy · Dictate · Cancel (Cut and
  Copy only with Full Access, as the strip's clipboard; without it they aren't offered): Delete
  puts the cursor at the range's end with `adjustTextPosition` and deletes its characters one by
  one (a 👍🏽 is one); Cut copies then deletes; Copy copies and puts the cursor at the end;
  Delete and Cut leave it at the range's start; Cancel changes nothing; and any other key
  dismisses the actions without acting. Dictate leaves the range in place and starts the
  keyboard's usual dictation there (the warm Hex session or in-keyboard dictation). When its
  words are about to insert and the field still shows the range unchanged (the same field, and
  the text around it), they replace it; otherwise they insert at the cursor as any dictation
  would, with the range left alone. Cancelling or hearing nothing leaves the range untouched.
  Where dictation would open Hex instead (no Full Access, or the keyboard can't dictate here),
  tapping Dictate dismisses the actions and opens Hex (or shows the Full Access hint) with the
  text left in place. An empty range ends like a normal
  trackpad lift. Delete and Cut ask the host to delete once per Character: in a script whose
  host removes one code point per call (some Devanagari and Thai clusters), the last code
  points of a cluster can stay behind. The dictation bar's space bar is the same trackpad along the line (the same still hold or a 10 pt slide, a
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
- **Quick accents (full keyboard, Settings › Experiments):** Off by default
  (`experiments.quickAccents`, App Group). Holding a letter opens its callout with the letter's
  main accent selected — the language's preferred one (`KeyboardLanguage.quickAccentPrimaries`:
  Spanish o → ó, English e → é, Italian a → à, and Á with shift), else the first variant after
  the key's own in its holds (`AccentCatalog.primaryVariant`) — so letting go where the finger
  landed types it. The cell under the finger when the callout opened is remembered
  (`accentOpenCell`), so the preselection stands until the finger moves into another cell,
  where sliding picks that one as before; letting go far below still cancels, and another key
  going down while the callout is held commits the preselection. Only letters: the 123 and
  #+= pages' held symbols, and the mark keys ˇ and ʼ, keep the key's own selected. Off, the
  callout opens on the key's own letter. Unit tests only (`FullKeyboardEngineTests`): no CI
  compiles the iOS targets, and no physical iPhone run checks it.
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
    bar's top row (on an iPad it once offered nothing). A failure or note
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

- **Google Books Ngram data:** Spatial autocorrect's word and word-pair frequencies,
  German's capitalisation shares and French's list of words outside its model come from the Google Books Ngram Viewer data, version 3
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
Scoring lowercases, straightens curly `’`, trims leading and trailing apostrophes, and keeps
ASCII letters: a tap compares letters, and an apostrophe only when the recorded word has one;
a swipe compares letters only. Taps report that as `accuracy` over every in-dictionary word and
also `all-words-accuracy` over every word, so the gap between the two is what the
out-of-dictionary words contribute. Swipes report `accuracy` and `typed-or-offered` over the
swipe attempts (`attempts`): gestures that became swipes, ones whose path left the key by a
swipe's pitch and weren't promoted, and ones that read nothing. They also report
`in-dictionary-accuracy` over every in-dictionary word — taps included, a skipped gesture
counted wrong — and `all-words-accuracy` over every word, which is the end-to-end number:
what the person reading the field saw, with an out-of-dictionary word now able to count as
right when the field has it. `taps` and `taps-right` are the recordings whose path never
leaves its key by a swipe's pitch: the keyboard types that key's letter, and the field keeps
it, as for a person typing a one-letter word. Tap mode keeps Swipe typing attached as the
keyboard runs it, so a recorded tap whose samples drift can be promoted to a swipe;
`read-as-swipes` counts the recorded taps the plane took that way. `not-swipes` counts swipe
attempts the plane never took as swipes or that read nothing; `skipped` counts gestures that
began off the letter keys, and `clamped` the touches moved forward to the previous lift;
unlike `taps` and `taps-right`, which count in-dictionary words, those three count every
recorded touch, and `read-as-swipes` also counts every delivered tap, whatever its word. A
tap's words compare an apostrophe only when the recorded word has one; a swipe's by letters
only, because a swipe's path runs over letter keys and cannot draw one. The decoder's own
median and p90 milliseconds close the line, though a swipe's preview decode can overlap its
timed final decode, so a `decode-ms` sample may include that overlap; the words are
unaffected. `HEX_REPLAY_LIMIT` caps sentences per source; `HEX_REPLAY_OUT` writes each
sentence's intended and typed words (prefix both with `TEST_RUNNER_` too).

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
- **Show what I say and Type as I speak:** `LiveDictationTests` cover how results become the
  words shown, the switches' defaults, the end of a long dictation, and Type as I speak's
  typing, revising, taking back, stopping when the field changes or an app reports no text, and
  private fields. The simulator has no speech models, so there the preview and typing ran on a
  replay of Apple's results recorded on a Mac (`LiveTranscriptReplay`), in a host app, light and
  dark, upright and turned, on an iPhone SE's globe key and with no model at all (no preview).
  On an iPhone 17 Pro (iOS 27.2): the preview, Type as I speak, a 60 s dictation, memory with
  and without, and the keyboard hidden mid-dictation with Type as I speak (Home, then back: the
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
  `HangulComposerTests` the two-set composer and the keyboard typing through it,
  `FullKeyboardEngineTests` the swipe against the
  hold-then-slide trackpad and switching in a `FullKeyboardView` (English & Spanish too,
  and a picked Spanish variant surviving the swipe away and back), and
  `AutocorrectEngineTests` bilingual autocorrect on stub dictionaries and iOS's own
  (`testABilingualKeyboardKeepsBothLanguagesWords` prints the break counts). Hungarian's
  QWERTZ rows and held vowels, typing through them in a `FullKeyboardView`, and its accent
  fixes are unit tests too (`testHungariansRowsAndHoldMatchItsLayout`,
  `testHungarianTypesAndSwitches`, `testHungarianRestoresMissingDiacritics`); no
  simulator or iPhone typed on its keys for this change. Checked live in
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
An iPad still installs it (as an iPhone app) and runs its keyboard as an iPad keyboard: the full
keyboard or the dictation bar, as Settings › Keyboard chooses (see **iPad** under the full
keyboard; until build 117 an iPad always showed the bar). Docked, the bar keeps its top 44 pt clear:
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