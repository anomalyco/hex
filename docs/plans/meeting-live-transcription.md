# Live meeting transcription: Apple SpeechTranscriber vs Moonshine

Status: recommendation, not implemented. Measured 2026-10-16 on the Mac Studio
(M5 Max, macOS 27.0.1).

## Question

Should the live meeting draft use Apple's `SpeechAnalyzer` with
`SpeechTranscriber` (macOS 26+) instead of Moonshine, keeping Parakeet rolling
and final transcription as the quality pass?

## Measurements

Kit's 2026-10-16 call (18.7 min system, 18.3 min microphone) and a synthetic
5.7 min meeting (`say`, three voices, known text, 1.2 s gaps). Apple numbers
come from a scratch Swift harness that streams 100 ms Int16 buffers into one
`SpeechAnalyzer` per track with volatile results and audio time ranges.
Moonshine numbers come from the real live worker
(`meeting_echo_replay_tests::meeting_live_cpu`). CPU for Apple includes the
`localspeechrecognition` XPC service, where the work happens; the GPU and
Neural Engine are not attributable without `powermetrics`.

```ts
CPU per second of audio, both tracks          // percent of one core
├── Apple SpeechTranscriber      ~2%    (real call at 30x: 15.7 CPU s for 1123 s
│                                        of system audio; synthetic at 1x: 7.1 s / 343 s)
├── Apple DictationTranscriber   ~8%    (90 CPU s for 1123 s; could not keep up at 30x)
├── Moonshine, spinning ORT pools  113% real call, 67% synthetic   (before this change)
└── Moonshine, single-threaded      31% real call, 17% synthetic   (after this change)

Latency after the audio, synthetic, real time
├── Apple volatile text           at once (the range ends where audio was fed)
├── Apple final segment           p50 2.3 s, p95 3.8 s after the segment ends
├── Moonshine first text          p50 0.2 s, p95 0.3 s (1 s update cadence)
└── Moonshine completed line      p50 1.0 s, p95 1.5 s after the line ends

Word error rate vs the known text, synthetic (TTS speech is easy for all)
├── Moonshine live draft   1.7%
└── Apple SpeechTranscriber 3.0%

Dictation (Parakeet, 8 s clip, 20 runs) while each runs, p50 / p95
├── Alone                         74 / 257 ms (first-run outliers)
├── Two Apple analyzers           70 / 98 ms
├── Moonshine, single-threaded    70 / 78 ms
└── Moonshine, spinning pools     70 / 108 ms
```

Quality on the real call was not compared: the recording was lost with the
Studio's temporary directory before that run.

## Recommendation

Adopt SpeechTranscriber for the live draft as a follow-up, keeping Moonshine
as the fallback, but not in the echo fix:

- It costs roughly a tenth of single-threaded Moonshine's CPU and does not
  need the opt-in command model, which today blocks the live draft entirely.
- Its finals arrive about 1.3 s later than Moonshine's completed lines; its
  volatile results cover that gap in the pane.
- Quality is comparable for a draft that Parakeet replaces within a chunk.
- Dictation showed no contention on the Studio with either engine.

Moonshine's real cost was ONNX Runtime's spinning thread pools, now disabled
(`moonshine::run_sessions_single_threaded`), so the CPU emergency no longer
depends on switching engines.

## Scope of the switch

- A streaming Swift bridge: today's `apple_speech.swift` only transcribes whole
  buffers. It needs a session per track that accepts audio, yields volatile and
  final results with time ranges, and finishes on Stop. Buffers must be 16 kHz
  Int16; float buffers trap inside `AnalyzerInput`.
- Map results onto `meeting_live::Event`: one line id per final range, volatile
  text as `Updated`, finals as `Completed`, so `live_view`, echo judgment and
  rolling coverage stay unchanged.
- Asset reservation through the existing `AppleSpeechAssetReservations`, an
  availability check per locale, and the Moonshine path when SpeechTranscriber
  is unavailable (macOS before 26, unsupported language, assets missing).
- Re-run `meeting_echo_replay` with the new draft on a recorded speaker-mode
  call and check that the draft-only layer is no worse than Moonshine's.
