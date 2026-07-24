# HEX TypeScript Client

> **Status:** Private API preview. `@hex-ai/client` is not published to npm and
> the repository does not contain a runnable native helper. The examples below
> require an internally prepared Apple silicon macOS helper artifact.

Host the native HEX transcription helper as a direct child process. The host
application owns microphone capture and settings; HEX owns local model
preparation and transcription. Model artifacts remain shared per user while the
helper process and warm model belong to the host.

On Apple Silicon, `@hex-ai/service-darwin-arm64` is installed as an optional
dependency and selected automatically. The release workflow prepares its
signed/notarized executable; normal consumers never pass a command or path.

This document records the intended consumer API. Do not depend on package names,
versions, or distribution behavior until the first real consumer and clean
package installation have been validated.

## Promise API

```ts
import { create } from "@hex-ai/client"

const host = await create()

try {
  const models = await host.client.models.list()
  await host.client.models.prepare("parakeet_v2", {
    language: "en",
    onProgress: console.log,
  })

  const result = await host.client.transcribe({
    audio: { data: wav, contentType: "audio/wav" },
    model: "parakeet_v2",
    language: "en",
  })
  console.log(result.transcript)
} finally {
  await host.close()
}
```

`close()` is idempotent. It closes the helper's stdin lease, waits for graceful
shutdown, then terminates the exact child if it does not exit within the bounded
deadline.

## Effect API

The Effect entrypoint exposes scoped acquisition, typed schema-backed errors,
Effect operations, and a progress `Stream`:

```ts
import { Effect, Stream } from "effect"
import * as Hex from "@hex-ai/client/effect"

const program = Effect.scoped(Effect.gen(function* () {
  const host = yield* Hex.create()

  yield* host.client.models.prepare("parakeet_v2", {
    language: "en",
  }).pipe(Stream.runForEach((progress) => Effect.logInfo(progress)))

  return yield* host.client.transcribe({
    audio: { data: wav, contentType: "audio/wav" },
    model: "parakeet_v2",
    language: "en",
  })
}))

const result = await Effect.runPromise(program)
```

For dependency injection, `Hex.layer(options)` provides `Hex.Service` and owns
the helper for the layer's scope.

## Permissions

The helper needs zero macOS TCC permissions. The host application captures
microphone audio itself and sends encoded WAV, so the microphone prompt,
`NSMicrophoneUsageDescription`, orange-dot indicator, and Privacy & Security
entry all belong to the host's own bundle and signature. There is no second
permission identity to sign, prompt for, or keep in sync.

This is a deliberate decomposition: HEX owns model preparation and
transcription; the host owns capture and consent. A future opt-in mode where
the helper captures the microphone via TCC responsibility-chain inheritance
is tracked in https://github.com/anomalyco/hex/issues/18 and is intentionally
deferred until a real non-web consumer needs it.

## Host Boundary

- Use from Node or Electron main/preload code, not an untrusted renderer.
- The client automatically selects and locates its installed native helper
  package. Applications do not provide an executable path.
- Capture and encode audio in the host application.
- Persist model selection in the host application.
- Keep the helper executable signed and spawn it directly. Do not use
  LaunchServices, `open`, or `NSWorkspace`.
- Never expose the startup bearer token to renderer or web content.
