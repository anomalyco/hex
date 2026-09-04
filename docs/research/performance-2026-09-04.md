# Inference performance reconnaissance

September 4, 2026. No production performance defaults were changed.

## Outcome

Local inference is the first optimization target, especially occasional long
stalls rather than only the median. A smaller Parakeet CPU thread count showed
promising medians but inconsistent worst cases, so it was not adopted.

The existing [transcription benchmark](transcription-benchmark.md) provides
repeated runs, JSONL timing output, and word-error accounting. Its July results
are historical, not the baseline for this investigation.

## Method And Limits

The experiment ran on an M2 Max with 96 GiB RAM and AC power. Other builds and
applications were active. These are exploratory results, not a controlled
before/after performance claim. No microphone, personal audio, clipboard, live
provider, or updater was used.

The release benchmark used Parakeet Unified English Q8_0 and Cohere Q8_0 from
the pinned catalog, six synthetic English clips, one corpus warmup, and seven
measured passes. The model order was Parakeet, Cohere, Cohere, Parakeet. Clips
covered approximately 1.5, 2.6, 3, 19, 57, and 73 seconds, including pauses.
The long-form text comes from
[`cohere-long-form.txt`](../../tests/fixtures/cohere-long-form.txt); the
[regression script](../../scripts/test-cohere-long-form.sh) documents synthesis.

Warm median inference ranges across the two invocations:

- Parakeet: 79–115 ms for short clips, 276–350 ms for 19 seconds,
  713–759 ms for 57 seconds, and 873–896 ms for the paused 73-second clip.
- Cohere: 92–127 ms for short clips, 409–440 ms for 19 seconds,
  1,161–1,287 ms for 57 seconds, and 1,436–1,463 ms for the paused clip.
- Fresh-process model/session loading with reusable system caches was
  292–317 ms for Parakeet and 869–1,222 ms for Cohere. This is not first-install
  cold-start timing.

One Parakeet run on the paused fixture took 7,322 ms against an 873 ms median.
That establishes an outlier without the app UI or paste path, not its cause.
Parakeet also used substantially more CPU time than Cohere. The standard
benchmark omits saved language/hints and the production Unified English trailing
padding, so these values cannot be subtracted from live timings to infer overhead.

Synthetic speech is not a representative language-quality corpus. Both models
rendered spoken number words as digits, which the benchmark counted as word edits.
Do not optimize by accepting missing passages or undoing the Cohere window fix.

## Thread-Count Experiment

The pinned native Parakeet implementation uses a Metal encoder and a CPU
predictor/decoder. Automatic thread selection is at most eight usable CPUs.
The native `SessionOptions::n_threads` API permits a bounded comparison without
changing model weights or the backend.

A standalone harness used the same adapter and model with word timestamps,
minimum padding, and the production 200 ms Unified English audio tail. It tested
3/19/73-second fixtures in mirrored order: auto, 1, 2, 4, 8, 8, 4, 2, 1, auto.
Each session received one corpus warmup and seven measured passes. All 210
measured transcripts matched their corresponding automatic-thread warmup exactly.

Eight threads versus four, median wall time:

```ts
3-second fixture    // 75.38 -> 65.09 ms; 13.7% lower
19-second fixture   // 256.50 -> 225.59 ms; 12.1% lower
73-second fixture   // 852.57 -> 820.79 ms; 3.7% lower
```

Four threads did not consistently improve the tails: the medium fixture's worst
sample increased from 545 ms to 1,067 ms, including 729 ms of decode time.
One thread was slower on every median comparison. Automatic and explicit-eight
results also varied, showing run-order/host noise.

The Metal encoder dominated normal time in every configuration. With eight
threads, encode medians were 58/174/579 ms versus decode 13/75/265 ms.
CPU decoding contributed to several spikes, but thread count alone is not a
demonstrated repair. Fourteen measurements per fixture/configuration are too few
to claim a precise tail distribution. The standalone harness also did not apply
the live worker's user-initiated QoS policy.

**Decision:** keep the current defaults. A useful confirmation would interleave
four/eight-thread trials with production-equivalent QoS, more short-speech
fixtures, and separate quiet-host and contention conditions. Record native
mel/encode/decode timing and transcript equivalence together.

## Other Boundaries Worth Measuring

- Model preparation loads and prewarms a candidate, then drops it. After saving,
  the inference worker loads it again. Avoiding duplicate work requires a tested
  ownership and activation design, not removal of verification or prewarming.
- Optional generation combines service discovery and the generation request.
  Separate those timings before attributing the whole delay to a provider.
- Paste posts the shortcut before its 100 ms settling wait. Restoration runs
  separately, and the HUD has a completion animation. Completion time, visual
  dismissal, and target-app text consumption are different measurements.
- Sparkle checks are not an inference dependency. UI polling and native updater
  initialization need their own latency/energy measurements before optimization.
