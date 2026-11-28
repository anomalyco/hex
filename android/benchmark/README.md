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

For native engines, one process per model loaded weights once, decoded sequentially using four CPU threads, and recorded monotonic wall clock, process CPU time and process high-water RSS. File read and conversion to floats precede timing. Whisper used greedy decoding, no context from previous clips, no timestamps, and the **known input language**; Parakeet had no language hint. Tiny, base and small first ran with their standard full encoder window. A second small run used `audio_ctx=512`, which the app used for captures ≤8 s until 23 Nov 2026; it now sizes the window to the speech ([Android readme](../README.md), Speed). Longer dictations keep the full encoder window. The window was evaluated on this same corpus, not a held-out dataset.

| Engine | Weights¹ | Load, once | Warm p50 / p95 | Median process CPU | Peak native RSS |
|---|---:|---:|---:|---:|---:|
| Tiny | 32.2 MB | 43 ms | 390 / 493 ms | 1.39 s | 157 MiB |
| Base | 59.7 MB | 47 ms | 683 / 875 ms | 2.56 s | 224 MiB |
| Small, full | 190.1 MB | 93 ms | 2,570 / 3,039 ms | 9.86 s | 382 MiB |
| **Small, short** | **190.1 MB** | **115 ms** | **742 / 897 ms** | **2.81 s** | **344 MiB** |
| Parakeet | 487.2 MB archive | 2,451 ms | 113 / 143 ms | 0.43 s | 1,387 MiB |
| Android provider | OS language packs | opaque | 42 / 65 ms **after audio** | opaque | shared provider, not attributed |

¹ MB is decimal file size. Parakeet’s three int8 ONNX files and token table total 670.6 MB unpacked. RSS is binary MiB from `getrusage`. Timings are from this guest, not a phone. Peak native RSS excludes Android’s app UI and other processes. Across two real keyboard sessions, whole-app `VmHWM` reached 577 MiB and 639 MiB; at the time the app loaded and released the native context per dictation; since 23 Nov 2026 it keeps it loaded for 2 minutes after the last dictation.

**Platform provider.** A disposable app used `createOnDeviceSpeechRecognizer`, not the generic recognizer with an offline preference. The image initially reported zero installed languages. After confirming the OS downloads, `checkRecognitionSupport` reported installed `en-US`, `es-ES` and `de-DE`; it never advertised Norwegian. With airplane mode on and Wi-Fi off, there were two music-mixed English no-matches (09/22, error 7) and five unsupported Norwegian clips (error 12). **WER on the 33 returned results alone was 2.89%.** The device processes audio as it arrives: 3.68 s median from ready to result, including playback; 42 ms median from the end of the injected WAV to result (includes adb stop delivery and 10 ms clock granularity). Where an explicit stop came first, median device `stopListening`-to-callback was 6.5 ms (32 results). One endpoint completed on its own. These are **not batch inference times**, nor measurements of natural endpointing without a stop. An [earlier paced real-time injection run](provider-first-pass.tsv) returned 32/40: 20.00% all-clip WER, 2.15% completed-only WER, and 33 ms median after audio. It is kept separately from the primary results. The primary run paced 20 ms packets with queue back-pressure.

## Estimate for a mid-range phone

**Budget roughly 3–7 s of CPU inference for a typical short clip on a Galaxy A23 5G / Snapdragon 695-class 4–6 GB phone. This is an estimate, not a handset measurement.** Eight fixed clips (01, 02, 12, 19, 25, 26, 32, 37) reran at two threads pinned to two guest vCPUs. [Raw measurements](two-core.tsv). Small’s short window took **1.36 s median** on those cores. Assume two effective performance cores at 20–40% of the Studio guest’s per-core throughput: 1.36 / 0.4 to 1.36 / 0.2 = 3.4–6.8 s. For scale only, individual Geekbench 6 single-core scores were [903 on a Galaxy A23 5G](https://browser.geekbench.com/v6/cpu/10852045) and [4,263 on an M5](https://browser.geekbench.com/v6/cpu/14496729); Geekbench is neither Whisper nor this M5 Ultra VM. Scheduling, memory bandwidth, storage, thermals and other apps can move a phone outside the range. Loading and editor insertion are additional.

For comparison the two-thread guest medians were tiny **0.56 s**, base **1.26 s**, full-window small **5.42 s**, Parakeet **0.16 s**. Full-window small is appreciably slower, so the short-clip estimate must not be applied to a minute of dictation.

## Keyboard check

The native benchmark consumes files; the debug APK check fed real WAVs through the authenticated emulator **microphone**, pressed the actual Hex key, and read what another app’s `EditText` received. Airplane mode was on and Wi-Fi off. The original phase-1 notebook WAV (SHA-256 `9e95daa12e469102c75b892820ad7e901e8a2131a3ac7f3e00d312bdb9322828`) produced “Please bring the green notebook to the meeting.” in both the practice field and the external editor. A music-mixed keyboard take also got it right; a later take heard “meter.” All native file runs except tiny got the music-mixed sentence exactly; tiny heard “green note.” The clean file alone did not distinguish the engines.

The external editor received the notebook sentence **1.91 s after mic stop** and “The coffee is still hot.” **2.13 s after mic stop**. These two timings include model initialization, inference and insertion. Its media track remained playing and advanced 481,622 frames over 30.10 s at 16 kHz; no audio-focus loss was observed. Tapping ✕ during inference left its editor empty after native completion.

**Same WAV, same style, only cleanup changed:** Polished inserted “You know, the coffee is still warm.” with cleanup off and “The coffee is still warm.” with it on. Clip 13 inserted “Meet me on Tuesday, sorry, Wednesday.” with cleanup off and “Meet me on Wednesday.” with it on. Verbatim preserved the spoken hesitation and correction; Casual removed the final period; Notes produced two separate bullet lines. The compiled Android cleanup/formatter also matched all 41 applicable Spanish, German and Norwegian input/output examples drawn from iOS’s `SpokenCleanupRulesTests`. This checks those rules, not parity with iOS’s learned English tagger.

**Model setup:** before downloading, bundled tiny inserted “The coffee is still warm.” offline. Retrying online showed download progress, then ready; the file’s verified device SHA-256 matched the small pin below. Airplane mode with Wi-Fi disabled was sufficient for both paste languages afterwards.

## Faster English dictation (Labs)

On 24 Nov 2026, an API 35 Google ARM64 emulator (4 vCPUs, 4 GiB) on the M5 Ultra Studio, which was also running two other emulators (load average about 13), so the absolute times below are slower than the 20 Nov ones above: read the ratios. Both engines are the app's own release native builds through [native.cpp](native.cpp): `HEX_APP=1 ./whisperbench` is the downloaded small model exactly as `hexspeech.cpp` decodes it (window sized to the speech up to 8 s and, before #2716, the full window above; the token cap and single retry), and `./englishbench` (built with `-DTRANSCRIBE`) is Parakeet TDT-CTC 110M Q8_0 through transcribe.cpp as `hexenglish.cpp` runs it. Four threads; each clip's figure is the median of two runs. English only: the faster model is never used for another language.

| English clips | Small: wall / CPU | Faster: wall / CPU | Faster, ×less | Word errors (small / faster) |
|---|---:|---:|---:|---:|
| 24 short (2.2–3.7 s), median | 1,563 / 3,975 ms | 76 / 263 ms | 21× / 15× | 0 / 0 of 186 |
| 6 of 11.3–13.3 s, each | 2,552–2,698 / 9,603–10,082 ms | 207–222 / 786–852 ms | 12.0–13.1× / 11.8–12.8× | 8 / 0 of 186¹ |
| 3 of 23.9–26.0 s, each | 2,894–2,979 / 10,603–10,968 ms | 414–487 / 1,565–1,867 ms | 5.9–7.2× / 5.7–7.0× | 8 / 0 of 186¹ |
| One 60 s clip | 6,083 / 22,236 ms | 1,078 / 4,154 ms | 5.6× / 5.4× | — |
| 4 short at 2 threads on 2 vCPUs, median | 1,071 ms wall | 96 ms wall | 11× | 0 / 0 |

¹ Small leaves out the second of two identical sentences in the two clips that have one.

Small's time is nearly flat from 8 to 30 s (the full 30 s window), while Parakeet's grows with the speech, so the gain is largest for 8–13 s and smallest for a minute. Peak native RSS: small 417 / 462 / 500 MiB (short / long / 60 s), faster 260 / 355 / 431 MiB; its weights are a read-only file mapping. Load: 27–117 ms. All 40 clips: small 5.26% WER (the same as on 20 Nov), faster 35.44%, all of it in Spanish, German and Norwegian, which it never sees in the app.

### Through the keyboard, build 2448

On 24 Nov 2026, 20:23–21:08 ET (Studio load average 4.4–9.1 from another project's jobs), a clean debug build of `2278b31af` (Hex Preview build 2448: #2716's encoder window past 8 s and #2783's prompt retry are in) on the same emulator. Each figure is the native decode's time for a dictation made through the real Hex key, with the clip played into the emulator's microphone and no screen reading during the decode. The switch was on and off, twice each, alternating.

| English clips | Standard model | Faster English | Faster by (medians) |
|---|---:|---:|---:|
| 6 of 11–13 s (12 dictations each way) | 3,924–5,761 ms, median 4,694 | 700–994 ms, median 748 | 6.3× |
| 4 short, 2.2–3.7 s (8 each way) | 1,257–2,358 ms, median 1,786 | 109–325 ms, median 236 | 7.6× |

Through the keyboard the ratio is about half the harness's 12–13× above, for two reasons. The keyboard path adds fixed costs to both models. And the harness table predates #2716, which gave the standard model's encoder a window sized to the speech instead of Whisper's full 30 s: at 10–13 s that makes the standard model about a quarter faster. **The faster model didn't get slower**; the model it's compared with got faster. #2755 (timestamps past 30 s) changes only audio over 30 s; the only such clip above is the 60 s one, which has no accuracy figure. The word-error columns are from the native builds of 24 Nov, before both, and still stand for clips under 30 s.

- **First dictation after a restart** (the faster model is loaded, and its file hashed once, while the person speaks): the decode was 233–311 ms, no slower than later ones (151–394 ms). The load and hash stayed inside the 2.8 s of speech.
- **Switching language each dictation** (English, faster model ↔ Español, standard model): each switch loads the other model while the person speaks. The first decode after a switch was about 200 ms slower for English (410–418 ms against 209–231) and unchanged for Spanish (1,506–1,540 ms).
- **✕ while a 60 s dictation decodes:** the faster model finished 0.5 s after the ✕ and its text was dropped; the standard model stopped 1.5 s after it. The next dictation took 74 ms with the switch on and 1,641 ms off.
- **Sentence ends on the microphone set** (33 English clips, switch on): of 39 sentence ends inside dictations, 1 was missing ("just after midnight Can you") and 1 was added ("Bring the blue folder. I mean the green folder."). On the clean files above, the counts were 1 missing of 10 and none added.
- **The standard model against itself:** two runs of the build before Faster English (`0d6bddad9`) on the 7 clips whose text had differed between that build and the switch-off branch gave different text on 4 of them. So those differences are the injected audio's run-to-run variation, not the switch.

### On real phones

- **OnePlus 12 (Snapdragon 8 Gen 3; 1 prime, 5 performance and 2 efficiency cores), build 2448**, from the Diagnostics report's Last dictation:

| | Kept | Model | Decode | Split | Processor | Text in after ✓ |
|---|---:|---|---:|---|---:|---:|
| Switch on | 6.2 s | faster English, 4 threads, loaded while speaking | 411 ms | none reported | 1,579 ms (3.8×) | 533 ms |
| Switch off | 12.7 s | standard, 4 threads kept ready on cpu 2-7, encoder window 1280, loaded while speaking | 7,919 ms | encoder 7,218, decoder 305, prompt 309, other 87 ms | 32,670 ms (4.1×) | 8,030 ms |

  The faster model ran at the emulator's speed (about 410 ms for 6.2 s on both). The standard model ran about 1.7× slower than on the emulator for the same length: it's compute-bound in its encoder, at 91% of the decode with all four threads busy. The two dictations differ in length, so the phone's ratio for one length is an estimate: about 0.8 s against 7.9 s for 12.7 s, roughly **10×**. The owner's words: before, "a solid 4-5s … to dictate"; after, "Wow! Yes its instant now."
- **Seen in the release gate on the Studio's gate emulator** (build 2448's candidate, functional runs, not a timing harness): English decodes 0.1–1.6 s with the switch on against 1.8–10.9 s off, and the tester script's long text 873 ms against 5.4 s.

### Sentence ends

Parakeet TDT-CTC 110M sometimes runs two sentences together in its own output ("back of the bus We should", "Wednesday My brother"), from 11 s up; the 0.6B model and Whisper small wrote all 48 sentence ends of the long English clips. It isn't the attention window (forcing full attention on the 60 s clip left 4 of 30 missing) and no join is involved. The app puts them back in `SentenceEnds` (faster engine only, before Hex's shared formatting): a capitalised word that English writes in lowercase (in the English typing model, not on its capitals list, not "I") after a pause of at least 100 ms of quiet frames (MicCapture's −48 dBFS test) between the two words' starts. Counted on the host over every English clip (24 short, 9 long, the 60 s one, and 14 clips of 2–3 sentences full of names and places with 0.15 or 0.6 s between sentences), with word times from transcribe.cpp (which leave the text unchanged and cost no measurable time):

| Rule | Missing ends restored | Stray periods |
|---|---:|---:|
| Capitalised common word, any pause | 9 of 10 | 8 (Dune, Main, Target, New, Godfather, Play…) |
| … with ≥ 20 ms of quiet | 9 | 2 |
| … with ≥ 40 ms | 9 | 0 (1 unaligned) |
| … with ≥ 60 to ≥ 150 ms | 9 | 0 |
| … with ≥ 180 ms | 8 | 0 |
| … with ≥ 200 ms | 4 | 0 |
| Any capitalised word with ≥ 100 ms | 9 | 0 |

The one end not restored is "on Saturday I didn't": "I" is never taken as a sentence start. 100 ms is the middle of the band with no stray period; a capital alone after a pause added nothing here, so names are left as they are. Ten missing ends is a small sample; the rule has to be watched on real voices.

## Long clips and the encoder window

Past 8 s of audio the app gives the small model's encoder the speech plus 640 frames (12.8 s), rounded up to 64, instead of Whisper's full 1500 ([hexspeech.cpp](../app/src/main/cpp/hexspeech.cpp)). [long.tsv](long.tsv) joins the 40 utterances end to end into 26 long clips of 9.8–26 s in English, Spanish, German and Norwegian (the corpus's consecutive groups of 4 and 8 per language, and groups of 4 and 5 from other starting points). The check decodes them with the app's decode at that window and at the full one. **It passes when no clip has more word errors than with the full window.** Clips start to lose words from a margin of 576 frames, so don't lower the margin without rerunning this check, and the wider 53-clip set in #2716 when possible.

1. `python3 benchmark/generate.py "$WORK/wavs"` writes the long clips after the 40 (their digests change with the 40's; see step 1 below).
2. Build [native.cpp](native.cpp) as in step 2 below, or for the host with Apple clang against a host build of the same whisper.cpp (`-DGGML_NATIVE=OFF -DGGML_CPU_ARM_ARCH=armv8-a`, as the app builds it).
3. From the folder holding `wavs/`, with `awk -F '\t' 'NR>1{print $1,$2}' benchmark/long.tsv > long.list`, run `HEX_APP=full ./whisperbench models/small.bin 4 long.list > results/long-full.tsv` and `HEX_APP=1 ./whisperbench models/small.bin 4 long.list > results/long-app.tsv`. `HEX_APP` is the app's decode (a fresh state for every clip, its token limit and single retry, its window); `HEX_APP=full` keeps the full window past 8 s.
4. `python3 benchmark/score.py results` adds both to the per-clip output and exits with status 1, naming the clips, if any is worse, or if a clip past 30 s loses an utterance (half its words or more missing).

On 24 Nov 2026 (Apple M5 host, armv8-a kernels, four threads; 34 of the 40 WAVs matched [audio.tsv](audio.tsv) with that Mac's voices) both made 43 errors on 900 words: no clip worse, three reworded with the same error count. The median decode went from 2.63 s to 1.96 s on that host.

**Past 30 s** Whisper needs more than one 30 s window. Four of the clips (en30s3k12, es30l0m0m0, de30x3, no30x3: 35–47 s, sentences repeating as people repeat themselves) check that every utterance comes out. The app decodes audio past 30 s with timestamps, so each window ends at the last sentence it finished and the next starts there. Without them whisper.cpp moves on a whole 30 s at a time, cutting mid-sentence. On 24 Nov 2026 the decode before that change lost 7 of the 15 German utterances (score.py exited 1); with it every utterance came out (59 errors on 1,263 words against 108).

## A prompt on a short clip

The app gives Whisper the custom vocabulary, and with Labs › Learn from my fixes the words you meant, as its initial prompt, joined with ", ". In front of a short clip a prompt can make the model write the clip's sentence again and again until its token limit: "Sarah is at the station." (macOS Samantha, 1.6 s), prompted "stadium", came out as "Sarah is at the station" ten times, and its retry at 0.2 keeps the prompt and repeats too. Cut a little differently, it wrote the sentence exactly twice and stopped. So a prompted decode up to 30 s is decoded once more without the prompt, and that text is used, when it ends without end-of-text and its last 32 tokens fail Whisper's own repetition test (entropy under 2.4); when it is one phrase of three or more words written two or more times and nothing else, with more than 4 words a second of the audio; or when it writes nothing, or only the prompt's two or more terms ([hexspeech.cpp](../app/src/main/cpp/hexspeech.cpp)). score.py's `PROMPTED` lists the clips and prompts; generate.py writes the clips after the corpus.

1. `python3 benchmark/score.py --prompted-list > prompted.list` (id, language, prompt per line; [native.cpp](native.cpp) reads the rest of a line as the prompt).
2. `HEX_APP=1 ./whisperbench models/small.bin 4 prompted.list > results/prompted-app.tsv`; `HEX_NO_GUARD=1` decodes as the app did before the guard.
3. `python3 benchmark/score.py results` exits with status 1, naming them, if any prompted clip comes out with more than 1.5 times its words.

On 24 Nov 2026 (Apple M5 Studio host, armv8-a kernels, four threads) the decode before the guard wrote the sentence ten times for all three prompts and score.py exited 1; with it each came out "Sarah is at the station." The corpus and long-clip texts stayed byte for byte the same, as none has a prompt. The same decode on `sarah.wav` from file, its start and end cut 20–40 ms apart (−100 to 600 ms before the speech, 200 to 800 ms after): with no prompt 0 of 468 cuts repeated; "stadium" 193 of 288; "Sarah, stadium" 8 of 72; "Sarah, stadium, Hex, OpenCode" 53 of 72; "stadium.", with its period, 0 of 72. On 150 clips of 1.2–7 s that each say a name (macOS Samantha and Daniel, three cuts), a prompt repeated only up to 2.4 s (at most 4 of 30 per prompt), never wrote nothing or only itself, and got the name right far more often at every length: with the one name as the prompt, 50%, 30%, 67%, 60% and 43% from the shortest to the longest group, against 10%, 0%, 0%, 10% and 10% with none. So there is no length below which the prompt is left out. A one-word clip can repeat with no prompt too ("Jackson" a dozen times, 3 of 30 at 1.2–1.8 s); with no prompt to leave out, the guard leaves those alone.

## Automatic listens for the language

With Labs › Automatic listens for the language on, Automatic chooses among up to three languages: the keyboard's first, then the phone's other languages Hex dictates, then the last one picked by hand. [hexspeech.cpp](../app/src/main/cpp/hexspeech.cpp) hands them to [whisper-language-choice.patch](../app/src/main/cpp/whisper-language-choice.patch). The patch runs whisper.cpp's detection step (the start token alone through the decoder) on the first window the decode encodes anyway, at the app's window, and takes a softmax over the candidates only. It keeps the keyboard's language unless another reaches **0.99 under 4 s of audio, or 0.95 from 4 s**. Bokmål counts Whisper's Nynorsk with it. whisper.cpp's own detection ("auto") encodes a second, full 30 s window first: 1,964 ms at 3 s on the host, against 3 ms here, and no more accurate.

**The guard.** With `HEX_LANGS`, native.cpp decodes each clip once per other language listed, with Automatic choosing between the clip's own language (the keyboard's) and that one, as the app does. It stops at the first token, since the language is chosen before it. `score.py` exits 1, naming the pair, if any clip changes language. From the folder holding `wavs/`, with `all.list` the corpus's and long.tsv's two-column lists together:

    HEX_APP=1 HEX_LANGS=en,es,de,no,fr,it,nl,pt,pl,ru ./whisperbench models/small.bin 4 all.list > results/switch.tsv
    python3 benchmark/score.py results

On 24 Nov 2026 (Apple M5 host, armv8-a kernels, four threads, nice 19), 630 of 630 pairs kept the clip's language: the 40 clips and the 30 long ones, each against the nine other languages. The highest probability another language reached was 0.62 (clip 38, Norwegian, against German). With one language, the same build decodes byte for byte as one without the patch: the 70 clips at the app's window and the 30 long ones at the full window.

**With a prompt.** `HEX_CANDIDATES=es` (with `HEX_APP`) decodes each clip as the app does with the lab on, choosing among the clip's own language and those listed, including the decode again without the prompt (A prompt on a short clip): it chooses the same way, since the choice reads only the audio. Run the prompted list with it, and score.py's prompted check must still pass.

**How the threshold was chosen.** The test set was 2,466 cuts of synthetic macOS voices in 14 languages:
- the same ten everyday sentences in Spanish, German, Norwegian, Italian, Portuguese, Dutch and French, two voices each where macOS has two;
- the 40 clips' English, Spanish, German and Norwegian;
- the earlier Portuguese, Czech, Danish, Polish, Russian, Swedish and Ukrainian corpora;
- English and Dutch voices, and 66 Dutch sentences full of English words.

Each cut starts at a sentence, runs on through the next ones and stops at 1.5, 2, 3, 4, 5 or 10 s. These clips aren't in the repository. For every pair and every triple of Hex's ten languages, each cut was taken once with its own language as the keyboard's (a pick of any other is a wrong switch) and once with each other candidate as the keyboard's (a pick of its own language is a right switch). The probabilities came from this library, built from the repository's patch files, through the patch's own API. The two-candidate table reproduced the prototype's exactly.

| Speech | Window | Choice step, median / p95 | Wrong switches, 2 / 3 candidates | Right switches, 2 / 3 | Spanish, Italian, Portuguese pairs: wrong, right | Highest wrong-language probability |
|---|---|---|---|---|---|---|
| 1.5 s | 320 | 3.0 / 4.9 ms | 0 of 2,745 / 0 of 10,980 | 76.5% / 62.0% | 0 of 182, 63.2% | 0.982 |
| 2 s | 320 | 2.9 / 5.8 ms | 0 / 0 | 84.3% / 73.9% | 0, 75.3% | 0.975 |
| 3 s | 384 | 2.9 / 5.9 ms | 0 / 0 | 87.7% / 78.5% | 0, 79.7% | 0.953 |
| 4 s | 448 | 3.0 / 5.0 ms | 0 / 0 | 97.6% / 95.4% | 0, 93.4% | 0.900 |
| 5 s | 448 | 2.8 / 6.1 ms | 0 / 0 | 99.1% / 97.9% | 0, 93.4% | 0.561 |
| 10 s | 1152 | 3.7 / 8.1 ms | 0 / 0 | 99.5% / 99.0% | 0, 96.7% | 0.221 |

The thresholds sit above the highest wrong-language probability at each length.
- **Hardest to catch short:** Norwegian (51% at 2 s, 100% from 5 s), then German, Dutch and Portuguese (75–77% at 2 s).
- **Hardest pair:** a Spanish keyboard with Portuguese speech.
- **Bokmål:** without the Nynorsk merge, 8 of 25 two-second Norwegian dictations on a Norwegian keyboard went to English at 0.9. With it, all 25 stay Norwegian against each of the nine others, at every length. Whisper's own unrestricted detection called only 7 to 9 of 25 three-second Norwegian clips Norwegian, mostly Nynorsk.
- **Danish and Swedish** are never candidates; their speech went to Norwegian for an English keyboard in 0 to 1 of 20 cuts.
- **Dutch full of English words:** a Dutch keyboard never switched to English.

These are synthetic voices, so the lab stays off by default until real dictations and Diagnostics' Last dictation lines confirm them.

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
