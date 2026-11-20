# Android typing benchmark

Replay of the iOS decoder’s **held-out English test set** on the Android runtime. Each of 2,000 synthetic sentences supplies its intended text, the key hit by each tap, and the original x/y taps from the 402-point iOS layout. These are simulated taps on public Tatoeba English sentences, not a person’s keyboard events.

## Results

**Android reduced WER from 33.81% to 10.53%. iOS reached 8.39% with the identical Google Books file and no personal offsets.** Android made 298 more word errors than that iOS run.

| Decoder | Word errors / 13,870 | WER | Exact sentences / 2,000 |
|---|---:|---:|---:|
| Raw hit keys | 4,689 | 33.81% | 160 |
| iOS previous engine | 2,086 | 15.04% | 736 |
| iOS spatial, earlier prototype | 930 | 6.71% | 1,319 |
| iOS prototype with calibrated offsets | 869 | 6.27% | 1,355 |
| iOS spatial, shared Google Books v3 | 1,163 | 8.39% | 1,154 |
| iOS shared model with calibrated offsets | 1,137 | 8.20% | 1,170 |
| **Android Kotlin, shared Google Books v3** | **1,461** | **10.53%** | **983** |

| Android emulator latency | Samples | Median | p95 |
|---|---:|---:|---:|
| Replay: letter + candidates | 56,826 | 0.004 ms | 0.239 ms |
| Replay: space + correction + next words | 13,675 | 0.082 ms | 0.372 ms |
| Real Hex keyboard: touch-up to external editor, letter | 245 | 1.409 ms | 2.589 ms |
| Real Hex keyboard: touch-up to external editor, space | 70 | 3.161 ms | 4.428 ms |

The live timing used guest monotonic timestamps immediately before injecting key-up and at the unrelated editor’s text-change callback. All five passes of the requested sentence matched, with pop-ups and haptics on. It measures insertion, not display paint or finger-down time. [Replay aggregates, including maximum latencies](metrics.tsv) · [Same-model iOS aggregates](ios-shared.tsv).

## What the numbers mean

WER is word-level Levenshtein insertions, deletions and substitutions divided by reference words, ignoring case and trailing punctuation, as in the iOS scorer. A lost space therefore counts as a word error even when every letter is present. Both platforms use the **same 2,000 references**. The previous-engine and prototype rows use the recorded iOS outputs for commit `92faec63`. The Google Books rows come from the later Studio R1 runs (`output-r1-decoder-en.json` and `output-r1-decoder-en-offsets.json`); both R1 model files were compared byte-for-byte against the Android files. Only aggregate scores for synthetic IDs were exported from those runs. No personal offsets are fed to Android.

The Android runner executes the installed APK’s Kotlin classes on an Android 15/API 35 ARM64 emulator (4 vCPUs, 4 GiB configured, Apple M5 Ultra Studio). Timing wraps each letter or space, including Gaussian scoring, completions, correction, and next-word suggestions where applicable. It excludes JSON parsing, scoring the benchmark, the editor IPC and rendering. It measures the emulator, not a handset.

Android uses a 48-prefix beam, a Gaussian per tap and a previous-word probability with unigram backoff. Replacement must beat the literal by 2 nats (7 for a known word). On the Android layout, testing rapid, exact key-centre presses exposed a delayed space click and a valid missed-space reading below the margin. Committing space on key-up preserves order. A vertical spread floor of 0.43 key pitches across and a 3.9-nat missed-space cost (2% omitted separators in the generator) let `seehkw` clear the unchanged margin. These rules apply to all words. The held-out WER was measured afterward.

## Models and licences

[The shared v3 builder](build-typing-model.py) is an unmodified snapshot of the iOS Google Books builder (SHA-256 `cba009fe227632d9f8d176055f26ca34d5eb6f88a7cbd947955d8f9142878317`). It sums 1980–2019 counts in [Google Books Ngram v3, 2020-02-17](https://storage.googleapis.com/books/ngrams/books/datasetsv3.html): English has 24 unigram and 589 bigram shards; Spanish has 3 and 73. It filters spellings, takes the most frequent words, selects informative pairs and quantizes costs in eighths of a nat. Source counts are **CC BY 3.0**. Neither model uses SymSpell. The files are packaged uncompressed so Android can map them directly from its APK. [Pinned SHA-256 manifest](../../app/src/main/assets/typing/SHA256SUMS).

The Studio reconstruction used complete, cached source counts and the filtered vocabularies from the shared build. Both outputs were byte-for-byte identical to the iOS builder’s outputs:

| Language | Words | Word pairs | SHA-256 |
|---|---:|---:|---|
| English | 86,133 | 417,480 | `14370569c747a7efb5cd2f8e75b70d2899937c12e149671fe7a0929c5e0c4028` |
| Spanish | 110,000 | 326,366 | `02ffcac185eb832d766b300a7454ba069d0b26251469ea7cdc6eda36926ad5ad` |

English spellings are filtered using **SCOWL 2020.12.07**, sizes through 70; the source archive SHA-256 is `5587667caa20c4891390c2d42dbb4d5c4c3f41bee77af1457ece3ba23fb859cc`. The APK includes the collective permission and the component notices. Spanish spellings are from RLA-ES’s `es_ES` and `es_MX` Hunspell dictionaries under their **MPL 1.1** option. All four source files from the shared build matched [LibreOffice’s pinned sources](https://github.com/LibreOffice/dictionaries/tree/762abe74008b94b2ff06db6f4024b59a8254c467/es). [APK credits and complete notices](../../app/src/main/assets/licenses/credits.txt).

## Reproduce

1. From the iOS experiment, export only the synthetic rows. The exporter requires the exact IDs `test-0` through `test-1999`, empty context and no logged flag. It takes no personal labels, calibration data, metadata or logged sentences. Do not transfer the mixed input/results files themselves.

   ```sh
   python3 android/benchmark/typing/export_synthetic.py "$HOME/dev/experiments/2026-11-20-hex-typing-logs/bench" > "$LOCAL_SCRATCH/synthetic.jsonl"
   ferry cp "$LOCAL_SCRATCH/synthetic.jsonl" anomastudio:Library/Caches/hex-android-phase3/synthetic.jsonl
   ```
2. On the Studio, set `T` to that scratch directory, `JAVA_HOME` to JDK 21, `ANDROID_HOME` to the SDK, and `ANDROID_SERIAL` to the test emulator. From the synced `android/` directory, run:

   ```sh
   ./gradlew assembleDebug test
   adb install -r app/build/outputs/apk/debug/app-debug.apk
   sh benchmark/typing/replay.sh
   ```

   The replay loader and the synthetic sentences live only in scratch; neither is an APK feature. The output contains only aggregate counts and timing percentiles. To regenerate models from the public sources, invoke `python3 benchmark/typing/build-typing-model.py en <count-cache> en.hextm` and similarly for `es` (the Spanish source filter needs `spylls`).

For the R1 comparison, on the Studio where the iOS outputs already reside, run `python3 benchmark/typing/compare_ios.py "$T/synthetic.jsonl" ios_spatial_shared /private/tmp/hex-bench-decoder/output-r1-decoder-en.json` (and similarly for `-offsets`). It prints only the synthetic aggregate; the mixed iOS file stays in place.
