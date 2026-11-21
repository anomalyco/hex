# Android typing benchmark

**On the same iOS R1 word models and synthetic taps, Android with its default suggestion filter has 135 fewer English word errors and 13 fewer Spanish word errors than iOS.** The biggest part of the old 298-error English deficit was a missing step before spatial decoding: porting the English contraction and typo rules alone recovered 231 errors.

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

French and Italian have their own 2,000-sentence sets the same way (French on AZERTY, Italian on QWERTY; accented letters typed as their base key, elisions with their apostrophe). iOS types both with Apple's spelling dictionary and no typing model, and its harness maps only English and Spanish layouts, so there is no iOS column.

| French and Italian | French | Italian |
|---|---:|---:|
| Raw hit keys | 5,316 (38.91%) | 4,778 (39.73%) |
| Models from Google Books, words read after an elision | 1,177 (8.62%) | 895 (7.44%) |
| Accents the words around decide ("a"/"à", "e"/"è") | 947 (6.93%) | 789 (6.56%) |
| A missed space with a slip after it, or words run together | 939 (6.87%) | 763 (6.35%) |

French has 13,661 reference words, Italian 12,025.

The replay also counts what the suggestion strip offers, as iOS's harness does: the intended word among the slots once its last letter is typed, after its first two letters (words of three letters or more), and among the next-word predictions after the space before it.

| Strip hits | English | Spanish | German | Bokmål |
|---|---:|---:|---:|---:|
| iOS today: word / next / after two letters | 13,253 / 2,287 / 3,235 | 12,364 / 2,073 / 2,901 | — | — |
| Android before the strip work | 12,070 / 2,338 / 3,789 | 11,603 / 2,116 / 2,606 | 12,204 / 1,679 / 5,026 | 11,172 / 2,160 / 4,380 |
| Android with `strip()` and the new predictions | 13,180 / 2,349 / 4,390 | 12,407 / 2,110 / 3,236 | 12,918 / 1,680 / 5,624 | 11,988 / 2,159 / 4,875 |
| … and the likelier next letter's touch area | 13,219 / 2,349 / 4,686 | 12,398 / 2,112 / 3,455 | 12,941 / 1,679 / 6,019 | 11,991 / 2,165 / 5,148 |
| … and a missed space with a slip, or words run together | 13,219 / 2,350 / 4,685 | 12,398 / 2,115 / 3,455 | 12,941 / 1,679 / 6,019 | 11,991 / 2,167 / 5,150 |
| iOS at fc867484, with #2244 | 13,253 / 2,289 / 3,234 | 12,364 / 2,076 / 2,901 | — | — |

French and Italian strip hits (word / next / after two letters): 11,935 / 2,188 / 4,230 of 13,661 / 11,430 / 9,950, and 11,022 / 1,662 / 3,638 of 12,025 / 9,788 / 8,981.

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
| English | 86,133 | 417,480 | Google Books v3, 24 unigram / 589 bigram shards | SCOWL 2020.12.07 |
| Spanish | 110,000 | 326,366 | Google Books v3, 3 / 73 shards; existing model unchanged | RLA-ES Hunspell |
| German | 95,000 | 368,534 | Google Books v3, all 8 / 181 German shards | German alphabetic corpus spellings |
| Bokmål | 95,000 | 370,560 | NB N-gram 2026 book unigrams and bigrams, language `nob` | Norsk ordbank, Bokmål full forms |
| French | 99,974 | 380,439 | Google Books v3, all 6 / 115 French shards | French corpus spellings, with the books' elided forms |
| Italian | 99,996 | 371,292 | Google Books v3, all 2 / 60 Italian shards | Italian corpus spellings, with the books' elided forms |

French and Italian go through the same German adapter: every untagged shard of the language, 1980–2019, spellings from the corpus alone. The books keep many elisions as one token ("c'est", "qu'il", Italian "dell'anno"; French "l'" and "d'" are split off), so those stay words with their apostrophe, which the decoder reads without it being typed. The same letters run together without the apostrophe, ten times rarer ("cest", "quil", 26 French and 4 Italian forms), are left out so space puts it back. A word typed right after an elided article or pronoun ("l'", "qu'", "dell'") is read as a word of its own (`TypingWord.startsWord`).

The [shared iOS builder](build-typing-model.py), SHA-256 `cba009fe227632d9f8d176055f26ca34d5eb6f88a7cbd947955d8f9142878317`, sums counts from 1980–2019, selects informative pairs and writes quantized probabilities. The [German/Bokmål adapter](build-layout-model.py) uses the same writer. For German, it uses the iOS shard reader and filters lowercased words to its alphabet. For Bokmål, it NFC-normalizes and lowercases the words, filters vocabulary to standard Bokmål forms in the 1 February 2022 Ordbank `fullformsliste.txt` (`OPPSLAG`, normative and current in that snapshot), and only retains pairs whose two words are in that vocabulary. The September 2026 inflection export alone dropped common uninflected words such as `og`, `på` and `det`; it was not used in the shipped model. The complete snapshot supplied 617,139 distinct forms. Of 20,096,783 observed in-vocabulary pair types, 394,825 met the shared writer’s count and information thresholds; 370,560 fit the model budget.

- [Google Books Ngram v3 (2020-02-17)](https://storage.googleapis.com/books/ngrams/books/datasetsv3.html): **CC BY 3.0**. English and Spanish keep the original shared builds. German uses no second dictionary.
- [NB N-gram 2026](https://data.norge.no/nb/datasets/f2eebe8b-bd83-34e4-bcf3-c0c5e37a77f8/n-grammer-fra-nasjonalbiblioteket-2026): the National Library of Norway’s counts are **CC0 1.0**. The [Norsk ordbank – Bokmål 2005, February 2022](https://data.norge.no/en/datasets/f0db926d-a154-3a12-82b8-6573b57f6dbb/norsk-ordbank-norwegian-bokmal-2005) spelling filter is separately **CC BY 4.0**.
- [SCOWL](https://wordlist.aspell.net/) permits commercial redistribution subject to its [included notices](../../app/src/main/assets/licenses/scowl.txt). The English literal-validity list contains 45,303 unique spellings absent from the frequency model (440,142 bytes). It is built from the same public SCOWL archive, not from benchmark outputs or personal data. It cannot supply scored candidates or word probabilities.
- [RLA-ES es_ES/es_MX sources](https://github.com/LibreOffice/dictionaries/tree/762abe74008b94b2ff06db6f4024b59a8254c467/es) use their **MPL 1.1** option. [APK credits, source links and modification notices](../../app/src/main/assets/licenses/credits.txt) · [full licence texts](../../app/src/main/assets/licenses/).

**Capitals lists** (`en.capitals`, `es.capitals`, `de.capitals`) record how often the books write a word with a capital first letter, from the same Google Books v3 1-grams (1980–2019) as each model. A word typed in lowercase takes its capital when the books nearly always write it so (95%), as iOS does where its dictionary knows a word only capitalised: "mary" → "Mary", "monday" → "Monday", German "haus" → "Haus". A word that is also an ordinary lowercase word is held below that (English: one SCOWL spells in lowercase, such as "united" or "china"; Spanish: a model word under 98%). The English and Spanish lists also hold names outside the model, three letters or more, capitalised 90% of the time and used at least once per million words ("tom" in Spanish, "sami" in English): typed as they are, they count as real words, as iOS's spelling dictionary takes them, instead of being corrected into "toma" or "swami". Two model words run together whose pair is ten times as common ("iwant", "apartir") are left out. German's list is iOS's `de.capitals` (#2169), byte for byte; [build-capitals.py](build-capitals.py) builds the other two (en: 10,984 words, 121 KB; es: 7,586 words, 85 KB).

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
   python3 benchmark/typing/build-layout-model.py de "$T/source/de" "$T/models/de.hextm"
   python3 benchmark/typing/build-layout-model.py nb "$T/source/nb" "$T/models/nb.hextm"
   python3 benchmark/typing/build-layout-model.py fr "$T/source/fr" "$T/models/fr.hextm"
   python3 benchmark/typing/build-layout-model.py it "$T/source/it" "$T/models/it.hextm"
   curl -fsSL https://downloads.sourceforge.net/project/wordlist/SCOWL/2020.12.07/scowl-2020.12.07.tar.gz -o "$T/source/scowl.tar.gz"
   python3 benchmark/typing/build-valid-words.py "$T/source/scowl.tar.gz" \
     app/src/main/assets/typing/en.hextm "$T/models/en.valid"
   python3 benchmark/typing/build-capitals.py en app/src/main/assets/typing/en.hextm "$T/capitals/en" \
     "$T/models/en.capitals" "$T/source/scowl.tar.gz"
   for lang in es fr it; do
     python3 benchmark/typing/build-capitals.py $lang app/src/main/assets/typing/$lang.hextm "$T/capitals/$lang" "$T/models/$lang.capitals"
   done
   ```
