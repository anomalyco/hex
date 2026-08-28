# HEX TypeScript Client

`@kitlangton/hex` connects TypeScript applications to the locally installed HEX
desktop app. HEX owns its warm microphone and local transcription engine; the
client controls capture and receives transcripts, levels, and optional PCM.

Host the native HEX transcription helper as a direct child process. The host
application owns microphone capture and settings; HEX owns local model
preparation and transcription. Model artifacts remain shared per user while the
helper process and warm model belong to the host.

Alternatively, connect to the running HEX desktop app to use its already-warm
native microphone and recognition engine:

```ts
import { connect } from "@kitlangton/hex"

const hex = await connect()
const recording = await hex.dictation.start({ source: "tp7" })

void (async () => {
  for await (const { rmsDb, peakDb } of recording.levels) {
    console.log({ rmsDb, peakDb })
  }
})()

// Optional: subscribing activates a bounded best-effort mono Float32 PCM tap.
void (async () => {
  for await (const samples of recording.audio) {
    consumePcm(samples, recording.sampleRate)
  }
})()

const { transcript, durationMs } = await recording.finish()
```

Call `recording.cancel()` to discard instead. Desktop capture returns the raw
local transcript and never pastes into the focused application. Check
`capabilities().serviceCapture`: it is true only for a running desktop endpoint,
and false for the direct-child transcription helper.

The handle maintains a short server lease while capture is active. If its
process exits, HEX cancels the abandoned capture after lease expiry. Every
capture operation and observation stream is scoped by the handle's unguessable
`ownerToken`; callers normally do not need to use it directly.

Embedded hosting is also available for callers that provide an explicit native
service command. A bundled native helper is not published yet.

## Promise API

```ts
import { create } from "@kitlangton/hex"

const host = await create({ command: ["/path/to/hex-service", "service", "--embedded"] })

try {
  const models = await host.client.models.list()
  await host.client.models.prepare("parakeet_unified_en", {
    language: "en",
    onProgress: console.log,
  })

  const result = await host.client.transcribe({
    audio: { data: wav, contentType: "audio/wav" },
    model: "parakeet_unified_en",
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

Install Effect 4.0.0-rc.112 or newer within v4 to use `@kitlangton/hex/effect`.
Effect remains optional for the Promise API.

The Effect entrypoint exposes scoped acquisition, typed schema-backed errors,
Effect operations, and a progress `Stream`:

```ts
import { Effect, Stream } from "effect"
import * as Hex from "@kitlangton/hex/effect"

const program = Effect.scoped(Effect.gen(function* () {
  const host = yield* Hex.create({ command: ["/path/to/hex-service", "service", "--embedded"] })

  yield* host.client.models.prepare("parakeet_unified_en", {
    language: "en",
  }).pipe(Stream.runForEach((progress) => Effect.logInfo(progress)))

  return yield* host.client.transcribe({
    audio: { data: wav, contentType: "audio/wav" },
    model: "parakeet_unified_en",
    language: "en",
  })
}))

const result = await Effect.runPromise(program)
```

For dependency injection, `Hex.layer(options)` provides `Hex.Service` and owns
the helper for the layer's scope.

## Permissions

The direct-child helper needs zero macOS TCC permissions. The host application captures
microphone audio itself and sends encoded WAV, so the microphone prompt,
`NSMicrophoneUsageDescription`, orange-dot indicator, and Privacy & Security
entry all belong to the host's own bundle and signature. There is no second
permission identity to sign, prompt for, or keep in sync.

This is a deliberate decomposition: direct-child hosts own capture and consent,
while clients connected to the desktop app reuse capture owned by HEX's signed
desktop process. Neither mode opens a second microphone stream.

## Host Boundary

- Use from Node or Electron main/preload code, not an untrusted renderer.
- The client automatically selects and locates its installed native helper
  package. Applications do not provide an executable path.
- Capture and encode audio in the host application.
- Persist model selection in the host application.
- Keep the helper executable signed and spawn it directly. Do not use
  LaunchServices, `open`, or `NSWorkspace`.
- Never expose the startup bearer token to renderer or web content.
