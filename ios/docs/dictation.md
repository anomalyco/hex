# Dictation

Part of [Hex for iPhone](../README.md).

The dictation pipeline: engines and speech models, styles and cleanup,
dictation language and translation, warm sessions and live dictation,
capture, audio and the Action Button. The keyboard's own keys are in
[keyboard.md](keyboard.md).

## App status and engines

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
(`HEXKeyboard/KeyboardLocalDictation.swift`), or, with Settings › Dictation › Speech model in
keyboard on, with Parakeet on the Neural Engine in the keyboard's own process
(`HEXKeyboard/KeyboardParakeet.swift`, below). With Full Access, the web view
and its page load as the keyboard shows (unless memory is already short), without
the microphone, so no orange dot shows until you dictate; the full keyboard's
Hex key then starts capture as the finger lands, before the touch is a tap or
a hold, and a slide to the style menu or a cancelled touch drops it with
nothing transcribed (its orange dot shows for that moment). Until the first
real PCM audio buffer arrives, the keyboard shows a calm starting state
(dim resting dots and a spinner in Done's spot, with ✕ active to cancel) and
switches to active recording only once audio is flowing. iOS ends a keyboard that passes
its memory limit (about 60–90 MB depending on the model), so while less is left
than starting needs (`os_proc_available_memory`: 32 MB for the first
dictation, 24 MB once the capture web view is loaded) Dictate and the Hex key
open Hex instead, as before iOS 26. Audio never leaves the iPhone. From iOS 26 Hex
never asks for Speech Recognition access: that permission covers `SFSpeechRecognizer`,
which Hex uses only before iOS 26, and `SpeechAnalyzer` runs without it in the app and
the keyboard alike (a TestFlight tester who turned it off dictated, iOS 27.0.1, build 51).
Before iOS 26 Hex asks for it as it gets `SFSpeechRecognizer` ready (setup, or Set up),
never at a launch by itself.

## Warm session (`Off`, `5m`, `15m`, `1h`)

Picking a duration on **Keep
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

## Styles (`TranscriptFormatter`, run by `DictationPipeline` wherever Hex dictates: the app, the keyboard's warm session and cold start, the Action Button, Control Center and in-keyboard dictation)

`Polished` (full
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

## Lowercase (Settings › Dictation › Lowercase)

Off by default. After the
style and custom vocabulary's spellings, everything is lowercased, custom
vocabulary included, as desktop's Lowercase step (`str::to_lowercase` on the
final text) does, on every path (`DictationLowercaseSettings`, App Group key
`settings.dictationLowercase`).

## French spacing (every style but Verbatim)

Dictation in French, or translated
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

## Format lists (Settings › Dictation › Format lists)

On by default
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

## Spoken punctuation (Settings › Dictation › Spoken punctuation)

On by
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

## Spoken "@" (English, every style but Verbatim)

"at the rate" (India's
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

## Spoken counts (English, every style but Verbatim)

A count of three
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

## Numbers as digits (Settings › Dictation, on by default; English, every style but Verbatim)

Numbers said in words are written in digits, as Apple's
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
"$4.99"). All of a number converts or none of it ("a hundred and fifty" 150, "one point
twenty one" 1.21, "one in ten people" 1 in 10, "five to ten percent" 5 to 10%, "five foot
six" 5 foot 6, "oh four seven two" 0472 mid-sentence); arithmetic stays as said ("ten
minus five is five"), as do "two to three hundred" and "one or two hundred". A number the
engine capitalised as a name stays: mid-sentence or with a capital inside, before another
capitalised word ("I love Twenty One Pilots", "Ten Thousand Villages", "Fifty Shades"), but
"Area fifty one", "Forever Twenty One" and "Twenty people came" convert. Custom vocabulary
and learned words are never converted (`protectedTerms`, matched word by word without
case): adding "Five Guys" or "Twenty One Pilots" keeps them, the fix for brands the engine
writes in lower case, which no rule can tell from a count. Not yet: "one" alone ("one of them", "no one", "one more"), times and prices
without a marker ("three thirty", "four ninety nine", "twelve fifty"), ranges and scores
without a unit ("eight to twelve", "three to one"), fractions, ordinals outside dates
("finished first", "twenty first floor", "a second"), small numbers before anything else
("at eight", "GPT five"), and any run of number words that reads two ways ("one thirty").
Other languages keep their words: each needs its own number words, compounds (German
"fünfundzwanzig", French "quatre-vingt-dix") and separators ("5,5" and "15.000" in
German, French, Spanish, Italian and Dutch). `SpokenCleanupRulesTests` holds the 80
sentences as both engines wrote them in both voices.

## Mid-sentence dictation

When the text before the cursor doesn't end a
sentence (`.` `!` `?` `…` or a line break), the keyboard lowercases the
dictation's first word, except "I" and its contractions, acronyms, words
with inner capitals, and words the dictation language's `UITextChecker`
dictionary doesn't know in lowercase (likely names). German keeps the case,
since its dictionary takes nouns in lowercase; so does a language with no
dictionary on the iPhone (`TranscriptFormatter.insertionText`).

## While the keyboard records: Live words (Settings › Dictation, iOS 26+)

While the keyboard records by itself (in-keyboard dictation),
the capture's chunks also go to Apple's `SpeechAnalyzer` as they arrive
(`AppleLiveTranscriber`: `SpeechTranscriber` with volatile and fast results, or
`DictationTranscriber`'s progressive long dictation for its languages), only when the
language's model is already on the iPhone: it never downloads one. **Live words** is
one choice of three, kept in two App Group keys (`LiveDictationSettings`: Show what I say's
and Type as I speak's, the two switches it replaced): **Off**, **In bar** (the default)
or **Into field**. **In bar**: the full keyboard's keys go and the words so far take their place
under the recording bar (`LiveDictationPreviewView`), the settled words in the keys'
ink and the tail the recogniser may still change muted, the newest line in view with
whole lines above it. It takes every touch where the keys were, so nothing types while
it shows, and a tap on the words finishes the dictation as ✓ does (✕ is still the only
way to throw one away). Only a tap, by the keys' own measures: a touch that moves more
than a key's slop (`FullKeyboardEngine.accentSlop`, 10 pt) or is held as long as a key
takes to open its accents (`accentHoldDelay`, 0.36 s) does nothing, and so does one
landing within 0.4 s of the words appearing (their 0.3 s fade in and 0.1 s more: the end
of the tap that started the dictation, or a thumb settling); it finishes once. The
recording bar's ✕ and ✓ work as before, and an iPhone with a Home button keeps its globe
key where it was. VoiceOver reads it as one element, never announced as words change;
activating it, or its Finish dictation action, finishes the dictation. Its motion is one family, a spring with no bounce by its duration
(`LiveWordsMotion`, `LiveDictationPreviewHost.Motion`, and the recording bar's morph and
fades), and Core Animation runs every frame of it, so it keeps the display's pace however
busy the main thread is with the capture, the recogniser and the Dot Scope (a display link
drawing the words got a quarter of its frames on time on an iPhone 17 Pro): the keys fade
(0.2 s) as the words fade in rising 6 pt (0.3 s); whatever changes in the words (new words,
a settled tail turning to ink, a word the recogniser changed) cross-fades in place (0.3 s,
the cubic closest to the spring) while what didn't change stays exactly as it is; a new line
moves the block up (0.3 s) and the line leaving the top fades through a 14 pt band, then out
of it once the words are at rest. With Reduce Motion, the cross-fade only (0.2 s) and lines
jump. `LiveWordsView` draws the words once per update into one layer (TextKit 1) and drops
the lines above the box, whole, at rest. When the dictation goes in, the preview's words
fade drifting 6 pt up (0.15 s) as the keys come back (0.3 s), and they stop taking touches
at once; the field gets the dictation's own text, with its style, clean-up,
punctuation, held results and Undo dictation as before, so it can differ from the
preview: with the speech model in keyboard (Parakeet, English's default) the preview's
words are Apple's and the text Parakeet's ("10" can go in as "ten"). The words on screen
are never what goes in: ✓ (or a tap on the words) while some are still muted throws none
of what was said away, since the recogniser is stopped and the whole recording is read
again for the text. **Into field**:
the words are typed into the field as they come
(`LiveFieldTyping`), only the changed tail retyped. It keeps exactly what it typed and
edits only while the text before the cursor still ends with it; when the dictation's
text goes in, its words come out first, so the text goes in where and as it would without
them, in one step. ✕, a failure or nothing heard take them out. The keyboard hiding
mid-dictation takes them out where the app still takes edits; one gone to the background
doesn't, so they are noted (`LiveDictationSettings.leftover`) and the held text, inserted by
itself or with Paste dictation, replaces them where the field still ends with them.
Once the field changes under it (a key, a cursor move, a selection, or an app that
reports no text), it stops and leaves its words, and the text goes in at the cursor as
without it. One change is a send instead: the app emptying the field while it ends with
the words (a chat's Send, or select all and delete; `LiveFieldTyping.wasSent`) and still
empty 0.3 s later. Those words went out as typed: they count as a kept dictation for
`DictationEditTracker` (as a host clearing the field always has), Undo forgets them, and
the dictation so far ends there, its audio deleted. The microphone stays on: the audio from
the moment the field first read empty (held meanwhile, every chunk in exactly one
dictation) starts a new dictation with its own file, live words from the first, final text
at ✓, length and memory checks, so talk, Send, talk, Send works hands-free. An app that
changes the words without emptying the field (an edit, a mention) still stops the typing. Never in secure, password, one-time code or card number fields, over a
selection or a two-thumb Dictate's range, in an app that reports no text, or with Translate
what I say on; where it can't type a dictation for one of these, the words show over the keys
instead, as In bar does, so Into field never shows nothing (`LiveDictationSettings.liveWords`;
what is stored doesn't change). Both keys on (1.0.4's two switches together, until another
choice is picked) types into the field and shows the words over the keys. Neither runs
before iOS 26, for dictations recorded through Hex (Keep mic warm), when less than 32 MB
is left once the capture runs (`KeyboardLiveDictation`: the recogniser's share in the
keyboard with margin, and the speech model's transcription after it), on an error, or
when the recogniser isn't running 4–6 s into the audio; under 24 MB, or on a memory warning, the recogniser goes
and the dictation goes on. The dictation bar shows no preview (Into field works
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

## Apple Intelligence cleanup (Settings › Experiments › Clean up with Apple Intelligence, iOS 26+)

Off by default. For every style but `Verbatim`,
one on-device FoundationModels pass before formatting fixes clearly
misheard words and spoken self-corrections, within a 2 s budget. A
timeout, error, empty or implausible output keeps the raw transcript, and
the toggle is disabled with a reason when Apple Intelligence is unusable.

## Multilingual model (Settings › Experiments › Multilingual model (Parakeet), iOS 18+)

Off by default, and disabled on iPhones under 6 GB
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
turned off and on. Unless Settings › Dictation › Speech model in keyboard has it loaded in the
keyboard (below), the keyboard doesn't use it, so once Hex has it ready
(`MultilingualParakeetSettings.isModelReady`, set after a load, cleared while it's
missing, set aside or off) and it takes the dictation language, Dictate and the Hex
key open Hex, as before iOS 26 (a warm session still dictates without leaving the
app); until then the keyboard dictates itself. Measured on a Mac (`~/dev/experiments/2026-11-21-hex-multilingual-parakeet`):
mixed Dutch-English test sentences 7.5% WER against Apple Speech nl-NL's 26.9%,
pure Dutch 0.9% (0.4%), pure English 0% (0%). Load memory, compile time and speed
on an iPhone are unmeasured.

## Speech model in keyboard (Settings › Dictation › Speech model in keyboard, iOS 26+)

On unless the person turns it off, and disabled (shown off) on iPhones under
6 GB (`KeyboardParakeetSettings`, in the App Group; a value stored by hand wins, so a
switch turned off stays off). Off, nothing of it loads and the keyboard dictates exactly
as before. On, Hex downloads Parakeet V2 (464 MB, about 700 MB with the keyboard's
compiled copy) into the App Group (`KeyboardParakeetDownload`; turning it off stops and
deletes it unless Hex itself dictates with V2). By itself it downloads only where the
keyboard would use it (`KeyboardParakeetDownload.hold`): English dictation without
Two-language dictation or the Multilingual model, the keyboard's own memory limit
measured on this iPhone (the keyboard writes footprint plus `os_proc_available_memory`
to the App Group at each launch and tells Hex with a Darwin notification) and at least
`KeyboardParakeetBudget.requiredLimit`, a connection that isn't expensive, constrained or
cellular (`NWPathMonitor`, `KeyboardParakeetDownload.isUnmetered`: not cellular, even 5G that
iOS calls not expensive under "Allow More Data on 5G", a personal hotspot or Low Data Mode),
and 2 GB free.
It checks at launch, whenever Hex comes to the front, when such a connection returns and
when the keyboard reports its limit. Turned on by hand (a stored true, including from
the builds where it was off by default) it downloads at once on any connection. Until
then the row says what it waits for: "Downloads after you first use the Hex keyboard",
"Downloads on Wi-Fi", "Downloads once the iPhone has 2 GB free", "Only for English
dictation". Home shows the same status under "New: Hex's own speech model" wherever the
keyboard would use it (`SpeechModelHomeCard`: the switch on, an iPhone that fits, English
and no other engine); once it's ready in the keyboard the card can be dismissed, for good. The keyboard's own dictation transcribes with Parakeet in
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
that, and uses Apple's speech recognition exactly as with the switch off whenever
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
gets ready beside the engine that dictates. The Settings › Dictation row says what it's doing: downloading,
downloaded, getting ready in the keyboard, ready, or that the keyboard can't use it now
(`KeyboardParakeetSettings.keyboardState`, written by the keyboard). Dictating in another
language than English, Hinglish included, it says "Only for English dictation." even once
downloaded: Apple's speech recognition takes that dictation whatever the keyboard last wrote
(build 147 said "Ready in the keyboard", which reads as Parakeet taking Hinglish). Each dictation logs
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
5.9 to 12.6 MB and, with the switch off, its footprint at launch by about 0.2 MB
(writable data dyld fixes up), with no change to its first frame.

## Clean up what I say (Settings › Dictation › Clean up what I say)

Off by
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

## Custom vocabulary (Settings › Dictation › Custom vocabulary)

Off by default.
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
cleanup and again after formatting, so a style cannot undo a spelling. In
English, a name heard with another spelling then takes the listed one
(`SoundAlikeSpelling`; every engine writes a name's commonest spelling, so
TestFlight build 130's "Stacey" kept coming back "Stacy"): a word the engine
capitalized that reads the same as a listed term of three or more letters
starting with a capital, once interchangeable spellings are evened out (ph
and f; a hard c, k and ck; chr and kr; a soft c and s; ae and e; a leading
ai and a; a final y, ey, ie, ee and i; any other y and i; a silent h;
doubled letters), takes the term ("Stacy" → "Stacey", "John" → "Jon",
"Catherine's" → "Katherine's", "Amy" → "Aimee"). Inside a sentence, and not
right after another capitalized word, it may instead sound the same by a
Metaphone-style key (its consonants with voicing kept, first vowel sound,
vowel ending and vowel groups) and be at most two edits away ("Jeff" →
"Geoff", "Versal" → "Vercel"). Lowercase words, words in capitals, listed
words and two equally close terms stay as heard. The
listed spelling is kept exactly, even at a sentence start (an all-lowercase
term stays lowercase), and a term that spells its own boundaries ("iPhone 7",
"X-ray") matches those boundaries exactly. A word that carries its own
apostrophe only matches a term that has one, several transcript words are
joined only when the term reads as a compound, and compound terms like
"WeChat" can also match the plain phrase ("we chat"). On iOS 26 Apple Speech
the list is also sent as a best-effort recognition hint, in the app and in
in-keyboard dictation (through build 130 the keyboard sent only Learn how I
type's words: #2103 added the list's rewrite there but not its hint);
Parakeet's batch path gets no hint.

## Dictation language (Settings › Dictation › Dictation language, iOS 26+)

Automatic by
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
`settings.dictationLanguage.keyboardNeedsHex`). Only a language Hex itself set aside keeps
its sentence in the keyboard ("Russian isn't available on this iPhone yet. Try English."): the
keyboard sets nothing aside, and through TestFlight build 147 its own failed install of a
language picked by hand on an English iPhone (Hebrew, Norwegian, German) said it wasn't
available at every dictation and never went to Hex, which could download it. Every other
failure is Hex's to download, Hebrew included (`SpeechModelNeedsHex.keyboardError`).
Coming back to Hex after it was in the background starts over a check that failed or
never answered, and a check that gives up bumps its generation so a
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

## Voice isolation (Settings › Experiments › Voice isolation)

Off by default
(`experiments.voiceIsolation`, App Group). While on, the app records in
`AVAudioSession`'s `.voiceChat` mode instead of `.measurement`, so Control
Center's Mic Mode can offer Voice Isolation while Hex records, and
in-keyboard dictation asks `getUserMedia` for echo cancellation and noise
suppression (automatic gain stays off). Other audio may be lowered while
recording, and Bluetooth headphones may switch to their hands-free call
profile. With it off, both record exactly as before. Not verified on a
device: that Control Center actually offers Voice Isolation for Hex, and how
the mode changes a recording's level.

## Two-language dictation (Settings › Experiments › Two-language dictation, iOS 26+)

Off by default (`experiments.twoLanguageDictation`, App Group). While on, and while
dictation uses Apple Speech's `SpeechTranscriber`, one recording is read twice: in the
language dictation resolved (unchanged) and in the first other language in Settings ›
Keyboard › Languages whose base language differs from it, mapped by the same rule the
keyboard-follow language uses (`LanguageSettings.dictationLanguage(for:)`). Both must
resolve to `SpeechTranscriber`, and the second language's model must already be installed
(`AssetInventory.status`): dictation never downloads for it, and a language whose
`SpeechTranscriber` model was set aside as failing is skipped. So the switch does
nothing for a language Apple reads only with `DictationTranscriber` (on macOS 27.2.1
`SpeechTranscriber` lists 45 locales, without Ukrainian, Czech, Polish, Russian, Hebrew
or Dutch), on either side of the pair. Two `SpeechAnalyzer`s
read the same WAV, each with its own `AVAudioFile`, prepared with
`prepareToAnalyze(in: nil)` before starting, and a `SpeechTranscriber(locale:`,
`transcriptionOptions: [], reportingOptions: [],`
`attributeOptions: [.audioTimeRange, .transcriptionConfidence])`; both get custom
vocabulary's contextual strings. Each final result's timed runs give the merge its words
(the run's own characters, leading whitespace included, start, end, confidence), and a
run without letters or without a time range keeps its characters on the word before it.
`TwoLanguageMerger` cuts both readings at the earlier of two word ends within 150 ms,
and at any word end of one reading that no word of the other spans (more than 150 ms
either side): where the language really changes, the reading that can't follow it hears
a few long garbled words or none, so the two rarely end a word together there, and
without those cuts the speech on both sides of the change fell in one piece and one
language's words were dropped (an English-then-Chinese clip lost its last 6 s of
English). It scores each reading in a segment by its confidence-weighted seconds clipped to the
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

## Switching the dictation language in the keyboard (iOS 26+, Full Access)

The dictation bar shows the language beside the style ("Norsk ⌄") and opens a
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

## Translate what I say (Settings › Dictation, iOS 26+)

Off by default. Translates
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

Settings checks and downloads the pair the keyboard translates: from the
language the keyboard dictates in (Settings › Dictation language, else the
keyboard's language Automatic follows, else the language of the keyboard's
last dictation on Automatic, which the keyboard writes as a code and a time in
`experiments.translateDictation.keyboardAutomaticLanguage`, else the iPhone's
first language Hex dictates, as the keyboard's Automatic picks), never Hex's
own Automatic pick, which follows Hex's language when it is set apart from the
iPhone's in iOS Settings (the keyboard keeps the iPhone's: TestFlight build
149, Hex in English on a Chinese iPhone). Turning the switch on or picking a
target there checks the pair and, when its packs aren't on the iPhone, shows
Apple's download sheet at once; opening the page only shows where the pair
stands ("Chinese to English is ready. Works offline.", or the download
button), and before Hex knows the spoken language it says to dictate once
with the Hex keyboard. A first dictation from the keyboard's switch or menu
still finds out from the keyboard's note. Debug simulator builds take
`-hexTranslationStatus installed|supported|unsupported` in place of Apple's
answer, which the simulator reports as unsupported for every pair.

## Screen stays on while dictating in Hex

Recording and transcribing set
`isIdleTimerDisabled`, and every way out (done, cancel, failure) hands the
screen back to Auto-Lock. It is Hex's own idle timer ("for the app", Apple's
`UIApplication.isIdleTimerDisabled`), so it keeps the screen on only while Hex
is in front: the handoff screen and in-app recording, not a warm session or
in-keyboard dictation, where another app is in front. A keyboard extension has
no public way to keep the screen on: an app extension can't reach
`UIApplication.shared` (Apple's App Extension Programming Guide, "Some APIs Are
Unavailable to App Extensions"). Apple doesn't say in so many words that the
timer counts only while the app is in front, and the simulator has no
Auto-Lock to check it.

When the iPhone locks (Auto-Lock, or the side button), Hex's own recording
goes on: the app, a warm session and the Action Button record in the
background (`audio` background mode), up to the app's 60 s limit, and their
text goes where it would have (the app's transcript, Paste dictation, the
clipboard). In-keyboard dictation stops at the lock, since a keyboard
can't record in the background, and keeps what was said up to it (below).
Measured on the iOS 27.2 simulator (its lock button), locking 20, 45 and 80 s
into a two-minute dictation: the keyboard kept 20.1, 44.9 and 79.5 s, and the
app, a warm session and the Action Button each kept 59.8 s.

## Resilient capture & expiry contract (`DictationController`)

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
- An in-keyboard dictation the keyboard is dismissed from (the screen locks,
  or another app comes forward) stops its capture at once, keeps what was
  said, and finishes without the keyboard; its text is held in the App Group
  (`keyboard.heldDictation`), goes into the same field if the keyboard
  comes back there within 30 s, and is offered as Paste dictation
  anywhere else or later. A capture that goes quiet for 6 s finishes with
  what it heard, and a transcription stuck for 2 minutes is given up.
- iOS freezes a keyboard seconds after the screen locks (11 s in the
  simulator) and may end it before it has transcribed: the text then never
  came, and the recording stayed in the keyboard's temporary folder, unheard
  (simulator, the keyboard's process ended 20 s after the lock). Now the
  recording of a dictation the keyboard closed during stays on disk, recorded
  in the App Group (`keyboard.interruptedRecording`, `KeyboardInterruptedRecording`,
  one at a time), until its text is held; the next keyboard that finds one
  no process is transcribing transcribes it and holds its text as the first
  would have, for up to 24 hours, after which it is deleted unheard. A
  transcription the keyboard gave up on while it was away stays for the
  next keyboard too. Paste dictation for a dictation cut short says for 8 s
  where it stopped and why, "Stopped at 0:45" over "The screen locked or you
  left the app" (the full keyboard's strip, with Paste), or on one line in
  the dictation bar: a keyboard can't tell the screen locking from leaving
  the app, so it names both. What was said after the lock is not recorded.
- ✓, a tap on the live words or a push-to-talk release keeps what was said up to
  the tap. The capture page hands its audio over 4,096 samples (85 ms at 48 kHz)
  at a time, and its stop used to drop the part-filled buffer, so the recording
  ended 50–110 ms before the finger lifted (TestFlight build 147 and App Store
  1.0.4, Live words on or off, simulator); a tap as the last syllable still
  sounded could lose its end. The stop now waits for that buffer (at most 85 ms,
  250 ms for a context that went quiet) before it lets the microphone go, so the
  spinner shows that much later; a start meanwhile waits for it. The keyboard
  hiding still finishes at once.
- An in-keyboard capture that another sound interrupts waits for it instead
  of ending: a notification, an alarm or a call takes the audio session, and
  WebKit interrupts or suspends the capture page's `AudioContext` and mutes
  (or, if its capture fails, ends) the microphone track. The page reports each
  `mute`, `unmute` and `ended` of its track and each `statechange` of its
  context, with its time, and the keyboard writes every one to the probe log
  (`page statechange at +4.2 s: context interrupted, track muted`). Every
  250 ms the page resumes a context that isn't running, then opens the
  microphone again for a track that ended or stayed muted for 3 s (again
  after 1, 2, 4 and then every 8 s), into the same context and capture: the
  same dictation goes on, with its file, live words and Type as I speak, and
  the time away is missing audio. While the page says it's interrupted, the
  6 s quiet rule doesn't count, and quiet counts again from when it's back
  (`KeyboardCaptureWatch`). Interrupted for more than 15 s
  (`KeyboardJobWatchdog.captureInterruption`: a call, an alarm), it
  finishes with what was said, as before. In the simulator, which has no
  microphone and no interruptions, `capture-debug.js` left in the App Group
  runs once in the capture page (to stand a recording in for the microphone,
  or suspend the context, mute or stop the track by hand), and each recording
  is kept there as `capture-debug.wav`. What iOS sends a capture for a
  notification on a phone, with the ringer on and on silent, is unverified.
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

## Action Button, Control Center, Back Tap & Live Activity

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
