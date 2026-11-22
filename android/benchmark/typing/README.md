# Android typing benchmark

**On the same iOS R1 word models and synthetic taps, Android with its default suggestion filter has 135 fewer English word errors and 13 fewer Spanish word errors than iOS.** The biggest part of the old 298-error English deficit was a missing step before spatial decoding: porting the English contraction and typo rules alone recovered 231 errors.

## Run the per-language quality gate

From the repo root, with Python 3, JDK 21 and the Android project's SDK installed:

```sh
android/benchmark/typing/quality-gate.py all
```

Use a language code instead of `all` for one language. A language with hold-only letters but fewer than 50 unambiguous word occurrences prints `too few (n)` and skips only that threshold. The script compiles the current app's Kotlin classes with the Gradle wrapper and runs `Replay.java` on the host JVM. It prints the table, writes counts, rates, critical-case outputs and failed checks to `android/build/typing-quality.json` (override with `--output PATH`), and exits 1 if any language fails. No device or OS spelling dictionary is involved.

[Thresholds](thresholds.json) apply to unrounded rates in every language:

- **Noisy taps:** word edit distance on the original 2,000 sentences, before and after decoding. WER is edits / reference words; reduction is (raw edits − decoded edits) / raw edits (minimum 70%).
- **Correct words changed:** replay the intended spelling, including spaces and apostrophes, on key centres. Letters available only by long press use the IME's explicit-letter path. Count reference words no longer present on a minimum-edit alignment (maximum 0.5%). Compare final sentence output, so revisions of the preceding word count. Input preserves the original reference capitals. Scoring still ignores case-only differences so the metric measures spelling changes, directly comparable with the previous lowercase-input gate.
- **Accents restored:** type each eligible reference on centres, substituting only letters a typist would otherwise have to hold. The [per-language mappings](accent-keys.json) are checked at runtime against the model alphabet, [KeyboardLetters.rows](../../app/src/main/java/ly/anoma/hex/keyboard/KeyboardLetters.kt) and [LetterFeedback.options](../../app/src/main/java/ly/anoma/hex/keyboard/LetterFeedback.kt). Dedicated letters (ñ, ä/ö/ü, å/ø/æ) remain literal. The default substitute is the key itself; a data entry can specify the hold key and a different typed sequence. German ß is reached by holding s but is typed as **ss** when omitted (Straße → Strasse). English's model alphabet has no hold-only letter and skips this metric.
- **Which restorations count:** compare complete affected words on the final minimum-edit alignment. “Unambiguous” means the plain spelling is absent from both HXTM vocabulary and the language’s shipped `.valid` list. With at least 50 such occurrences, 85% must be restored, including out-of-vocabulary targets. Below 50 the observed counts remain in JSON but do not decide the gate. “Ambiguous” means both spellings are listed; report restoration with no threshold. If only the plain spelling is listed, JSON reports that third bucket separately. The clean replay also reports **ambiguous harm**: switches between different listed spellings with the same plain form, divided by all correctly typed members of those families in the 2,000 clean sentences. Those switches are already in “correct words changed”, not an additional threshold. A zero denominator is shown as a dash, not a 0% success rate.
- **Critical cases:** type each [named input](critical-cases.json) on centres, finish with space, and compare the entire sentence. Every case must pass, including valid ambiguous spellings that should remain literal.

### Held-out inputs and attribution

The six `<lang>.tsv.gz` files in [held-out/](held-out/) contain **only the existing synthetic test split** from the benchmark below, in its original order: no dev rows, typing logs, personal offsets, predictions or model-building input. Each gzip UTF-8 TSV row is `Tatoeba sentence ID · contributor · normalized reference · literal hit keys (including final space) · space-separated x,y taps`. Test ID is the zero-based row number prefixed as in the original replay. All six raw error totals reproduce the existing benchmark. The case-aware gate may make different decoder decisions from the historical lowercase APK replay. Gate inputs must remain evaluation-only.

Sentences: [Tatoeba](https://tatoeba.org/), [CC BY 2.0 FR](https://creativecommons.org/licenses/by/2.0/fr/) or CC0 where released as such. Punctuation is normalized as in the original generators; the clean references retain the source sentence’s case. For noisy taps, the generators’ intended word-end indices identify exactly which tap had Shift, even across an omitted separator or apostrophe. Shift changes the case of the actual letter hit, never the hit key or coordinates. All 12,000 references and noisy strings lowercase to the original fixtures. ID and contributor are from the [per-language detailed exports](https://downloads.tatoeba.org/exports/per_language/) (2026-11-21); `\N` means Tatoeba lists no owner. Each source is at `https://tatoeba.org/en/sentences/show/<ID>`. The accent replay takes the first 500 eligible references in the fixed test order. German originally excluded ß and only five Bokmål references contain é. For those two alone, [accent-de.tsv](held-out/accent-de.tsv) and [accent-nb.tsv](held-out/accent-nb.tsv) add 500 and 53 attributed references **only to the accent replay**, never the noisy/clean comparison or model input. From the same detailed export: retain 3–14 words in the benchmark alphabet (plus ß for German), require a hold-only letter and a listed contributor, normalize as the existing generator does, exclude reference text in that language's existing dev/test split, keep the first of each normalized sentence, then shuffle in export order with Python seed 2374 and take up to the remaining quota. All 53 qualifying Bokmål additions are included; its two unambiguous word occurrences skip the threshold. The two models were built from Google Books (de) and NB N-gram/Ordbank (nb), not these Tatoeba supplements.

**Polish** (`pl.tsv.gz`, same format) is made by [polish_set.py](polish_set.py) as [synthetic_sets.py](synthetic_sets.py) makes German's and Italian's: 2,300 sentences of 3–14 words in a–z and ą ć ę ł ń ó ś ź ż from Tatoeba's detailed Polish export (2026-11-22), shuffled with seed 23, the last 2,000 kept as the test split (10,735 reference words, 4,071 with a Polish letter), each tap sampled from the English benchmark's measured offsets (seed 20261125) on QWERTY at 39.5 pt, Hex's Polish letter rows. Polish's letters have no key of their own on Hex's iOS Polish layout (each is held on its base letter), so they are typed as their base key, ł as l (rerunning the script on a later export can pick other sentences: the committed file is the set); references keep the source's case and the contributor column is the export's. No model gates it yet: `quality-gate.py` gates the languages that have both a model and a set, so Polish's accent keys (ą→a, ć→c, ę→e, ł→l, ń→n, ó→o, ś→s, ź→z, ż→z) and critical cases ("to jest zolw" → "to jest żółw", "nie mam czasu", "bartek ma kota", "wiem ze tak" → "wiem że tak", "ide ze szkoly" → "idę ze szkoły") join [accent-keys.json](accent-keys.json) and [critical-cases.json](critical-cases.json) with the model. On today's iOS engine with a Polish model from `ios/scripts/build-typing-model.py pl` and that model's own words as the dictionary, the set measures 84.3% fewer errors than raw taps (58.28% raw WER), 0.25% of correct words changed and 97.2% of 864 unambiguous marks restored; without a model, −1.8% and 0%.

### The same gate on iOS

`ios/scripts/typing-gate.py` runs these held-out inputs through the iOS engine (`FullKeyboardEngine` with the language's bundled model and the simulator's own spelling dictionary, as the keyboard runs) and scores them exactly as `quality-gate.py` does: the same alignment, the same three numbers, the same thresholds and critical cases. It reads `held-out/`, `accent-keys.json`, `critical-cases.json` and `thresholds.json` here, so the two platforms can't drift apart. From `ios/`, after `xcodegen generate` and with a booted simulator:

```sh
python3 scripts/typing-gate.py run --destination 'id=<SIM_UDID>' en es de fr it
```

It writes `ios/build/typing-gate/input.json`, then runs the opt-in `HEXMobileTests/TypingGateReplayTests` once per language (one fresh process each, so one language's dictionary state never meets another's), and prints the same table as `quality-gate.py`. `typing-quality.json` there has the counts, critical outputs and failed checks. The test skips unless `HEX_TYPING_GATE` is set, so an ordinary `xcodebuild test` doesn't run it. `prepare` and `score` run the two halves separately.

- **Reduction:** the noisy taps were made on the synthetic layout above (402 pt; iOS's measured one for English and Spanish). Each tap is moved onto Hex's layout from the key it hit. Hex's rows are shorter, so a tap that would land outside Hex's letter keys or on 123, shift or delete goes to the nearest letter or space bar instead.
- **Correct words changed:** each reference is typed at the key centres. A letter without a key of its own is picked from its key's long-press menu, and an apostrophe without a key comes from the 123 page; French's apostrophe key (#2354) is tapped.
- **Accents restored:** each reference is typed with `accent-keys.json`'s substitutes and bucketed by `<lang>.hextm` and `<lang>.valid` as on Android. iOS ships no `.valid` lists yet, so the Android ones are read.
- **Critical cases:** typed at the key centres.
- **Capitals (iOS report, no threshold):** the three numbers ignore case, so iOS also types each reference all lowercase at the key centres, leaving the keyboard's shift as it is (it capitalises a sentence's start). After a sentence's first word, it counts the reference words with a capital (names, "I") that come out with it (**names kept**), and the words without one that gain one (**capital added**).

Outputs also drop the `. , ! ? ; :` and quotes the engine may add, and ’ reads as '. German reads its model as with its experiment on. Bokmål has no iOS model, so it types as the keyboard does without one. Simulator dictionaries aren't a phone's: iOS 27.2's `nb_NO` takes any letters for a word.

iOS numbers quoted elsewhere in this file (`ZZDecoderBench`, "iOS today") were made before the nearest-key rule, which moved them by 1–1.6 points of word error rate.

## Against today's iOS engine

The same 2,000 held-out English and Spanish sentences, typed by today's iOS engine (`origin/dev` 96b4f365, Spatial autocorrect on, iPhone 17 simulator, its taps moved onto today's layout key by key) and by the Android replay, which finishes each word with `TypingWord.finish` and moves each letter tap to the likelier next letter (`likelierKey`, touch areas tiling the rows) as the keyboard does. German and Bokmål have their own 2,000-sentence sets ([synthetic_sets.py](synthetic_sets.py)): held-out Tatoeba sentences typed with the same touch model on Android's eleven-key rows at a 36 pt pitch. iOS ships no Bokmål model and keeps German's behind an experiment, so those two have no iOS column.

| Engine | English errors (WER) | Spanish | German | Bokmål |
|---|---:|---:|---:|---:|
| iOS today | 798 (5.75%) | 1,205 (9.06%) | — | — |
| Android before the parity work | 1,028 (7.41%) | 1,374 (10.33%) | 825 (6.09%) | 692 (5.54%) |
| Aimed taps, accents from the base key, a space read on the bottom row | 913 (6.58%) | 1,373 (10.32%) | 812 (5.99%) | 678 (5.43%) |
| Words the context decides, the word before revised, a space tapped as a letter | 818 (5.90%) | 1,342 (10.09%) | 774 (5.71%) | 654 (5.24%) |
| Names kept as typed, and capitals from the books | 770 (5.55%) | 1,133 (8.52%) | 774 (5.71%) | 654 (5.24%) |
| The likelier next letter claims up to 5 dp of its neighbour's touch area | 721 (5.20%) | 1,090 (8.19%) | 729 (5.38%) | 647 (5.18%) |
| A missed space with a slip after it, or three or more words run together (iOS #2244; not German) | 707 (5.10%) | 1,058 (7.95%) | 729 (5.38%) | 631 (5.05%) |
| iOS at fc867484, with #2244 | 768 (5.54%) | 1,146 (8.62%) | — | — |
| Real words the books use outside the models (validity lists; English's SCOWL list unchanged) | 707 (5.10%) | 1,033 (7.77%) | 714 (5.27%) | 627 (5.02%) |

French and Italian have their own 2,000-sentence sets the same way (French on AZERTY, Italian on QWERTY; accented letters typed as their base key, elisions with their apostrophe). iOS types both with Apple's spelling dictionary and no typing model, and its harness maps only English and Spanish layouts, so there is no iOS column.

| French and Italian | French | Italian |
|---|---:|---:|
| Raw hit keys | 5,316 (38.91%) | 4,778 (39.73%) |
| Models from Google Books, words read after an elision | 1,177 (8.62%) | 895 (7.44%) |
| Accents the words around decide ("a"/"à", "e"/"è") | 947 (6.93%) | 789 (6.56%) |
| A missed space with a slip after it, or words run together | 939 (6.87%) | 763 (6.35%) |
| Real words the books use outside the models | 935 (6.84%) | 753 (6.26%) |
| French's apostrophe key read as the keyboard reads it (replay only) | 883 (6.46%) | |
| The apostrophe key likeliest right after an elided "l", "d", "qu" | 864 (6.32%) | 753 (6.26%) |

French has 13,661 reference words, Italian 12,025.

The replay also counts what the suggestion strip offers, as iOS's harness does: the intended word among the slots once its last letter is typed, after its first two letters (words of three letters or more), and among the next-word predictions after the space before it.

| Strip hits | English | Spanish | German | Bokmål |
|---|---:|---:|---:|---:|
| iOS today: word / next / after two letters | 13,253 / 2,287 / 3,235 | 12,364 / 2,073 / 2,901 | — | — |
| Android before the strip work | 12,070 / 2,338 / 3,789 | 11,603 / 2,116 / 2,606 | 12,204 / 1,679 / 5,026 | 11,172 / 2,160 / 4,380 |
| Android with `strip()` and the new predictions | 13,180 / 2,349 / 4,390 | 12,407 / 2,110 / 3,236 | 12,918 / 1,680 / 5,624 | 11,988 / 2,159 / 4,875 |
| … and the likelier next letter's touch area | 13,219 / 2,349 / 4,686 | 12,398 / 2,112 / 3,455 | 12,941 / 1,679 / 6,019 | 11,991 / 2,165 / 5,148 |
| … and a missed space with a slip, or words run together | 13,219 / 2,350 / 4,685 | 12,398 / 2,115 / 3,455 | 12,941 / 1,679 / 6,019 | 11,991 / 2,167 / 5,150 |
| … and the validity lists | 13,219 / 2,350 / 4,685 | 12,401 / 2,113 / 3,566 | 12,949 / 1,678 / 6,019 | 11,994 / 2,166 / 5,150 |
| iOS at fc867484, with #2244 | 13,253 / 2,289 / 3,234 | 12,364 / 2,076 / 2,901 | — | — |

French and Italian strip hits (word / next / after two letters): 11,979 / 2,272 / 4,248 of 13,661 / 11,430 / 9,950, and 11,028 / 1,712 / 3,638 of 12,025 / 9,788 / 8,981, with their own words for the strip between words (iPhone Hex's lists). A word typed with its elision ("l'homme") can't be a strip hit once its apostrophe is typed: the strip then reads "homme".

The replay types French's apostrophe as the keyboard does: the AZERTY key sits on the letter rows, so a touch on it goes to the likelier neighbour as a letter's does ("u'e" -> "une"), and a touch near it can type it ("jnetais" -> "j'étais"). Right after an elided article or pronoun the apostrophe counts as the likeliest key (`TypingWord.likelierKey`): the books split "l'" and "d'" off, so the word list alone would send "l'" to "ln".

**End-to-end check.** The same taps typed through the installed keyboard into a real text field (API 35 emulator, an outside test app, touches injected at the benchmark's points scaled onto the keyboard's own keys) land where the replay says, once the replay is given the keyboard's measured key centres and touch cells (`Replay MODEL SET LANG OUT GEOMETRY.json`). Of 300 English, 300 Spanish, 300 German, 200 Bokmål, 200 French and 200 Italian sentences, the differences are the keyboard's double space putting in ". " (the synthetic typist sometimes taps space twice), twice a sentence's first or second tap lost (not seen again on retyping), and French: the keyboard measures AZERTY's pitches from q, w and a, which sit on other rows there (160 × −155 px instead of 107 × 155). Given those same pitches, the replay matches 198 of 200 French sentences.

Out of 13,870 / 11,639 / 10,874 (English), 13,301 / 11,058 / 9,383 (Spanish), 13,550 / 11,326 / 12,413 (German) and 12,492 / 10,297 / 9,789 (Bokmål).

## Same-model results

Each language has 2,000 held-out synthetic Tatoeba sentences. English has 13,870 reference words; Spanish has 13,301. Errors are word insertions, deletions and substitutions, ignoring case and terminal punctuation. A missing space can change the word count. No user-specific offsets are applied.

| Decoder | English errors | English WER | English exact / 2,000 | Spanish errors | Spanish WER | Spanish exact / 2,000 |
|---|---:|---:|---:|---:|---:|---:|
| Raw hit keys | 4,689 | 33.8068% | 160 | 5,343 | 40.1699% | 94 |
| Phase 3 Android | 1,461 | 10.5335% | 983 | 1,338 | 10.0594% | 1,086 |
| iOS R1, same HXTM, no offsets | 1,163 | 8.3850% | 1,154 | 1,387 | 10.4278% | 1,064 |
| Phase 4 Android | 1,027 | 7.4045% | 1,249 | 1,374 | 10.3301% | 1,070 |
| **Android, default suggestion filter** | **1,028** | **7.4117%** | **1,248** | **1,374** | **10.3301%** | **1,070** |

The before/after replay used the installed APKs on the same Studio API 35 emulator:
one additional English error out of 13,870 reference words (+0.0072 percentage points)
and no change across 13,301 Spanish reference words.

The installed APK ran on the Studio’s Android 15/API 35 ARM64 emulator (4 vCPUs, 4 GiB guest RAM). It received the original 402-point iOS key geometry, hit-key sequence, coordinates and separators. Letters request suggestions and spaces request replacements and predictions, exactly as in the Android IME. English and Spanish HXTM files are byte-identical across platforms. The iOS reference is the recorded `output-r1-decoder-{en,es}.json` output, before calibration. The earlier iOS prototype’s 6.71% English WER used a **different** model; it is not the comparison target. [Phase 3 measurements](metrics.tsv) · [historical iOS aggregate, including calibration](ios-shared.tsv) · [Phase 4 replay counts and latency](phase4.tsv).

## Where the 298 English errors came from

We changed one decision rule at a time on the same 2,000 English inputs. The diagnostic runner used scratch copies of the Kotlin classes on Android. Its phase 3 and phase 4 error and agreement counts matched the installed APKs. Deltas below are conditional on the preceding rows; they are not independent effects.

| Cumulative change | Errors | Δ errors | Normalized outputs matching iOS / 2,000 |
|---|---:|---:|---:|
| Phase 3 | 1,461 | — | 1,517 |
| Apply iOS’s English contractions/common typos before search | 1,230 | −231 | 1,724 |
| Increase English word/pair weight from 1.25 to 1.5 | 1,255 | +25 | 1,740 |
| Score only the five best whole-word spatial readings | 1,207 | −48 | 1,762 |
| Use R1’s 0.26-row-pitch vertical spread on the recorded layout | 1,186 | −21 | 1,832 |
| Use R1’s 6.0 missed-space penalty | 1,196 | +10 | 1,864 |
| **Protect valid English spellings outside HXTM (phase 4)** | **1,027** | **−169** | **1,773** |
| Diagnostic only: fixed Android 3.9 split penalty even at iOS pitches | 1,007 | −20 | 1,751 |

The R1 scoring and correction order alone closed **265 of the 298** errors. The remaining 33 were a net WER difference, not 33 sentences with one known cause. The intermediate decoders disagreed on 138 alignment spans; in 54, iOS retained a literal absent from HXTM. iOS can ask `UITextChecker` whether such a spelling is valid. Android had treated every missing spelling as a nonword. It now uses the same valid-literal cost (15 instead of 22) and replacement margin (7 instead of 2), with a bundled public SCOWL list. That list is not Apple’s dictionary. Lower WER is not identical decisions: the final engines produce the same normalized word sequence on 1,773 English and 1,906 Spanish sentences. Spanish made 36 more errors than phase 3 but agreed with iOS on 84 more sentences and still beat the same-model iOS WER.

The production IME scores **actual** key centres. Android’s rows are shorter than the test layout, so its vertical spread has a floor based on the iOS reference aspect ratio (56 / 39.5). This floor is inactive on the replay geometry. A fixed 6.0 split penalty ranked “seeks” ahead of “see how” for the real centre taps for `seehkw`. The IME now scales that squared-distance cost with row height, floored at the existing Android 3.9 prior (2% omitted separators). The measured Android pitch was 106.2 × 131 px, giving a cost of about 4.5; the recorded iOS layout uses exactly 6.0. This keeps the iOS behavior on its geometry and fixes the Android regression without a sentence-specific exception. The external editor received `we need to do research into this and see how well we can get it` from `we need tk dk research into this and seehkw well we can get it`. Tapping space after `we need tk` produced `we need to `; one backspace restored `we need tk`.

## Checks in an external editor

The Studio sent touch events to Hex’s visible keys and suggestion strip, then read the separate editor’s text. With Wi-Fi off and airplane mode on:

| Layout | Typed and completed with a model suggestion | Space-triggered correction |
|---|---|---|
| Español | `Nos vemos mañan` → **Nos vemos mañana**; ¿ and ¡ inserted from the second symbols page | `nos vemos mañnaa` → **nos vemos mañana** |
| Deutsch | `Bären mögen süße Äpfe` → **Bären mögen süße Äpfel**; held **s** and slid to **ß** | `vielen dnak` → **vielen dank** |
| Norsk (Bokmål) | `Jeg lærer norsk på lørda` → **Jeg lærer norsk på lørdag**; next words included **kveld**, **morgen** and **og** | `tusen takkk` → **tusen takk** |

Automatic on a fresh install displayed English, followed the system locale to Español, and returned to English when the system did. With layout pinned to Deutsch and dictation set to Español, the keyboard kept the German letters and showed Español on the space bar.

## Models and licences

All four HXTM v3 assets are uncompressed and memory-mapped from the APK; Gradle verifies the [pinned hashes](../../app/src/main/assets/typing/SHA256SUMS). There is no SymSpell data. German and Bokmål have the same binary format as the iOS files; the Studio checked them with the Swift model reader as well as the Android IME. The two new files are also in the iOS checkout’s `ios/HEXKeyboard/TypingModel/` folder, local commit `230a437e`. The Swift reader round-tripped every spelling and every stored pair cost. This verifies file compatibility, not iOS device behavior.

| Language | Words | Word pairs | Source of counts | Source of spelling |
|---|---:|---:|---|---|
| English | 86,247 | 417,182 | Google Books v3, 24 unigram / 589 bigram shards | SCOWL 2020.12.07, plus contractions and Hex's own [chat and tech words](en-extra-words.tsv) |
| Spanish | 110,000 | 326,366 | Google Books v3, 3 / 73 shards; existing model unchanged | RLA-ES Hunspell |
| German | 95,000 | 368,534 | Google Books v3, all 8 / 181 German shards | German alphabetic corpus spellings |
| Bokmål | 95,000 | 370,560 | NB N-gram 2026 book unigrams and bigrams, language `nob` | Norsk ordbank, Bokmål full forms |
| French | 99,974 | 380,439 | Google Books v3, all 6 / 115 French shards | French corpus spellings, with the books' elided forms |
| Italian | 99,996 | 371,292 | Google Books v3, all 2 / 60 Italian shards | Italian corpus spellings, with the books' elided forms |

English was rebuilt with the shared builder from the same 24 unigram and 589 bigram shards and SCOWL. Run with the earlier rules, the same counts reproduce the earlier `en.hextm` byte for byte (SHA-256 `14370569…c4028`), so the new file (`c23e9c54…df30`) differs only by two additions. Google Books splits "don't" into "do" and "n't", so its own count of the contraction is tiny ("don't" cost 14.4 nats, "it's" 16.2, and SCOWL lists "I'm", "I'll", "I've" and "I'd" only with a capital, so they were missing): each contraction in the builder's `CONTRACTIONS` now counts as often as its expansion's pair ("do not") when that is commoner, and the missing ones are added in lowercase. And [en-extra-words.tsv](en-extra-words.tsv), 166 chat and tech words written for Hex ("app", "lol", "csv"), joins the vocabulary at its tier's cost (10, 12 or 13.5 nats), a word the books have only getting commoner. Both join before pairs are counted. `en.valid` and `en.capitals` are rebuilt from the new model (their builders also reproduce the earlier files from the earlier model); `build-capitals.py` holds Hex's own words at 94% like SCOWL's lowercase words, so "plz" and "okie" are never capitalised. English gate before → after: iOS (`ios/scripts/typing-gate.py`) 773 → 734 word errors (83.5% → 84.4% fewer than raw taps, correct words changed 0.27% → 0.26%), Android (`quality-gate.py`) 718 → 680 (84.7% → 85.5%, 0.32% both), critical cases passing.

French and Italian go through the same German adapter: every untagged shard of the language, 1980–2019, spellings from the corpus alone. The books keep many elisions as one token ("c'est", "qu'il", Italian "dell'anno"; French "l'" and "d'" are split off), so those stay words with their apostrophe, which the decoder reads without it being typed. The same letters run together without the apostrophe, ten times rarer ("cest", "quil", 26 French and 4 Italian forms), are left out so space puts it back. A word typed right after an elided article or pronoun ("l'", "qu'", "dell'") is read as a word of its own (`TypingWord.startsWord`).

The [shared iOS builder](build-typing-model.py), SHA-256 `2442a46ca46145f7d48efc8e73015299e7d60010810a8916239f170024a80446`, sums counts from 1980–2019, selects informative pairs and writes quantized probabilities. The [German/Bokmål adapter](build-layout-model.py) uses the same writer. For German, it uses the iOS shard reader and filters lowercased words to its alphabet. For Bokmål, it NFC-normalizes and lowercases the words, filters vocabulary to standard Bokmål forms in the 1 February 2022 Ordbank `fullformsliste.txt` (`OPPSLAG`, normative and current in that snapshot), and only retains pairs whose two words are in that vocabulary. The September 2026 inflection export alone dropped common uninflected words such as `og`, `på` and `det`; it was not used in the shipped model. The complete snapshot supplied 617,139 distinct forms. Of 20,096,783 observed in-vocabulary pair types, 394,825 met the shared writer’s count and information thresholds; 370,560 fit the model budget.

- [Google Books Ngram v3 (2020-02-17)](https://storage.googleapis.com/books/ngrams/books/datasetsv3.html): **CC BY 3.0**. Spanish keeps the original shared build; English was rebuilt from the same shards (below). German uses no second dictionary.
- [NB N-gram 2026](https://data.norge.no/nb/datasets/f2eebe8b-bd83-34e4-bcf3-c0c5e37a77f8/n-grammer-fra-nasjonalbiblioteket-2026): the National Library of Norway’s counts are **CC0 1.0**. The [Norsk ordbank – Bokmål 2005, February 2022](https://data.norge.no/en/datasets/f0db926d-a154-3a12-82b8-6573b57f6dbb/norsk-ordbank-norwegian-bokmal-2005) spelling filter is separately **CC BY 4.0**.
- [SCOWL](https://wordlist.aspell.net/) permits commercial redistribution subject to its [included notices](../../app/src/main/assets/licenses/scowl.txt). The English literal-validity list contains 45,303 unique spellings absent from the frequency model (440,142 bytes). It is built from the same public SCOWL archive, not from benchmark outputs or personal data. It cannot supply scored candidates or word probabilities.
- [RLA-ES es_ES/es_MX sources](https://github.com/LibreOffice/dictionaries/tree/762abe74008b94b2ff06db6f4024b59a8254c467/es) use their **MPL 1.1** option. [APK credits, source links and modification notices](../../app/src/main/assets/licenses/credits.txt) · [full licence texts](../../app/src/main/assets/licenses/).

**Capitals lists** (`en.capitals`, `es.capitals`, `de.capitals`) record how often the books write a word with a capital first letter, from the same Google Books v3 1-grams (1980–2019) as each model. A word typed in lowercase takes its capital when the books nearly always write it so (95%), as iOS does where its dictionary knows a word only capitalised: "mary" → "Mary", "monday" → "Monday", German "haus" → "Haus". A word that is also an ordinary lowercase word is held below that (English: one SCOWL spells in lowercase, such as "united" or "china"; Spanish: a model word under 98%). The English and Spanish lists also hold names outside the model, three letters or more, capitalised 90% of the time and used at least once per million words ("tom" in Spanish, "sami" in English): typed as they are, they count as real words, as iOS's spelling dictionary takes them, instead of being corrected into "toma" or "swami". Two model words run together whose pair is ten times as common ("iwant", "apartir") are left out. German's list is iOS's `de.capitals` (#2169), byte for byte; [build-capitals.py](build-capitals.py) builds the other two (en: 10,984 words, 121 KB; es: 7,586 words, 85 KB).

**Validity lists** (`<lang>.valid`) are what Android has for iOS's spelling dictionary: spellings outside the model that are still real words. English's is SCOWL (above). Spanish, German, French and Italian take every spelling the same Google Books v3 1-grams (1980–2019) use at least a cut-off number of times, in lowercase or with a capital; Bokmål takes Norsk ordbank's standard full forms that the NB N-gram 2026 books use at least that often. Left out: two model words run together whose pair is ten times as common ("apartir"), a French or Italian elision without its apostrophe ("cest"), and a model word with an accent changed or left out ("volvere", "brulant"), which books misspell as typists do. Each line carries the word's cost from its count, in eighths of a nat, as the model stores its own words'. The decoder reads a listed word at that cost instead of the 22 nats of a spelling nobody knows: a one-word fix still needs only the 2-nat margin, but splitting it into words needs a known word's 7, and the missed-space fallback leaves it alone. A list without costs (English) keeps the flat known-word reading (15 nats, 7-nat margin). [build-valid-words.py](build-valid-words.py) builds all six.

The cut-off was chosen per language on both counts below, not on word errors alone: real words outside the model typed exactly that are now kept, and typos that are list words now wrongly kept. Errors are the replay's; the APK stores each list uncompressed.

| Language | Cut-off (uses, 1980–2019) | Spellings | Adds to the APK | Word errors | Rare words typed exactly, kept | Typos wrongly kept |
|---|---:|---:|---:|---:|---:|---:|
| Spanish | 3,000 | 99,228 | 1.29 MB | 1,058 → 1,033 | 228 → 242 of 262 | 3 of 594 listed typos |
| German | 10,000 | 132,272 | 2.14 MB | 729 → 714 | 83 → 94 of 107 | 3 of 199 |
| Bokmål | 1,000 | 51,437 | 0.84 MB | 631 → 627 | 35 → 39 of 104 | 1 of 13 |
| French | 3,000 | 108,519 | 1.45 MB | 939 → 935 | 201 → 201 of 211 | 0 of 121 |
| Italian | 3,000 | 80,990 | 1.11 MB | 763 → 753 | 82 → 89 of 99 | 0 of 96 |

Lower cut-offs kept a few more rare words for far more bytes (Spanish at 1,000: 1,030 errors, 246 kept, 3.5 MB; German at 3,000: 712, 96, 6.9 MB; Italian at 1,000: 748, 95, 3.0 MB; Bokmål at 100: 625, 41, 3.5 MB). A flat known-word reading for every listed word, as iOS gives its dictionary's, made Spanish 1,058 → 1,227 (cut-off 100): books use many typos and unaccented forms often enough to list them.

The separate [never-suggest list](../../app/src/main/assets/typing/never-suggest.tsv) is
adapted from Shutterstock's [LDNOOBW revision 5faf2ba](https://github.com/LDNOOBW/List-of-Dirty-Naughty-Obscene-and-Otherwise-Bad-Words/tree/5faf2ba42d7b1c0977169ec3611df25a3c08eb13),
**CC BY 4.0**. Hex reviewed the English, Spanish, German and Norwegian lists for keyboard use:

- Keep identity slurs always suppressed. Common profanity and obscene slang are opt-in.
  Add common missing forms explicitly, including plurals, verb inflections and spelling
  variants. Match whole words; *assume* and *fuchsia* remain eligible completions.
- Keep ordinary anatomy and health terms available: *anus, clitoris, fecal, genitals,
  nipple, penis, rectum, semen, vagina, vulva*. Also omit ordinary senses such as
  *escort, fingering, hardcore, scat, snatch, suck* and neutral sexuality or reporting
  terms such as *sex, sexuality, rape*. Search-site phrases such as *jelly donut* or
  *tied up* do not become keyboard bans. Retained two-word expressions match as pairs.
- Apply each language's list to its own layout, plus unambiguous shared loanwords.
  Spanish *negro* (black), German *dick* (thick), and Bokmål *fag* (subject) remain
  available. An unscored listed spelling is protected when typed just like a model word.

## Reproduce on the Studio

1. Export only the allowlisted synthetic IDs **where the iOS inputs already reside**. The exporter accepts `test-0…1999` (English) or `es-test-0…1999` (Spanish), empty context, no logged flag, and key-aligned finite x/y pairs. It exports only ID, intended text, synthetic hit keys/taps and the matching uncalibrated iOS output. Never transfer the mixed source, labels or personal offsets.

   ```sh
   mkdir -p "$T/bench"
   for lang in en es; do
     python3 benchmark/typing/export_synthetic.py "$IOS_BENCH/input.json" \
       "$IOS_BENCH/output-r1-decoder-$lang.json" "$lang" > "$T/bench/$lang.jsonl"
   done
   ```

2. In the synced Android project with JDK 21, Android SDK 35 and the designated emulator:

   ```sh
   ./gradlew assembleDebug test
   adb install --no-incremental -r app/build/outputs/apk/debug/app-debug.apk
   sh benchmark/typing/replay.sh
   ```

   For German, Bokmål, French and Italian, put Tatoeba's `deu.tsv`, `nob.tsv`, `fra.tsv` and `ita.tsv` in `$T/tatoeba` and run `python3 benchmark/typing/synthetic_sets.py "$IOS_BENCH/input.json" "$T/tatoeba" "$T/bench"` first; the replay runs every set it finds and writes each sentence's output to `$T/outputs`.

   The Java replay shim executes the installed APK’s Kotlin classes with Android’s `app_process`. The output is aggregate WER, normalized sentences matching iOS and per-key timing (median/p95/max). Timings exclude loading, JSON parsing, evaluation, editor IPC and screen paint. Phase 3 timed the separate editor’s text-change callback from key-up (letter median/p95: 1.409/2.589 ms; space: 3.161/4.428 ms). The Phase 4 replay measures decoder work, not that end-to-end path or a physical handset.

3. To rebuild the new typing models, use Python 3.9+, `curl`, `gzip` and `zstd` with scratch storage. German streams each shard and caches its counts. The Bokmål adapter resumes the two large NB archives in validated byte ranges and verifies their SHA-256 alongside the Ordbank snapshot. The SCOWL English validity list has a separate pinned-source generator:

   ```sh
   mkdir -p "$T/models" "$T/source"
   python3 benchmark/typing/build-typing-model.py en "$T/source/en" "$T/models/en.hextm"
   python3 benchmark/typing/build-layout-model.py de "$T/source/de" "$T/models/de.hextm"
   python3 benchmark/typing/build-layout-model.py nb "$T/source/nb" "$T/models/nb.hextm"
   python3 benchmark/typing/build-layout-model.py fr "$T/source/fr" "$T/models/fr.hextm"
   python3 benchmark/typing/build-layout-model.py it "$T/source/it" "$T/models/it.hextm"
   curl -fsSL https://downloads.sourceforge.net/project/wordlist/SCOWL/2020.12.07/scowl-2020.12.07.tar.gz -o "$T/source/scowl.tar.gz"
   python3 benchmark/typing/build-valid-words.py en "$T/source/scowl.tar.gz" \
     app/src/main/assets/typing/en.hextm "$T/models/en.valid"
   for lang in es de fr it; do  # the counts build-capitals.py caches; German's are counted here first
     python3 benchmark/typing/build-valid-words.py $lang app/src/main/assets/typing/$lang.hextm "$T/capitals/$lang" "$T/models/$lang.valid"
   done
   python3 benchmark/typing/build-valid-words.py nb app/src/main/assets/typing/nb.hextm "$T/source/nb-valid" "$T/models/nb.valid"
   python3 benchmark/typing/build-capitals.py en app/src/main/assets/typing/en.hextm "$T/capitals/en" \
     "$T/models/en.capitals" "$T/source/scowl.tar.gz"
   for lang in es fr it; do
     python3 benchmark/typing/build-capitals.py $lang app/src/main/assets/typing/$lang.hextm "$T/capitals/$lang" "$T/models/$lang.capitals"
   done
   ```
