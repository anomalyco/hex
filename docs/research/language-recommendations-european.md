# Per-language model recommendations: English and European languages

Researched 2026-10-22. Builds on `model-watch.md` (parakeet-ultra, Moonshine,
Nemotron are not re-argued here). Scope: English, the 25 Parakeet v3 languages
in `PARAKEET_V3_LANGUAGES`, and the two other European entries Hex lists
(Turkish `tr`, Macedonian `mk`, which fall into the Qwen branch today).

## Verdict

```ts
English
├── Recommended: keep Parakeet Unified. It is the only Hex English model that streams,
│   and its offline accuracy is at least on par with ultra (FLEURS en 3.99 vs 4.26)
├── Most accurate: Cohere's lead is real only on LibriSpeech clean (1.27 vs 1.60/1.80/1.91)
│   and the Open ASR leaderboard (6.26 vs 6.51 v3); on FLEURS en it is the worst of the set (5.08)
│   └── Qwen3-ASR 1.7B beats it on both FLEURS en (3.23) and the leaderboard (5.75): evaluate as the replacement
├── parakeet-ultra for English: no. Same size, but offline-only, so it loses the streaming pipeline
└── Canary 1B v2 for English: no. 4.47 FLEURS, 1.14 GB, half the speed, no streaming

European (Parakeet v3 set)
├── Recommended: parakeet-ultra replaces v3 everywhere except Greek (lower WER in all 25, same size and speed)
├── Most accurate: Canary 1B v2 where it beats ultra by more than 1.5 points (12 languages)
├── Greek: Parakeet is broken for it (32 to 35% WER). Use Cohere (8.96) or Whisper turbo (13.26)
└── Turkish and Macedonian: Whisper turbo beats Qwen3-ASR 0.6B by 2.4x and 2x. Switch Recommended
```

Two findings matter more than the ultra swap:

1. **Greek is routed to a model that gets one word in three wrong.** v3 35.33%,
   ultra 32.25%, Canary v2 26.02%, Whisper turbo 13.26%, Cohere 8.96% (FLEURS).
2. **Turkish and Macedonian are routed to Qwen3-ASR 0.6B** at 16.74% and 35.09%
   against Whisper turbo's 6.97% and 17.85%.

## Evidence and its limits

Almost all per-language numbers are FLEURS test WER on Q8_0 GGUFs, from the
model cards in `handy-computer/transcribe.cpp` `docs/models/*.md`, measured by
the same project with one pipeline. That is the best like-for-like data
available, but:

- **It is not the HF Open ASR Leaderboard.** The multilingual tracks (FLEURS,
  Common Voice, MLS) are not in `hf-audio/open-asr-leaderboard-results`; that
  dataset only publishes `english_short_latest.csv`. So I could not use a
  leaderboard multilingual ranking. English leaderboard numbers below come from
  that CSV (via `model-watch.md`).
- **FLEURS is ~860 read utterances per language, one run.** Treat gaps under
  about 0.7 points absolute (or 10% relative) as ties.
- **Canary 1B v2 was measured at an earlier commit (2026-05-08)** than ultra
  (2026-10-05). The runs should be comparable, but I did not rerun them.
- Greek and Maltese scores depend heavily on text normalization; Whisper's very
  high Maltese error (70.92%) probably includes script and tokenization issues,
  but it is what Hex users would see.
- Moondream's own card claims 1.41% on LibriSpeech clean for ultra
  (Open ASR normalizer); transcribe.cpp measures 1.80% with kestrel. The
  FLEURS table is transcribe.cpp's, not vendor-only. The 1.41% is vendor-only.
- Voxtral Mini 3B, Qwen3-ASR 1.7B, Granite 4.1 numbers are likewise
  transcribe.cpp measurements, not vendor claims.

## Runtime constraints (transcribe.cpp)

```ts
model                    license        GGUF Q8   speed M4 Max Metal*   languages           hints            streams   crate
Parakeet Unified EN      NVIDIA OML     731 MB    188x                  en                  none             yes       0.2.4
Parakeet v3              CC-BY-4.0      740 MB    182x                  25 EU               none             no        0.2.4
parakeet-ultra           CC-BY-4.0      740 MB    207x                  same 25             none             no        0.4.0 (see below)
Canary 1B v2             CC-BY-4.0      1.14 GB   105x                  same 25             --prefix only    no        0.2.4 (arch/canary present)
Canary 1B flash          CC-BY-4.0      -         118x                  en de es fr         prefix           no        0.2.4
Whisper large-v3-turbo   MIT            886 MB    36x                   ~100 (list below)   vocabulary+prompt no       0.2.4 (Hex ships it)
Cohere Transcribe        Apache-2.0     2.41 GB   77x                   14 (8 are EU)       none             no        0.2.4 (Hex ships it)
Qwen3-ASR 1.7B           Apache-2.0     ~2 GB     45x                   30                  context (unverified in Hex)  no  0.2.4 (card exists)
Qwen3-ASR 0.6B           Apache-2.0     850 MB    75x                   30                  none in Hex       no        0.2.4
Voxtral Mini 3B 2507     Apache-2.0     large     13x                   7 EU                 -                no        0.2.4 (arch/voxtral)
Granite Speech 4.1 2B    Apache-2.0     -         76x                   en de es fr pt      keyword biasing (upstream)  no   newer than 0.2.4
Nemotron 3.5 streaming   OpenMDW-1.1    751 MB    144x                  32                  prompt           yes       0.2.4
```

\* jfk 11 s clip, transcribe.cpp cards. Whisper's 36x is the card's number; Hex's
catalog says ~19x, so Hex's own measurement is more conservative.

- **parakeet-ultra needs a newer crate than Hex's pin.** The port landed in
  transcribe.cpp commit `37273409` (2026-10-05), after v0.3.1 (10-04), so
  v0.4.0 (10-12) is the first release that can contain it; 0.2.4's `parakeet`
  arch has no voice-activity segmentation. A 0.4.0+ bump is required for the
  long-audio behavior and is the safe assumption for the GGUF at all. 0.4.1 is
  current. `orukeet` (CC-BY-SA-4.0, a share-alike license; not recommended) and
  `parakeet-primeline` (German) are also parakeet fine-tunes.
- **Hints.** Only Whisper has vocabulary and prompt biasing in the GGUF path,
  which is why it stays the Recognition hints pick for every language. Canary
  v2 takes a transcript prefix, which is not vocabulary biasing.
- **Streaming.** No multilingual model is worth streaming. Nemotron 3.5 streams
  but is far worse than ultra (de 10.33 vs 4.58, fr 10.78 vs 4.66, English
  regressed on the leaderboard to 10.51 vs 6.89). Voxtral Realtime is 4.7 GB
  at 6-7x realtime. Multilingual dictation stays non-streaming.
- **Canary needs an explicit language.** Hex's per-language picker supplies one;
  it must not be offered under Auto.
- **No GGUF for newer candidates.** Granite Speech 5.0 2B (leaderboard #2,
  5.06) and TurboCTC multilingual revisions are not in transcribe.cpp;
  Kyutai STT (PyTorch/MLX) and TheWhisper have no GGUF. Excluded.

## English

```ts
                       FLEURS en   LS clean   Open ASR English avg (current)
Qwen3-ASR 1.7B           3.23        -          5.75
Voxtral Mini 3B          3.89        -          -
Parakeet Unified EN      3.99        1.60 (Hex) -   (8-set offline 5.91 older board)
Granite 4.1 2B           4.14        -          -
parakeet-ultra           4.26        1.80       -   (older board 5.32 vs v3 6.32, from model-watch)
Whisper turbo            4.38        -          8.39
Canary 1B v2             4.47        1.91       -
Parakeet v3              4.83        1.94       6.51
Cohere Transcribe        5.08        1.27       6.26
```

Current: Recommended Parakeet Unified, Most accurate Cohere, Hints Whisper.
Proposed: Recommended Unified (keep), Most accurate Qwen3-ASR 1.7B (medium
confidence) or keep Cohere until Hex's own English corpus is run, Hints Whisper
(keep). Fastest is already Unified/ultra territory; no separate row is needed.

- Unified vs ultra: ultra is likely a bit better offline on mixed data (older
  eight-set average), but FLEURS en favors Unified and only Unified streams.
  Hex's capture pipeline is built around streaming with a whole-clip fallback
  (AGENTS.md), so switching English to an offline-only model costs latency for
  no demonstrated accuracy gain. Keep Unified. Offer ultra only as an optional
  offline alternative if English users want its long-audio segmentation.
- Canary v2 and Voxtral 3B do not beat Unified on English. Not proposed.
- Qwen3-ASR 1.7B: the best English FLEURS and best current-leaderboard open
  model Hex could run (Apache-2.0, 45x on M4 Max). It costs a ~2 GB download and
  no streaming, the same trade Cohere already makes. Confidence medium: the
  FLEURS and leaderboard numbers point the same way, but no one has run it on
  Hex's dictation audio, and Hex marks Qwen as having no recognition hints.
- Cohere's "most accurate" label rests on LibriSpeech clean (read audiobooks)
  and a 0.25-point leaderboard edge over v3; its FLEURS en is the worst here.
  Medium-low confidence in keeping that label unchanged.

## Per-language table (FLEURS test WER %, Q8_0)

Columns: ultra, v3, Canary 1B v2 (cv2), Whisper turbo (wt), plus the best
other available model. Best-in-row is bold. Current for every language below is
"Recommended v3, Hints Whisper turbo" unless stated.

```ts
lang  ultra   v3     cv2    wt     other             proposed (Recommended / Most accurate / Hints)         conf
bg    11.04  12.81   9.22   13.58  -                 ultra / Canary v2 (-1.8) / Whisper                    medium
cs    11.04  12.31   8.56   11.81  Qwen1.7 22.97     ultra / Canary v2 (-2.5) / Whisper                    medium
da    15.98  18.64  11.34   13.60  -                 ultra / Canary v2 (-4.6) / Whisper                    high
de     4.58   5.24   4.46   4.54   Qwen1.7 4.25      ultra / (tie, none) / Whisper                         medium
el    32.25  35.33  26.02   13.26  Cohere 8.96       Cohere / Whisper / Whisper (hints)                    high
en     see English section above
es     3.06   3.65   3.10   3.12   Qwen1.7 3.31      ultra / (tie, none) / Whisper                         high
et    14.44  17.96  12.72   18.44  -                 ultra / Canary v2 (-1.7) / Whisper                    medium
fi    11.14  13.30   8.86   8.29   -                 Whisper / Canary v2 / Whisper (hints)                 medium
fr     4.66   5.30   5.09   5.51   Voxtral3B 4.51    ultra / (tie, none) / Whisper                         medium
hr    11.30  12.59   8.40   12.54  -                 ultra / Canary v2 (-2.9) / Whisper                    medium
hu    13.09  16.06  13.06   15.07  -                 ultra / (tie, none) / Whisper                         medium
it     2.34   3.02   3.10   2.77   Voxtral3B 2.56    ultra / (none) / Whisper                              high
lt    17.06  22.20  13.45   25.11  -                 ultra / Canary v2 (-3.6) / Whisper                    medium
lv    18.04  23.77  10.41   19.53  -                 Canary v2 / ultra / Whisper                           high
mt    17.01  20.63  19.75   70.92  -                 ultra / (none) / (no hints: Whisper unusable)         medium
nl     6.70   7.66   6.28   5.98   Voxtral3B 6.57    ultra / (tie, none) / Whisper                         medium
pl     6.16   7.37   6.88   5.81   Cohere 6.15       ultra / (tie, none) / Whisper                         medium
pt     4.16   4.96   4.50   4.17   Voxtral3B 3.84    ultra / (tie, none) / Whisper                         medium
ro    10.12  12.62   6.87   10.90  -                 ultra / Canary v2 (-3.3) / Whisper                    high
ru     5.63   6.54   7.83   5.93   Qwen1.7 6.25      ultra / (none) / Whisper                              high
sk     7.83  10.19   6.84   10.21  -                 ultra / Canary v2 (-1.0, borderline) / Whisper        low
sl    18.98  24.30  12.76   20.56  -                 Canary v2 / ultra / Whisper                           high
sv    12.83  15.25   9.74    8.72  -                 Whisper / Canary v2 / Whisper (hints)                 medium
uk     5.71   6.84  10.58    7.31  -                 ultra / (none) / Whisper                              high
tr     -      -      -      6.97   Qwen1.7 9.46      Whisper / (none) / Whisper                            high
mk     -      -      -     17.85   Qwen1.7 18.22     Whisper / (none) / Whisper                            medium
```

`tr` and `mk` currently get Qwen3-ASR 0.6B as Recommended (16.74 and 35.09);
Qwen3-ASR 1.7B is nearly as good as Whisper there, but Whisper already ships
and also offers hints.

Reading the table:

- **ultra vs v3.** Lower in all 25 languages, by 0.5 points on the strong
  Romance languages up to 5 to 6 points on lv, sl, lt, bg, da. Same size,
  languages and API, so the swap is a catalog entry plus the crate bump. Keep
  v3 only as a fallback if the crate bump slips. High confidence for the swap
  (every language moves the same direction, and the card is a like-for-like run
  of the same pipeline).
- **Canary v2 as "Most accurate"** (CC-BY-4.0, 1.14 GB, 105x on M4 Max, still
  fast): clear wins on bg cs da et hr lt lv ro sk sl, and on fi and sv close to
  Whisper. It is not better on de es fr it nl pl pt ru uk. It adds an explicit
  language requirement, no timestamps, and no vocabulary biasing.
- **Whisper turbo** wins or ties on fi, nl, pl, sv, tr, mk and is the only
  hint-capable model, but it is the slowest (36x) and collapses on mt (70.92),
  lv, lt, sl, et. For those languages Hex should not advertise it as a general
  option; the Recognition hints row is a trade, not a recommendation.
- **Qwen3-ASR 0.6B is poor outside the big languages** (cs 44.50, fi 46.49,
  hu 56.24); it should not be Recommended for any European language.
- **Cohere** covers de es fr it nl pl pt el in Hex, but only wins on Greek.
  Its FLEURS (de 5.06, fr 5.23, es 3.97) is behind ultra.

## What to change in `choices_for_runtime`

1. Add parakeet-ultra to `MODELS` and make it Recommended in the Parakeet branch
   (requires bumping `transcribe-cpp` past 0.4.0, and the Nix `outputHashes`
   if it touches the build). Keep v3 out of the picker or as a fallback.
2. Add Canary 1B v2 as Most accurate for bg cs da et hr lt lv ro sk sl (and
   possibly fi sv). Never under Auto.
3. Route `el` away from Parakeet: Cohere Recommended (add `el` to its branch
   before the Parakeet match), Whisper for hints.
4. Route `tr` and `mk` to Whisper turbo as Recommended instead of Qwen 0.6B.
5. English: keep Unified; evaluate Qwen3-ASR 1.7B against Cohere on Hex's own
   English clips before changing the Most accurate label.

## Not measured

- No local runs: nothing here was rerun on Kit's hardware, and Apple Silicon
  speeds are the transcribe.cpp cards' M4 Max numbers.
- Real dictation (noisy, short, accented) is not FLEURS. A cheap check is the
  `benchmark-transcription` corpus per language for the changed rows only.
- Parakeet ultra on 0.2.4 was not tried; the 0.4.0 requirement is inferred from
  dates and the absent VAD code in the 0.2.4 sources.
- Qwen3-ASR 1.7B Q8 size and Canary v2's behavior under Hex's language-hint
  plumbing need a check before adding.

## Sources

- transcribe.cpp (MIT) `docs/models/`: `parakeet-ultra`, `parakeet-tdt-0.6b-v3`,
  `parakeet-unified-en-0.6b`, `canary-1b-v2`, `canary-1b-flash`,
  `whisper-large-v3-turbo`, `cohere-transcribe-03-2026`, `qwen3-asr-0.6b`,
  `qwen3-asr-1.7b`, `voxtral-mini-3b-2507`, `granite-speech-4.1-2b`,
  `nemotron-3.5-asr-streaming-0.6b`, `orukeet`, `parakeet-primeline`;
  releases v0.2.4 (2026-09-25), v0.3.1 (10-04), v0.4.0 (10-12), v0.4.1 (10-22);
  commit `37273409` "parakeet ultra" (2026-10-05).
- Hex's pinned `transcribe-cpp-sys` 0.2.4 sources (`src/arch/`).
- HF `hf-audio/open-asr-leaderboard-results` (`english_short_latest.csv`) and the
  `huggingface/open_asr_leaderboard` README (multilingual benchmark description).
- `model-watch.md` for the English leaderboard rows and the ultra, Unified,
  Nemotron, and Moonshine analysis.
