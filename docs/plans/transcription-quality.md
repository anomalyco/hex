# Transcription Quality, Custom Vocabulary, and Runtime Unification

**Status:** Plan, 2026-10-22. Synthesizes [`docs/research/model-watch.md`](../research/model-watch.md) and [`docs/research/custom-vocabulary.md`](../research/custom-vocabulary.md) against the current tree (`2.3.1`).

## Summary

Hex's default model (`ParakeetUnifiedEnglish`) is fast (~188x realtime on Metal) and accurate on general English (5.91% 8-set WER), but it has no vocabulary input today and misrecognizes domain terms and names into common English words (`"Hex"` -> `"Hanks"`, `"Abattoir"` -> `"Avatar"`, `"OpenCode"` -> `"open code"`). Blind find-and-replace cannot fix real-word collisions like `"Hanks"` or `"Avatar"` without corrupting legitimate sentences. Meanwhile, in-call meeting transcripts are moving onto rolling Parakeet chunks on the shared `TranscriptionEngine` (with Moonshine being dropped from `src/meeting_live.rs`), leaving voice commands (`src/moonshine.rs`) as the last consumer of the ONNX Runtime + `libmoonshine` stack.

```ts
Transcription quality plan (ordered by value / effort)
├── Phase 1: Term & false-replacement eval harness (2d)
│   └── Extends `hex benchmark-transcription` + corpus scripts with `expected_terms` and a term-free guard set
├── Phase 2: Shared vocabulary, Rune UI, and replacement fixes (4d)
│   └── App-wide vocabulary + "heard as" aliases in Settings, History 1-click add, #139/#177 fixes, OpenCode & meeting wiring
├── Phase 3: `transcribe-cpp` 0.2.4 -> 0.4.1 & catalog refresh (2d)
│   └── Unlocks `RunOptions::vocabulary`, Qwen3-ASR hints (#129), and `parakeet-ultra` (replaces Parakeet v3)
├── Phase 4: Parakeet greedy-decode phrase boost in `transcribe.cpp` (5d spike + 5d ship)
│   └── Host CPU logit biasing + SentencePiece tokenizer + margin gate (`"Hanks"` -> `"Hex"`, `"Avatar"` -> `"Abattoir"`)
├── Phase 5: In-call meeting transcripts & optional Apple live tail (3d)
│   └── Tightens rolling Parakeet pause cuts (6-10 s) with shared vocabulary; optional ~2% CPU `SpeechTranscriber` draft tail
└── Phase 6: Moonshine Streaming GGUF for voice commands (5d)
    └── Replaces `libmoonshine` + ONNX Runtime + `download.moonshine.ai` with 199 MB Q8 GGUF on `transcribe-cpp`
```

### Hard invariants preserved across all phases

- **Dictation is never delayed by meetings:** meeting chunks run on `TranscriptionEngine` at utility QoS only while `!parakeet::dictation_busy()`, never interrupt a live stream, and finish within the 300 ms hold threshold + 200 ms release tail.
- **One loaded dictation model, one active stream per `Model`:** `transcribe-cpp` 0.4.1 still enforces a single active compute/stream lease per `Model` (`Session::stream` returns `Error::Busy` on concurrent use). No second Parakeet `Model` (+0.74 GB RAM) is ever loaded for meetings or commands.
- **Local-only by default, strict GPU where required:** Metal on macOS (`src/parakeet.rs`) and Vulkan on Linux / Windows x64 (`src/local_transcriber.rs`).

---

## Phase 1: Eval Corpus and Benchmark Gate

- **Goal:** Measure term recall and false-replacement regressions before shipping any vocabulary, post-processing, or decoder change.
- **Changes:**
  - Extend `ClipDefinition` in `src/transcription_benchmark.rs` (`hex benchmark-transcription`) with optional `expected_terms: Vec<String>` and add `--vocabulary <path>` to load a vocabulary/alias list into `Parakeet::load_for_benchmark`.
  - Emit two new summary metrics in `src/transcription_benchmark.rs` JSONL output:
    - `term_recall`: fraction of `expected_terms` present in the output (case- and spacing-normalized).
    - `false_replacements`: count and rate of vocabulary terms appearing in transcripts when absent from `expected`.
  - Build a two-part local benchmark manifest reusing the corpus infrastructure landed in `f4755e4` (`scripts/dictation-corpus.sh` and `src/dictation_corpus_tests.rs`, merged from `test/dictation-corpus`):
    1. **Target-term set (~100 clips):** `say` voices, ElevenLabs clips, and Kit's real recordings containing `Hex` (including the real `"Hanks"` miss), `Abattoir` (the `"Avatar"` miss), `OpenCode`, `Rune`, `Scrawl`, `Effect`, `Anomaly`, and teammate names.
    2. **Term-free guard set (200 LibriSpeech `test-clean` clips + Kit's term-free dictation clips):** includes legitimate sentences using common words like `"Tom Hanks"` and `"Avatar"` to catch over-eager biasing.
- **Measurement & gate:** Harness runs end-to-end via `cargo run --release -- benchmark-transcription` and reports baseline `ParakeetUnifiedEnglish` WER, `term_recall`, `false_replacements`, `median_rtf`, `p95_clip_ms`, and `/usr/bin/time -l` peak resident memory. Every subsequent phase gates on: **WER delta <= +0.3 pt on the term-free set, false replacements <= 0.5%, and p95 latency delta < 20 ms on a 10 s clip.**
- **Effort:** 2 days.
- **Risk:** Low. Synthetic TTS clips alone overestimate phonetic regularity; mitigated by requiring real microphone and meeting-miss clips in the manifest.
- **Unblocks:** Objective go/no-go gates for Phases 2, 3, and 4.

---

## Phase 2: Shared Vocabulary List, Replacement Fixes, and Rune UI

- **Goal:** Give users one place to define names, jargon, and known misrecognitions that works across every model, mode, dictation, and meeting transcript.
- **Changes:**
  - **Fix `src/text_replacements.rs` bugs (#139, #177):**
    - `#177` (CJK never matches): `phrase_boundary` currently requires `!word_character` (`^\w$`) on both sides, which rejects CJK matches inside unspaced sentences. Require word-character boundaries only on edges where the matched phrase itself starts or ends with a space-delimited script (Latin/Cyrillic/Greek/digits); CJK characters self-delimit.
    - `#139` (overlapping matches skipped): `matcher.find_iter(text)` skips overlapping occurrences of the same rule if an earlier match failed `phrase_boundary`. Scan from the next UTF-8 character boundary after any rejected match.
  - **Data model (`src/app_settings.rs`):**
    - Add `pub vocabulary: Vec<VocabularyEntry>` (`{ term: String, aliases: Vec<String> }`) to `AppSettings` in `settings.json`.
    - Add a one-time migration in `MIGRATIONS` that imports any non-empty `transcription.recognition_hints` and `transcription_recents[*].recognition_hints` (comma/newline-split) into `vocabulary`, and removes the `!model.supports_recognition_hints` error from `transcription_models::validate` so switching models never fails when hints are present.
  - **Pipeline wiring (`src/parakeet.rs`, `src/local_transcriber.rs`, `src/dictation_processor.rs`, `src/meeting_transcript.rs`):**
    - **Model hints:** Pass `vocabulary` terms (capped at 64 terms to prevent Whisper silence hallucinations, #108/#193) to models with `supports_recognition_hints`.
    - **Global alias replacements:** Compile unambiguous `aliases -> term` pairs ( plus multi-word compound joins like `"open code" -> "OpenCode"`) into a shared `ReplacementSet` that runs before mode-specific `replacements` in `dictation_processor.rs`, `dictation_modes.rs`, and on rolling/final chunks in `meeting_transcript.rs`.
    - **OpenCode rewrite prompt (`src/dictation_processor.rs`):** When a mode enables OpenCode rewriting, append a compact `Vocabulary` block (`"Prefer these exact spellings when a word sounds like one: Hex (heard as: Hanks), Abattoir (heard as: Avatar), OpenCode, Rune, Scrawl, Effect…"`). Because the LLM sees sentence context, it resolves real-word collisions (`"Hanks"`, `"Avatar"`) without corrupting ordinary prose.
  - **Rune UI (`src/rune_ui/settings.rs`, `src/rune_ui/history.rs`, `src/desktop_i18n.rs`):**
    - Follow [`docs/design-system.md`](../design-system.md): replace the Whisper-only `Recognition hints` row in `settings::dictation_section` with a `Vocabulary` `section` card visible for every model.
    - Compose existing `rune_ui/controls.rs` pieces: `card_header` with a Ghost `PLUS` `IconButton` (`"Add term"`) and a Secondary `"Import…"` / `"Export…"` action (`#160`); rows with `TextInput` for `Term (e.g. Abattoir)` and optional `Heard as (e.g. Avatar)`, plus `remove_button`.
    - In `src/rune_ui/history.rs`, add a Secondary `"Add to vocabulary…"` action in `detail_header` when `raw_text` differs from `final_text` or is selected, prefilling the misheard phrase.
    - Add all five language translations in `src/desktop_i18n.rs` and update `--snap` scenes (`mac-settings`, `history`) in light and dark.
- **Measurement & gate:**
  - Unit tests in `src/text_replacements.rs` covering CJK phrases (`#177`) and overlapping candidates (`#139`).
  - Phase 1 benchmark: 100% recall on unambiguous aliases (`"open code" -> "OpenCode"`, `"scrawl"`, `"rune"`) and LLM-rewritten `"Hanks"` / `"Avatar"` sentences; `cargo test --bin hex rune_ui` green with updated light/dark goldens.
- **Effort:** 4 days.
- **Risk:** Low. Users could enter a common word as an unconditional alias (e.g. `"Hanks" -> "Hex"`); mitigate in Phase 4 by routing single-word real-word collisions through acoustic margin gating when Parakeet is active, or warning inline when an alias is a common dictionary word.
- **Unblocks:** Feeds a single user vocabulary into Qwen3-ASR hints (Phase 3), Parakeet decoder boosting (Phase 4), and meeting transcripts (Phase 5).

---

## Phase 3: `transcribe-cpp` Bump (`0.2.4` -> `0.4.1`) and Catalog Refresh

- **Status (2026-10-26):** The dependency bump is done with behavior unchanged: same transcripts on the vocabulary corpus, weights now mmap'd (peak footprint about 0.9 GB to 0.15 GB on Metal). Hints, the catalog refresh, and Vulkan prewarm verification remain.
- **Goal:** Upgrade `transcribe-cpp` to `0.4.1` to unlock generic `RunOptions::vocabulary` / `prompt`, enable Qwen3-ASR recognition hints (`#129`), and replace `ParakeetV3` with `parakeet-ultra`.
- **Changes:**
  - **Dependency bump (`Cargo.toml`, `src/gguf_session.rs`, `src/parakeet.rs`, `src/local_transcriber.rs`):**
    - Bump `transcribe-cpp` from `=0.2.4` to `=0.4.1` at all three target entries in `Cargo.toml` (default, Linux `vulkan`, Windows x64 `vulkan`).
    - Update `model.capabilities()` call sites (`src/gguf_session.rs:110`, `src/parakeet.rs:1073`, `src/local_transcriber.rs:154`) to handle `Result<Capabilities>`.
    - Replace the Whisper-only `RunExtension::Whisper` hint wiring with `0.4.1`'s generic `RunOptions::vocabulary` / `prompt` capability bits (`docs/prompting.md`), and set `supports_recognition_hints: true` on `Qwen3Asr06B` in `src/transcription_models.rs` (`#129`).
  - **Model catalog (`src/transcription_models.rs`):**
    - Add **`ParakeetUltra`** (`handy-computer/parakeet-ultra-gguf`, `parakeet-ultra-Q8_0.gguf`, ~740 MB, CC-BY-4.0, 25 languages): Moondream's post-trained v3 that drops LibriSpeech clean WER from 1.94% to 1.80% (8-set leaderboard 5.32% vs v3's 6.32%) and cuts long audio at natural pauses (30 s segments).
    - Recommend `ParakeetUltra` in `choices_for_runtime` for the 24 non-English European languages where `ParakeetV3` is recommended today; keep `ParakeetV3` deserializable in `TranscriptionModelId` so existing `settings.json` files continue to work or migrate cleanly.
- **Measurement & gate:**
  - `hex benchmark-transcription` comparing `0.2.4` vs `0.4.1` on `ParakeetUnifiedEnglish`: no WER regression, `median_rtf` and `p95_clip_ms` within 5%, and 8 s stream prewarm (`STREAM_PREWARM_SAMPLES`) passing on Metal and Vulkan.
  - Verify `Qwen3Asr06B` with Phase 2 vocabulary hints on English/French/Mandarin product-name clips (`#129`).
  - Compare `ParakeetUltra` vs `ParakeetV3` on the benchmark manifest (WER and pause segmentation on >30 s clips).
- **Effort:** 2 days.
- **Risk:** Low. `0.4.1` changes C++ internals across architectures; prewarm and peak RSS must be verified on macOS Metal before pushing.
- **Unblocks:** Provides the `0.4.1` `RunOptions::vocabulary` plumbing and C++ base for Phase 4.

---

## Phase 4: Parakeet Greedy-Decode Phrase Boost

- **Goal:** Bias Parakeet's acoustic decoder toward the user's vocabulary during inference so `"Hex"`, `"Abattoir"`, `"OpenCode"`, `"Rune"`, `"Scrawl"`, `"Effect"`, and people's names win over phonetically similar common words (`"Hanks"`, `"Avatar"`) without an LLM call or a second model.
- **Why this works in Parakeet:**
  - In `transcribe.cpp` (`src/arch/parakeet/decoder.cpp`), `decode_rnnt_greedy` (Unified English), `decode_tdt_greedy` (v2/v3/ultra), and streaming RNN-T all run on the host CPU (`n_threads = 1` on GPU devices in `src/gguf_session.rs`).
  - At every step, `joint_step` copies the joint network output into a host `std::vector<float> logits`, applies `log_softmax`, and selects `argmax_range(token_logits, n_token_cls)` with `blank_id = vocab`.
- **Changes:**
  - **Step 4a: 5-day spike in a local `transcribe.cpp` 0.4.1 checkout:**
    1. **SentencePiece encoder:** Implement tokenization for Parakeet's 1,024-piece SentencePiece vocabulary stored in the GGUF metadata (replacing the `INT_MIN` stub in `transcribe_tokenize`), generating casing variants (`"Hex"`, `" hex"`, `"Abattoir"`, `" abattoir"`).
    2. **Aho-Corasick / prefix trie shallow fusion (GPU-PB style, arXiv 2508.07014):** Build a token-level prefix trie with failure links from `RunOptions::vocabulary`.
    3. **Acoustic margin gate + prefix rollback:** At each `joint_step`, only apply `boost` (e.g. +1.5 to +3.0 nats) to a trie-advancing token `t` when `logits[best_non_blank] - logits[t] <= margin` (the acoustics already support `t`). To prevent a multi-token term (`"Abattoir"`, `"OpenCode"`) from stranding a broken first subword if a later subword fails the acoustic margin gate, keep a single shadow predictor state while inside an active multi-token trie prefix and commit it only when the full term completes (zero extra compute outside active prefixes).
  - **Step 4b: Ship upstream + fork bridge (`Cargo.toml`, `src/parakeet.rs`, `src/local_transcriber.rs`):**
    - Wire `Parakeet` and `LocalTranscriber` to pass `settings.vocabulary` terms into `RunOptions::vocabulary` (both offline `run` and `with_stream`).
    - **Upstream vs. fork strategy:** Open an upstream PR to `handy-computer/transcribe.cpp` filling Parakeet's currently empty `vocabulary` slot in `docs/prompting.md` and `transcribe_tokenize`. While upstream reviews, carry a narrow `[patch.crates-io]` git patch pointing to `anomalyco/transcribe.cpp` (pinned rev, updating `nix/package.nix` hashes if needed) so Hex ships immediately, then return to the crates.io pin once upstream releases.
    - **Fallback if greedy spike misses the gate:** Confidence-gated phonetic snap in Rust using `Transcript::tokens` (`Token::p`) and word timestamps already returned by `transcribe_segments` (`src/parakeet.rs:1190`), with FluidAudio-style guards (>=80% similarity for <=4-char words, >=85% with stop words).
- **Measurement & gate:**
  - Must pass the Phase 1 gate on `ParakeetUnifiedEnglish`: **term recall >= 70% (including `"Hanks" -> "Hex"` and `"Avatar" -> "Abattoir"` real clips), false-replacement rate <= 0.5% on the 200-clip LibriSpeech + term-free guard set, overall WER delta <= +0.3 pt, and p95 decode overhead < 5 ms per clip.**
- **Effort:** 5 days (spike) + 5 days (streaming path, `[patch.crates-io]`, upstream PR).
- **Risk:** Medium. Over-boosting short 1-token words (`"Hex"`) can cause false replacements on similar rhymes (`"checks"`, `"text"`, `"necks"`). Mitigated by scaling the required acoustic margin tighter for short words (<=4 chars) and allowing per-entry `"heard as"` aliases to restrict when a short word boosts.
- **Unblocks:** Zero-latency custom vocabulary in hotkey dictation and rolling meeting transcripts without requiring OpenCode.

---

## Phase 5: In-Call Meeting Transcripts and Live Drafts

- **Goal:** Make meeting transcripts accurate and up-to-date *during* the call (both in the Meetings pane and `Option-Control-V` meeting paste) without loading a second GGUF model or delaying dictation.
- **Coordination with in-flight work (`ses_e992a1790ffej3W41HZ62GVxQb`):**
  - The other session is switching in-call copies/views to rolling Parakeet chunks on the shared `TranscriptionEngine` and removing Moonshine from `src/meeting_live.rs`.
- **Changes:**
  - **Faster pause-aware rolling chunks (`src/meeting_transcript.rs`):**
    - Today `CHUNK_MIN = 10 s`, `CHUNK_TARGET = 20 s`, `CHUNK_MAX = 30 s` (`src/meeting_transcript.rs:36-40`), which leaves in-call rolling transcripts 20-30 s behind live speech.
    - Lower `CHUNK_MIN` to `5 s` and `CHUNK_TARGET` to `8 s` (keeping `CHUNK_MAX = 25 s` and `CHUNK_OVERLAP = 2 s` for continuous talkers). At ~190x realtime on Metal, an 8 s chunk takes ~42 ms on the shared engine thread, finishing well inside dictation's 300 ms hold threshold + 200 ms release tail.
  - **Vocabulary in rolling and final meeting chunks (`src/meeting_transcript.rs`):**
    - Apply Phase 2's shared vocabulary replacements and Phase 4's `RunOptions::vocabulary` to rolling and final meeting chunks so `"Hex"`, `"Abattoir"`, `"OpenCode"`, and participant names are right on the first rolling pass.
  - **Optional sub-second volatile tail via Apple `SpeechTranscriber` (`src/apple_speech.rs`, `src/meeting_live.rs`, macOS 26+):**
    - Once Moonshine is removed from `meeting_live.rs`, the only gap is the 0-8 s sentence currently being spoken before a pause cut lands.
    - If Kit wants live word-by-word captions for that active sentence, stream 100 ms Int16 buffers per track into Apple's on-device `SpeechAnalyzer` / `SpeechTranscriber` (`docs/plans/meeting-live-transcription.md`), emitting `TranscriptPhase::Updated` / `Completed` into `transcript.live.ndjson` after `rolling.covered_ms`. Rolling Parakeet overwrites the tail as each 5-8 s chunk completes.
- **Measurement & gate:**
  - Replay a recorded speaker-mode call through `meeting_echo_replay`:
    - Rolling Parakeet turn latency after a pause drops from ~20 s to **p50 <= 6 s, p95 <= 10 s**.
    - Zero increase in speaker-echo leaks (`You` false positives) or dropped local `You` turns.
    - Dictation latency while a meeting records stays at baseline (**p50 ~70 ms, p95 < 110 ms** on an 8 s dictation clip).
    - Peak resident memory drops by **~400 MB** compared to the old dual-stream Moonshine live worker (and stays at 0 MB extra model RAM if Apple `SpeechTranscriber` (~2% CPU) is enabled).
- **Effort:** 3 days (1 day for rolling cadence + vocabulary; 2 days if adding the `SpeechTranscriber` volatile tail).
- **Risk:** Low. More frequent 5-8 s chunks slightly increase Metal wakeups (~15 chunks/min across two tracks = ~0.6 s of GPU work per minute, ~1% duty cycle), still yielding immediately whenever `parakeet::dictation_busy()` is true.
- **Unblocks:** Complete removal of Moonshine from meetings, freeing Phase 6 to simplify `src/moonshine.rs` around commands alone.

---

## Phase 6: Moonshine Streaming GGUF for Voice Commands

- **Goal:** Replace the ONNX Runtime + `libmoonshine` C library (`src/moonshine.rs`) and `download.moonshine.ai` downloader with `moonshine-streaming-small` Q8_0 GGUF through `transcribe-cpp`.
- **Why Moonshine GGUF instead of streaming Parakeet for commands:**
  - Always-on commands cannot share the dictation model's single `Session::stream` lease without blocking dictation, and a second Parakeet `Model` costs +740 MB RAM and continuous 5.6 s left-context GPU recomputation.
  - At command latency (<300 ms), `moonshine-streaming-small` (199 MB Q8_0, 7.84% 8-set WER, 2.54% LibriSpeech clean) beats Parakeet Unified at 160 ms (8.44% WER) and runs at 129x realtime on Metal or 64x on CPU.
- **Changes:**
  - Add `moonshine-streaming-small-Q8_0.gguf` (199 MB, `handy-computer/moonshine-streaming-small-gguf`, pinned revision and SHA-256) to Hex's Hugging Face model downloader (`src/transcription_models.rs`).
  - Rewrite `src/moonshine.rs` on top of `transcribe-cpp`'s `moonshine_streaming` architecture:
    - Because `transcribe-cpp`'s Moonshine stream returns committed/tentative text and caps a single stream at 81.9 s without built-in VAD line events, drive line lifecycle with Hex's existing 16 kHz `PauseSegmenter` (or `transcribe-cpp`'s built-in Silero VAD v6.2).
    - Start a stream on speech onset (`TranscriptPhase::Started`), emit `TranscriptPhase::Updated` when tentative/committed text changes (preserving low-latency voice-delimited dictation triggers), and finalize on pause (`TranscriptPhase::Completed`) for ordinary command resolution in `src/recognition.rs`.
  - Delete `libmoonshine.dylib` dynamic loading (`libloading`), `MOONSHINE_ORT_SINGLE_THREAD` in `src/main.rs`, the 8-file `.ort` manifest, and the `download.moonshine.ai` host requirement.
- **Measurement & gate:**
  - Replay the `moonshine_lab` command fixture suite (`cargo run --profile moonshine-lab -- ...`) comparing ONNX Moonshine vs GGUF Moonshine small:
    - Command match accuracy and wake-phrase recall >= ONNX baseline.
    - Update latency **p50 <= 35 ms, p95 <= 70 ms**.
    - Active command CPU <= **20% of one core** (vs 22% single-threaded ONNX today) and resident memory <= **300 MB** (down from ~400 MB).
- **Effort:** 5 days.
- **Risk:** Medium. Command responsiveness depends on endpointing hangover when mapping VAD segments onto `Started`/`Updated`/`Completed` lines; tuning against `moonshine_lab` fixtures before deleting the ONNX path prevents regressions.
- **Unblocks:** One inference engine (`transcribe.cpp`) and one model downloader (Hugging Face with SHA-256 receipts) across the entire codebase.

---

## Decisions for Kit

1. **Where should the shared Vocabulary UI live?**
   - *Recommendation:* Put the shared **Vocabulary** card in **Settings -> Dictation** (visible for every model, replacing the Whisper-only `Recognition hints` row) plus a 1-click **"Add to vocabulary"** button on History entries, while keeping per-mode **Corrections** in **Modes** for mode-specific text rules.
2. **Carry a temporary `anomalyco/transcribe.cpp` `[patch.crates-io]` fork for Phase 4 Parakeet phrase boosting, or wait for upstream merge?**
   - *Recommendation:* **Carry a pinned `[patch.crates-io]` fork while the upstream PR is in review** so Hex gets Parakeet vocabulary boosting immediately once the 5-day spike clears the benchmark gate.
3. **In-call meetings after Moonshine is dropped: is 5-8 s rolling Parakeet enough, or also add the Apple `SpeechTranscriber` (~2% CPU, 0 MB RAM) volatile tail?**
   - *Recommendation:* **Ship 5-8 s pause-cut rolling Parakeet with shared vocabulary first (1 day)**; add the Apple `SpeechTranscriber` volatile tail only if you miss seeing words appear mid-sentence before a pause.
4. **Replace `Parakeet v3` in the picker with `parakeet-ultra` in Phase 3, or list both?**
   - *Recommendation:* **Replace `Parakeet v3` with `parakeet-ultra` in the picker** (keeping `parakeet_v3` working in existing `settings.json` files), since `parakeet-ultra` is the same 740 MB Q8 size and 25-language footprint with strictly better WER and pause-aware long-audio segmentation.
