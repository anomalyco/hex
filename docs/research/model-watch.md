# Model watch

Weekly running log of local ASR models and runtimes relevant to Hex's compiled catalog; newest entry first, each dated, each ranked against what Hex ships today.

## 2026-10-22: streaming Parakeet vs Moonshine, and what to add

Question (Kit): do we still need Moonshine if Parakeet can stream? Is always-on
streaming Parakeet better? Is there a better small model than Moonshine?

### Verdict

```ts
Moonshine?
├── Drop the ONNX Moonshine stack (moonshine-voice C lib, ORT, download.moonshine.ai)
│   └── Replace with moonshine-streaming-small Q8 GGUF through transcribe-cpp (already in the pinned 0.2.4)
├── Keep a small streaming model for commands and the live draft
│   └── Always-on streaming Parakeet is not better: same WER at command latency, 4-5x the parameters,
│       a second 0.7 GB model copy, and (buffered Unified) a recomputed 5.6 s left context per chunk
└── Meetings only: Apple SpeechTranscriber beats both on cost (Hex measured ~2% CPU, no model memory);
    plans/meeting-live-transcription.md already recommends it
```

Confidence: moderate. Everything below is vendor-reported or Hex's own
measurements; nothing here was re-run on Kit's hardware. Unmeasured and
decisive: command-recognition latency and miss rate of the GGUF Moonshine
behind Hex's line segmentation, and Metal power for any always-on stream.

### 1. Streaming Parakeet vs Moonshine

What Hex runs today: `moonshine.rs` loads Moonshine Streaming **Small** (the
`small-streaming-en` quantized ONNX bundle, about 250 MB on disk, 140M-class)
through the moonshine-voice C library, single-threaded. Hex's own numbers
(`docs/features/meetings.md`, Mac Studio/Pro, 2026-10-22): two meeting-draft
streams 31% of a core, command recognizer 22% of a core, about 0.4 GB resident
per Moonshine, and a second Parakeet copy costs about +0.74 GB at meeting peak
(2808 vs 2072 MB).

**Accuracy by latency.** NVIDIA's own table on the eight Open ASR datasets
(AMI, Earnings22, GigaSpeech, LibriSpeech clean/other, SPGI, TED-LIUM,
VoxPopuli), average WER %. Moonshine is from its paper, same datasets.

```ts
                                latency (chunk+right)   avg WER
Moonshine v2 small (123M)       80 ms lookahead         7.84    // paper Table 3
Moonshine v2 medium (245M)      80 ms lookahead         6.65
Parakeet Unified 0.6B   offline                         5.91
                        2.08 s  (Hex default 5.6/1.04/1.04)   6.14
                        1.12 s                          6.29
                        0.56 s                          6.52
                        0.32 s                          6.92
                        0.24 s                          7.35
                        0.16 s                          8.44
                        0.08 s                         15.63    // collapses
Nemotron Speech Streaming EN 0.6B (cache-aware)
                        offline                         6.92
                        1.12 s                          6.92
                        0.56 s                          7.09
                        0.16 s                          7.84
                        0.08 s                          8.70
Parakeet TDT v2 0.6B buffered at 1.12 s                22.83    // not trained for it
```

Reading it:

- At command latency (under 0.3 s) Moonshine small (7.84) already matches
  Nemotron at 0.16 s (7.84) and beats Unified at 0.16 s (8.44). Moonshine
  medium (6.65) matches Nemotron at 1.12 s (6.92) at 40% of the size.
- Unified wins only when you accept 0.56 to 2 s of delay (6.14 to 6.52), which
  is fine for a caption draft and irrelevant to a spoken command.
- The Moonshine paper times end-of-utterance latency on an M3 MacBook:
  small 148 ms and 18% compute load, medium 258 ms and 29% (Table 2). It does
  not report power. Nobody I found publishes Metal power for Parakeet or
  Nemotron streaming.

**Compute shape.** Unified is *buffered* streaming: NVIDIA's card says "left
context is recomputed for each chunk", and Hex's `gguf_session.rs` uses 5.6 s
left, 1.04 s chunk, 1.04 s right. Each step encodes about 7.7 s of audio for
1.04 s of new audio, roughly 7.4x redundancy (12x at the 0.56 s setting). At the
cards' M4 Max Metal throughput of about 190 to 230x realtime offline, that is
my derivation of a few percent of the GPU per continuous stream, ignoring
per-chunk overheads; it is not measured. Nemotron is cache-aware (constant
caches, no overlap), so a stream costs about one offline pass. Moonshine's
encoder is also incremental (sliding window, 50 Hz frontend).

**Throughput on Apple Silicon** (transcribe.cpp cards, M4 Max, Q8_0, Metal,
offline compute, x realtime on an 11 s clip / 35 s clip):

```ts
Parakeet Unified          188 / 228
Nemotron EN               207 / 225
Nemotron 3.5 (multiling.) 144 / 138
Moonshine streaming tiny  239 / 112   (CPU 271 / 169)
Moonshine streaming small 129 /  59   (CPU  64 /  49)
Moonshine streaming med    87 /  36   (CPU  47 /  32)
Voxtral Realtime 4B         6 /   7
```

Moonshine's autoregressive decoder dominates long transcripts, and its CPU path
is close to its Metal path, so it does not need the GPU. Nemotron/Unified are
GPU models: CPU is 32 to 39x on the same chip.

**Can one loaded Parakeet serve several streams?** Not in Hex's binding.
Checked against `transcribe-cpp` 0.2.4 (Hex's pin) and upstream:

- `transcribe.h` (upstream main): "KNOWN 0.x LIMITATION — concurrent COMPUTE is
  not yet supported: at most one transcribe_run / transcribe_run_batch / active
  stream may be in flight across ALL sessions of a given model." Callers wanting
  parallel transcription "should load one model per worker"; serialized use of
  many sessions on one model is supported.
- `docs/bindings.md`: the C header would let several sessions each hold an
  active stream and interleave feeds under caller serialization, but "a binding
  allows one active stream per model".
- `src/session.rs` (0.2.4) `Session::stream` takes a per-model lease for the
  stream's whole life; a second stream or any `run` returns `Error::Busy`. That
  is the lease Hex's engine thread holds during every dictation capture.
- `run_batch` batches offline utterances only. There is no batched streaming.

So: each concurrent stream needs its own `Model` (Hex measured about 0.7 GB for
a second Parakeet), and an always-on stream on the dictation model would block
dictation (or be blocked by it). transcribe.cpp's README says CPU and Metal
weights are memory-mapped, so two loads of one GGUF may share pages; Hex's
+0.74 GB measurement says do not assume it without measuring. A "per-session
backend architecture" lifting the limit is described as planned, with no date.

**(a) Two-track live captions.** Moonshine remains the cheapest correct fit,
but both its current runtime and Parakeet share the same constraint: one stream
per model object, so two tracks mean two models. With 199 MB Q8 GGUFs
(mmap-able, Metal or CPU) that is about 0.4 GB, the same as today, minus ORT
and its "one streaming state per transcriber" problem that dropped 553 to 631
packets per run in Hex's measurement. A Parakeet pair is about 1.5 GB for a
draft that rolling Parakeet replaces within a chunk anyway. Apple
SpeechTranscriber (Hex measured ~2% of a core for both tracks, finals about
2.3 s p50 after the segment ends, 3.0% vs Moonshine's 1.7% WER on a synthetic
call) is the cheapest by an order of magnitude and needs no model memory.

**(b) Always-on voice commands.** Ordinary resolver commands run only from
*completed* lines (AGENTS invariant), so partial-hypothesis latency matters only
for the voice-delimited exceptions. Candidates:

- Moonshine small GGUF behind a VAD line segmenter: 199 MB, CPU or Metal,
  2.54% LibriSpeech clean (card), about the same as the ONNX build.
- `parakeet-tdt_ctc-110m` offline on VAD-segmented utterances: 135 MB Q8,
  2.43% LibriSpeech clean, English, CC-BY-4.0. Offline decode of a one-second
  command is tens of milliseconds, so the cost is the endpoint hangover, not
  the model. Untested here.
- Always-on Nemotron or Unified: needs a dedicated 0.7 GB model, a 12 Hz (80
  ms chunk) wake-up loop on the GPU, and gives no accuracy gain at these
  latencies. Only worth it if Kit wants dictation-grade punctuation in the live
  draft.

Moonshine's one structural advantage over the transcribe.cpp route is that
moonshine-voice ships its own VAD and line events. transcribe.cpp's Moonshine
stream returns committed/tentative text, caps one stream at 81.9 s, and has no
line segmentation; Hex would drive it with Silero VAD v6.2 (shipped in
transcribe.cpp) or `pause_segmenter` and start a new stream per line.

### 2. Landscape (as of 2026-10-22)

Two leaderboards are in play and must not be compared across:

- **Eight-set average** (older; NVIDIA and Moonshine report on it).
- **Current Open ASR Leaderboard** (results commits through 2026-10-19):
  nine public sets, including Voice Arena Monsoon and URGENT24 noisy/reverberant
  speech. Scores are higher than on the eight-set average for the same model. Moonshine, Parakeet Unified and Kyutai are not
  in the current results file.

Current leaderboard, `english_short_latest.csv` (dataset
`hf-audio/open-asr-leaderboard-results`, commit 2026-10-19), open models only.
RTFx is on an A100 and says nothing about a Mac:

```ts
model                           params  avg WER  RTFx   license     GGUF / transcribe.cpp
Granite Speech 5.0 2B           2.3B    5.06     545    Apache-2.0  not yet (4.0/4.1 and 5.0 TurboCTC only)
Qwen3-ASR 1.7B                  2.0B    5.75     836    Apache-2.0  yes (qwen3-asr-1.7b)
Cohere Transcribe 03-2026       2B      6.26     924    Apache-2.0  yes (already in Hex)
Canary-Qwen 2.5B                2.5B    6.37     882    CC-BY-4.0   yes
Parakeet TDT 0.6B v2            0.6B    6.43     6148   CC-BY-4.0   yes (already in Hex)
Parakeet TDT 0.6B v3            0.6B    6.51     6228   CC-BY-4.0   yes (already in Hex)
Qwen3-ASR 0.6B                  0.8B    6.60     750    Apache-2.0  yes (already in Hex)
Granite 5.0 TurboCTC (new rev)  0.47B   6.84     21134  Apache-2.0  old English-only rev only
Nemotron Speech Streaming EN    0.62B   6.89     1114   NVIDIA OML  yes
Distil-Whisper large-v3.5       0.8B    7.02     898    MIT         not in the model list
Whisper large-v3-turbo          0.8B    8.39     811    MIT         yes (already in Hex)
Voxtral Mini 4B Realtime        4B      9.01     103    Apache-2.0  yes, 4.7 GB Q8, 6-7x realtime on M4 Max
Nemotron 3.5 ASR Streaming      0.64B   10.51    1348   OpenMDW-1.1 yes (multilingual; English worse)
```

Streaming-capable and small, with how Hex would run each:

```ts
Moonshine v2 Streaming tiny/small/med  MIT; 44M/140M/266M (GGUF params); English only
│   streaming yes (50 Hz frontend, 80 ms feed); 8-set WER 12.01/7.84/6.65; LS clean Q8 4.52/2.54/2.16
│   GGUF Q8 50/199/296 MB in transcribe.cpp. No multilingual in this family; other-language Moonshine is non-streaming
├── Parakeet Unified EN 0.6B   NVIDIA OML; buffered streaming, 160 ms to 2.08 s; Hex default; 731 MB Q8
├── Nemotron Speech Streaming EN 0.6B   NVIDIA OML; cache-aware, 80 ms to 1.12 s; 730 MB Q8
│   LibriSpeech clean streaming = offline = 2.31% Q8 (byte-equal to NeMo at R=13); supported by 0.2.4
├── Nemotron 3.5 ASR Streaming 0.6B   OpenMDW-1.1; 32 supported locales, prompt-conditioned; 751 MB Q8
│   LS clean 3.05% (vs 2.31 EN-only); English regressed on the current leaderboard (10.51 vs 6.89)
├── Multitalker Parakeet Streaming 0.6B   NVIDIA OML; Nemotron fine-tune with an embedded Sortformer diarizer
│   (up to 4 speakers); the only local route to "who said it" in a stream; unmeasured by Hex
├── Voxtral Mini 4B Realtime   Apache-2.0; 480 ms default delay; auto language detection; too heavy (4.7 GB, 6-7x realtime)
├── Kyutai STT 2.6B EN / 1B EN-FR   CC-BY-4.0; 2.5 s / 0.5 s delay; PyTorch/MLX, no GGUF
│   old 8-set avg 6.4; 2.6B is too large for an always-on slot
├── Apple SpeechAnalyzer / SpeechTranscriber   system model, macOS 26+, no download; Hex already ships
│   it as a batch model. Independent tests: 2.12% clean / 4.56% other (M2 Pro, 5,559 LibriSpeech
│   utterances, Lyonesse, July 2026), 1.98% on a 40-clip snapshot (IraVoice, M5 Max, p50 about 130 ms
│   post-speech). Parakeet v2 CoreML int8 2.01 / 3.40 in the same Lyonesse round
└── Granite Speech 5.0 TurboCTC   Apache-2.0; offline only despite the "streaming-style" encoder;
    21k RTFx on A100; the Oct 21 revision adds PnC and 7 languages but is not in transcribe.cpp yet
```

Offline accuracy tier, for reference (not streaming): Granite Speech 5.0 2B is
#2 open on the current leaderboard (5.06, released 2026-10-20, Apache-2.0,
translation and 2,000-term keyword biasing, 6 languages); Qwen3-ASR 1.7B 5.75
(52 languages); `parakeet-ultra` (Moondream's post-trained v3, CC-BY-4.0, 25
languages) lists 1.80% vs v3's 1.94% on LibriSpeech clean in transcribe.cpp,
and was 5.32 vs 6.32 on the earlier leaderboard average per the HF dataset
listing. TheWhisper large-v3-turbo (TheStage, CC-BY-4.0) is 6.07 on the current
leaderboard with CoreML and a streaming tag, but ships no GGUF.

### 3. Catalog additions, ranked

Hex's catalog today (`transcription_models.rs`): Parakeet Unified EN, v2, v3,
Whisper large-v3-turbo, Qwen3-ASR 0.6B, SenseVoice Small, Cohere Transcribe,
Apple Speech. Moonshine is a separate ONNX path outside the catalog.

```ts
1. Moonshine Streaming small (and medium as an option), GGUF    // replaces the ONNX Moonshine
│   Gain: removes ORT, moonshine-voice (header 20000 now, 30000 breaking), the download.moonshine.ai
│   host and its failure mode; one runtime, one downloader, Metal or CPU; weights 199 MB vs ~250 MB;
│   same accuracy (card LS clean 2.54% vs Useful Sensors' 2.49%); medium is 6.65 avg for 296 MB.
│   Cost: medium. Rewrite the thin layer behind moonshine.rs: VAD line segmentation (Silero VAD is in
│   transcribe.cpp), a stream per line (81.9 s cap), committed vs tentative text to LineStarted/
│   Updated/Completed, one Model per meeting track and one for commands.
│   Needs no crate bump (0.2.4 has moonshine_streaming). Gate: replay Hex's command fixtures
│   (moonshine_lab) and the meeting_echo_replay call against both backends before deleting ONNX.
│
2. parakeet-ultra (GGUF, offline)                                // replaces Parakeet v3
│   Gain: same size and API as v3 (740 MB Q8), lower error on English and every FLEURS language,
│   cuts long audio at pauses by itself (30 s segments), which suits meeting chunks.
│   Cost: low. Catalog entry plus a crate bump: the port is pinned 2026-10-04, newer than 0.2.4
│   (2026-09-25); 0.4.1 is current (2026-10-22). Q8_0 (740 MB, 1.80%) and smaller quants are published.
│
3. Nemotron Speech Streaming EN 0.6B (GGUF, cache-aware)         // new "Live" tier, not a dictation default
│   Gain: streaming WER equals offline (2.31% LS clean), 80 to 1120 ms chunks, constant memory, no 5.6 s
│   recompute, native PnC. Right choice if the live meeting draft should read like final text, or if
│   Kit wants always-on streaming with a Parakeet-family model.
│   Cost: medium. gguf_session.rs only sets the buffered Parakeet extension; Nemotron needs
│   StreamExtension::ParakeetStream{att_context_right}. Runs in 0.2.4. License is the NVIDIA Open
│   Model License. Do not make it the dictation default: Unified is more accurate offline (5.91 vs 6.92).
│
4. Granite Speech 5.0 2B and TurboCTC                            // wait for the port
│   2B is the accuracy leader (5.06) but needs a 2B LLM decoder; TurboCTC is the fastest and Apache but
│   its revised multilingual/PnC weights are not in transcribe.cpp (README lists the August revision).
│
5. Qwen3-ASR 1.7B                                                // quality tier for final meeting passes
│   5.75 current, 52 languages, Apache-2.0, already in transcribe.cpp. 2B-class decode cost, so
│   final-pass only. Hex already carries the 0.6B.
│
Not recommended: Voxtral Realtime (4.7 GB, 6-7x realtime, 9.01 avg), Nemotron 3.5 for English
(10.51 current vs 6.89), Kyutai (no GGUF, 2.6B), Moonshine tiny (12.01 avg).
```

Dropping Moonshine entirely is possible for the *meeting draft* only: use Apple
SpeechTranscriber on macOS 26+ and Nemotron or nothing elsewhere. For commands,
a small streaming or VAD-endpointed model stays, and Moonshine small is the
smallest one with a published streaming port at command-grade WER.

### Next measurements

- GGUF Moonshine small vs the ONNX build on `moonshine_lab` fixtures: miss rate,
  update p50/p95, CPU, resident memory (M5 Max and a base M-series).
- A second `Model` of the same Parakeet GGUF in-process: real resident delta
  with mmap on Metal.
- Nemotron EN at 160 and 560 ms against Unified (5.6/0.56/0.56) on Hex's call
  replay: caption latency, CPU/GPU time per audio minute, `powermetrics` energy.
- Whether the HEAD of transcribe.cpp has lifted the one-compute-per-model rule
  (watch `docs/bindings.md` and the 0.4.x notes).

### Sources

- Moonshine v2 paper (Kudlur et al., 2026-02-12), arXiv 2602.12241: Tables 2 and
  3, encoder window (16,4)/(16,0), 80 ms lookahead.
- UsefulSensors/moonshine-streaming-small model card (MIT);
  moonshine-ai/moonshine releases (v0.1.5, 2026-08-24).
- transcribe.cpp (MIT) `docs/models/`: `moonshine-streaming*.md`,
  `parakeet.md`, `parakeet-unified-en-0.6b.md`, `parakeet-ultra.md`,
  `nemotron-speech-streaming-en-0.6b.md`, `nemotron-3.5-asr-streaming-0.6b.md`,
  `voxtral-realtime.md`, `granite-speech-5.0-turboctc.md`, `bindings.md`;
  `include/transcribe.h`; Hex's pinned `transcribe-cpp`/`-sys` 0.2.4 sources
  (`session.rs`, `lib.rs`, `family.rs`, `arch/`). crates.io: 0.2.4 on
  2026-09-25, 0.4.1 on 2026-10-22.
- nvidia/parakeet-unified-en-0.6b (2026-04-07) and nvidia/nemotron-speech-
  streaming-en-0.6b (2026-03-13) model cards; arXiv 2604.19079 and 2312.17279.
- Open ASR Leaderboard results file `english_short_latest.csv` and its commit
  log (2026-10-19), arXiv 2510.06961; IBM Granite Speech 5.0 blog (2026-10-21).
- kyutai/stt-2.6b-en model card; Mistral Voxtral Mini 4B Realtime via the
  transcribe.cpp card above.
- Apple on-device comparisons are third-party and from interested publishers:
  Lyonesse (2026-07-13/15), IraVoice `mac-asr-benchmark` (2026-08-01), Mumr
  meeting benchmark (2026-09-30). Hex's own measurements:
  `docs/features/meetings.md` and `docs/plans/meeting-live-transcription.md`.
