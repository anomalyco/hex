# Custom vocabulary for Parakeet

**Status:** Research, 2026-10-22. Effort figures are my estimates. Claims that
come from a vendor's own benchmark or a third party's write-up are labeled as
such. **Update 2026-10-28:** a vocabulary boost is now implemented as
post-decode rescoring rather than a decode-loop bonus; see
[`vocabulary-boost-results.md`](vocabulary-boost-results.md).

**Question (Kit):** People want voice replacements and vocabulary, and Hex's
default model, Parakeet, has no hints. Can we build something ourselves,
including training or modifying the model?

**Short answer:** Yes, and most of it is cheaper than training.

- The model is not the obstacle. Hex's Parakeet decode loop runs on the host
  CPU, one token at a time, over a plain logits array (see
  [Why Parakeet can be biased at all](#why-parakeet-can-be-biased-at-all)).
  Adding a vocabulary boost is a small change to that loop. It is not a model
  change.
- Today's replacements cannot fix Kit's real misses. "Hanks" and "Capuchin" are
  real words. A blind replace corrupts every legitimate use.
- Per-user fine-tuning is not worth it. Training once, centrally, on a
  developer-vocabulary corpus is a plausible later bet.

## 1. What users ask for

### Hex's own issues

Both repos were read in full through the issue list on 2026-10-22.
`kitlangton/Hex` is the legacy Swift app; `anomalyco/hex` is this repo.

| Issue | Ask | Notes |
|---|---|---|
| kitlangton/Hex #154 (Jan 2026) | "Custom Dictionary or Default Dictionary" | Parakeet turns `Context7 MCP` into `Contact7 MCP`, and `Oxfmt`/`Oxlint` into other words. The reporter keeps hand-written replacements and says they work "8/10 times". They also point at Aqua's Avalon model as the thing that fixes this properly. |
| kitlangton/Hex #118 (Nov 2025) | "Add dictionaries for transcription" | Closed as "already exists" (Word Remappings). The answer given was that Parakeet "doesn't support custom dictionaries at the transcription level". |
| kitlangton/Hex #160 | Bulk import/export of remappings (CSV) | Workaround is hand-editing `hex_settings.json`. |
| kitlangton/Hex #187 (Mar 2026) | Screen-aware context: OCR or Accessibility text fed to the recognizer | Same problem class as #154. The reporter notes Parakeet has no prompt API, so this only helps Whisper. They claim Wispr Flow works this way. |
| anomalyco/hex #129 (Oct 2026) | Recognition hints for Qwen3-ASR | The reporter dictates English and French with many product names, and the names come out wrong. They found that transcribe-cpp 0.4.0 added generic `vocabulary` and `prompt` run options, and that Qwen3-ASR supports them. They have a working patch outline. A second user (China, English loanwords like "issue" and "token") asked for the same. |
| anomalyco/hex #177, #139 | Replacement matching bugs | CJK text never matches, and an overlapping phrase is skipped. These bugs block replacements from being the main vocabulary tool. |
| anomalyco/hex #97 | Filler-word removal | Wants regex-style matching, which the current Corrections can only approximate. |
| anomalyco/hex #15 | Suggest vocabulary corrections from the user's explicit edits | Closed "not planned" on 2026-07-25. There is no suggestion code in `src/`. |

Nothing I found asks for training. Every request is "make my words come out
right", and the reporters assume a list of terms.

### Competitors

All claims below are from each product's own docs unless noted.

| Product | What "vocabulary" is | How it works |
|---|---|---|
| Superwhisper | A short word list plus a separate Replacements list | Words are "sent alongside your audio as recognition hints". The docs warn that too much vocabulary "can confuse the transcription model", affects punctuation, and hurts language detection, and say to "keep the vocabulary list short". A troubleshooting page for "Gibberish, repeated, or made-up text" says to trim the vocabulary. Replacements run after transcription, case-insensitively, with "no AI interpretation". A CLI (`superwhisper vocab add`) lets a coding agent mine history for terms. A third-party Chirp spec says Superwhisper does not use hints for Parakeet. I did not verify that against Superwhisper itself. |
| Wispr Flow | Personal dictionary of words, plus "Correct a misspelling" pairs | The cloud transcribes, then an LLM pass edits. The dictionary shapes both. The docs say Flow "uses names and context visible on screen, such as email recipients, to recognize proper nouns". It auto-adds words when the user corrects dictated text (proper nouns, trade terms, uncommon names). It keeps file names seen in Cursor, Windsurf and VS Code. It has a team-shared dictionary. Limit is 60 characters per entry. |
| MacWhisper | Global find-and-replace only | Case-sensitive and whole-word switches, JSON import. No recognition-time bias. |
| VoiceInk | Dictionary and replacements | The docs page was a client-rendered shell and I could not read it. Not characterized. |
| Aqua Voice | Custom dictionary, plus its own model | Its Avalon model was trained on data for how people talk to AI tools. Its blog reports 97.4% key-term accuracy on its own AISpeak-10 benchmark, against 51.5% for Canary 1B and 65.1% for Whisper Large v3. That is a vendor-run benchmark. The blog says outright that models like Parakeet "make very silly and obvious mistakes in real-world testing". |
| TypeWhisper (Mac) | Terms plus corrections | Terms can be passed to engines that accept them. For Parakeet it offers an opt-in "vocabulary boosting" that "rescores" Parakeet's first result against dictionary terms on device, with Precise/Balanced/Strong similarity thresholds. It also learns corrections from edits, shows "Saved to Dictionary: wrong -> right" with Undo, and skips sentence rewrites, deletions and ambiguous edits. |
| MacParakeet, FluidAudio, Chirp | Open-source Parakeet apps and libraries | Three different mechanisms: CTC rescoring (FluidAudio and MacParakeet), beam search with hotwords (Chirp via sherpa-onnx), and plain regex fixups (stefanlindqvist/parakeet-dictation). |
| Apple | `AnalysisContext.contextualStrings`, about 100 short phrases | Documented for `DictationTranscriber` only. Posts I found say `SpeechTranscriber`, which Hex's `apple_speech` adapter uses, has no equivalent. I did not confirm that in Apple's docs. `SFCustomLanguageModelData` allows custom pronunciations. |
| Willow | Not found | Aqua's comparison page says Willow does not disclose its model. |

I found no Reddit or X threads that were specifically about Hex and vocabulary.
The search tools I had returned almost none. The user signal above comes from
the issue trackers.

### What the market converged on

1. **A short curated list.** Every vendor says it is not a dump of common words.
2. **Two layers:** a hint that goes to the recognizer, and a literal
   replacement applied afterwards. Superwhisper and Flow both keep these
   separate in the UI.
3. **Learn from the user's edits, with confirmation and Undo.** Flow and
   TypeWhisper both do this.
4. **Screen or app context** as a source of terms. Flow and Aqua advertise it.

## 2. Technical options for Parakeet

### Why Parakeet can be biased at all

I read the pinned `transcribe-cpp-sys` 0.2.4 source (Hex's version) and the
upstream repo head (`handy-computer/transcribe.cpp`, 2026-10-22, release
0.4.1).

- Every Parakeet head (TDT, RNN-T, CTC) is decoded on the host.
  `src/arch/parakeet/decoder.h` says "the decoder runs entirely as ggml graphs
  on ONE shared, per-decode CPU threadpool", even when the encoder is on the GPU.
- In `decoder.cpp`, `decode_tdt_greedy` and `decode_rnnt_greedy` (and the
  streaming RNN-T variant) each call `joint_step`, which copies the joint
  output into a host `std::vector<float> logits` and applies a log-softmax.
  The next token is then `argmax_range(token_logits, n_token_cls)`.
  Blank is `blank_id = vocab`, so it is part of that same slice.
- So adding a bias to selected token logits before the argmax is a few lines
  in a loop that already exists. A per-token confidence is already computed
  (`token_confidence`, entropy-based), and the public API returns it as
  `Token::p`, along with word and token timestamps. Hex already calls
  `transcribe_segments` and reads `transcript.words`; it does not read
  `tokens` or `p` today.
- Upstream's `docs/prompting.md` defines generic `vocabulary` and `prompt`
  fields with feature bits. The table lists Whisper, Qwen3-ASR, Granite,
  Fun-ASR-Nano and MOSS. **Parakeet is not listed.** Its `vocabulary` slot is
  unfilled.
- `transcribe_tokenize` returns `INT_MIN` for Parakeet ("SentencePiece;
  currently returns INT_MIN"). Encoding a user term into tokens is therefore
  unimplemented. The GGUF carries the SentencePiece vocabulary (1,024 pieces
  for the English models, per NVIDIA's model card), so someone has to write the
  encoder.
- There is no LoRA or adapter loading anywhere in transcribe.cpp. A search of
  `src/` and `docs/` found none, apart from an unrelated "adapter" layer in
  Moonshine Streaming.
- The default model, Parakeet Unified English 0.6B, is RNN-T with no CTC head in
  the GGUF. The hybrid `parakeet-tdt_ctc-110m` port explicitly drops its CTC
  head at convert time.

### Options, ranked by quality gained per unit of effort

#### A. Vocabulary list that feeds what already exists (certain, about 1 week)

One vocabulary list, shared by all modes (the model is not required to support
hints). It feeds:

1. **Hints** for Whisper today, and for Qwen3-ASR once transcribe-cpp moves from
   0.2.4 to 0.4.x. #129 describes the change. The only API break the reporter
   hit was `Model::capabilities()` returning a `Result`.
2. **The OpenCode rewrite prompt:** "these terms exist; prefer these spellings
   when a word sounds like one". This is the cheapest option that can fix
   "Hanks" and "Capuchin", because an LLM sees the whole sentence. It costs
   latency and is optional per mode.
3. **Replacements:** keep as the deterministic layer, with bulk import and the
   #139 and #177 fixes first.

Limits: an LLM can over-correct and add latency, and hints can induce Whisper
hallucination. Hex already has reports of Whisper inventing text on silence
(anomalyco/hex #193 and #108, kitlangton/Hex #206). Superwhisper's own docs say the same
about long vocabularies. Keep hint lists short and cap them.

#### B. Confidence-gated phonetic snap (about 1 to 2 weeks)

A pure-Rust pass after recognition. For each 1-3 word span of the transcript,
compare it to each vocabulary term phonetically (a grapheme-to-phoneme
edit-distance or a Double Metaphone key, with a letter-similarity check as a
second signal). Replace when the match is close and the span is not a
high-confidence common word, using `Token::p` and the word timestamps Hex
already has.

- It runs on every model, and it needs no model change.
- "Hex" and "Hanks" are close in sound (`hɛks` and `hæŋks`), and so are
  "Cappuccino" and "Capuchin". The weak spot is the second case: Parakeet is likely
  to be **confident** about "Capuchin", since it is a common word. A confidence
  gate would then skip the exact miss Kit reported. I have not measured this.
  Treat the gate as a false-positive limiter, not a recall driver.
- FluidAudio's rescorer has the same shape of guards, and it is a useful list of
  what to copy: a minimum similarity, a stricter bar for short words (80% for
  four characters or fewer), a stricter bar when the span contains stop words
  (85%), and a bar that rises with vocabulary size.
- Learned "sounds like" aliases (see D) make this much better. They are
  effectively a per-user confusion table.

#### C. Decoder-time phrase boosting inside transcribe.cpp (best quality, about a 1-week spike, 3 to 5 weeks to ship)

NVIDIA's GPU-PB is exactly this idea: "applied only at the decoding step in
shallow fusion mode", it "rescales ASR model scores with a boosting tree at the
token level", needs no retraining, and supports greedy decoding for CTC,
RNN-T/TDT and Canary. The paper (arXiv 2508.07014) says it works "without
noticeable speed degradation, even with a vast number of key phrases (up to 20K
items)". sherpa-onnx implements the same thing as an Aho-Corasick automaton over
tokenized hotwords, boosting every matched prefix token and cancelling the boost
if the path fails to complete.

For Hex:

1. Implement SentencePiece encoding for Parakeet. Parakeet emits cased text, and
   NeMo's docs say key phrases must be capitalized in advance, so use several
   casings per term.
2. Build a trie or Aho-Corasick graph in the decode loop. After `joint_step`,
   add `boost` to the logits of tokens that extend a live partial match. Remove
   the boost when the path fails.
3. Gate it so a boost cannot hijack ordinary speech: only apply it when the
   boosted token is within some margin of the best token, and cap the total
   boost per term.
4. Expose it as the unfilled `vocabulary` run option for Parakeet, and send
   upstream, or carry a fork. Hex pins `transcribe-cpp =0.2.4`, and upstream is
   at 0.4.1.

Unknowns I would settle with the spike before committing:

- **Greedy boosting quality.** Chirp's design notes say sherpa-onnx only
  supports hotwords in `modified_beam_search`, and cite an upstream issue about
  roughly 20% hallucination in TDT beam search (secondhand; I did not verify
  it). A greedy loop with a margin gate sidesteps beam search, but NVIDIA's
  paper is the only evidence that greedy boosting works well, and it was run in
  NeMo, not here.
- **Scale.** NVIDIA's NIM docs advise dynamic boosting "only up to approximately
  500 words" for RNN-T and TDT models, with larger lists built statically.
- **The streaming path.** The streaming decoder is separate, so it needs the
  same hook. Dictation streaming is speculative and falls back to the whole
  clip, so the whole-clip path can ship first.
- **Upstream acceptance** is not in our control.

#### D. Learn terms from the user's explicit edits (about 2 weeks, depends on A or B)

Wispr and TypeWhisper both do this, and anomalyco/hex #15 specified a careful
version (repeated, bounded evidence tied to Hex output; confirmation; no
keylogging). It was closed "not planned", so this is a product decision to
reopen. The input is the raw text, the final text, and a user edit after paste.
Hex's History stores raw and final text but not the user's edit. The Accessibility
adapter can read the focused field, but that is a privacy-sensitive surface and
needs the same limits as #15. The output is a vocabulary term plus a "heard as"
alias.

Cheaper first step with no new surface: let the user add a miss from History
("Hex was heard as Hanks"), one click, which creates a term plus alias.

#### E. CTC word spotting and rescoring (validated by others, bigger lift for Hex)

This is NVIDIA's CTC-WS (arXiv 2406.07096) as shipped by FluidAudio. A separate
CTC model scores the audio against a context graph, and its detections replace
the transducer's words where its acoustic evidence is stronger. This is the only
approach with independent measurements on Parakeet:

MacParakeet's Phase 0 note (third party, synthetic macOS `say` audio, 50
terms, FluidAudio 0.15.4):

| TDT v3 condition | Term recall | False vocabulary replacements |
|---|---|---|
| No vocabulary | 11/50 (22%) | 0 |
| FluidAudio default gate | 42/50 (84%) | 9 on the OOV set; 52 on the first 200 LibriSpeech clips (WER +1.2 pt) |
| `minSimilarity = 0.65` | 37/50 (74%) | 1 on the OOV set; 48 across full test-clean (WER 2.312% to 2.414%) |

The same note found throughput dropping from 103 RTFx to 31 RTFx, and found the
Unified model unsupported: the CTC sidecar detected terms, but the library had
no hook to apply them, so recall stayed at 13/50. Two points matter for Hex:
the gate threshold decides everything (false replacements hit WER hard), and
Hex's default model is Unified.

What it would need in Hex: a CTC log-prob API from transcribe.cpp, which does
not exist (the public header exposes token `p` only); a second encoder pass
through a CTC model; and the DP and rescoring code. It is more work than C for a
similar result. Treat it as the fallback if the decoder spike disappoints.

#### F. LLM correction with the vocabulary

Already covered under A.2. Apple's FoundationModels is a possible local
alternative to OpenCode. The Show HN post for `sayline.app` reports the Apple
model's content filter "blocked nearly every useful request" in a
dictation-rewrite setting (anecdotal, one author). anomalyco/hex #167 already
tracks Foundation Model support for Voice Action.

#### G. Personalization and training (not recommended per user)

- **Data volume.** NVIDIA's own guidance, in the NIM customization docs: "We
  recommend fine-tuning ASR models only with sufficient data, on the order of
  several hundred hours of speech. If such data is not available, it can be more
  useful to adapt the LM on an in-domain text corpus". One user's retained
  dictation is minutes to a few hours, mostly their normal speech. That risks
  forgetting and gives little signal for the dozen terms that matter.
- **Edits as training pairs.** Hex's history stores raw and final text pairs and
  can keep WAVs only under `HEX_RETAIN_DICTATION_AUDIO` (explicit, bounded,
  owner-only). The pairs are useful to mine for terms and aliases (D), and as a
  private eval set. They are too few and too noisy to train on.
- **Text-only adaptation exists.** NeMo has a hybrid ASR-TTS model that
  fine-tunes an ASR model on text alone by routing text through a frozen
  FastPitch text-to-mel generator, which lets you mix text-only data with audio.
  It supports CTC and RNN-T BPE models. Synthetic speech for rare terms is the
  other route, and Hex's dictation-corpus branch already generates synthetic
  clips with macOS `say` and ElevenLabs.
- **No Mac path, no adapter path.** NeMo is a PyTorch and CUDA toolkit, and I
  found no Mac-supported training recipe (not verified). transcribe.cpp has no
  adapter loading, so a user-specific result would ship as a full merged GGUF
  (about 0.6 to 2.5 GB). That is not realistic per user.
- **A central model is a real option, later.** Aqua's Avalon is the existence
  proof that a model trained for AI and coding vocabulary changes the numbers
  (vendor-reported 97.4% against 51.5% to 65.1% key-term accuracy on its own
  benchmark). An Anomaly-trained "Parakeet for developers" fine-tune on the
  Unified English base, using synthetic TTS data for tool, product and person
  names plus real data, converted to a GGUF through the existing transcribe.cpp
  conversion path, is a project of weeks and a GPU budget, and carries a risk of
  regressing general WER. Check the NVIDIA Open Model License terms for
  derivative redistribution first. The model's own docs name that license; I did
  not read its derivative terms.

## 3. Recommendation

### Build first (about 2 to 3 weeks total)

1. **One vocabulary list per Hex, with import/export.** Terms, optional "heard
   as" aliases, optional per-mode scope. Fix #139 and #177. Add CSV or text
   bulk import (kitlangton/Hex #160). One-click "add from History".
2. **Feed it to what exists:** Whisper hints (cap the list), the OpenCode
   rewrite prompt for modes that have rewriting on, and deterministic
   replacements for aliases. Bump transcribe-cpp to 0.4.x for Qwen3-ASR hints
   (#129) as a separate step, since it changes the pinned dependency.
3. **Confidence-gated phonetic snap (option B),** off by default, with the
   FluidAudio-style guards. Ship it behind the same benchmark gate as step 4.
4. **The eval, before any of it ships.** See the next section. Do this first,
   because the false-replacement rate is what decides whether any of B, C or E
   is acceptable.

### Bigger bet: decoder-time boosting (option C)

Do a one-week spike in a transcribe.cpp checkout: hard-code a boost list through
an environment variable in `decode_rnnt_greedy` and `decode_tdt_greedy`, with a
margin gate, and run it on the term corpus against Parakeet Unified and v2.
Success looks like recall at or above 70% with false replacements near zero and
a WER change under 0.3 pt (MacParakeet used the same gate). If it clears that,
write the SentencePiece encoder and send it upstream as Parakeet's
`vocabulary`. If it does not, fall back to E.

This is the only option that fixes a term the model never produces as a
candidate word, and it needs no second model.

### Not worth it

- Per-user LoRA or fine-tuning, and training on a user's edits. Too little data,
  no Mac path, and no adapter format in transcribe.cpp.
- Hot-swapping the default to a different Parakeet variant just for vocabulary.
  Variants differ in speed and accuracy, and none has a CTC head in the GGUF.
- Screen OCR for context (kitlangton/Hex #187) as a first step. Permission cost
  is high, and Parakeet cannot take a prompt anyway. Revisit when a boosting
  list exists, since on-screen terms are just a dynamic vocabulary.
- Blind replacements for real words. "Hanks to Hex" and "Capuchin to Cappuccino"
  corrupt legitimate uses. Keep them behind a mode, a context, or an alias that
  only exists after a confident phonetic match.

## 4. How to measure

Hex already has most of the pieces:

- `hex benchmark-transcription <manifest.json>` runs a corpus of
  `{id, audio, expected}` clips through the production `transcribe.cpp` path
  and reports WER and RTF. It is documented in
  `docs/research/transcription-benchmark.md`.
- The `test/dictation-corpus` branch has a corpus harness
  (`scripts/dictation-corpus.sh`, `src/dictation_corpus_tests.rs`) that reads
  `synthetic/say_manifest.json` and `synthetic/elevenlabs_manifest.json`
  alongside real clips.

Add:

1. **A term corpus.** Text only to start: about 100 sentences, each containing
   one or more target terms (OpenCode, Rune, Scrawl, Effect, Anomaly,
   Cappuccino, Hex, plus real names). Render with several `say` voices, at least
   one ElevenLabs voice, and a handful of Kit's own recordings, including the
   meeting clips where "Hanks" and "Capuchin" occurred. Real clips matter more
   than synthetic ones: MacParakeet's note itself says synthetic `say` audio is
   no substitute for accents and microphones.
2. **Two metrics, not one.**
   - **Term recall:** the exact expected term appears in the output (case and
     spacing normalized).
   - **False replacement rate:** terms inserted where the reference does not
     have them, measured on a **term-free** corpus (LibriSpeech test-clean, plus
     Kit's own recordings with no target terms). WER alone hides this.
3. **A gate:** WER change at most 0.3 pt on the term-free corpus, as in
   MacParakeet's note, and p95 added latency under about 100 ms for a 10 s clip
   (my number). Run baseline, A, B, and the spike as separate columns.
4. **A fixture of real misses.** Keep `Hanks` and `Capuchin` as permanent
   regression clips.

Extend the manifest with `expected_terms` and add a `--vocabulary <file>`
option to `benchmark-transcription` so every approach runs through one harness.

## Sources

- Hex: `src/parakeet.rs` (hints are Whisper-only), `src/transcription_models.rs`
  (`supports_recognition_hints`), `src/dictation/text_replacements.rs`,
  `src/dictation_processor.rs`, `src/history.rs`, and the issues above.
- transcribe.cpp:
  `handy-computer/transcribe.cpp` at `4d7c1cc` (`docs/prompting.md`,
  `include/transcribe.h`, `src/arch/parakeet/decoder.cpp`, `decoder.h`,
  `docs/models/parakeet-tdt_ctc-110m.md`), and `transcribe-cpp-sys` 0.2.4 as
  vendored by Hex.
- NVIDIA NeMo: Word Boosting
  (`docs.nvidia.com/nemo-framework/.../asr_customization/word_boosting.html`);
  GPU-PB, arXiv 2508.07014; CTC-WS, arXiv 2406.07096; the NIM customization
  docs for the 500-word and several-hundred-hour guidance; the ASR-TTS hybrid
  model section of the NeMo models page.
- sherpa-onnx hotwords documentation (`k2-fsa.github.io/sherpa/onnx/hotwords`).
- FluidAudio `Documentation/ASR/CustomVocabulary.md`; MacParakeet
  `docs/research/2026-07-04-custom-vocab-phase0.md`; Chirp
  `docs/superpowers/specs/2026-03-30-vocabulary-hotwords-design.md`.
- Superwhisper, Wispr Flow, MacWhisper, TypeWhisper and Aqua Voice
  documentation and blog pages, and Apple's `AnalysisContext` documentation.
