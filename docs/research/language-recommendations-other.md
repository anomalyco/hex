# Language recommendations: zh, yue, ja, ko, the Qwen set, and Auto

Research, 2026-10-22. Builds on `model-watch.md` (not repeated). Scope: every
language in `LANGUAGES` that Parakeet v3 does not cover, plus Auto-detect.
Read-only against `src/transcription_models.rs`; nothing here is implemented.

## Bottom line

```ts
Today                                       Evidence says
├── zh|yue|ja|ko → Qwen3-ASR 0.6B           ├── zh: a tie (all within CI). Keep Qwen 0.6B.
│   Fastest SenseVoice, hints Whisper       ├── yue: Qwen (7.9 / 6.1 CER) wins by 4x over Whisper turbo
│                                           │   (34.6) and SenseVoice (37.4). SenseVoice must stop being "Fastest" here.
│                                           ├── ja: Whisper turbo (4.82) and Fun-ASR-MLT (2.10) beat Qwen 0.6B (8.61)
│                                           └── ko: Whisper turbo (5.24) beats Qwen 0.6B (5.82); Qwen 1.7B best (4.60)
├── other Qwen languages → Qwen 0.6B        ├── Qwen 0.6B loses to Whisper turbo on ar, tr, ms, fil, fa, mk, id
│                                           └── it wins only on hi, th (and ties zh, vi)
└── Auto → Whisper + Qwen both "Recommended"└── Auto → Whisper turbo Recommended; drop SenseVoice as Fastest
    + SenseVoice Fastest
```

Every model proposed below already runs in the pinned `transcribe-cpp` 0.2.4
(`src/arch/` has `qwen3_asr`, `funasr_nano`, `sensevoice`, `whisper`, `cohere`).
No crate bump is needed for any proposal except possibly the Arabic Cohere
GGUF (same `cohere` arch; confirm it loads on 0.2.4).

## Method and trust

One source does almost all the work: the per-model docs in transcribe.cpp
(`docs/models/*.md`, handy-computer, MIT), which score the **same Q8_0 GGUFs
Hex would ship** on **FLEURS test** with one harness. That is third-party to
the model vendors and apples-to-apples across models. Its limits:

- FLEURS is read Wikipedia sentences, one run, about 900 utterances per
  language. The bootstrap 95% CI on zh is roughly ±0.9 CER points, so
  differences under about 1.5 points are noise. It says nothing about
  dictation, noise, accents, or code-switching.
- Normalization is the harness's, not each language's best. Cantonese and
  Chinese script (Traditional vs Simplified) can inflate CER: SenseVoice 37.4
  and Whisper turbo 34.6 on yue are probably partly script mismatch, not only
  recognition. Unverified here; test before telling users "SenseVoice cannot
  do Cantonese".
- Fun-ASR-MLT ja 2.10 CER is an outlier next to every other model (4.8 to
  8.6). Possible training-data overlap with FLEURS; I did not check. Treat as
  promising, medium confidence.
- **Vendor-only**: the Qwen3-ASR technical report (arXiv 2601.21337) and model
  card (MLS, Common Voice, MLC-SLM, Fleurs, WenetSpeech, internal sets). Used
  below only as a cross-check and marked "vendor".
- AISHELL, CSJ, KsponSpeech, and Common Voice per-language numbers for the
  GGUFs: **none found** from an independent party. The HF Open ASR leaderboard
  results dataset (`hf-audio/open-asr-leaderboard-results`, checked 2026-10-22)
  publishes only `english_short_latest.csv`; there is no public multilingual
  per-language results file to cite.

## Candidate models (what can actually ship)

All Q8_0, Metal speed on M4 Max (transcribe.cpp, `asr-publication-v2`, jfk 11 s
/ dots 35 s, x realtime). Only Whisper supports Hex's recognition hints today.

```ts
model                      size     speed (jfk/dots)  license         langs   hints in transcribe.cpp        notes
Qwen3-ASR 0.6B (shipped)   850 MB   75x / 64x         Apache-2.0      30 (52) vocabulary + prompt           echoes vocabulary into output often
Qwen3-ASR 1.7B             2.19 GB  45x / 37x         Apache-2.0      30 (52) vocabulary + prompt           Q4_K_M 1.32 GB (+0.2 EN WER); Q5_K_M 1.52 GB
Whisper large-v3-turbo     886 MB   36x / 51x         MIT             100     vocabulary, prompt, prefix    shipped; hint path works
Whisper large-v3           1.67 GB  slower            Apache-2.0      100     same                          better on yue/th than turbo; 2x size
SenseVoice Small (shipped) 253 MB   245x / 289x       FunASR license  5       none                          zh/yue/en/ja/ko only
Fun-ASR-Nano-2512          891 MB   83x / 73x         FunASR license  zh,en,ja vocabulary (hotwords)        Mandarin/Japanese specialist
Fun-ASR-MLT-Nano-2512      891 MB   75x / 65x         FunASR license  31      vocabulary (hotwords)         ITN on by default; LS-clean EN 1.72
Cohere Transcribe (shipped)2.41 GB  76x / 75x         Apache-2.0      14      none                          already in catalog, not offered for non-English
Cohere Transcribe Arabic   2.41 GB  ~Cohere           Apache-2.0      ar, en  none                          FLEURS ar 11.06; upstream repo is gated
Nemotron 3.5 ASR Streaming 751 MB   144x / 138x       OpenMDW-1.1     32      auto-LID tag; streaming       multilingual accuracy is weak (below)
```

FunASR Model License v1.1 allows use, copy, modify, share with attribution and
the model name retained; it has a non-disparagement termination clause. It is
redistributable, but a legal glance is warranted before shipping two more
FunASR-licensed weights. SenseVoice already carries it.

Not available through transcribe.cpp (checked `docs/models/` and `src/arch/`
at v0.4.1): Dolphin, FireRedASR, Paraformer/FunASR standalone, kotoba-whisper,
Belle-whisper, Voxtral for these languages (Voxtral Mini 3B lists only en, fr,
de, es, it, pt, nl, hi). Granite Speech 4.1 2B covers ja among these (6.30 CER,
2.56 GB) and is not better than Whisper turbo. `breeze-asr-25` (Taiwan Mandarin
Whisper fine-tune) is in the Whisper family list but is outside Hex's
language set. Whisper fine-tunes such as kotoba-whisper could in principle be
converted through the Whisper path, but nothing is published as GGUF there, so
they are out under the "published GGUF" constraint.

crate/version note: Hex pins `transcribe-cpp` 0.2.4 (2026-09-25); 0.3.0
(2026-10-03), 0.3.1, 0.4.0 and 0.4.1 (2026-10-22) exist. 0.4.x adds `ecapa_tdnn`
(language ID, VoxLingua107) and `silero_vad` arches. No new ASR arch landed
after 0.2.4 for these languages.

## Evidence: FLEURS test, Q8_0, transcribe.cpp harness

CER for zh, yue, ja, ko, th; WER elsewhere. Lower is better. Bold is the best
among models that fit under about 1 GB (the default download class).

```ts
lang  Qwen0.6  Qwen1.7  Whisper-turbo  Whisper-v3  SenseVoice  FunASR-Nano  FunASR-MLT  Cohere  Nemotron3.5
zh     7.57     7.14      8.50           7.98        10.12       8.59         7.33        11.18   18.87
yue    7.91     6.13     34.62          22.06        37.44       n/a          8.39        n/a     n/a
ja     8.61     5.29      4.82           4.81         7.63       8.50         2.10        5.13    13.52
ko     5.82     4.60      5.24           4.89         8.27       n/a          5.41        6.57     8.89
ar    24.51    14.91     15.48          14.92         n/a        n/a         25.88       13.60    15.93   // Cohere-Arabic 11.06
hi    12.68     7.84     18.85          17.06         n/a        n/a         23.07        n/a      8.61
vi     9.32     6.15      9.48           8.74         n/a        n/a          8.22        7.39    13.96
tr    16.74     9.46      6.97           6.51         n/a        n/a          n/a         n/a     15.40
th     8.81     6.89     13.15           8.78         n/a        n/a          7.28        n/a      n/a
id     8.49     5.37      7.20           6.08         n/a        n/a          7.32        n/a      n/a
ms    17.18    10.42      8.64           7.59         n/a        n/a          9.11        n/a      n/a
fa    50.30    28.29     30.56          30.11         n/a        n/a          n/a         n/a      n/a
fil   35.43    24.29     12.08          11.82         n/a        n/a         14.29        n/a      n/a
mk    35.09    18.22     17.85          15.09         n/a        n/a          n/a         n/a      n/a
```

Aggregate over the 30 FLEURS languages Qwen and Whisper share (same harness):
mean WER/CER Qwen 0.6B 21.4, Qwen 1.7B 12.5, Whisper turbo 10.6, Whisper v3
9.2; medians 13.3, 7.6, 8.6, 7.8. Qwen 0.6B is the outlier-prone one, and
Whisper turbo is the better all-rounder under 1 GB. Qwen's own report agrees
(vendor): on its 30-language Fleurs set Whisper-large-v3 scores 8.16 vs
Qwen3-ASR 1.7B 12.60 and 0.6B 21.80 ("performance degrades on the full
30-language setting"); Qwen leads only on the 12- and 20-language subsets of
major languages. Hex's `QWEN_LANGUAGES` is the full 30-language set, so the
hard tail (fa, mk, fil, el, hu, cs, da, fi, ro, sv) is exactly where 0.6B is
worst. Note that those tail languages other than fa, fil, mk are Parakeet v3
languages in Hex and never reach the Qwen branch.

## Per-language table: current → proposed

Roles: Rec = Recommended, Fast = Fastest, Acc = Most accurate, Hint = Recognition hints.
"Rec" speed class assumes the model above. Confidence is about the ranking,
not the absolute WER.

```ts
lang  current                          proposed                                              evidence (FLEURS)                                   conf
zh    Rec Qwen0.6; Fast SenseVoice;     Rec Qwen0.6 (keep); Fast SenseVoice (keep);          Qwen0.6 7.57, MLT 7.33, Qwen1.7 7.14, turbo 8.50,    low
      Hint Whisper turbo                Acc Qwen3-ASR 1.7B (+Fun-ASR-Nano as a Mandarin      SenseVoice 10.12: all inside ±1 CI except SenseVoice.
                                        option, hotwords); Hint Whisper turbo                 Vendor-only AISHELL/WenetSpeech claims; no indep. run.
yue   Rec Qwen0.6; Fast SenseVoice;     Rec Qwen0.6; Fast: none (drop SenseVoice);            Qwen0.6 7.91, Qwen1.7 6.13, MLT 8.39 vs turbo 34.62, high on
      Hint Whisper turbo                Acc Qwen 1.7B; Hint Whisper large-v3 if offered       v3 22.06, SenseVoice 37.44 (script caveat above).   the gap
                                        (turbo is unusable for yue)
ja    Rec Qwen0.6; Fast SenseVoice;     Rec Whisper turbo (or Fun-ASR-MLT, 2.10); Fast       turbo 4.82, v3 4.81, Qwen1.7 5.29, Cohere 5.13,     medium
      Hint Whisper turbo                Qwen0.6/SenseVoice (7.63); Acc Fun-ASR-MLT or         SenseVoice 7.63, Qwen0.6 8.61, FunASR-Nano 8.50,
                                        Qwen 1.7B; Hint Whisper turbo                         MLT 2.10 (outlier, verify). Qwen 0.6B is not the best
                                                                                              choice for ja on this data.
ko    Rec Qwen0.6; Fast SenseVoice;     Rec Whisper turbo or keep Qwen0.6 (statistical        Qwen1.7 4.60, v3 4.89, turbo 5.24, MLT 5.41,        low
      Hint Whisper turbo                near-tie 5.24 vs 5.82); Fast Qwen0.6 (SenseVoice      Qwen0.6 5.82, Cohere 6.57, SenseVoice 8.27.
                                        8.27 is 40% worse); Acc Qwen 1.7B; Hint Whisper
ar    Rec Qwen0.6; Hint Whisper         Rec Whisper turbo; Acc Cohere Arabic 11.06 (or       turbo 15.48, v3 14.92, Qwen1.7 14.91, Cohere 13.60, high that
                                        shipped Cohere 13.60, or Qwen 1.7B); Fast: none      Cohere-Arabic 11.06 vs Qwen0.6 24.51 and MLT 25.88.  Qwen0.6
                                                                                                                                                  is wrong here
hi    Rec Qwen0.6; Hint Whisper         Rec Qwen0.6 (keep, 12.68 vs turbo 18.85); Acc        Qwen1.7 7.84, Nemotron3.5 8.61, Voxtral3B 8.93,      high
                                        Qwen 1.7B; Hint Whisper (turbo is weak, 18.85)        Qwen0.6 12.68, turbo 18.85, MLT 23.07
vi    Rec Qwen0.6; Hint Whisper         Rec Qwen0.6 (keep, ties turbo); Acc Qwen 1.7B or     Qwen1.7 6.15, Cohere 7.39, MLT 8.22, Qwen0.6 9.32,   medium
                                        shipped Cohere (7.39); Hint Whisper                   turbo 9.48
tr    Rec Qwen0.6; Hint Whisper         Rec Whisper turbo; drop Qwen 0.6 (16.74)             turbo 6.97, v3 6.51, Qwen1.7 9.46, Qwen0.6 16.74,    high
                                                                                              Nemotron3.5 15.40
th    Rec Qwen0.6; Hint Whisper         Rec Qwen0.6 (keep); Acc Qwen 1.7B; Hint Whisper      Qwen1.7 6.89, MLT 7.28, Qwen0.6 8.81, v3 8.78,       medium
                                        (turbo CER 13.15 is bad; use large-v3 8.78 if you    turbo 13.15
                                        offer it for Thai hints)
id    Rec Qwen0.6; Hint Whisper         Rec Whisper turbo (also gives hints); Fast Qwen0.6;  Qwen1.7 5.37, v3 6.08, turbo 7.20, MLT 7.32,         low
                                        Acc Qwen 1.7B                                         Qwen0.6 8.49
ms    Rec Qwen0.6; Hint Whisper         Rec Whisper turbo; drop Qwen0.6 as Rec (17.18)       v3 7.59, turbo 8.64, MLT 9.11, Qwen1.7 10.42,        medium
                                                                                              Qwen0.6 17.18. Qwen's own report: id/ms confusion
fa    Rec Qwen0.6; Hint Whisper         Rec Whisper turbo or Qwen 1.7B (both poor); flag    Qwen1.7 28.29, v3 30.11, turbo 30.56, Qwen0.6 50.30  high on
                                        as low accuracy in the UI                                                                                  "0.6 is bad"
fil   Rec Qwen0.6; Hint Whisper         Rec Whisper turbo                                    turbo 12.08, v3 11.82, MLT 14.29, Qwen1.7 24.29,     high
                                                                                              Qwen0.6 35.43
mk    Rec Qwen0.6; Hint Whisper         Rec Whisper turbo; Acc Whisper large-v3 (15.09)     v3 15.09, turbo 17.85, Qwen1.7 18.22, Qwen0.6 35.09  high
```

A coherent small change set, if you want the minimum: (1) in the generic
`QWEN_LANGUAGES` branch, make Whisper turbo Recommended for ar, tr, ms, fil, fa,
mk, id and keep Qwen 0.6B Recommended only for hi, th, vi; (2) for ja use
Whisper turbo; (3) remove SenseVoice as Fastest for yue (and consider ja/ko);
(4) add Qwen3-ASR 1.7B as Most accurate wherever Qwen is offered.

The fallback arm `_ => Whisper` in `choices_for_runtime` is unreachable from
the UI: every code in `LANGUAGES` is covered by Auto, Parakeet v3, or the Qwen
set. It is only a guard.

## Recognition hints

Hex currently enables hints only for Whisper (`supports_recognition_hints:
true`). transcribe.cpp also documents `--vocabulary`/`--prompt` for Qwen3-ASR
0.6B and 1.7B ("system message"; the 0.6B "echoes the dictionary into the output
more often than the 1.7B") and `--vocabulary` hotwords for Fun-ASR-Nano and
MLT. Two options for hint-heavy users in these languages:

- Keep Whisper turbo as the Hints entry (works today), but know it is weak for
  yue (34.6), th (13.2), hi (18.9). Whisper large-v3 (1.67 GB) is the fix for
  yue (22.1) and th (8.8) and no better elsewhere.
- Qwen 1.7B with `vocabulary`: better accuracy than Whisper turbo on ar, hi,
  vi, th, id, ja, ko, yue and tied on zh. This would need
  `supports_recognition_hints: true` on the Qwen models and a check that Hex
  maps its hint string to the vocabulary list. Not tested here; do not
  enable on 0.6B without testing the echo behavior.

## Auto-detect

Today `AUTO_LANGUAGE` lists Whisper (Recommended), SenseVoice (Fastest) and
Qwen 0.6B (also Recommended).

- **Drop SenseVoice as Fastest.** It recognizes five languages
  (`zh/yue/en/ja/ko`). Auto-detect over arbitrary speech would route French or
  Hindi to a model that cannot transcribe them. SenseVoice is fine in Auto only
  if the user's languages are all five.
- **Whisper turbo as the single Recommended.** 100 languages, hint support,
  and per-language accuracy on FLEURS beating Qwen 0.6B in 9 of the 14
  Qwen-only languages above. Its weak spots are yue, th, hi, and short-clip
  language misdetection. Qwen's report (vendor) gives LID accuracy on Fleurs
  30 languages: Whisper large-v3 94.6, Qwen 0.6B 97.1, Qwen 1.7B 98.7, with
  most Qwen errors being ms/id confusion. That is the one place Qwen has an
  edge, and it is vendor-reported on the vendor's chosen languages.
- **Qwen3-ASR 1.7B as Most accurate for Auto** (median 7.6 vs turbo 8.6,
  30 languages), with Qwen 0.6B as Fastest only if you accept its tail
  languages (mean 21.4 across the set). A 1.7B model costs 2.19 GB and 45x.
  The VoxLingua107 ECAPA model that 0.4.x adds could do a cheap language
  pre-pass and route each clip to the right per-language Recommended model;
  that is a design choice, not tested here.
- Confidence: medium. Detect quality on short dictation utterances was not
  measured by anyone I could find; the FLEURS numbers are for known-language
  decoding.

## What would raise confidence

- Hex's own fixtures for zh/ja/ko/yue with Traditional and Simplified scoring,
  across Qwen 0.6B and 1.7B, Whisper turbo, Fun-ASR-MLT/Nano, on dictation
  audio (short, noisy), not read Wikipedia.
- A Cantonese run scored script-normalized to see how much of SenseVoice's
  37.4 and Whisper turbo's 34.6 is the Traditional/Simplified mismatch.
- Check whether Fun-ASR-MLT's ja 2.10 holds on Common Voice ja or CSJ.
- A short-utterance language-ID check for Whisper turbo vs Qwen 1.7B on the
  Auto set, since dictation clips are 2-10 s.
- Apple Silicon speed for each GGUF on a base M-series chip, not an M4 Max.

## Sources

- transcribe.cpp (MIT, `handy-computer/transcribe.cpp`) `docs/models/`:
  `qwen3-asr-0.6b.md`, `qwen3-asr-1.7b.md`, `whisper-large-v3-turbo.md`,
  `whisper-large-v3.md`, `sensevoice-small.md`, `fun-asr-nano-2512.md`,
  `fun-asr-mlt-nano-2512.md`, `cohere-transcribe-03-2026.md`,
  `cohere-transcribe-arabic-07-2026.md`, `nemotron-3.5-asr-streaming-0.6b.md`,
  `voxtral-mini-3b-2507.md`, `granite-speech-4.1-2b.md`; `README.md` model
  table; tags v0.2.4, v0.3.0, v0.4.1 `src/arch/`. All FLEURS numbers above are
  from these pages, Q8_0.
- crates.io `transcribe-cpp` release dates (0.2.4 2026-09-25, 0.4.1 2026-10-22).
- Qwen3-ASR Technical Report, arXiv 2601.21337, and the Qwen/Qwen3-ASR-1.7B model
  card (vendor): MLS/Common Voice/MLC-SLM/Fleurs, LID accuracy. Vendor-only.
- FunASR Model Open Source License v1.1 (`modelscope/FunASR` `MODEL_LICENSE`).
- `hf-audio/open-asr-leaderboard-results` file listing (English only).
