# On-device speech benchmark · Android 15

**Default: multilingual Whisper small Q5_1.** It made 15 word errors on 285 reference words in Hex’s short-dictation configuration, against 50 for the bundled tiny fallback. Small was also the most accurate in the equal full-context comparison. Base was lighter but made twice as many errors as the default. Parakeet was the fastest batch decoder, but used about four times the native memory. Android’s streaming service was excellent when it returned text, but had incomplete coverage.

## What ran

On 20 Nov 2026, an API 35 Google ARM64 emulator ran on an Apple M5 Ultra Studio, with four guest CPUs and 4 GiB guest RAM (`emu64a:15/AE3A.240806.043/12960925`). [Corpus](corpus.tsv): 24 English, 6 Spanish, 5 German and 5 Norwegian utterances, nine macOS `say` voices, 157–196 words/minute. [Audio manifest](audio.tsv): 40 mono 16 kHz PCM16 WAVs, 2.20–3.72 s including padding (median 2.94 s); 24 clean, 16 mixed with a synthesized music bed at 8 or 14 dB SNR. These are short synthetic utterances, not human conversations. The two notebook clips use different voices/rates; the comparison across engines always uses identical bytes for a given clip.

**Word error rate (WER)** is total word edits divided by total reference words, not a mean of clip percentages. NFC, case folding and word tokenization ignore punctuation and preserve accents/internal apostrophes. A clip without a result counts as deleting every reference word. English supplies 186/285 words; Spanish 37, German 33 and Norwegian 29. These small language samples do not establish general language accuracy.

| Engine | Returned | WER | Clean | Music | EN | ES | DE | NO |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Whisper tiny Q5_1 | 40/40 | 17.54% | 11.66% | 25.41% | 2.15% | 21.62% | 27.27% | 100.00% |
| Whisper base Q5_1 | 40/40 | 10.53% | 7.36% | 14.75% | 0.54% | 2.70% | 24.24% | 68.97% |
| Whisper small Q5_1, full context | 40/40 | 5.96% | 3.07% | 9.84% | 0.00% | 2.70% | 15.15% | 37.93% |
| **Whisper small Q5_1, Hex’s short window** | **40/40** | **5.26%** | **3.07%** | **8.20%** | **0.00%** | **2.70%** | **9.09%** | **37.93%** |
| Parakeet TDT 0.6B v3 int8 | 40/40 | 11.58% | 8.59% | 15.57% | 0.54% | 0.00% | 21.21% | 86.21% |
| Android strictly on-device | 33/40 | 17.54% | 12.27% | 24.59% | 9.68% | 0.00% | 9.09% | 100.00% |

For native engines, one process per model loaded weights once, decoded sequentially using four CPU threads, and recorded monotonic wall clock, process CPU time and process high-water RSS. File read and conversion to floats precede timing. Whisper used greedy decoding, no context from previous clips, no timestamps, and the **known input language**; Parakeet had no language hint. Tiny, base and small first ran with their standard full encoder window. A second small run used `audio_ctx=512`; the app uses that only for captures ≤8 s. Longer dictations keep the full encoder window. The window was evaluated on this same corpus, not a held-out dataset.

| Engine | Weights¹ | Load, once | Warm p50 / p95 | Median process CPU | Peak native RSS |
|---|---:|---:|---:|---:|---:|
| Tiny | 32.2 MB | 43 ms | 390 / 493 ms | 1.39 s | 157 MiB |
| Base | 59.7 MB | 47 ms | 683 / 875 ms | 2.56 s | 224 MiB |
| Small, full | 190.1 MB | 93 ms | 2,570 / 3,039 ms | 9.86 s | 382 MiB |
| **Small, short** | **190.1 MB** | **115 ms** | **742 / 897 ms** | **2.81 s** | **344 MiB** |
| Parakeet | 487.2 MB archive | 2,451 ms | 113 / 143 ms | 0.43 s | 1,387 MiB |
| Android provider | OS language packs | opaque | 42 / 65 ms **after audio** | opaque | shared provider, not attributed |

¹ MB is decimal file size. Parakeet’s three int8 ONNX files and token table total 670.6 MB unpacked. RSS is binary MiB from `getrusage`. Timings are from this guest, not a phone. Peak native RSS excludes Android’s app UI and other processes. Across two real keyboard sessions, whole-app `VmHWM` reached 577 MiB and 639 MiB; the app loads/releases the native context per dictation instead of retaining the model while idle.

**Platform provider.** A disposable app used `createOnDeviceSpeechRecognizer`, not the generic recognizer with an offline preference. The image initially reported zero installed languages. After confirming the OS downloads, `checkRecognitionSupport` reported installed `en-US`, `es-ES` and `de-DE`; it never advertised Norwegian. With airplane mode on and Wi-Fi off, there were two music-mixed English no-matches (09/22, error 7) and five unsupported Norwegian clips (error 12). **WER on the 33 returned results alone was 2.89%.** The device processes audio as it arrives: 3.68 s median from ready to result, including playback; 42 ms median from the end of the injected WAV to result (includes adb stop delivery and 10 ms clock granularity). Where an explicit stop came first, median device `stopListening`-to-callback was 6.5 ms (32 results). One endpoint completed on its own. These are **not batch inference times**, nor measurements of natural endpointing without a stop. An [earlier paced real-time injection run](provider-first-pass.tsv) returned 32/40: 20.00% all-clip WER, 2.15% completed-only WER, and 33 ms median after audio. It is kept separately from the primary results. The primary run paced 20 ms packets with queue back-pressure.

## Estimate for a mid-range phone

**Budget roughly 3–7 s of CPU inference for a typical short clip on a Galaxy A23 5G / Snapdragon 695-class 4–6 GB phone. This is an estimate, not a handset measurement.** Eight fixed clips (01, 02, 12, 19, 25, 26, 32, 37) reran at two threads pinned to two guest vCPUs. [Raw measurements](two-core.tsv). Small’s short window took **1.36 s median** on those cores. Assume two effective performance cores at 20–40% of the Studio guest’s per-core throughput: 1.36 / 0.4 to 1.36 / 0.2 = 3.4–6.8 s. For scale only, individual Geekbench 6 single-core scores were [903 on a Galaxy A23 5G](https://browser.geekbench.com/v6/cpu/10852045) and [4,263 on an M5](https://browser.geekbench.com/v6/cpu/14496729); Geekbench is neither Whisper nor this M5 Ultra VM. Scheduling, memory bandwidth, storage, thermals and other apps can move a phone outside the range. Loading and editor insertion are additional.

For comparison the two-thread guest medians were tiny **0.56 s**, base **1.26 s**, full-window small **5.42 s**, Parakeet **0.16 s**. Full-window small is appreciably slower, so the short-clip estimate must not be applied to a minute of dictation.

## Keyboard check

The native benchmark consumes files; the debug APK check fed real WAVs through the authenticated emulator **microphone**, pressed the actual Hex key, and read what another app’s `EditText` received. Airplane mode was on and Wi-Fi off. The original phase-1 notebook WAV (SHA-256 `9e95daa12e469102c75b892820ad7e901e8a2131a3ac7f3e00d312bdb9322828`) produced “Please bring the green notebook to the meeting.” in both the practice field and the external editor. A music-mixed keyboard take also got it right; a later take heard “meter.” All native file runs except tiny got the music-mixed sentence exactly; tiny heard “green note.” The clean file alone did not distinguish the engines.

The external editor received the notebook sentence **1.91 s after mic stop** and “The coffee is still hot.” **2.13 s after mic stop**. These two timings include model initialization, inference and insertion. Its media track remained playing and advanced 481,622 frames over 30.10 s at 16 kHz; no audio-focus loss was observed. Tapping ✕ during inference left its editor empty after native completion.

**Same WAV, same style, only cleanup changed:** Polished inserted “You know, the coffee is still warm.” with cleanup off and “The coffee is still warm.” with it on. Clip 13 inserted “Meet me on Tuesday, sorry, Wednesday.” with cleanup off and “Meet me on Wednesday.” with it on. Verbatim preserved the spoken hesitation and correction; Casual removed the final period; Notes produced two separate bullet lines. The compiled Android cleanup/formatter also matched all 41 applicable Spanish, German and Norwegian input/output examples drawn from iOS’s `SpokenCleanupRulesTests`. This checks those rules, not parity with iOS’s learned English tagger.

**Model setup:** before downloading, bundled tiny inserted “The coffee is still warm.” offline. Retrying online showed download progress, then ready; the file’s verified device SHA-256 matched the small pin below. Airplane mode with Wi-Fi disabled was sufficient for both paste languages afterwards.

## Sources and licences

- OpenAI’s [multilingual Whisper weights](https://github.com/openai/whisper#license), **MIT**; Q5_1 files from [ggerganov/whisper.cpp at 5359861…](https://huggingface.co/ggerganov/whisper.cpp/tree/5359861c739e955e79d9a303bcbc70fb988958b1). SHA-256: tiny `818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7`; base `422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898`; small `ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb`. [whisper.cpp/ggml v1.9.5](https://github.com/ggml-org/whisper.cpp/tree/d1be6fde11ac6e0407606b4e42fe72d34add8037) **MIT**.
- NVIDIA’s [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) **CC BY 4.0**; its 25-language list does not include Norwegian. [Int8 conversion](https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2): SHA-256 `1495e91b59d7fc53ddca91f3431758ec99f8fa253fdb0e1e851be0f72b14b0f9`. [sherpa-onnx 2.0.2](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v2.0.2) **Apache 2.0**, [ONNX Runtime](https://github.com/microsoft/onnxruntime/blob/main/LICENSE) **MIT**. Android runtime archive SHA-256 `08d22f7eb5c49bd161d248633eeb66e84735725706e21202b684800e5081a029`. Benchmark only; not distributed with Hex.
- Android’s [on-device API](https://developer.android.com/reference/android/speech/SpeechRecognizer#createOnDeviceSpeechRecognizer(android.content.Context)) is a platform service; Google supplied the proprietary language packs on this image. Hex does not redistribute or require them.

## Reproducing the measurements

1. On macOS, run `python3 benchmark/generate.py "$WORK/wavs" > "$WORK/audio-sha.tsv"` with `say`, the named voices, and ffmpeg installed. Compare sample counts and digests with [audio.tsv](audio.tsv); voice assets can change with macOS. Generate the input list with `awk -F '\t' 'NR>1{print $1,$2}' benchmark/corpus.tsv`.
2. Build the Android app as in the [Android readme](../README.md). Compile [native.cpp](native.cpp) for arm64 with the Android NDK (`aarch64-linux-android31-clang++ -O2 -std=c++17 -static-libstdc++`). For Whisper use the app’s whisper/ggml headers and link `libwhisper.a` and `libggml*.a` in a linker group plus `-landroid -llog`. For Parakeet define `PARAKEET`, use the matching 2.0.2 C API header and link `libsherpa-onnx-c-api.so` from the **Android** runtime’s `jniLibs/arm64-v8a`, alongside its `libonnxruntime.so`.
3. Push the executables, weights, `wavs/` and two-column list to the same guest scratch directory. From there run `./whisperbench models/small.bin 4 list`, and repeat for base/tiny. Run `HEX_SHORT_WINDOW=1 ./whisperbench models/small.bin 4 list` for the shipping setting; `LD_LIBRARY_PATH=$PWD ./parabench parakeet 4 list` for Parakeet. Capture stdout as `results/{tiny,base,small,small-short,parakeet}.tsv` and stderr separately.
4. Build [SystemProbe.java](SystemProbe.java) with its [manifest](AndroidManifest.xml) using JDK 21 and SDK 35 (javac, d8, aapt, zipalign, apksigner). Install the disposable APK with `adb install -g`. Start `local.hex.measure/.SystemProbe` with `--es op download --es id pack --es lang <locale>` for each pack and confirm the OS dialogs. Force-stop **the probe** and start it fresh with `--es op support --es id packs --es lang en-US` to check installed locales and reconnect the service. Enable airplane mode and disable Wi-Fi. From the WAV parent, with emulator gRPC Python bindings on `PYTHONPATH`, run `python3 <benchmark>/system_runs.py > results/android.tsv`.
5. `python3 benchmark/score.py results > per-clip.tsv` emits each reference-word count, edit count, outcome, original hypothesis and timing, plus summaries on stderr. [Recorded per-clip results](per-clip.tsv). Only synthetic benchmark transcripts appear here.
