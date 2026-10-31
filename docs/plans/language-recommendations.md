# Per-Language Model Recommendations Plan

**Status:** Approved 2026-10-22; Phase A implemented, Phase B waits for the `transcribe-cpp` bump. Prompting existing users about a better model is deferred until the Rune port is done (see `ROADMAP.md` "Recommend Better Models To Existing Users"); Phase A adds no new UI. Synthesizes [`docs/research/language-recommendations-european.md`](../research/language-recommendations-european.md) (`ebd9fc8`), [`docs/research/language-recommendations-other.md`](../research/language-recommendations-other.md) (`fde2bb3`), and [`docs/research/model-watch.md`](../research/model-watch.md), and integrates with [`docs/plans/transcription-quality.md`](transcription-quality.md) (which owns the `transcribe-cpp` `0.2.4 -> 0.4.1` bump, shared vocabulary, and `parakeet-ultra` runtime work).

## Summary

Hex's current `choices_for_runtime` (`src/transcription_models.rs:494-567`) routes 40 language entries (`auto` plus 39 languages) through four coarse buckets. Apples-to-apples FLEURS test evaluations on the exact Q8_0 GGUF artifacts in `handy-computer/transcribe.cpp` expose three classes of fixes:

```ts
Per-language recommendation updates
├── Phase A: Ship now on pinned transcribe-cpp 0.2.4 (0.5d, zero new models)
│   ├── Greek (el): Parakeet v3 gets 1 in 3 words wrong (35.33% WER) -> route to shipped Cohere (8.96%) + Whisper (13.26%)
│   ├── Qwen 0.6B tail failures: Whisper turbo replaces Qwen 0.6B as Recommended on tr, mk, ar, ms, fil, fa, ja (-3.8 to -23.4 pts)
│   ├── Nordic Parakeet gap: Whisper turbo becomes Recommended on fi (8.29 vs 13.30) and sv (8.72 vs 15.25), Parakeet as Fastest
│   ├── Auto & CJK cleanup: drop 5-language SenseVoice from Auto, yue (37.44% CER), and ko (8.27%); Qwen 0.6B becomes Auto Fastest
│   └── Maltese (mt): drop Whisper turbo (70.92% WER) from Recognition hints
└── Phase B: Ship with transcribe-cpp 0.4.1 in transcription-quality.md Phase 3 (+1d)
    ├── ParakeetUltra (740 MB): replaces Parakeet v3 across 24 EU languages (-0.6 to -6.2 pts everywhere, 30 s pause cuts)
    ├── Canary1BV2 (1.14 GB): Recommended on lv, sl (-6.2 to -7.6 pts vs ultra); Most accurate on bg, cs, da, et, hr, lt, ro (-1.7 to -4.6 pts)
    └── Qwen3Asr17B (2.19 GB): Most accurate on auto, yue, ko, hi, vi, th, id, fa (and en pending ses_e98f7b094ffe1Q3KtZFS6Np4KL)
```

---

## 1. Final Per-Language Table

**Rules applied across all 40 entries in `LANGUAGES` (`src/transcription_models.rs:437-478`):**
- **Noise floor (`~1.5 points`):** Differences under ~1.5 WER/CER points on FLEURS test (~860–900 read utterances per language, bootstrap 95% CI ~±0.9 pts) are treated as ties. We do not churn an existing recommendation or add a `Most accurate` card for a `<1.5 pt` edge (e.g., `zh` Qwen 0.6B 7.57% vs 1.7B 7.14%; `ko` Qwen 0.6B 5.82% vs Whisper 5.24%; `id` Qwen 0.6B 8.49% vs Whisper 7.20%; `sk` Parakeet Ultra 7.83% vs Canary v2 6.84%; `de`/`es`/`fr`/`hu`/`it`/`nl`/`pl`/`pt`/`ru`/`uk` Ultra vs Canary/Whisper/Voxtral).
- **Roles:** `Rec` = `Recommended` (first entry is also `recommended_selection(lang)` for first-run setup), `Fast` = `Fastest`, `Acc` = `Most accurate`, `Hint` = `Recognition hints`. When `Whisper turbo` is `Rec`, no duplicate `Hint` row is shown because it already supports hints.
- **Metrics:** CER % for `zh`, `yue`, `ja`, `ko`, `th`; WER % for all other languages (Q8_0 GGUF on FLEURS test via `transcribe.cpp`).

| Code | Language | Current (`0.2.4`) | Proposed (`Phase A: 0.2.4` → `Phase B: 0.4.1`) | Evidence (FLEURS Q8_0 WER/CER %) | Conf. | Runtime |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `auto` | Auto | Rec: Whisper turbo<br>Fast: SenseVoice<br>Rec: Qwen 0.6B | **A:** Rec: Whisper turbo; Fast: Qwen 0.6B (drop SenseVoice)<br>**B:** + Acc: Qwen 1.7B | 30-lang median: Qwen 1.7B **7.6**, Whisper turbo **8.6**, Qwen 0.6B 13.3 (mean 21.4). SenseVoice covers only 5 langs. | Med | `0.2.4` (`1.7B` in B) |
| `en` | English | Rec: Parakeet Unified<br>Acc: Cohere<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Unified; Acc: Qwen 1.7B (or Cohere, gated on audit); Hint: Whisper turbo | FLEURS: Qwen 1.7B **3.23**, Unified **3.99**, Ultra 4.26, Whisper 4.38, Canary 4.47, v3 4.83, Cohere 5.08. Open ASR avg: Qwen 1.7B **5.75**, Cohere 6.26, v3 6.51. | High (Rec)<br>Med (Acc) | `0.2.4` |
| `zh` | Mandarin Chinese | Rec: Qwen 0.6B<br>Fast: SenseVoice<br>Hint: Whisper turbo | **Keep current** (no churn; top models within 0.43 pt noise band) | Qwen 1.7B 7.14, Fun-ASR-MLT 7.33, Qwen 0.6B **7.57**, Whisper turbo 8.50, SenseVoice 10.12 (245x). | Med | `0.2.4` |
| `yue` | Cantonese | Rec: Qwen 0.6B<br>Fast: SenseVoice<br>Hint: Whisper turbo | **A:** Rec: Qwen 0.6B; Hint: Whisper turbo (drop SenseVoice Fast)<br>**B:** + Acc: Qwen 1.7B (Qwen hints enabled in `0.4.1`) | Qwen 1.7B **6.13**, Qwen 0.6B **7.91** vs Whisper turbo 34.62, SenseVoice 37.44 (+29.5 pts; verify script norm). | High | `0.2.4` (`1.7B` in B) |
| `ja` | Japanese | Rec: Qwen 0.6B<br>Fast: SenseVoice<br>Hint: Whisper turbo | **A/B:** Rec: Whisper turbo; Fast: SenseVoice | Whisper turbo **4.82**, Cohere 5.13, Qwen 1.7B 5.29, SenseVoice **7.63** (245x), Qwen 0.6B 8.61 (-3.79 pts). (Fun-ASR-MLT 2.10 outlier deferred). | Med-High | `0.2.4` |
| `ko` | Korean | Rec: Qwen 0.6B<br>Fast: SenseVoice<br>Hint: Whisper turbo | **A:** Rec: Qwen 0.6B; Hint: Whisper turbo (drop SenseVoice Fast)<br>**B:** + Acc: Qwen 1.7B | Qwen 1.7B **4.60**, Whisper turbo 5.24, Qwen 0.6B **5.82** (0.58 pt gap vs Whisper is noise; Qwen 2x faster), SenseVoice 8.27 (+2.45 pts worse). | Med | `0.2.4` (`1.7B` in B) |
| `es` | Spanish | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Ultra **3.06**, Canary v2 3.10, Whisper 3.12, Qwen 1.7B 3.31, v3 3.65 (all top models within 0.25 pt). | High | `0.4.1` (Ultra) |
| `fr` | French | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Ultra **4.66**, Canary v2 5.09, v3 5.30, Whisper 5.51 (Voxtral 3B 4.51 is 0.15 pt noise at 13x speed). | High | `0.4.1` (Ultra) |
| `de` | German | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Qwen 1.7B 4.25, Canary v2 4.46, Whisper 4.54, Ultra **4.58**, v3 5.24 (all within 0.33 pt noise). | High | `0.4.1` (Ultra) |
| `pt` | Portuguese | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Ultra **4.16**, Whisper 4.17, Canary v2 4.50, v3 4.96 (Voxtral 3B 3.84 is 0.32 pt noise). | High | `0.4.1` (Ultra) |
| `it` | Italian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Ultra **2.34**, Whisper 2.77, v3 3.02, Canary v2 3.10. | High | `0.4.1` (Ultra) |
| `ar` | Arabic | Rec: Qwen 0.6B<br>Hint: Whisper turbo | **A/B:** Rec: Whisper turbo; Acc: Cohere Transcribe | Cohere (shipped) **13.60**, Qwen 1.7B 14.91, Whisper turbo **15.48** vs Qwen 0.6B 24.51 (-9.03 to -10.91 pts). | High | `0.2.4` |
| `hi` | Hindi | Rec: Qwen 0.6B<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Qwen 0.6B; Acc: Qwen 1.7B; Hint: Whisper turbo | Qwen 1.7B **7.84** (-4.84 pts), Qwen 0.6B **12.68**, Whisper turbo 18.85. | High | `0.2.4` (`1.7B` in B) |
| `vi` | Vietnamese | Rec: Qwen 0.6B<br>Hint: Whisper turbo | **A:** Rec: Qwen 0.6B; Acc: Cohere; Hint: Whisper turbo<br>**B:** Rec: Qwen 0.6B; Acc: Qwen 1.7B; Hint: Whisper turbo | Qwen 1.7B **6.15** (-3.17 pts), Cohere (shipped) **7.39** (-1.93 pts), Qwen 0.6B **9.32**, Whisper turbo 9.48. | Med | `0.2.4` (`1.7B` in B) |
| `ru` | Russian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Ultra **5.63**, Whisper 5.93, Qwen 1.7B 6.25, v3 6.54, Canary v2 7.83. | High | `0.4.1` (Ultra) |
| `uk` | Ukrainian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Ultra **5.71**, v3 6.84, Whisper 7.31, Canary v2 10.58. | High | `0.4.1` (Ultra) |
| `nl` | Dutch | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Whisper 5.98, Canary v2 6.28, Ultra **6.70**, v3 7.66 (0.72 pt Whisper edge is within 1.5 pt noise; Whisper already shown as Hint). | Med | `0.4.1` (Ultra) |
| `pl` | Polish | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Whisper 5.81, Cohere 6.15, Ultra **6.16**, Canary v2 6.88, v3 7.37 (0.35 pt gap is noise). | Med | `0.4.1` (Ultra) |
| `tr` | Turkish | Rec: Qwen 0.6B<br>Hint: Whisper turbo | **A/B:** Rec: Whisper turbo | Whisper turbo **6.97**, Qwen 1.7B 9.46 vs Qwen 0.6B 16.74 (**-9.77 pts**). | High | `0.2.4` |
| `sv` | Swedish | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Rec: Whisper turbo; Fast: Parakeet v3<br>**B:** Rec: Whisper turbo; Fast: Parakeet Ultra | Whisper turbo **8.72**, Canary v2 9.74 (within 1.02 pt of Whisper), Ultra **12.83**, v3 15.25 (**-6.53 pts** vs v3, **-4.11 pts** vs Ultra). | Med-High | `0.2.4` (Ultra in B) |
| `da` | Danish | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Rec: Whisper turbo; Fast: Parakeet v3<br>**B:** Rec: Parakeet Ultra; Acc: Canary 1B v2; Hint: Whisper turbo | Canary v2 **11.34** (-4.64 pts vs Ultra), Whisper **13.60** (-5.04 pts vs v3), Ultra **15.98**, v3 18.64. | High | `0.2.4` / `0.4.1` |
| `fi` | Finnish | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Rec: Whisper turbo; Fast: Parakeet v3<br>**B:** Rec: Whisper turbo; Fast: Parakeet Ultra | Whisper turbo **8.29**, Canary v2 8.86 (within 0.57 pt of Whisper), Ultra **11.14**, v3 13.30 (**-5.01 pts** vs v3, **-2.85 pts** vs Ultra). | Med-High | `0.2.4` (Ultra in B) |
| `cs` | Czech | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Acc: Canary 1B v2; Hint: Whisper turbo | Canary v2 **8.56** (**-2.48 pts** vs Ultra), Ultra **11.04**, Whisper 11.81, v3 12.31. | Med | `0.4.1` |
| `el` | Greek | Rec: Parakeet v3<br>Hint: Whisper turbo | **A/B:** Rec: Cohere Transcribe; Hint: Whisper turbo | Cohere (shipped) **8.96**, Whisper turbo **13.26** vs Canary v2 26.02, Ultra 32.25, v3 35.33 (**-26.37 pts**). | High | `0.2.4` |
| `ro` | Romanian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Acc: Canary 1B v2; Hint: Whisper turbo | Canary v2 **6.87** (**-3.25 pts** vs Ultra), Ultra **10.12**, Whisper 10.90, v3 12.62. | High | `0.4.1` |
| `hu` | Hungarian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Canary v2 13.06, Ultra **13.09** (0.03 pt tie), Whisper 15.07, v3 16.06. | Med | `0.4.1` (Ultra) |
| `id` | Indonesian | Rec: Qwen 0.6B<br>Hint: Whisper turbo | **A:** Keep current (1.29 pt gap is <1.5 pt noise)<br>**B:** Rec: Qwen 0.6B; Acc: Qwen 1.7B; Hint: Whisper turbo | Qwen 1.7B **5.37** (**-3.12 pts** vs 0.6B), Whisper turbo 7.20, Qwen 0.6B **8.49**. | Med | `0.2.4` (`1.7B` in B) |
| `ms` | Malay | Rec: Qwen 0.6B<br>Hint: Whisper turbo | **A/B:** Rec: Whisper turbo | Whisper turbo **8.64**, Qwen 1.7B 10.42 vs Qwen 0.6B 17.18 (**-8.54 pts**). | Med-High | `0.2.4` |
| `th` | Thai | Rec: Qwen 0.6B<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Qwen 0.6B; Acc: Qwen 1.7B; Hint: Whisper turbo | Qwen 1.7B **6.89** (**-1.92 pts** vs 0.6B), Qwen 0.6B **8.81**, Whisper turbo 13.15. | Med | `0.2.4` (`1.7B` in B) |
| `fa` | Persian | Rec: Qwen 0.6B<br>Hint: Whisper turbo | **A:** Rec: Whisper turbo<br>**B:** Rec: Whisper turbo; Acc: Qwen 1.7B | Qwen 1.7B **28.29** (-2.27 pts vs Whisper), Whisper turbo **30.56** vs Qwen 0.6B 50.30 (**-19.74 pts**). | High | `0.2.4` (`1.7B` in B) |
| `bg` | Bulgarian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Acc: Canary 1B v2; Hint: Whisper turbo | Canary v2 **9.22** (**-1.82 pts** vs Ultra), Ultra **11.04**, v3 12.81, Whisper 13.58. | Med | `0.4.1` |
| `hr` | Croatian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Acc: Canary 1B v2; Hint: Whisper turbo | Canary v2 **8.40** (**-2.90 pts** vs Ultra), Ultra **11.30**, Whisper 12.54, v3 12.59. | Med | `0.4.1` |
| `et` | Estonian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Acc: Canary 1B v2; Hint: Whisper turbo | Canary v2 **12.72** (**-1.72 pts** vs Ultra), Ultra **14.44**, v3 17.96, Whisper 18.44. | Med | `0.4.1` |
| `lv` | Latvian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Canary 1B v2; Fast: Parakeet Ultra; Hint: Whisper turbo | Canary v2 **10.41** (**-7.63 pts** vs Ultra, **-13.36 pts** vs v3), Ultra **18.04**, Whisper 19.53, v3 23.77. | High | `0.4.1` |
| `lt` | Lithuanian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Acc: Canary 1B v2; Hint: Whisper turbo | Canary v2 **13.45** (**-3.61 pts** vs Ultra), Ultra **17.06**, v3 22.20, Whisper 25.11. | Med-High | `0.4.1` |
| `mt` | Maltese | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Rec: Parakeet v3 (drop Whisper Hint)<br>**B:** Rec: Parakeet Ultra | Ultra **17.01**, Canary v2 19.75, v3 **20.63** vs Whisper turbo 70.92 (unusable on Maltese). | Med | `0.2.4` / `0.4.1` |
| `sk` | Slovak | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Parakeet Ultra; Hint: Whisper turbo | Canary v2 6.84 vs Ultra **7.83** (-0.99 pt is inside 1.5 pt noise floor; omit Canary v2), v3 10.19, Whisper 10.21. | Med | `0.4.1` (Ultra) |
| `sl` | Slovenian | Rec: Parakeet v3<br>Hint: Whisper turbo | **A:** Keep current<br>**B:** Rec: Canary 1B v2; Fast: Parakeet Ultra; Hint: Whisper turbo | Canary v2 **12.76** (**-6.22 pts** vs Ultra, **-11.54 pts** vs v3), Ultra **18.98**, Whisper 20.56, v3 24.30. | High | `0.4.1` |
| `mk` | Macedonian | Rec: Qwen 0.6B<br>Hint: Whisper turbo | **A/B:** Rec: Whisper turbo | Whisper turbo **17.85**, Qwen 1.7B 18.22 vs Qwen 0.6B 35.09 (**-17.24 pts**). | High | `0.2.4` |
| `fil` | Filipino | Rec: Qwen 0.6B<br>Hint: Whisper turbo | **A/B:** Rec: Whisper turbo | Whisper turbo **12.08** (`tl` runtime hint) vs Qwen 1.7B 24.29, Qwen 0.6B 35.43 (**-23.35 pts**). | High | `0.2.4` |

---

## 2. New Catalog Entries (Small-Catalog Rule)

A model earns a slot in `MODELS` (`src/transcription_models.rs`) only if it is `Recommended` or `Most accurate` across several languages by `>1.5 points` or materially better for a major language. Three models qualify; seven candidates are rejected.

### Qualifying additions (Phase B, `transcribe-cpp` `0.4.1`)

| `TranscriptionModelId` | Wire name | Display name | Size (Q8_0) | Speed (M4 Max) | License | Pinned artifact repository / filename | Architecture / Variant | Role |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `ParakeetUltra` | `parakeet_ultra` | `Parakeet Ultra` | ~740 MB | ~207x | CC-BY-4.0 | `handy-computer/parakeet-ultra-gguf`<br>`parakeet-ultra-Q8_0.gguf` | `parakeet` / `ultra-0.6b` | Replaces `ParakeetV3` in picker across 24 EU languages (`Rec` for 20, `Fast` for 4); 30 s pause-aware VAD cuts (`0.4.1`). |
| `Canary1BV2` | `canary_1b_v2` | `Canary 1B v2` | ~1.14 GB | ~105x | CC-BY-4.0 | `handy-computer/canary-1b-v2-gguf`<br>`canary-1b-v2-Q8_0.gguf` | `canary` / `canary-1b-v2` | `Rec` for `lv`, `sl` (-6.2 to -7.6 pts vs Ultra); `Acc` for `bg`, `cs`, `da`, `et`, `hr`, `lt`, `ro` (-1.7 to -4.6 pts). Explicit language required (`supports_language_detection: false`). |
| `Qwen3Asr17B` | `qwen3_asr17_b` | `Qwen3-ASR 1.7B` | 2.19 GB | ~45x | Apache-2.0 | `handy-computer/Qwen3-ASR-1.7B-gguf`<br>`Qwen3-ASR-1.7B-Q8_0.gguf` | `qwen3_asr` / `qwen3-asr-1.7b` | `Acc` for `auto`, `yue`, `ko`, `hi`, `vi`, `th`, `id`, `fa` (and `en` pending audit); supports vocabulary hints in `0.4.1` without `0.6B`'s dictionary echo. |

*(Note: `MoonshineStreamingSmall` is tracked separately in Phase 6 of [`docs/plans/transcription-quality.md`](transcription-quality.md) as the internal voice-command recognizer replacing `libmoonshine`, not a dictation-picker entry.)*

### Already-shipped model unlocked in Phase A (`0.2.4`)

- **`CohereTranscribe` (`cohere_transcribe`, 2.41 GB, Apache-2.0):** Already compiled into `MODELS` with `"el"`, `"ar"`, and `"vi"` in `COHERE_LANGUAGES` (`src/transcription_models.rs:243-245`), but currently offered only for `"en"`. Exposing it as `Recommended` for Greek (`el`, 8.96% vs Parakeet v3 35.33%) and `Most accurate` for Arabic (`ar`, 13.60% vs Qwen 0.6B 24.51%) requires zero new catalog entries.

### Rejected candidates

- **`Fun-ASR-MLT-Nano-2512` (891 MB, FunASR Model License v1.1):** Only stands out on Japanese (`2.10%` CER vs Whisper turbo `4.82%`), which looks like FLEURS training-set overlap, carries a custom license with a non-disparagement termination clause, and enables ITN by default. Deferred unless non-FLEURS Japanese benchmarks confirm a real gain.
- **`Fun-ASR-Nano-2512` (891 MB, FunASR license):** Trails `Qwen 0.6B` on `zh` (8.59% vs 7.57%) and `Whisper turbo` on `ja` (8.50% vs 4.82%).
- **`Cohere Transcribe Arabic` (`cohere-transcribe-arabic-07-2026`, 2.41 GB, Apache-2.0):** Single-language 2.41 GB download from a gated upstream repo for a 2.54 pt gain over the already-shipped 14-language `CohereTranscribe` (`11.06%` vs `13.60%` on `ar`).
- **`Whisper large-v3` (1.67 GB, Apache-2.0):** 2x the size and slower than `Whisper turbo`; only materially beats turbo where `Qwen3-ASR` already wins (`yue`, `th`) or ties (`mk`).
- **`Voxtral Mini 3B 2507` (Apache-2.0, 13x Metal):** Within 0.15–0.32 pts of `ParakeetUltra` on `fr`/`it`/`nl`/`pt` at 1/15th the speed.
- **`Nemotron 3.5 ASR Streaming 0.6B` (751 MB, OpenMDW-1.1):** Regressed accuracy on English (10.51%) and European languages (`de` 10.33%, `fr` 10.78%).
- **`Granite Speech 4.1 2B` / `5.0 2B` (Apache-2.0):** `4.1` does not beat Unified/Whisper; `5.0 2B` (5.06% Open ASR Leaderboard) is not yet ported to `transcribe.cpp`.

---

## 3. The English Decision

`ParakeetUnifiedEnglish` (`parakeet_unified_en`, 731 MB, ~188x Metal, 3.99% FLEURS `en`) **stays the default (`Recommended`) model for English**. It is the only model in Hex's catalog that streams on GPU during hotkey capture (`streams: true`), so switching the default to an offline-only model (`parakeet-ultra` or `Qwen3-ASR 1.7B`) would sacrifice release-to-paste latency on every dictation clip.

Two decisions depend on Kit's in-progress own-recordings audit (`ses_e98f7b094ffe1Q3KtZFS6Np4KL`, comparing `ParakeetUnifiedEnglish`, `Qwen3-ASR 1.7B`, and `parakeet-ultra`):

1. **Default (`Recommended`) threshold:** Keep `ParakeetUnifiedEnglish` as `Recommended` unless `parakeet-ultra` or `Qwen3-ASR 1.7B` beats `ParakeetUnifiedEnglish` by **>= 1.5 WER points** on Kit's real dictation corpus *after* accounting for shared vocabulary (Phase 2/4 of `transcription-quality.md`) **and** keeps p95 post-release transcription latency **<= 150 ms** on a 10 s clip.
2. **`Most accurate` slot (`Qwen3-ASR 1.7B` vs `CohereTranscribe`):** `CohereTranscribe` leads only on read audiobooks (LibriSpeech clean 1.27%), while `Qwen3-ASR 1.7B` beats it on both FLEURS `en` (3.23% vs 5.08%, **-1.85 pts**) and the 9-dataset Open ASR Leaderboard (5.75% vs 6.26%) at a slightly smaller size (2.19 GB vs 2.41 GB) and adds vocabulary hint support in `0.4.1`. Replace `CohereTranscribe` with `Qwen3-ASR 1.7B` as English `Most accurate` in Phase B if `Qwen3-ASR 1.7B` ties or beats `CohereTranscribe` on `ses_e98f7b094ffe1Q3KtZFS6Np4KL`.

---

## 4. Verification Before Shipping

Run targeted local checks via `cargo run --release -- benchmark-transcription` on 30–50 clip subsets before locking disputed rows:

1. **Cantonese (`yue`) script normalization (`0.2.4`, before Phase A):**
   - *Claim to check:* Whether `SenseVoice Small` (37.44% CER) and `Whisper turbo` (34.62% CER) fail on Cantonese acoustics or merely emit Simplified characters against Traditional FLEURS references.
   - *Check:* Run 30 `yue` clips through `Qwen3Asr06B`, `SenseVoiceSmall`, and `WhisperLargeV3Turbo` and score both raw and Traditional/Simplified-normalized CER. Even if script-normalized CER improves, check whether users dictating Cantonese receive the expected Traditional script.
2. **Greek (`el`) and Arabic (`ar`) with shipped `CohereTranscribe` (`0.2.4`, before Phase A):**
   - *Claim to check:* Confirm `CohereTranscribe` loads and decodes `el` and `ar` cleanly under Hex's 35 s chunk cap (`max_audio_chunk_samples` in `src/transcription_models.rs:597-609`) and matches the ~9% (`el`) / ~13.6% (`ar`) FLEURS WER on a 30-clip subset.
3. **`Canary 1B v2` language conditioning (`0.4.1`, before Phase B):**
   - *Claim to check:* Verify `canary-1b-v2-Q8_0.gguf` honors `RunOptions` language hints for `lv`, `sl`, `da`, `ro`, `cs`, `hr`, `lt`, `bg`, `et` without requiring a `--prefix` workaround, and measure M-series Metal load/prewarm time and RSS.
4. **`Qwen3-ASR 0.6B` vs `1.7B` vocabulary hints (`0.4.1`, Phase B / `transcription-quality.md` Phase 3):**
   - *Claim to check:* `transcribe.cpp` notes that `0.6B` echoes `--vocabulary` dictionary terms into the output more often than `1.7B`. Run the Phase 1 term-free guard set (`transcription-quality.md`) on both models before setting `supports_recognition_hints: true` on `0.6B` vs `1.7B` only.
5. **`Fun-ASR-MLT-Nano-2512` Japanese outlier (optional, only if considering adding it later):**
   - *Claim to check:* Test `Fun-ASR-MLT` vs `Whisper turbo` on 30 non-FLEURS Japanese clips (Common Voice `ja`) to see if `2.10%` CER is real or FLEURS training contamination.

---

## 5. Migration

Changing `choices_for_runtime` and adding catalog entries must never disrupt existing installs:

1. **Never auto-switch or auto-download:**
   - `AppSettings::load_from` (`src/app_settings.rs:1086`) validates saved selections through `transcription_models::validate(&settings.transcription)` (`src/transcription_models.rs:569-591`), which checks `model.available()` and `model.supports_language(&selection.language)` — **not** `choices_for_runtime()`.
   - A user whose saved selection is `(ParakeetV3, "de")`, `(ParakeetV3, "el")`, or `(Qwen3Asr06B, "tr")` keeps their exact model and language on upgrade with zero background downloads or migrations.
   - `ParakeetV3` (`"parakeet_v3"`), `CohereTranscribe` (`"cohere_transcribe"`), `Qwen3Asr06B` (`"qwen3_asr06_b"`), and `SenseVoiceSmall` (`"sense_voice_small"`) remain `available() == true` in `MODELS` with stable wire names.
2. **Menu-bar picker stays intact:**
   - `src/macos_rune/status_item.rs:340` lists all `MODELS` that are `available()` and `is_installed()`, remembering each model's last language via `transcription_recents`. Users with `ParakeetV3` installed can still switch to and from it in the menu bar.
3. **Keep active/installed legacy models visible and deletable in the Settings picker (`src/rune_ui/picker.rs`):**
   - Today `picker::modal` (`src/rune_ui/picker.rs:170`) only renders cards from `choices_for_runtime(&language)`. If a user upgrades with `ParakeetV3` active on French (or installed on disk using 740 MB) after `ParakeetUltra` replaces `ParakeetV3` in `choices_for_runtime("fr")`, `ParakeetV3` would disappear from the French picker modal, hiding both its `Active` indicator and its `Delete` button.
   - **Fix in `picker.rs`:** Make `ModelChoice::recommendation` an `Option<Recommendation>` (or append a tail iterator in `picker::modal`) for any model in `MODELS` that supports `language`, is not already in `choices_for_runtime(&language)`, and is either the user's active selection for `language` or `is_installed()`. Render its card without a recommendation badge so the user sees `Active` and can click `Delete` to reclaim the 740 MB / 850 MB after switching to the new `Recommended` model.

---

## 6. Phasing and Integration with `transcription-quality.md`

```ts
Rollout timeline
├── Phase A (now, on transcribe-cpp 0.2.4, 0.5d)
│   └── Pure routing & picker cleanup in src/transcription_models.rs + src/rune_ui/picker.rs
└── Phase B (with transcription-quality.md Phase 3, transcribe-cpp 0.4.1, +1d)
    └── Add ParakeetUltra, Canary1BV2, Qwen3Asr17B + full European/Asian table
```

### Phase A: Immediate Routing Fixes on `0.2.4` (Effort: 0.5 days)

Ships immediately before any crate bump or new model downloads:
- **`src/transcription_models.rs` (`choices_for_runtime`):**
  - Route `"el"` (Greek) to `[Rec: CohereTranscribe, Hint: WhisperLargeV3Turbo]` before the `PARAKEET_V3_LANGUAGES` arm.
  - Route `"fi"` and `"sv"` to `[Rec: WhisperLargeV3Turbo, Fast: ParakeetV3]`.
  - Route `"da"` to `[Rec: WhisperLargeV3Turbo, Fast: ParakeetV3]` (until `ParakeetUltra` + `Canary1BV2` land in Phase B).
  - Route `"ja"` to `[Rec: WhisperLargeV3Turbo, Fast: SenseVoiceSmall]`.
  - Drop `SenseVoiceSmall` (`Fastest`) from `"yue"` and `"ko"`.
  - Split the `QWEN_LANGUAGES` arm: keep `Qwen3Asr06B` `Recommended` for `"hi" | "vi" | "th" | "id"` (adding `Acc: CohereTranscribe` on `"vi"`), and route `"ar" | "tr" | "ms" | "fil" | "fa" | "mk"` to `Rec: WhisperLargeV3Turbo` (adding `Acc: CohereTranscribe` on `"ar"`).
  - In `AUTO_LANGUAGE`: drop `SenseVoiceSmall` and change `Qwen3Asr06B` from a second `Recommended` to `Fastest` (`[Rec: WhisperLargeV3Turbo, Fast: Qwen3Asr06B]`).
  - In `"mt"` (Maltese): omit `WhisperLargeV3Turbo` (`RecognitionHints`) where it gets 70.92% WER.
- **`src/rune_ui/picker.rs`:**
  - Show installed or active models supporting `language` that are no longer in `choices_for_runtime(&language)` without a recommendation badge so users can see their active model and delete old weights.
  - Update unit tests in `src/transcription_models.rs`.

### Phase B: New Models with `transcribe-cpp` `0.4.1` (Effort: +1 day inside [`docs/plans/transcription-quality.md`](transcription-quality.md) Phase 3)

Integrates directly into Phase 3 of `docs/plans/transcription-quality.md` (which already budgets 2 days for the `0.2.4 -> 0.4.1` bump and `ParakeetUltra`):
- Pin `ParakeetUltra`, `Canary1BV2`, and `Qwen3Asr17B` in `MODELS` with verified byte counts and SHA-256 checksums.
- Swap the `PARAKEET_V3_LANGUAGES` branches from `ParakeetV3` to `ParakeetUltra`, add `Canary1BV2` as `Recommended` on `lv`, `sl` (with `ParakeetUltra` as `Fastest`) and as `Most accurate` on `bg`, `cs`, `da`, `et`, `hr`, `lt`, `ro`.
- Add `Qwen3Asr17B` as `Most accurate` on `auto`, `yue`, `ko`, `hi`, `vi`, `th`, `id`, `fa`, and (if confirmed by `ses_e98f7b094ffe1Q3KtZFS6Np4KL`) `en`.
- Enable `supports_recognition_hints: true` on `Qwen3Asr17B` (and `Qwen3Asr06B` if it passes the Phase 4 dictionary-echo check).

---

## Decisions for Kit

1. **Ship Phase A (`el`, `tr`, `mk`, `ar`, `ms`, `fil`, `fa`, `ja`, `fi`, `sv`, `auto` fixes on `0.2.4`) now, or batch everything into the `0.4.1` bump?**
   - *Recommendation:* **Ship Phase A now (0.5d)** — it fixes broken defaults like Greek (35.3% -> 9.0% WER), Filipino (35.4% -> 12.1%), Persian (50.3% -> 30.6%), Macedonian (35.1% -> 17.9%), and Turkish (16.7% -> 7.0%) using models Hex already ships.
2. **Which new models earn a catalog slot in Phase B alongside `ParakeetUltra`: add both `Canary 1B v2` (1.14 GB) and `Qwen3-ASR 1.7B` (2.19 GB), or only one?**
   - *Recommendation:* **Add both `Canary 1B v2` and `Qwen3-ASR 1.7B` (3 new models total with `ParakeetUltra`), and reject the other 7 candidates** — `Canary 1B v2` cuts WER by 1.7–7.6 points across 9 European languages, while `Qwen3-ASR 1.7B` is the `Most accurate` tier for `Auto`, Asian/Middle Eastern languages, and potentially English.
3. **On Latvian (`lv`) and Slovenian (`sl`), where `ParakeetUltra` is 18–19% WER and `Canary 1B v2` is 10.4–12.8% WER, should `Canary 1B v2` be `Recommended` (1.14 GB default download) or `Most accurate`?**
   - *Recommendation:* **Make `Canary 1B v2` `Recommended` and `ParakeetUltra` `Fastest` on `lv` and `sl`** — a 6.2 to 7.6 point WER drop (nearly cutting errors in half at 105x realtime) is worth the extra 400 MB on first download.
