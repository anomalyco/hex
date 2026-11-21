# Meeting speakers: on-device diarization of "Them"

2026-11-04. A measurement spike. Hex's behaviour doesn't change.

Question: which local model should split a meeting's `system.wav` ("Them")
into Speaker 1..N after a call, and which can later recognise known voices
across meetings from saved voiceprints?

## Verdict

```ts
Diarize "Them" after the call
├── Model: NVIDIA Nemotron-3 Diarization (Sortformer "V3"), OpenMDW-1.1, 107 MB q8_0 GGUF
│   ├── up to 8 speakers; 9.1–9.5% DER on degraded synthetic calls (pyannote community-1: 14.3%)
│   └── 400–640× realtime with the offline preset on Apple Silicon (Metal)
├── Runtime: transcribe.cpp, which Hex already links
│   ├── Hex's pinned rev already exposes transcribe_cpp::DiarizeSession and runs Sortformer v2.1
│   │   (11.8% DER, 180× realtime, but a hard 4-speaker cap)
│   └── work: port the V3 (8-speaker) head into transcribe.cpp's Sortformer family;
│       NeMo-Speech.cpp's ggml version (Apache-2.0) already runs V3 and is the reference
├── Lines: give each transcript line its majority speaker (segment level);
│   word-level timing gains under 1 point because "Them" lines are short
└── Voiceprints: a separate speaker-embedding model over each speaker's solo stretches
    ├── CAM++ named 81–94% of cross-call appearances correctly (nearest match)
    └── no fixed threshold rejected an unknown voice → suggest names, don't auto-assign
```

If 4 speakers is enough for a first version, Sortformer v2.1 runs through
`transcribe_cpp::DiarizeSession` today with no new dependency.

## Synthetic benchmark

Five calls of 2.3–4.6 minutes, with 2–5 of 8 voices and 3–5% overlapped
speech. The voices were made with Qwen3-TTS: VoiceDesign 1.7B created one
reference clip per voice, then Base 1.7B cloned every line. Overlaps come from
backchannels, barge-ins and early starts. Voices repeat across calls, and one
voice appears only once. Each call was then degraded like a real call: 16 kHz,
Opus or AAC at 16–32 kb/s, pink noise at 20–28 dB SNR, optional mains hum, and
up to 9 dB of level difference between speakers. Ground-truth RTTM comes from
each clean line's energy, split at pauses of 0.4 s or more. Because that truth
also counts short pauses inside a turn as speech, every DER below includes a
missed-speech floor of about 9 points.

Scoring: DER with a 0.25 s collar, overlap scored (pyannote.metrics). Voiceprints
come from one CAM++ ONNX extractor run on each found speaker's solo stretches
of 0.5 s or more, matched at cosine ≥ 0.5. "Cross-call" enrols each voice from
the other calls; "clean ref" enrols from one clean 10 s clip. Word and segment
attribution use Parakeet TDT 0.6B v3 word times. Speed is audio seconds per
wall second, not counting model load, measured on a shared M2 Max.

| System | Runtime | DER % | Confusion % | Count exact | x-RT | Peak MB | Cross-call | Clean ref | Word / segment % |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Nemotron-3 | NeMo-Speech.cpp, v3-offline preset | 9.2 | 2.4 | 4/5 | 642 | 145 | 81 | 75 | 97.1 / 96.8 |
| Nemotron-3 | NeMo-Speech.cpp, streaming | 9.1 | 2.4 | 4/5 | 56 | 169 | 94 | 75 | 97.0 / 96.8 |
| Nemotron-3 | FluidAudio CoreML, offline | 9.5 | 2.1 | 4/5 | 393 | 105 | 88 | 69 | 97.6 / 97.1 |
| Nemotron-3 | FluidAudio CoreML, fast32 | 9.8 | 2.7 | 4/5 | 139 | 91 | 81 | 75 | 97.0 / 96.4 |
| Nemotron-3 | NeMo-Speech.cpp, `--offline` full attention | 24.0 | 8.9 | 3/5 | 966 | 146 | 73 | 67 | 79.5 / 80.4 |
| Sortformer 4spk v2 | NeMo-Speech.cpp, streaming | 6.9 | 2.5 | 4/5 | 41 | 181 | 88 | 75 | 97.1 / 96.7 |
| Sortformer 4spk v2 | NeMo-Speech.cpp, offline preset | 13.3 | 8.2 | 4/5 | 80 | 177 | 75 | 69 | 91.8 / 91.3 |
| Sortformer 4spk v2.1 | transcribe.cpp (Hex's pin), default | 11.8 | 8.1 | 4/5 | 180 | 169 | 80 | 87 | 92.4 / 92.0 |
| pyannote community-1 | pyannote.audio 4, MPS | 14.3 | 9.8 | 3/5 | 11 | 6965 | 73 | 73 | 90.1 / 89.5 |
| pyannote community-1 | FluidAudio CoreML, VBx | 18.9 | 9.0 | 3/5 | 111 | 795 | 73 | 67 | 87.9 / 88.3 |
| seg-3.0 + WeSpeaker | sherpa-onnx | 15.5 | 11.0 | 4/5 | 6 | 250 | 81 | 56 | 88.1 / 88.3 |
| seg-3.0 + CAM++ | sherpa-onnx, threshold 0.5 | 63.5 | 57.8 | 0/5 | 12 | 311 | 35 | 29 | 40.0 / 38.9 |
| Silero + CAM++ + AHC | ONNX Runtime, 1.5 s windows | 32.1 | 23.5 | 1/5 | 61 | 209 | 25 | 58 | 70.7 / 71.0 |

Notes:

- Every system got the speaker count right on the 2-, 3- and 4-voice calls
  (pyannote merged two voices on one 3-voice call). On the 5-voice call, every
  system that is supposed to handle 5 or more speakers still merged two voices;
  one of them speaks for only 18 s.
- Nemotron-3's `--offline` full-attention mode is worse than its streaming
  presets and is limited to about 6.6 minutes, so don't use it for meetings.
- Sortformer v2's 6.9% edges out Nemotron-3 on these short calls with 2–5
  speakers, but its 4-speaker cap rules it out for bigger meetings (below).
- sherpa-onnx with CAM++ over-splits at both thresholds tried (0.5 and 0.8).
  Its clustering threshold needs tuning for each embedder.
- The baseline's 1.5 s windows and a single AHC threshold can't resolve quick
  turns.

## Real meetings

Four real meetings of 16–85 minutes, run only on the owner's machines. The
audio, transcripts and results stayed local and are not in this repository.
Findings that don't depend on the content:

- On meetings with 6–7 remote voices, Nemotron-3 and pyannote agree on the
  count. Sortformer v2.1 has to fold the extra voices into 4 slots, and its
  line agreement with Nemotron-3 drops to 71–79%.
- On one call, pyannote and Sortformer v2.1 both found a second voice of about
  1.5–2.5 minutes that Nemotron-3 merged into the main speaker. Treat
  Nemotron-3's speaker count as a suggestion the user can split.
- At 85 minutes, both ggml runtimes (NeMo-Speech.cpp and transcribe.cpp) peak
  at about 2 GB, because memory grows with the length of the buffer. pyannote
  on MPS peaks at 8.7 GB. Hex should feed long meetings in streaming chunks.
- Speed on a shared M4 Pro: Nemotron-3 v3-offline ran at 310–410× realtime,
  Sortformer v2.1 (transcribe.cpp) at about 190×, and pyannote MPS at 22–26×.

## Candidates

| Model | Licence | Download | Max speakers | Voiceprint | From Rust |
| --- | --- | --- | --- | --- | --- |
| Nemotron-3 Diarization | OpenMDW-1.1 | 107 MB q8_0 GGUF; 190 MB per CoreML preset | 8 | No | NeMo-Speech.cpp C ABI (`nemo_speech/diar.h`, Apache-2.0; ships its own ggml dylibs, ~9 MB for diarization) or a V3 port into transcribe.cpp |
| Sortformer 4spk v2 / v2.1 | CC-BY-4.0 / NVIDIA Open Model License | 147 / 139 MB q8_0 GGUF | 4 | No | Already in Hex's pinned transcribe.cpp: `Model::diarize_info`, `DiarizeSession::run(&pcm, &opts)` |
| Multitalker Parakeet | NVIDIA Open Model License | 0.6B ASR with an embedded Sortformer | 4 | No | In transcribe.cpp's catalog. Speaker-tagged ASR is a bigger change than diarizing after the call; not run |
| pyannote community-1 | CC-BY-4.0 weights, MIT code | 32 MB PyTorch; 21 MB CoreML | Any (clustering) | Yes, 256-d WeSpeaker | No Rust path: PyTorch, or FluidAudio's CoreML port from Swift |
| FluidAudio (SDK) | Apache-2.0 | per model | 8 / any | Yes (community-1 pipeline) | A Swift package behind a bridge like `apple_speech.swift`; macOS only |
| Silero + CAM++ + AHC | MIT + Apache-2.0 | 0.6 MB + 29 MB ONNX | Any | Yes, 192-d | `ort` crate (Hex ships no ONNX Runtime) or a ggml port |
| sherpa-onnx seg-3.0 + embedder | Apache-2.0 / MIT | 6 MB + 26–29 MB ONNX | Any | Yes | sherpa-onnx C API, which bundles ONNX Runtime |

Sortformer v2.1 vs Multitalker Parakeet: the multitalker model is a Parakeet
ASR model fed by a Sortformer like v2.1. It writes speaker-tagged words
directly, but has the same 4-speaker limit, and would replace Hex's meeting
transcription instead of adding to it.

FluidAudio was strong: it reached Nemotron-3 accuracy at the lowest memory, and
it runs on the Neural Engine, which keeps it off the GPU dictation uses. It
needs a SwiftPM dependency, though, and only runs on macOS. Since Hex already
ships transcribe.cpp, that's the better home for the model.

## Voiceprints

- Naming by nearest match works. Nemotron-3 segments with CAM++ named 15 of 16
  cross-call appearances. The miss was the voice that appears only once, which
  was confidently matched to someone else.
- On codec audio, CAM++ similarities bunch together: the same voice scores
  0.85–0.94 and the closest other voice 0.81–0.89. No single threshold rejects
  strangers. TTS clones from one model probably make this worse than real
  people would. Calibrate on real enrolments (score normalisation, or a margin
  over the second-best match) before auto-assigning names.
- To stay on ggml, a speaker-verification ECAPA-TDNN fits transcribe.cpp's
  existing ECAPA family (used today for language ID). It hasn't been measured
  yet.

## Reproduce

Scripts are in `research/meeting-speakers/`. They need an Apple Silicon Mac
with `uv` and `ffmpeg`:

```sh
research/meeting-speakers/setup.sh                       # venvs, ONNX models, NeMo-Speech.cpp
research/meeting-speakers/run_synthetic.sh all           # TTS, mix, Parakeet, every diarizer, score
research/meeting-speakers/run_transcribecpp.sh synthetic # Sortformer v2.1 through transcribe.cpp
research/meeting-speakers/run_real.sh                    # private meetings in $SPIKE/private/*.wav
python research/meeting-speakers/build_review.py ...     # private review pages (local only)
```
