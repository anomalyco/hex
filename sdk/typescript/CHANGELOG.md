# @kitlangton/hex

## 0.3.0

### Minor Changes

- c5bffe3: Add a model-bound `create({ model, language, onProgress })` API that returns a
  ready transcriber for WAV bytes. The Promise transcriber provides explicit,
  idempotent cleanup; the Effect transcriber is scope-owned. Failed creation and
  in-flight transcription cancellation wait for helper cleanup. Existing low-level
  `create()` and desktop `connect()` remain available.

  Effect scope cleanup failures now surface as defects instead of being logged and
  discarded. The low-level and model-bound option types are distinct so shared
  configuration cannot accidentally change the returned resource's shape.

  Startup cleanup failures are surfaced instead of hiding a potentially live child.
  Cancellation also takes precedence over a buffered result that has not settled.

  Embedded use still requires an explicit native helper command until bundled
  platform artifacts are published.

## 0.2.2

### Patch Changes

- e163753: Support Effect 4.0.0-rc.112 and newer in the Effect client, using the current tagged-error API.

## 0.2.1

### Patch Changes

- c5ba7d4: Stop dictation heartbeat timers after terminal capture errors while preserving retries for transient failures.

## 0.2.0

### Minor Changes

- Require local API v2 for owner-scoped dictation capture, rejecting legacy HEX
  apps before a recording can start and become orphaned.
