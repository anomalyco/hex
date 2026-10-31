# Vocabulary on Kit's real dictation clips

**Status:** Research, 2026-10-23. No product change. Follows
[`parakeet-boost-spike.md`](https://github.com/anomalyco/hex/blob/spike/parakeet-boost/docs/research/parakeet-boost-spike.md)
(branch `spike/parakeet-boost`), [`custom-vocabulary.md`](custom-vocabulary.md)
and [`../plans/transcription-quality.md`](../plans/transcription-quality.md).

**Question:** On Kit's own everyday dictations, does Qwen3-ASR 1.7B with
recognition hints fix the vocabulary that Parakeet Unified misses, and at what
cost?

**Answer:** Partly. Hints raise term recall from 7/13 (what Hex pasted) to
10/13, but most of that gain does not come from hints. Two of the six misses
were `Rune` heard as "Ruin" and "Roon". Parakeet's own whole-clip decode
already gets both right, so they are streaming errors. Plain Qwen already hears
"grill". The hint list itself fixes only casing (`Hex` once, `HiveMind`) and
`dotfiles`, and `dotfiles` only after it was added to the list from these
clips. Qwen doubles or triples overall WER and takes about 5× Unified's offline
compute. The longer hint list made Qwen return **empty text on 4 of 49 short
clips**. Don't switch dictation to Qwen for vocabulary on this evidence.

## Method

- **Clips.** 50 retained WAVs from `diagnostic-dictation-audio` (Oct 22 23:06
  to Oct 23 14:46), converted to 16 kHz mono PCM16 copies. The filename is the
  save time, and its history entry follows 140–400 ms later, one to one. 49
  matched a history entry. One 1.1 s clip had no entry and is unscored. 219.9 s
  of scored audio, 733 reference words. Every clip went to OpenCode Beta.
- **References.** Each started from the history text (raw = final, since Kit
  has no replacements, transformations or post-processing configured). Before
  any model ran, I made contextual fixes: grow→grill (the grilling skill),
  defiles→dotfiles, Ruin/Roon→Rune, hex→Hex. I also flagged 12 clips with
  unclear wording. After inference I made two more edits where models agreed
  and context supported them, and flagged both: "only worth"→"probably worth"
  (Unified offline and every Qwen arm agree) and "macro gear change"→"MacBook
  Air change" (every Qwen arm agrees, and the Air left the fleet that week).
  That second edit favours Qwen, so `MacBook Air` is left out of the recall
  column below. No audio was played, so these are judged proxies, not a human
  audit.
- **Terms.** 13 pre-registered occurrences in 10 clips: Dax ×3, Rune ×4, Hex ×2,
  HiveMind, dotfiles, grill, Adam. Names are matched case-sensitively, so
  "hex" counts as a miss. Every arm gets Hex if casing is ignored.
- **Hints.** `hints17` is the boost spike's frozen list (Hex, Cappuccino,
  OpenCode, Rune, Scrawl, Effect, Anomaly, Dax, Aiden, Kit, Parakeet, Moonshine,
  GPUI, Sparkle, Argon, Astra, HiveMind). It was fixed before these clips
  existed. `hints27` adds Psychopomp, dotfiles, Qwen, Claude, Opus, Sonnet,
  Gemini, skulk, Zen and "grill me". `dotfiles` and "grill me" come from these
  clips' misses, so its recall is optimistic. Both use the spike's prompt:
  `Names and technical terms: <list>.`
- **Runner.** The spike's `model-runner`, unpatched `transcribe-cpp 0.4.1`, on
  the M4 Pro Mac mini (64 GB, strict Metal, one CPU thread, fresh session per
  clip, load average about 6.5). Studio's HiveMind refuses cargo work and the
  Pro had a load average of 51. The run was one warm pass plus two measured
  passes, with arm order rotated per clip. Accuracy comes from pass 1, and no
  transcript changed in pass 2. Unified streaming at 0.4.1 reproduces what
  production pasted on 48 of 49 clips, apart from punctuation.
- **Corrections arm (d).** Kit's `dictation_processing` has no replacements or
  rewriting, and `personal-commands.json` has no transformations. Parakeet plus
  corrections is therefore exactly the production row.

## Results

| System | Term recall (13) | False hint insertions | WER, all 49 (733 w) | WER, 37 confident (598 w) | Empty outputs | Median ms per audio-s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Production paste = Parakeet + corrections (0.2.4 stream) | 7 | 0 | 0.95% | 1.17% | 0 | — |
| Parakeet Unified, streaming | 7 | 0 | 1.09% | 1.17% | 0 | 38.2 total compute / **11.9 finalize** |
| Parakeet Unified, whole clip | 9 | 0 | 3.00% | 2.01% | 0 | **14.6** |
| Qwen3-ASR 1.7B, no hints | 8 | 0 | 6.55% | 5.02% | 0 | 70.3 |
| Qwen3-ASR 1.7B, hints17 | **10** | 0 | 6.00% | 4.18% | 0 | 74.9 |
| Qwen3-ASR 1.7B, hints27 | **10** | 0 | 8.19% | 6.35% | **4** | 73.1 |

- Counting the post-inference `MacBook Air` term adds one hit to every Qwen arm
  and none to Parakeet.
- Median per-clip time is 64 ms for Unified whole clip, 53 ms for the streaming
  finalize, and 267, 295 and 310 ms for the three Qwen arms. Qwen's p95 is 519
  to 565 ms, against 100 ms for Unified.
- With hints, Qwen's ms per audio-second is 5.1× Unified offline and 6.3× the
  streaming finalize, which is roughly how long the user waits after release
  in production. On a 5 s dictation that is about 370 ms against 60 ms.
- WER is proxy disagreement. Production text seeded the references, so it
  favours the Parakeet rows.

**No hint term was ever inserted where it wasn't said.** The arms fail in
other ways. Short clips under hints27 came back empty (2.0–2.6 s). Unhinted
Qwen turned the one silent, unscored clip into Chinese ("玩呢啊。"), which
Hex would have pasted. Qwen also rephrases.

## The five most telling clips

| Clip | Reference | Production / Unified stream | Unified whole clip | Qwen no hints | Qwen hints17 | Qwen hints27 |
| --- | --- | --- | --- | --- | --- | --- |
| 1792736041649 | push the **dotfiles** commits and what's up with the **MacBook Air** change | push the defiles commits … macro gear change | defiles … macro gear change | push the .diff files commits … MacBook Air change | push the death files commits … MacBook Air change | push the **dotfiles** commits … MacBook Air change |
| 1792735783602 | anything else you'd like to ask me or **grill** me about | grow me | grow me | **grill** me | grill me | grill me |
| 1792736219057 | Can we talk to **Rune** and get this started? | talk to Roon | talk to **Rune** | Rune | Rune | Rune |
| 1792735967611 | Why don't you measure first | Why don't you measure first | Why don't you measure first? | Why don't you measure first? | Why don't you measure first? | *(empty)* |
| 1792735980294 | … the A-B test. Do whatever you want to do | … A-B test. Do whatever you want to do | same | … AB test. Do it in VLAN two. | … A/B test. Do it in VLAN two. | same as hints17 |

- **Dotfiles** only comes back once the word itself is in the hint list. The
  frozen list produced "death files".
- **Grill** is not a hint win. Qwen hears it without any prompt.
- **Rune** is a Parakeet streaming error, not a vocabulary gap. The same
  happens in 1792736124209: the stream gives "I own Ruin", while the whole clip
  gives "Rune" both times. In that clip every Qwen arm also rewrites the
  sentence as "what I own Rune, we both work on" and drops one Rune.
- **Empty output** is the longer prompt's failure mode, and it hit 4 of 49
  clips.
- **VLAN two** looks like a Qwen invention. Both Parakeet modes agree on the
  original wording, but nobody listened to the clip, so it stays flagged.

## Takeaways

1. On Kit's ordinary dictation, vocabulary misses are rare: 6 of 13 term
   occurrences in about 3.7 minutes of speech. Most are not caused by a missing
   vocabulary list.
2. The cheapest real gain is Parakeet's whole-clip decode, which fixed both
   Rune misses for about 15 ms per audio-second. It also dropped trailing words
   in three clips ("nice to", "other principles", "permodal change"), so test
   tail padding before using it as a final pass.
3. Qwen with hints recovers names that need the list (`dotfiles`, casing) and
   inserted nothing false. It still costs about 5× compute, does not stream,
   triples proxy WER here, and fails to empty output as the list grows. That
   matches the spike's meeting result: better recall on names, worse
   everything else.
4. A literal replacement (`defiles`→`dotfiles`, `Roon`/`Ruin`→`Rune`) would fix
   these clips, but only because the list is built from the same clips.
   `custom-vocabulary.md`'s collision warning still applies.

## Caveats

- 49 clips, 13 term occurrences, one day, one speaker, one microphone. A single
  clip moves recall by about 8 points.
- References are judged from model agreement and context, not from listening.
  Twelve clips are flagged as unclear (the confident-WER column excludes them),
  and two edits came after inference.
- Timings come from a shared worker with both models resident, and cover the
  decoder only. They are not shortcut-to-paste latency.
- Aggregates and per-clip counts are in
  [`vocabulary-real-clips.json`](vocabulary-real-clips.json). Per-clip
  transcripts stay out of git.
