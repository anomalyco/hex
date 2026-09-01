# HEX Local Transcription Service and SDK

**Status:** The internal service and direct-child mode are implemented.
`@kitlangton/hex@0.3.0` publishes ready transcribers through Promise and Effect
entrypoints. Local Electron integration has been exercised, but the native helper
package remains private and is not included as a client dependency. An explicit
helper command is still required; signed consumer distribution remains unverified.

**Authority:** This document describes the embedded service protocol and labels
proposed distribution separately. The [SDK README](../../sdk/typescript/README.md)
is the current consumer guide. Delivery scope, evidence, and acceptance gates live in
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
levels and waveform        PCM WAV       resampling and inference
capture UX                 ---------->   final raw transcript
settings and model choice
```

The host's signed app owns the microphone grant. A correctly configured packaged
host prompts under its own identity, not HEX; a development Electron runtime may
appear as Electron. The embedded helper never opens the microphone.

## Decisions

| Decision | Reason |
| --- | --- |
| Host owns microphone capture | Correct TCC identity, intuitive permission prompt, no shared permission broker. |
| Service owns models and inference | Hosts share verified model artifacts while each helper owns its warm runtime. |
| Completed audio goes over localhost | Simple final-only contract; no partial transcripts or bidirectional session protocol. |
| Model preparation is explicit | A transcription call never hides a large network download. |
| Installed-but-cold models may reload automatically | A request for a different model in the same helper replaces its warm runtime. |
| No product duration limit | Hotkey capture and service transcription end explicitly; operational resource exhaustion remains a typed failure. |
| Raw transcripts only | The host owns meaning, rewriting, and insertion. |
| Trusted Node/Electron main process | Ordinary browser code and untrusted renderers must not receive native process authority or service credentials. |
| Host-spawned helper | The host owns the process, endpoint, and lifetime instead of discovering an independently elected service. |
| Shared model artifacts, private warm runtime | Hosts reuse verified downloads without coupling their model choice or process lifecycle. |

## Proposed Embedded Distribution (Not Yet Shipping)

The target SDK will declare signed, notarized helpers as optional
architecture-specific packages and select the current platform package
automatically. Applications will call `create()` without locating an executable.
On explicit host use, the SDK will spawn the included helper directly as a child
without an npm install script, administrator access, login-item approval, Dock icon,
or foreground window. The host must not launch the helper through Launch Services.

Because the embedded service does not capture audio, it does not need microphone
TCC identity. Full `Hex.app` already owns its global-hotkey capture and
transcription pipeline; desktop `connect()` can also use the app's microphone
through its separate capture API.

Verified model artifacts remain shared per user under Application Support.
Each embedded helper has its own warm model and inference queue. Multiple hosts
may reuse the same verified file, but they do not share Metal allocations or
runtime selection. The current cache is filename-based; immutable revision-safe
storage for independently versioned helpers remains planned.

## Embedded Startup and Authentication

The host spawns `hex-service service --embedded` with piped stdin and stdout.
The helper binds `127.0.0.1:0` and writes exactly one JSON line to stdout after
the authenticated API is ready:

```json
{
  "type": "ready",
  "url": "http://127.0.0.1:49731",
  "token": "hex_a1b2c3...",
  "apiVersion": "2",
  "pid": 12345
}
```

The host keeps the child's stdin open as its lifetime lease. EOF requests a
clean shutdown; the helper force-exits after a five-second grace period if
native work does not return. Embedded mode writes no shared discovery file and
acquires no global service lock, so two host applications can run independent
helpers.

The current executable starts lease monitoring after local API initialization,
not at process entry. Startup-failure and descendant-process cleanup remain
native-distribution acceptance gates in the delivery plan.

Every request requires `Authorization: Bearer <token>`. The token is delivered
only over the child's stdout pipe. The server deliberately sends no CORS
headers. A parsed request with a missing or incorrect bearer credential returns
an empty `401` response; malformed HTTP can be rejected before authentication.
The standalone HEX application may still publish owner-only discovery for its
own existing local API. Embedded `create()` uses the child handshake; desktop
`connect()` remains a separate discovery-based path.

## Wire Protocol (API 2)

```text
GET  /health
GET  /capabilities
GET  /models
POST /models/{id}/prepare     SSE preparation progress, then ok or error
POST /transcriptions          audio/wav body, final transcript response
```

`POST /transcriptions` requires a `model` query parameter. Preparation and
transcription accept an optional `language` query parameter, defaulting to `en`.
`GET /models?language=...` uses that language for readiness metadata, not to filter
the returned catalog. Transcription requires `Content-Length`; chunked transfer
encoding is not supported.

`POST /transcriptions` accepts completed PCM WAV audio. The service reads
request bodies incrementally into a bounded buffer, then validates WAV metadata
before allocating decoded samples and running inference. It downmixes and
resamples internally. API 2 accepts integer PCM and 32-bit float WAV at 8 kHz
through 192 kHz with one through eight channels. Uploads are capped at 64 MiB;
source frames and normalized 16 kHz mono Float32 samples are each capped at
16,777,216 (64 MiB of Float32 samples). The frame cap permits about 5.8 minutes at
48 kHz or 17.5 minutes at 16 kHz, before other limits. Headers have a two-second
deadline and uploads a thirty-second deadline. These limits are not a total
process-memory cap.

The HTTP transcription path does not save uploaded WAVs. The current executable
still initializes shared process logging and inherits native diagnostic flags
such as `TRANSCRIBE_DUMP_DIR`, which can write audio-derived tensors. Do not treat
this as an audited no-persistence boundary; embedded diagnostic isolation remains
part of the distribution plan.

There is no separate duration cutoff. One upload owns the audio-memory admission
slot until its queued or active inference releases the clip. Absolute header
and upload deadlines, byte and normalized-sample budgets, and typed resource
refusal bound admission from defective local clients. Detected client cancellation
or service shutdown suppresses results, but native inference may finish internally.
The ready SDK transcriber instead closes its owned helper on in-flight cancellation
and waits for cleanup before rejecting; low-level request abortion alone does not
prove native work stopped.

### Health

```typescript
export interface Health {
  version: string;
  apiVersion: "2";
}

export interface Capabilities {
  audioFormats: readonly ["audio/wav"];
  partialTranscripts: false;
  serviceCapture: false;
  developerControl: false;
}
```

These capability values describe the embedded helper. The desktop local API can
advertise capture and debug-only developer control. The SDK exposes
`serviceCapture` as a boolean and omits the native-only `developerControl` field.

### Models

```typescript
export type ModelId =
  | "parakeet_unified_en"
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
  supportsLanguageDetection: boolean;
}

export type ModelProgress =
  | { type: "downloading"; downloadedBytes: number; totalBytes: number }
  | { type: "verifying" }
  | { type: "loading" };
```

`ModelId` is the SDK's closed set of identifiers, not an availability guarantee.
The native catalog currently excludes `apple_speech`, and preparation and
transcription reject it. The retained Apple Speech implementation and
system-managed metadata do not make it a supported service model.

For the available GGUF models, preparation means download if needed, checksum
verification (or reuse of a matching verification receipt), strict-Metal load, and
prewarm. `managed` means system-managed, not downloaded by HEX, so these models
report `managed: false`. Preparation activates the helper's warm model but does
not change full `Hex.app` settings. The service serializes preparation, publishes
only verified replacement files, and preserves the previous warm runtime if
candidate loading fails. A verified download can remain installed after a load
failure; artifact publication and runtime activation are separate steps.

### Ready Transcriber

SDK `0.3.0` adds a model-bound operation to both entrypoints:

```ts
import { create } from "@kitlangton/hex"

const transcriber = await create({
  command: ["/path/to/hex-service", "service", "--embedded"],
  model: "parakeet_unified_en",
  language: "en",
  onProgress,
})
try {
  const result = await transcriber.transcribe(wav, { signal })
  input.insert(result.transcript)
} finally {
  await transcriber.close()
}
```

Creation resolves after preparation. Reuse the transcriber across recordings;
close when the host no longer needs it. Cancelling an in-flight request closes the
helper, so subsequent work needs a new transcriber. An already-aborted request
does not close a healthy transcriber. Failed cleanup is reported, not hidden.
Effect provides the same ready shape with scope-owned cleanup instead of `close()`.

Progress callbacks describe model setup, not inference percentage. Transcription
returns only final text; partial transcripts and numerical transcription progress
are not exposed by this protocol.

### Low-Level TypeScript Surface

The [SDK README](../../sdk/typescript/README.md) documents host creation and
client usage. The [public types](../../sdk/typescript/src/types.ts) define the
complete client, audio, progress, and result contracts; this spec owns the wire
protocol and lifecycle guarantees rather than a second copy of those interfaces.

The common host flow is explicit about preparation:

```typescript
await client.models.prepare("parakeet_unified_en", { onProgress });

// The host records, meters, and encodes the clip.
const result = await client.transcribe({
  audio: { data: wav, contentType: "audio/wav" },
  model: "parakeet_unified_en",
  signal,
});

input.insert(result.transcript);
```

The `@kitlangton/hex/effect` entrypoint provides scoped helper acquisition,
schema-backed typed errors, Effect operations, an Effect `Stream` for model
preparation progress, and a `Layer` for dependency injection. Closing the scope
closes the host lease and bounds exact-child termination.

`transcribe()` never downloads a model. It returns `model-not-ready` when the
artifact has not been prepared. It may transparently reload an installed but
cold model because model residency is transient.

## First Embedded Release Non-Goals

- No service-owned microphone capture, levels, hotkeys, paste, or clipboard.
- No partial transcripts.
- No compressed audio codecs.
- No OpenCode rewrite profiles.
- No non-localhost access.
- No ordinary browser bootstrap.
- No cross-host warm-runtime sharing or service election.
- No long-form segmentation, speaker turns, resumability, or search until a
  real consumer requires them.
