---
"@kitlangton/hex": minor
---

Add a model-bound `create({ model, language, onProgress })` API that returns a
ready transcriber for WAV bytes. The Promise transcriber provides explicit,
idempotent cleanup; the Effect transcriber is scope-owned. Failed creation and
in-flight transcription cancellation wait for helper cleanup. Existing low-level
`create()` and desktop `connect()` remain available.

Effect scope cleanup failures now surface as defects instead of being logged and
discarded. The low-level and model-bound option types are distinct so shared
configuration cannot accidentally change the returned resource's shape.

Embedded use still requires an explicit native helper command until bundled
platform artifacts are published.
