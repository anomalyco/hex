# Vocabulary evaluation corpus

Run these fixed WAVs through `hex benchmark-transcription manifest.json` to
measure proper-name recall and unwanted vocabulary insertions. They are
synthetic fixtures, not recordings of Kit speaking.

The manifest contains 51 target clips (three sentences for each of 17 terms)
and 18 distractors. ElevenLabs generated mono 16 kHz PCM using Kit's authorized
cloned voice. `provenance` records the provider, model and settings; each clip
has a SHA-256. References are the exact TTS inputs, not human-audited transcripts.
No audio was played aloud during generation. The synthetic audio and authored
sentences are provided under this repository's MIT license.

The original 14 terms came from the approved transcription-quality plan.
Argon, Astra and HiveMind came from a read-only 30-day history query on
2026-10-22. The saved seven-day retention yielded 2,000 entries, all with equal
raw and final text; this was not 30 days of retained speech. HiveMind also
appeared as “hive mind”. No private history text is included here.

Distractors include genuine uses of “avatar”, “hanks” and “rune”, short-word
neighbors such as “checks” and “text”, and ordinary uses of vocabulary words.
An ordinary “rune” is already in the reference and therefore is not a false
replacement. Scoring ignores case and internal spacing but respects word boundaries.

## Reproduce and extend

```sh
# No synthesis or credits without --generate. Existing WAVs are reused.
python3 scripts/vocabulary-corpus.py tests/fixtures/vocabulary
# Supply ELEVENLABS_API_KEY securely, then add --generate to render missing WAVs.

# Run downloads/conversion and inference through HiveMind on a worker.
umask 077
python3 scripts/vocabulary-librispeech.py perf/transcription-corpus/librispeech
python3 scripts/vocabulary-eval.py merge .local-corpus/eval.json \
  tests/fixtures/vocabulary/manifest.json \
  perf/transcription-corpus/librispeech/manifest.json
hex benchmark-transcription .local-corpus/eval.json --runs 7 > .local-corpus/baseline.jsonl
python3 scripts/vocabulary-eval.py score .local-corpus/eval.json .local-corpus/baseline.jsonl
```

The public guard fetcher retains the first 200 FLAC members of LibriSpeech
`test-clean` in archive order, with the supplied references. It is deterministic
but not speaker-balanced. [LibriSpeech](https://www.openslr.org/12) is CC BY 4.0;
downloaded audio stays outside git.

Append `.local-corpus/manifest.json` to the merge command to include private
recordings. That directory is ignored. Keep source WAVs read-only, copy selected
clips with owner-only permissions, and use the `private` group. Match diagnostic
WAV timestamps to history carefully: nearby timestamps alone are not an audited
reference. Private history-derived WER is diagnostic only, never a release gate.

Reserve each term's `-3` sentence as a held-out check when tuning; use `-1` and
`-2` to choose parameters. Do not select parameters on the public guard set.

## Metrics and gate

The Rust harness emits `quality` for each clip and overall, plus `groups` in
the summary. WER remains word-based (OpenCode vs “open code” counts as edits).
Term recall counts expected term/clip pairs, normalized for case and spacing.
False replacements count vocabulary occurrences beyond their count in the
reference, including repetitions. The rate is affected clips / clips, not
insertions / words. A group without expected terms reports null recall.

The scorer accepts either the Hex JSONL or the isolated decoder runner's JSONL.
It refuses missing, duplicated or unknown clip results. Quality uses the first
measured pass; latency uses every measured pass, including Hex's `measurement`
records. Model loading and warmups are excluded.

Compare target recall (at least 70%), combined ordinary/distractor false-replacement
rate (at most 0.5%), ordinary-speech WER change (at most +0.3 percentage points),
and repeated paired latency measurements (p95 added decode time below 5 ms;
at most 20 ms end-to-end on a 10 s clip). Require zero false insertions on the
explicit distractors so a large ordinary corpus cannot hide a failure.
The synthetic clips are short; use the public guard's longer clips for latency.
Keep the scoring vocabulary fixed across baseline and candidate runs, including
when merging private clips: merge unions all inputs' vocabularies.
The approved spike also requires
real Hanks→Hex evidence; synthetic success cannot replace it.
