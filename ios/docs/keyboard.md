# Keyboard

Part of [Hex for iPhone](../README.md).

The full keyboard (`HEXKeyboard`) and its dictation bar: keys and
gestures, autocorrect and the typing models, learning, languages and
layouts, geometry, the iPhone Duo and iPad, emoji, and the Dot Scope.
Dictation itself is in [dictation.md](dictation.md).

## Full keyboard Hex key (Settings › Keyboard › Full, the default)

A tap, or a
still hold of 0.35 s, starts dictation: in a warm session, else inside
the keyboard (in-keyboard dictation, iOS 26+), else by opening Hex.
In-keyboard dictation listens from touch-down, so a tap's or hold's first
words are kept; moving
more than 10 pt first opens the style menu, dropping that capture; releasing over a row picks it, and releasing elsewhere leaves
the menu open for taps (`HexKeyGesture`). VoiceOver activation dictates,
and the **Dictation styles** action opens the menu.

## Auto-start

Settings › Keyboard › Auto-start (`FullKeyboardSettings.dictationBarAutoStart`)
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

## Push to talk

Holding still for 0.35 s on the full keyboard's Hex key
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
Apple's. It needs no Full Access, which only Paste does. When iOS prompts
Allow Paste (read ≥ 0.3 s) or refuses a paste read, the strip shows a
tappable two-line notice (`Paste without asking` / `Paste from Other Apps ›
Allow` with a **Settings** chip that routes through
`hex-dictation://keyboard/paste-settings` straight into `Settings › Apps ›
Hex`), and Hex records `keyboard.clipboard.allowPastePrompted` in the App
Group so Home, Settings › Keyboard, and Setup surface the same one-tap Open
Settings banner once iOS has created that row. A later fast cross-app paste
(ignoring Hex's own copies via `markOwnedCopy`) or tapping dismiss clears
it. With Full Access, when the keyboard appears while it would dictate English
with Hex's speech model but the model isn't downloaded, the strip shows
`New speech model` / `Downloads in Hex on Wi-Fi` with an **Open Hex** chip
that opens Hex as it is, where the download starts on Wi-Fi
(`KeyboardParakeetNotice`): at most once a day and three times in all, never
again once tapped or put away with its ✕ (counts in the App Group), gone as
soon as you type or dictate, and never in a password or one-time-code field.
Turning off Speech model in keyboard (Settings › Dictation) stops it too.

## Learn how I type (Settings › Privacy › Learn how I type)

On by default (an
unset switch reads on; turning it off is saved and stays off), and
shown whether or not the full keyboard is on. It learns only from typing and
dictating with Hex, on this iPhone, never from what you write elsewhere. A
private journal in the App Group (`typing-journal.json`) that never stores
what you write: per-key tap offsets, and word-level pairs with counts and at
most one word of context on each side. Only plain text fields that allow
autocorrect count (`TypingJournal.allows`). Nothing leaves the iPhone.

- **Typing:** words you erase and retype, autocorrect fixes you undo from the
  strip, suggestions you pick, and autocorrections you turn down by
  tapping what you typed. A word you erase and retype counts only when it was
  plausibly a slip of the new one (`TypingLearning.isSlip`): the same word with
  other capitals or an apostrophe ("Im" then "I'm"), or, two letters or more,
  starting alike (the same letter, a neighbouring key, or the first letter
  missed or one typed before it) with at most one slip for every four letters,
  where a neighbouring key, a missing accent, two swapped letters and a doubled
  letter count half ("Ciry" then "City"). A change of mind ("Sunda" then
  "Saturday") or an unfinished word finished teaches nothing, and one a journal
  already holds is not counted, listed or applied. A typo you fixed the same way
  twice (never a completion, never fixed two ways) becomes a fix on space, only
  a suggestion when what you typed is a real word; an autocorrection you turned
  down twice never applies again (`TypingLearning`, read by `AutocorrectEngine`).
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
  Typing insights lists these dictation fixes with a switch each and the share of
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

## Auto-capitalization (Settings › Keyboard › Auto-capitalization)

On by default,
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

## Spatial autocorrect (Settings › Keyboard › Spatial autocorrect)

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
Polish reads `pl.hextm` (150,000 words, 184,327 pairs, 3.60 MB) and `pl.valid` (525,084 words,
8.29 MB), both built by `scripts/build-typing-model.py pl` from NKJP's n-grams, ParlaMint and
PoliMorf (Settings › Acknowledgements). Its letters are held on their base keys, so the model
puts them back ("zolw" -> "żółw", "wiem ze" -> "wiem że"). iOS's own Polish dictionary is no
use without Apple's word data (it takes any letters for a word), and the list replaces it. On
the held-out Polish replay (`scripts/typing-gate.py run … pl`): 85.98% fewer word errors than
the raw taps, 0.22% of correctly typed words changed, 95.46% of the marks
restored where the plain spelling is no word, 5/5 critical cases. `pl.capitals`
(`android/benchmark/typing/build-capitals.py pl`, case counted in ParlaMint) gives names
their capital: of the names typed all lowercase, 10.65% come out capitalised (0% with the
phone's dictionary on the simulator, which has no Polish word data, and 0% without the list).
With English as the second language, Polish makes 85.0% fewer errors (86.4% alone: English
takes "cos", "troche", "beda" as its words) and English is unchanged.

## German spatial autocorrect (Settings › Experiments › German spatial autocorrect)

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
the letter odds or model completions, and no model for a bilingual partner. Off, swipe
typing never reads German, even with Swipe typing in more languages on.

## Adaptive touch (Settings › Experiments › Adaptive touch)

Off by default
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

## Offensive words (Settings › Keyboard › Suggest swear words)

Like Apple's
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

## Swipe typing (Settings › Keyboard › Swipe typing)

On by default (stored as
`experiments.swipeTyping` from when it was an experiment off by default: unset or true
is on, false off, so testers who turned it off keep it off), for the
English and Spanish layouts (every other layout keeps a drag as a slide to
another key), and with Settings › Experiments › Swipe typing in more languages on
(`experiments.swipeMoreLanguages`, off by default) for every language with a typing
model too: French (France), Polish, and German while Spatial autocorrect and German
spatial autocorrect are on. Off under VoiceOver. A touch that goes down on a letter and
travels more than a key's pitch (width plus gap) is a swipe (`SwipeTyping`): its key
no longer types, a light trail (a shape layer, the last 0.45 s of the path) follows the finger,
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
Known gaps in these languages:

- A word with ß (German) or œ/æ (French) has no key, so a swipe drops it from its
  candidates ("groß", "Straße", "cœur").
- A swiped German noun goes in lowercase: the noun capitalisation applies to tapped
  words only.
- French and Polish have no language-specific never-suggest rows (only the shared
  ones), so a swipe's top reading isn't filtered for language-specific offensive
  words there.
- The decoder's parameters are tuned on English and Spanish, so French, Polish and
  German accuracy is unmeasured.

## Record typing logs (Settings › Privacy › Advanced › Record typing logs)

Off by default,
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
[`typing-logs.md`](typing-logs.md).

## Keyboard languages (Settings › Keyboard › Languages, with Full chosen)

Automatic (the
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
superseding the extension's Info.plist `mul`, but whether the host follows it (Notes's red
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

## Layout per language (Settings › Keyboard › Languages › Layout)

Each language the keys
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
top-row keys are narrower), and `;` takes from the left space bar. A URL, email or web search
field keeps its own `.` key instead of row 1's, and Experiments › Comma and period keys adds no second pair
beside the space bar where this experiment put them on row 1. Off, every layout is key for key
the one it was. Kept per
language in the App Group
(`settings.keyboardLanguages.layouts`, `LanguageSettings.letterLayout(for:)`); unset,
not offered, or unreadable (an iPhone that keeps the App Group from a keyboard without
Full Access), a language types on its own layout. Spatial autocorrect, the touch decoder,
swipe typing, next-key growth and the neighbouring-key cost read the keys as drawn, and
Adaptive touch learns each arrangement as its own key plane; typing logs' `layout` lines
name it (`letters`).

## Keyboard layouts (each language's letters)

English,
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
Hex reports (`primaryLanguage`), which is derived from this choice, and Hex's own entry
among the iPhone's keyboards says `mul` (the extension's Info.plist `PrimaryLanguage`),
so iOS lists Hex as multilingual ("multilingue", where it said "anglais") and neither
Automatic nor "On this iPhone" takes it for English. With `en-US` there, Hex listed above
Apple's French keyboard typed QWERTY on a French iPhone without Full Access, or before Hex
had first resolved a dictation language, and Languages offered English "On this iPhone"
(TestFlight build 140, iOS 27.2 simulator). Rows of 11 keys narrow
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

## Size (full keyboard)

On an iPhone the suggestion strip is Apple's prediction bar,
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

## Number row (full keyboard, iPhone)

Settings › Keyboard › Number row (off by default,
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

## Web search fields (full keyboard)

A `.webSearch` field (Safari's address bar, and any search field that asks for it) has
Apple's `.` key between the space bar and return, as Apple's keyboard has (iPhone 17 Pro,
iOS 26.5 and 27.2 simulators). Upright on an iPhone (and on a floating iPad keyboard) the
row stands on Apple's columns: 123 (and emoji) to z's right edge, the space bar from x, a
letter-wide `.`, and a return 1.75 keys' pitch wide to the edge, so the bar ends a quarter
pitch sooner than in a text field (Apple's: 6.7–99.3, 105.3–287, 293–326.7 and 332.3–395.7 pt
on a 402 pt iPhone; Hex's within 0.5 pt). Turned, and on the Duo folded and split, it takes
the URL and email fields' row, the `.` a letter wide beside the bar. A docked iPad keeps its
text row, as Apple's does (its `.` stays on row 3, iPad mini, iPadOS 27.2). Only that key
changes: everything else treats the field as text (`KeyboardFieldKind.takesWords`), so
autocorrect, swipe typing, suggestions, smart punctuation, the emoji key and emoji
shortcodes, Learn how I type and Adaptive touch work as the field's own traits allow, as
they did before, and the Paste and Code chips read the field's content type as everywhere
else. Holding the `.` offers `.` and `…` (the symbol pages' list); Apple's offers
`.us .edu .com .net .org` there and in email fields, which Hex doesn't yet. Unit tests
(`KeyboardLayoutTests`, `FullKeyboardEngineTests`, `ShiftStateMachineTests`) and simulator
screenshots beside Apple's in Safari; no physical iPhone run checks it.

## Comma and period keys (full keyboard, iPhone)

Settings › Experiments › Comma and
period keys (off by default, `experiments.commaPeriodKeys`, App Group) puts a comma just
left of the space bar and a period just right of it, both a letter wide, on the letters page
in a plain text field. On the joined keyboard the comma starts where the bar used to start
and the period ends where it used to end, the bar moving inward at both ends; on the Duo's
split keyboard the comma takes from the left half's bar and the period from the right
half's, each bar keeping its inner edge. Neither is added unless the bar still holds a
letter-wide space after them (on an upright iPhone showing both the globe and emoji keys
with the Hex key beside space there isn't room, so they're skipped). They type `,` and `.`
with shift on too (they have no shifted symbols). The 123 and #+= pages, a URL, email or web
search field (each keeps its own `.`, so there is never a second one), an iPad, and English's Dvorak letters while Dvorak
punctuation puts both on row 1 are unchanged. Off, every layout is key for key the one it
was. Unit tests only (`KeyboardLayoutTests`, `FullKeyboardEngineTests`): no CI compiles the
iOS targets, and no physical iPhone run checks it.

## Word suggestions (full keyboard)

The strip above the keys shows the words the engine
offers as you type, the emoji of the word being typed, and after a space the finished
word's emoji (Emoji below). Settings › Experiments ›
Hide word suggestions (off by default, `experiments.hideWordSuggestions`, App Group) stops
the suggestions: the strip keeps its height, the Hex key, Undo, status, notices and the Paste
chip, but shows no typing or next-word suggestions, swipe readings, typed or finished-word
emoji, or the chip that keeps what you typed while a correction is still ahead (keep literal).
Autocorrect still corrects on space, and the chip that puts back what it replaced (`.revert`)
stays after Delete. With no words offered, tapping the left slot to keep what you typed
mid-word and holding a word to add it to Custom vocabulary are gone. Off, the strip suggests
exactly as it did.

## Background (full keyboard and dictation bar)

Hex draws nothing behind its keys, so
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

## Split keyboard (iPhone Duo)

Unfolded and held upright (the inner screen wider than
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

## iPad

The app is iPhone-only (`TARGETED_DEVICE_FAMILY` 1), but iPadOS runs its keyboard
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

## Touch targets (full keyboard)

Every point of the key area types a key: gaps
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

## Spaces (full keyboard)

A suggestion (a word, what you typed, or an emoji)
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

## Smart punctuation (full keyboard)

As on Apple's keyboard (black box, iOS 27.2), the
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

## Delete after an autocorrection (full keyboard)

As on Apple's keyboard (black box,
iOS 27.2), delete takes only the space or mark after the corrected words, and the strip's
left slot offers what you typed, highlighted with an undo arrow (Apple's text view shows
it in a bubble under the word, which a keyboard can't draw); tapping it puts that back,
without the space, and turns the fix down for the session. Space keeps the fix. The offer
comes back whenever the cursor is at the end of the latest autocorrection again, revisions
("well see" to "we'll see") and joins included. Text replacements offer none, as on Apple's.

## Key clicks

Every key of the full keyboard (delete, shift, 123 and return included, and
each delete repeat), its strip, and the dictation bar's keys ask for the system click on
touch-down (`UIDevice.playInputClick`), which plays only because the keyboard's root view
(`KeyboardInputView`) adopts `UIInputViewAudioFeedback`: the full keyboard view adopted it
before, as a subview, and never clicked. It follows Settings › Sounds & Haptics › Keyboard
Feedback › Sound, with no setting of Hex's. Apple's keyboard has other sounds for delete and
the modifier keys, but the click is the only keyboard sound a keyboard may play, so every key
gets it. In the iOS 27.2 simulator each press reaches the system click (system sound 1104,
in the keyboard's log) with Full Access or without, and none with the Sound switch off; on
an iPhone, Apple's guide says it needs Allow Full Access.

## Slides from 123, #+= and shift (full keyboard)

As on Apple's keyboard, holding 123
(or ABC, or #+=) shows its page, and sliding shows the pop-up of the key under the
finger (or darkens it with Key pop-ups off); letting go types that key and goes back to
the page it came from. Letting go back on the page key types nothing and goes back too;
off the keys it types nothing and the page stays. Holding shift and sliding to a letter
shows and types its capital. None of them is a swipe, and a slid-to key opens no
accent menu (Apple's does after a hold).

## Quick accents (full keyboard, Settings › Experiments)

Off by default
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

## Emoji shortcodes (full keyboard, Settings › Experiments)

Off by default
(`experiments.emojiShortcodes`, App Group). In a plain text field that takes emoji, with
autocorrect on and secure entry off (not the emoji search), a colon and a name after
whitespace or at the start of the field offer up to three emoji in the strip's slots in
place of its words: the name's first character is a letter or digit, at least two of its
characters are letters or digits, and the rest may also be "_", "+" and "-"
(":pizza", ":thumbs_up", case-insensitive); the shapes of chat emoticons (":-D", ":+1") are
not tokens. An exact name or alias comes first, then the names and keywords the token
starts, one per family, with your emoji use breaking exact ties and moving prefix matches
up (`EmojiShortcode` over `EmojiLibrary`, whose words come from
`scripts/build-emoji-data.py`). A tap takes the whole token back and types the emoji with
its skin tone, counted like one from the panel; typing the closing ":" of a name typed in
full (":pizza:") types the emoji instead. No match, a colon inside a word ("abc:pizza"), a
field with autocorrect off, a secure field, the emoji search, or the experiment off leaves
the strip and typing as they were. Only names the index weighs as names or aliases
complete: `:heart:`, `:joy:` and `:tada:` are keywords in the data and stay text, and
`:smile:` completes to 😊, the first emoji `emoji-aliases-en.txt` lists for it, not 😄;
adding them needs more aliases and a regenerated index, which
`scripts/build-emoji-data.py` can't make from the repo alone (it downloads Unicode's
emoji-test.txt 18.0 and CLDR 49, and no copy is checked in). Unit tests only
(`EmojiShortcodeTests`): no CI compiles the iOS targets, and no physical iPhone run checks
it.

## Emoji (Settings › Keyboard › Emoji, for the full keyboard)

On by default (stored as
`experiments.emoji` from when it was an experiment off by default: unset or true is on,
false off, so testers who turned it off keep it off). An emoji key
right of 123 (after the globe) in text and web search fields, not web address, email or
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
tones offers five; the one you pick is typed and kept for that emoji. While a word that
names an emoji is typed ("smile" → 😊, "fuego" → 🔥 on the Spanish layout), the strip
offers that one emoji: in the next empty slot, else in place of the right slot's runner-up
word, else the centre's. A word names an emoji when it is the emoji's whole name or one of
Hex's chat words, in the layout's language or English, plurals and endings too, never a
prefix ("smil") or a word only part of several names ("face", "hand"). "heart" is one of
Hex's chat words, so it offers ❤️ though 45 emoji share it. A phrase is offered at the word
that names it ("thumbs" → 👍), not at its last word ("thumbs up"): the strip reads one word. The emoji never
takes the place of what you typed, a revert, the word space will put in, or your own
words, and isn't offered while space would put in another word, for a swiped word, or in a
password, one-time code, username, email address or web address field, a secure one or
one with autocorrect off (`FullKeyboardView.withTypedWordEmoji`). A tap types it with a
space in the word's place ("smile" → "😊 "); Delete then takes the space and the left slot
offers the word back, as after an autocorrection, and Learn how I type records nothing
(`FullKeyboardEngine.acceptEmoji`). Testers describe Apple's keyboard doing this on an
iPhone; in the iOS 26.5 and 27.2 simulators it offers no emoji for "smile", "pizza" or
"love", before or after the space, so the slot follows Hex's strip, not a measurement.
After a space, an empty strip offers the finished word's own emoji ("pizza " → 🍕),
never in place of word suggestions, and a tap adds it after the word. The App Group keeps a use count per emoji that halves every 10
days and each emoji's skin tone, no words (`EmojiUsageStore`); Clear emoji history
empties it. The index is `HEXShared/FullKeyboard/Emoji/emoji-index-<lang>.txt`,
built by `scripts/build-emoji-data.py` from Unicode's emoji-test.txt 18.0 and CLDR
49 (Unicode License v3, `LICENSE-Unicode.txt` beside it, which permits commercial
use with the notice) and `scripts/emoji-aliases-en.txt` (its chat words apply on every
layout); a word several emoji share counts for less, except for the emoji it is the
whole name of; the keyboard maps it and
binary-searches it in place (about 0.7 MB on the heap), loading it on the first
emoji key press or word typed in a field that takes emoji. Emoji newer than 15.1 are listed only where the
emoji font draws them. Nothing leaves the iPhone.

## GPU Dot Scope (`HEXShared/DotScope.swift`, `DotScopeShader.metal`)

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
