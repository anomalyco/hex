# HEX Local Transcription Service and SDK - Implementation Plan

**Status:** Active. Service phases 1-3, the signed macOS artifact proof, and the
direct-child embedded runtime tracer are implemented. Promise and Effect
TypeScript wrappers now pass fake-helper tests. Signed helper packaging, an
Electron bridge, release validation, and a real consumer remain.

Companion to
[`../specs/local-transcription-service.md`](../specs/local-transcription-service.md).
The host application owns microphone capture and levels. HEX Service owns model
preparation, warm runtime state, and inference.

## Invariants

- The service binds only loopback and authenticates every route before dispatch.
- Request parsing, uploads, preparation, and inference queues remain bounded.
- The service never requests microphone permission or captures audio.
- Submitted audio is never persisted.
- A failed model preparation preserves the current artifact, warm runtime, and
  full HEX.app selection.
- `transcribe()` never initiates a model download.
- There is no product duration limit. Operational resource refusal is typed and
  based on actual process safety, not an arbitrary number of seconds.
- Full HEX hotkey capture has no automatic duration limit; release, explicit
  stop, or Escape ends capture.

## Phase 1 - Service Skeleton (Implemented)

- Bounded synchronous HTTP server on `127.0.0.1:0`.
- Fixed worker pool, bounded connection queue, read/write deadlines, and
  request header/path limits.
- 256-bit bearer token and atomic owner-only discovery file.
- Stale discovery recovery and generation-safe cleanup.
- Auth-first `GET /health` and `GET /capabilities`.
- Coalesced authentication-failure observations.
- Shared synchronized NDJSON event writer.
- App and headless service lifecycle integration.

## Phase 2 - Model Surface (Implemented)

- Stable explicit model wire IDs.
- `GET /models` with installed, verified, managed, size, and language
  metadata.
- `POST /models/{id}/prepare` with SSE download, verification, and loading
  progress.
- One bounded preparation operation; duplicate attempts receive `409`.
- Verification receipts invalidate when the artifact changes.
- Existing final artifacts remain untouched until replacements verify.
- Cancellation during verification never deletes the current model.
- Runtime load/prewarm validates a candidate without selecting it.

## Phase 3 - Host-Audio Transcription (Implemented)

- Extend the bounded HTTP parser with incrementally read request bodies and an
  explicit maximum safe byte budget. Require `Content-Length` in v1; reject
  chunked transfer until it has a bounded implementation.
- Add `POST /transcriptions` accepting `Content-Type: audio/wav`.
- Validate WAV structure, PCM sample type, channel count, sample rate, and
  declared frames before inference.
- Downmix and resample in the service using the existing transcription path.
- Add a service-process model runtime manager that keeps one model warm,
  serializes switches, and preserves the old warm model on failure.
- Admit one upload/inference clip at a time so encoded, decoded, queued, and
  active audio share one aggregate memory bound.
- Return `{ transcript, durationMs }` with no processing, paste, clipboard, or
  last-result side effects.
- Map request abort to queued-job cancellation and result suppression. Native
  inference may finish internally when it cannot be interrupted.
- Return typed errors including `model-not-ready`, `invalid-audio`,
  `unsupported-audio`, `queue-full`, `resource-exhausted`,
  `transcription-failed`, and `service-gone`.
- Test audio body bounds, hostile metadata, malformed/truncated WAV, mono/stereo
  conversion, sample-rate conversion, arbitrary duration above 60 seconds,
  queue pressure, cancellation, model-switch failure, and raw transcript output.

## Phase 4 - Embedded Helper Bootstrap (In Progress)

- Build the minimal headless service artifact without GPUI, Sparkle, Moonshine,
  microphone entitlement, or App Sandbox.
- Developer ID sign, notarize, staple, and produce a content-addressed archive.
- Ship the helper as inert data in optional architecture-specific npm packages;
  use no `postinstall` script.
- On explicit host use, verify digest, Team ID, signing identity, and
  notarization, then spawn the helper directly from the host process. Do not
  launch it through Launch Services.
- Read the one-line authenticated endpoint handshake from stdout, keep stdin
  open as the host-lifetime lease, and terminate the exact child on shutdown.
- Keep verified model artifacts in one shared per-user store. Selection, warm
  runtime, inference queue, endpoint, and lifecycle remain per host.
- Define helper replacement and cleanup ownership without introducing service
  election or a detached daemon.

The signed artifact proof still builds a 19 MB notarized arm64
`HEX Service.app` without microphone entitlement. The embedded runtime tracer
now directly spawns the executable, returns its endpoint over stdout, publishes
no discovery file, and exits when the host closes stdin. The packaging proof
must next become a directly spawned helper artifact rather than a
LaunchServices-owned application.

`@hex-ai/service-darwin-arm64` now defines the first platform package and
`scripts/prepare-typescript-sdk.sh` inserts the signed/notarized helper from the
existing release proof. `@hex-ai/client` installs it optionally and resolves it
automatically; the packages remain private until artifact validation and the
first consumer are complete.

## Phase 5 - TypeScript SDK (In Progress)

- `sdk/typescript/` now uses Bun, TypeScript, and Vitest. It remains private;
  add Changesets when the package becomes publishable.
- The Promise entrypoint implements direct spawning, bounded handshake parsing,
  authentication, health, capabilities, model listing/preparation, SSE parsing,
  audio upload, cancellation, and exact-child shutdown.
- The `/effect` entrypoint implements scoped acquisition, schema-backed tagged
  errors, Effect operations, model-progress Streams, and a service Layer using
  the local Effect v4 API as the source of truth.
- The client now selects and resolves an automatically installed
  architecture-specific helper package; an explicit command remains only as an
  advanced test override.
- Finish release-time helper identity verification and protocol version
  negotiation once the signed package artifact is prepared.
- Provide a narrow Electron preload/main bridge example. The host renderer owns
  `getUserMedia`, recording, levels, and WAV encoding.
- Fake-helper tests cover successful Promise and Effect flows, progress ordering,
  scope and idempotent shutdown, invalid handshakes, and early helper exit. Add
  bounded upload failures, interruption during preparation and inference, and
  packaged-helper concurrency coverage.
- Inspect the packed npm artifacts and verify from a clean Electron consumer
  before publication.

## Phase 6 - First Consumer and Hardening

- Port one real consumer, preferably OpenCode Desktop.
- Add the host application's microphone usage description and audio-input
  entitlement; verify the prompt names OpenCode, never HEX Service.
- Measure capture-to-result latency, model cold/warm behavior, IPC copy cost,
  memory use, and long-audio behavior.
- Add long-form chunking or another API only when real audio demonstrates a
  semantic need for progress, segmentation, speaker turns, or resumability.
- Add Linux x86_64 parity using the same protocol and host-owned capture.
- Reserve the final npm name, add README integration guidance, and publish only
  after the real consumer succeeds.

## Validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
./scripts/build-service-app.sh
./scripts/prepare-service-app.sh
./scripts/smoke-service-app.sh
./scripts/smoke-embedded-service.sh
cd sdk/typescript && bun run check && bun run test && bun run build
```
