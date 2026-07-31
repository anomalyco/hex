# HEX Local Transcription Service and SDK

**Status:** The internal service, direct-child mode, and Promise and Effect
wrappers are implemented. The npm packages are private previews; signed helper
packaging, clean-consumer validation, and a real consumer remain.

**Authority:** This document defines implemented internal service behavior and
the target private SDK contract. It does not claim a published package.
Implementation sequencing lives in
[`../plans/typescript-sdk.md`](../plans/typescript-sdk.md).

## Product Boundary

HEX Service owns local transcription models and inference. Host applications
own recording.

```text
Host application                         HEX Service
----------------                         -----------
microphone permission                    service discovery and authentication
device selection                         model download and verification
capture start/stop                       strict-Metal load and prewarm
levels and waveform        WAV/PCM       resampling and inference
capture UX                 ---------->   final raw transcript
settings and model choice
```

This gives macOS permission to the application the user intentionally used. An
OpenCode Desktop microphone button prompts for OpenCode microphone access, not
HEX access. HEX Service has no microphone entitlement and never captures audio.

## Decisions

| Decision | Reason |
| --- | --- |
| Host owns microphone capture | Correct TCC identity, intuitive permission prompt, no shared permission broker. |
| Service owns models and inference | Hosts share verified model artifacts while each helper owns its warm runtime. |
| Completed audio goes over localhost | Simple final-only contract; no partial transcripts or bidirectional session protocol. |
| Model preparation is explicit | A transcription call never hides a large network download. |
| Installed-but-cold models may reload automatically | Warmth is transient and can change because of another client or memory pressure. |
| No product duration limit | Hotkey capture and service transcription end explicitly; operational resource exhaustion remains a typed failure. |
| Raw transcripts only | The host owns meaning, rewriting, and insertion. |
| Node/Electron main or preload only | Browsers and renderers cannot read owner-only discovery state or bootstrap a native process. |
| Host-spawned helper | The host owns the process, endpoint, and lifetime instead of discovering an independently elected service. |
| Shared model artifacts, private warm runtime | Hosts reuse verified downloads without coupling their model choice or process lifecycle. |

## Service Distribution

The SDK declares signed, notarized helpers as optional architecture-specific
packages and selects the current platform package automatically. Applications
call `create()` without locating an executable. On explicit host use, the SDK
spawns the included helper directly as a child without an npm install script,
administrator access, login-item approval, Dock icon, or foreground window. The
host must not launch the helper through Launch Services.

Because the service does not capture audio, it does not need microphone TCC
identity. Full `Hex.app` remains responsible for its own global-hotkey capture and
can submit completed audio to its own transcription runtime later.

Verified model artifacts remain shared per user under Application Support.
Each embedded helper has its own warm model and inference queue. Multiple hosts
may read the same immutable artifact, but they do not share Metal allocations or
runtime selection.

## Embedded Startup and Authentication

The host spawns `hex-service service --embedded` with piped stdin and stdout.
The helper binds `127.0.0.1:0` and writes exactly one JSON line to stdout after
the authenticated API is ready:

```json
{
  "type": "ready",
  "url": "http://127.0.0.1:49731",
  "token": "hex_a1b2c3...",
  "apiVersion": "1",
  "pid": 12345
}
```

The host keeps the child's stdin open as its lifetime lease. EOF requests a
clean shutdown; the helper force-exits after a five-second grace period if
native work does not return. Embedded mode writes no shared discovery file and
acquires no global service lock, so two host applications can run independent
helpers.

Every request requires `Authorization: Bearer <token>`. The token is delivered
only over the child's stdout pipe. The server deliberately sends no CORS
headers. A malformed or incorrect credential returns an empty `401` response.
The standalone HEX application may still publish owner-only discovery for its
own existing local API; SDK hosts do not use it.

## Wire Protocol V1

```text
GET  /health
GET  /capabilities
GET  /models
POST /models/{id}/prepare     SSE preparation progress, then ok or error
POST /transcriptions          audio/wav body, final transcript response
```

`POST /transcriptions` accepts completed PCM WAV audio. The service reads
request bodies incrementally, validates the declared format and resource use
before allocation and inference, downmixes and resamples internally, and does
not persist audio. V1 accepts 8 kHz through 192 kHz audio with one through eight
channels and a 64 MiB upload and normalized-audio memory budget.

There is no public duration limit. One upload owns the audio-memory admission
slot until its queued or active inference releases the clip. Absolute header
and upload deadlines, byte and normalized-sample budgets, and typed resource
refusal prevent a defective local client from exhausting the process. A socket
abort or service shutdown cancels queued work and suppresses a result; native
inference may finish internally when it cannot be interrupted safely.

### Health

```typescript
export interface Health {
  version: string;
  apiVersion: "1";
}

export interface Capabilities {
  audioFormats: readonly ["audio/wav"];
  partialTranscripts: false;
  serviceCapture: false;
}
```

### Models

```typescript
export type ModelId =
  | "parakeet_v2"
  | "parakeet_v3"
  | "whisper_large_v3_turbo"
  | "qwen3_asr06_b"
  | "sense_voice_small"
  | "cohere_transcribe"
  | "apple_speech";

export interface ModelInfo {
  id: ModelId;
  name: string;
  installed: boolean;
  verified: boolean;
  managed: boolean;
  downloadBytes: number | null;
  languages: readonly string[];
}

export type ModelProgress =
  | { type: "downloading"; downloadedBytes: number; totalBytes: number }
  | { type: "verifying" }
  | { type: "loading" };
```

For managed GGUF models, preparation means download, checksum verification,
strict-Metal load, and prewarm. System-managed runtimes such as Apple Speech
perform their platform-specific availability and readiness checks instead. A
preparation does not select the model or change full `Hex.app` settings. The
service serializes preparation and preserves the existing artifact and warm
runtime when a replacement fails.

### TypeScript Surface

```typescript
export function create(options?: CreateOptions): Promise<HexHost>;

export interface HexHost {
  pid: number;
  client: HexClient;
  close(): Promise<void>;
}

export interface HexClient {
  health(): Promise<Health>;
  capabilities(): Promise<Capabilities>;
  models: {
    list(options?: { language?: string; signal?: AbortSignal }): Promise<ModelInfo[]>;
    prepare(
      id: ModelId,
      options?: {
        language?: string;
        onProgress?: (progress: ModelProgress) => void;
        signal?: AbortSignal;
      },
    ): Promise<void>;
  };
  transcribe(request: TranscriptionRequest): Promise<TranscriptionResult>;
}

export interface AudioClip {
  /** PCM WAV bytes. */
  data: ArrayBuffer | Uint8Array;
  contentType: "audio/wav";
}

export interface TranscriptionRequest {
  audio: AudioClip;
  model: ModelId;
  language?: string;
  signal?: AbortSignal;
}

export interface TranscriptionResult {
  transcript: string;
  durationMs: number;
}
```

The common host flow is explicit about preparation:

```typescript
await client.models.prepare("parakeet_v2", { onProgress });

// The host records, meters, and encodes the clip.
const result = await client.transcribe({
  audio: { data: wav, contentType: "audio/wav" },
  model: "parakeet_v2",
  signal,
});

input.insert(result.transcript);
```

The `@hex-ai/client/effect` entrypoint provides scoped helper acquisition,
schema-backed typed errors, Effect operations, an Effect `Stream` for model
preparation progress, and a `Layer` for dependency injection. Closing the scope
closes the host lease and bounds exact-child termination.

`transcribe()` never downloads a model. It returns `model-not-ready` when the
artifact has not been prepared. It may transparently reload an installed but
cold model because model residency is transient.

## Non-Goals V1

- No service-owned microphone capture, levels, hotkeys, paste, or clipboard.
- No partial transcripts.
- No compressed audio codecs.
- No OpenCode rewrite profiles.
- No non-localhost access.
- No ordinary browser bootstrap.
- No cross-host warm-runtime sharing or service election.
- No long-form segmentation, speaker turns, resumability, or search until a
  real consumer requires them.
