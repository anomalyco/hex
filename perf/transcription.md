# Transcription Runtime Benchmark

## Goal

Compare HEX transcription runtimes on the same M2 Max inputs without changing
the production backend based on published results from other machines.

The primary performance metric is median warm real-time factor (`median_rtf`),
where lower is better. Secondary metrics are model load time, median and p95
per-clip latency, peak memory measured externally, and aggregate word error rate.

## Corpus

The corpus is local and ignored by Git because HEX does not retain dictation
audio by default. Create `perf/transcription-corpus/manifest.json` and keep the
WAV files beside it:

```json
{
  "clips": [
    {
      "id": "short-product-name",
      "audio": "short-product-name.wav",
      "expected": "OpenCode should keep this sentence concise."
    }
  ]
}
```

Record each clip once, transcribe the spoken ground truth manually, and then
keep both fixed across backend runs. Do not derive references from either model.
Use mono or multichannel PCM WAV files; the harness downmixes and resamples them
to the same 16 kHz mono input before timing inference.

Build a representative dictation corpus of at least 12 clips covering:

- one-phrase, one-sentence, and multi-sentence dictation
- clean speech and ordinary room or keyboard noise
- immediate speech onset and a natural pause before speech
- product names, technical prose, numbers, and punctuation requests
- corrections, hesitations, and repeated words
- clip durations from roughly 1 to 20 seconds

The production runtime pads clips shorter than 1.5 seconds. The harness applies
the same padding but uses the original spoken duration when calculating RTF.

## Command

Use a release build and save the production `transcribe.cpp` JSONL output:

```sh
cargo run --release -- benchmark-transcription \
  perf/transcription-corpus/manifest.json \
  > perf/transcription-corpus/transcribe-cpp-metal-v2-q8.jsonl
```

The command defaults to the installed GGUF model. Pass `--model` only to test a
different GGUF. Run the opt-in legacy ONNX backend against the same manifest:

```sh
cargo run --release --features onnx-benchmark -- \
  benchmark-transcription perf/transcription-corpus/manifest.json \
  --backend onnx \
  > perf/transcription-corpus/transcribe-rs-onnx-int8.jsonl
```

## Provisional M2 Max Comparison

On 2026-07-16, ten clips cut from existing local microphone recordings were
used to exercise both backends. They contain 127 seconds of technical terms,
ordinary prose, hesitations, and repeated-token failure cases. Each backend ran
two full-corpus warmups followed by seven measured passes. The Metal experiment
was repeated after another ONNX run.

| Metric | ONNX Int8 | `transcribe.cpp` Metal Q8 |
| --- | ---: | ---: |
| Model load, cached Metal library | 620 ms | 269-277 ms |
| Model load, first launch after build | not observed | 9,921 ms |
| First clip | 296 ms | 142-150 ms |
| Median corpus time | 3,703 ms | 2,056-2,318 ms |
| Median clip time | 368 ms | 216-234 ms |
| p95 clip time | 509 ms | 303-356 ms |
| Maximum clip time | 551 ms | 356-567 ms |
| Peak resident memory | 1.73 GB | 1.05 GB |
| Provisional WER | 8.07% | 9.82% |

With native diagnostics disabled for both backends, Metal Q8 cut median
inference time by roughly 37-44%, p95 latency by roughly 30-40%, and peak
resident memory by 40%. Its transcripts differed by five additional word edits
over 285 reference words. Some differences were clear improvements and others
were regressions.

The first launch of a freshly linked Metal binary spent about 9.9 seconds
initializing its embedded Metal library. Immediate subsequent launches loaded
the model in roughly 270 ms. A production spike must account for this
installation-time cold cost and must not report the listener ready before the
model and Metal pipelines are usable.

This corpus is useful for runtime comparison but not for the migration quality
gate. Its references came from earlier ONNX meeting transcripts, the clips were
cut at model-generated segment boundaries, and the speech is not representative
hold-to-dictate input. Record independent human references before selecting the
production backend.

The default is one full-corpus warmup excluded from summary statistics followed
by seven measured passes. Warmup lines preserve cold inference costs, the model
line reports cold load time, clip lines preserve first-pass transcripts and
quality counts, measurement and run lines expose spread, and the final summary
contains the comparison metrics.

## Harness Validation

On 2026-07-16, the ONNX backend was run on an M2 Max against one existing
8.38-second microphone recording to verify the release-mode harness end to end:

- model load: 706.8 ms
- median warm inference: 275.5 ms
- median RTF: 0.0329 (30.4x real time)
- p95 inference: 333.0 ms
- reference: 19 words, reproduced exactly

This is not migration evidence. The clip was meeting speech rather than a
representative dictation corpus, and its reference came from an earlier run of
the same ONNX backend. It proves only that decoding, preprocessing, repeated
measurement, JSONL reporting, and quality accounting work together.

Measure peak resident memory around the command separately because allocator
and backend-owned memory are not reliably visible from inside the process:

```sh
cargo build --release
/usr/bin/time -l target/release/voice-control benchmark-transcription \
  perf/transcription-corpus/manifest.json \
  > perf/transcription-corpus/transcribe-cpp-metal-v2-q8.jsonl
```

## Decision Rule

Retain the ONNX benchmark until `transcribe.cpp` is measured against
independently referenced hold-to-dictate speech. Keep the production migration
only if transcript quality has no material regression and the measured latency
and memory wins survive packaging, signing, and foreground-app smoke testing.

Record every backend revision, model and quantization, macOS version, power
state, and result filename. Run comparisons plugged in, with no other inference
process active, and alternate backend order when repeating the experiment.
