# HEX TypeScript Client

Host the native HEX transcription helper as a direct child process. The host
application owns microphone capture and settings; HEX owns local model
preparation and transcription. Model artifacts remain shared per user while the
helper process and warm model belong to the host.

The package is private while helper packaging and the first real Electron
consumer are being validated.

On Apple Silicon, `@hex-ai/service-darwin-arm64` is installed as an optional
dependency and selected automatically. The release workflow prepares its
signed/notarized executable; normal consumers never pass a command or path.

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

## Host Boundary

- Use from Node or Electron main/preload code, not an untrusted renderer.
- The client automatically selects and locates its installed native helper
  package. Applications do not provide an executable path.
- Capture and encode audio in the host application.
- Persist model selection in the host application.
- Keep the helper executable signed and spawn it directly. Do not use
  LaunchServices, `open`, or `NSWorkspace`.
- Never expose the startup bearer token to renderer or web content.
