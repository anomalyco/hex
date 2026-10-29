# Vocabulary boost on real clips

**Status:** Measured, 2026-10-28. Shipped dormant behind the `vocabulary`
experiment. Follows [`custom-vocabulary.md`](custom-vocabulary.md),
[`vocabulary-real-clips.md`](vocabulary-real-clips.md) and the no-ship
`spike/parakeet-boost` branch.

**Question:** Can Parakeet write the user's Vocabulary terms ("Abattoir",
"OpenCode", "Scrawl") when it mishears them, without writing them where they
weren't said?

**Answer:** Yes, at the default strength (Normal, 2.5 nats per piece). With
"Abattoir" in the list, Hex writes it right **12 of 19** times on Kit's real
clips (was 4), and **13 of 19** when streaming (was 6). Across every listed term,
recall goes from 136 to 146 of 157. Word error rate is no worse in any group.
Clips that contain no term are unchanged, and every new term the boost wrote
was right. The cost is time: rescoring adds about 0.05–0.15 s (median) to a clip's
finish when the clip has a candidate, and nothing when it doesn't.

## How it works

The change lives in the `transcribe.cpp` fork (`anomalyco/transcribe.cpp`,
branch `hex/parakeet-vocabulary`, pinned at `9badd2a`) under
`src/arch/parakeet/vocabulary.cpp`.

```ts
greedy decode, unchanged ─▶ words with their frames
  └─ candidate finder: spans of 1–4 words that look like a term
       ├─ same letters, a sounds-like spelling, or close letters
       ├─ or one consonant sound off, only in names with 5+ sounds
       └─ skip: case-only, inflections, short terms without a sounds-like,
                addresses and paths (. / @ : \ _)
  └─ for each candidate: transducer forward score on the same frames,
     from the same predictor state
       └─ replace when log P(term) + boost × pieces ≥ log P(heard)
```

Hex passes the list at the start of each run or stream
(`gguf_session::with_vocabulary`), only to Parakeet models that accept it and
only when no Whisper-style hint family is set. `local_transcriber` shares that
path, so macOS, Linux and Windows all get it. The list is capped at 64 terms and
each term at 12 pieces, with at most 16 candidate spans per decode. Rescoring runs on the transcription engine thread after
decoding, never on the audio thread.

### Why not boost tokens during decoding

The spike added a bonus to term pieces at each greedy step, behind a margin
gate. On real speech the gate was either too tight or too loose. Parakeet is
very sure of "a" at the start of "Abattoir" (log-prob near 0), so a gate that
protected ordinary speech never let the term start: Abattoir went 0/11 → 1/11
on the microphone and "Avatar" stayed Avatar. A looser gate fired on ordinary
words and raised streaming WER by 0.9 points. Rescoring sees the whole span
before it decides. Where Kit said Abattoir and Parakeet wrote "avatar", the
frames score −0.2 as "avatar" and −9.2 as "Abattoir". Abattoir is 5 pieces, so a
2.5 boost (12.5 total) covers that gap and the term wins. Elsewhere "avatar"
scored −0.1 against −13.7. That gap is wider than the boost, so the word stayed.
The audio decides, not the spelling.

### Rejected: internal language model subtraction

Subtracting the predictor's own text prior (ILM, λ = 0.3–0.5) makes rare names
cheaper. It over-fired: "about" → Abattoir and "right" → Parakeet. The boost
stays a plain value per piece.

## Method

- **rve** (real-voice-expansion): 168 of Kit's real clips: 20 dictations, 78
  meeting microphone, 70 meeting system audio. 19 contain "Abattoir" and 43
  contain no term.
- **tc** (transcription-corpus): 274 clips: 51 synthesized term clips, 18
  synthesized look-alikes, 200 ordinary synthesized sentences, 5 private.
- **Vocabulary:** Hex, Abattoir, OpenCode, Rune, Scrawl, Effect, Anomaly, Dax,
  Aiden, Kit, Parakeet, Moonshine, GPUI, Sparkle, Argon, Astra, HiveMind.
- **Model:** `parakeet-unified-en-0.6b-Q8_0`, Metal, on the MacBook Pro. Each
  clip runs offline and streaming, with and without the list.
- **WER** is normalized, and "term-normalized" also joins "open code",
  "GP UI", "hive mind" and "moon shine" on both sides. Without that, a correct
  "OpenCode" against a reference "open code" counts as two errors, which made
  raw WER rise even though every change was a fix.
- **False insertions:** a term written where the reference has none. Each one
  was checked by hand.

## Results

Recall counts listed terms written correctly. WER is term-normalized.

| rve, offline | Off | Normal (2.5) | Strong (4.0)¹ |
|---|---|---|---|
| Abattoir | 4/19 | **12/19** | 14/19 |
| All terms | 136/157 | **146/157** | 148/157 |
| WER, all | 5.05 | **4.90** | 4.88 |
| WER, dictation mic | 3.32 | 3.32 | |
| WER, meeting mic | 4.85 | 4.60 | |
| WER, meeting system | 5.63 | 5.59 | |
| WER, no-term clips | 3.14 | **3.14** | 3.24 |
| Wrong insertions | — | **0** | 1 ("Avatar") |

| rve, streaming | Off | Normal (2.5) |
|---|---|---|
| Abattoir | 6/19 | **13/19** |
| All terms | 139/157 | **146/157** |
| WER, all | 6.00 | **5.91** |
| WER, dictation mic | 0.26 | 0.26 |
| WER, meeting mic | 6.39 | 6.30 |
| WER, meeting system | 6.41 | 6.32 |
| WER, no-term clips | 2.26 | **2.26** |
| Wrong insertions | — | **0** |

| tc | Offline off | Offline Normal | Streaming off | Streaming Normal |
|---|---|---|---|---|
| Abattoir | 2/5 | 4/5 | 2/5 | 4/5 |
| All terms | 47/56 | 53/56 | 47/56 | 53/56 |
| WER, all | 1.82 | 1.69 | 1.65 | 1.54 |
| WER, no-term clips | 1.63 | 1.63 | 1.61 | 1.61 |
| Wrong insertions | — | 0 | — | 0 |

¹ Strong was measured at 3.0 per piece. Hex's Strong level is 4.0, a
deliberate step up for users who want it.

The scorer counted four "false" insertions on rve and one on tc at Normal. None
is an error. Two are "Scrawl" where the reference wrote "scroll" but Kit said
Scrawl. Two ("Kit", "Scrawl") are already there with the boost off. The tc one
is "Parakeet" in a clip whose rough reference spells it "parake". At 3.0, the
literal word "Avatar" became Abattoir once. That is why Normal is the default.

### Latency

Measured at the final fork commit on the MacBook Pro while it ran other build
jobs, so absolute numbers are noisy; the differences are what matter.

| Per 10 s of audio | Off | Normal |
|---|---|---|
| rve offline, median / p90 | 363 / 670 ms | 356 / 698 ms |
| tc offline, median / p90 | 695 / 957 ms | 763 / 1111 ms |
| rve streaming finish, median / p90 | 175 / 298 ms | 215 / 439 ms |

With this 17-term list, 81 of 168 rve clips had at least one candidate to
rescore. A clip with none pays nothing. Where there are candidates, rescoring
takes a median of 62 ms offline and 122 ms at a stream's finish, and at most
0.55 s. An earlier, looser candidate finder scored 146 clips for the same
results and cost about twice as much.

## Settings

Vocabulary › Boost: Off (0), Light (2.0), Normal (2.5, default), Strong (4.0).
CLI: `hex settings set vocabulary_boost normal`. The boost exists only while the
`vocabulary` experiment is on (`hex experiments enable vocabulary`).

## Next

- Rescoring cost grows with candidates per clip. If the streaming finish
  matters, score candidates in one batched joint pass instead of one at a time.
- Remaining misses are mostly "about"/"other" for Abattoir, where the audio
  really sounds like the common word. A user's sounds-like spelling ("a bat
  war") fixes those without a stronger boost.
