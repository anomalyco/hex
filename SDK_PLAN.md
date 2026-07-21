# HEX Local Transcription Service and SDK - Implementation Plan

Companion to `SDK_SPEC.md`. The host application owns microphone capture and
levels. HEX Service owns model preparation, warm runtime state, and inference.

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
- `GET /models` with selected, installed, verified, managed, size, and language
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

## Phase 4 - Frictionless Service Bootstrap

- Build the minimal headless service artifact without GPUI, Sparkle, Moonshine,
  microphone entitlement, or App Sandbox.
- Developer ID sign, notarize, staple, and produce a content-addressed archive.
- Ship the archive as inert data in optional architecture-specific npm
  packages; use no `postinstall` script.
- On explicit host use, acquire a bootstrap lock, verify digest, Team ID,
  signing identifier, signature, and notarization, then atomically activate a
  version in Application Support.
- Demand-launch and reuse a compatible winner with OpenCode-style election.
- Define compatible-version replacement, rollback, retained versions, and
  cleanup ownership.
- Keep full HEX.app and the service from competing for model runtime ownership;
  migrate full HEX inference to the service only after the external consumer
  works.

The current proof builds a 19 MB signed/notarized arm64 `HEX Service.app`
without microphone entitlement, installs it to versioned Application Support
storage, registers it with Launch Services, verifies authenticated discovery
and model listing, and shuts down cleanly.

## Phase 5 - TypeScript SDK

- Create `sdk/typescript/` using Bun and Vitest; add Changesets when the package
  becomes publishable.
- Implement Node/Electron-main discovery, authentication, version negotiation,
  launch/bootstrap, health, capabilities, model listing/preparation, SSE parsing,
  audio upload, cancellation, and typed errors.
- Provide a narrow Electron preload/main bridge example. The host renderer owns
  `getUserMedia`, recording, levels, and WAV encoding.
- Test every connect result, model progress ordering, service disappearance,
  bounded upload failures, abort, and a clean fake-service consumer flow.
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
```
