# HEX TypeScript Client

`@kitlangton/hex` provides local transcription through a native HEX helper, with
Promise and optional Effect APIs. The host application owns recording, microphone
permission, settings, and what happens to the resulting text. HEX owns model
preparation and inference.

**Distribution status:** a bundled native helper is not published yet. Embedded
consumers must currently supply an explicit native command. The examples below
use the HEX executable's `service --embedded` mode; users do not need to launch
the HEX desktop app. This package alone is not yet a turnkey native distribution.

## Promise API

Pass a model to `create()` to get a ready-to-use transcriber:

```ts
import * as Hex from "@kitlangton/hex"

const transcriber = await Hex.create({
  command: ["/path/to/hex-service", "service", "--embedded"],
  model: "parakeet_unified_en",
  language: "en",
  onProgress: console.log,
})

try {
  // WAV is an ArrayBuffer or Uint8Array supplied by your recording code.
  const result = await transcriber.transcribe(wav, {
    signal: abortController.signal,
  })
  console.log(result.transcript, result.durationMs)
} finally {
  await transcriber.close()
}
```

`create({ model })` starts the helper and waits for download (if necessary),
verification, and model loading. Progress reports `downloading`, `verifying`, and
`loading`. Model and language are captured when creation starts; language defaults
to `en`, independently of desktop settings. A failed or cancelled creation closes
the helper before rejecting. The optional creation `signal` covers startup and
preparation, not the returned transcriber's entire lifetime.

Keep the transcriber alive across recordings to reuse its loaded model. It exposes
the bound `model`, `language`, and diagnostic `pid`; audio requests need only WAV
bytes. `transcribe()` never initiates a download. Inference remains bounded by the
native service's admission limits; simultaneous requests can receive a busy error
instead of entering an unbounded client queue.

### Cancellation and ownership

`close()` is idempotent. It closes the helper's stdin lease, waits for graceful
shutdown, then terminates the exact child if it does not exit within the bounded
deadline. Close when the feature or application shuts down, not after every clip.
No model files are deleted.

Cancelling an **in-flight** `transcribe()` closes this transcriber's helper before
rejecting. This is intentional: aborting HTTP alone cannot interrupt native
inference. All other work on that transcriber also ends; create a new transcriber
before the next recording. An already-aborted request is rejected without closing
an otherwise usable transcriber. Ordinary request errors do not close it.

This distinction lets hosts that retain recording resources until cancellation
settles know that native inference has stopped. Cleanup failures remain visible;
they are not reported as successful cancellation. The host owns temporary audio
cleanup and any operation deadline, for example through `AbortSignal.timeout()`.

## Effect API

Effect is an optional peer. Use Effect `>=4.0.0-rc.112 <5.0.0` for this entrypoint;
older v4 betas are not supported. Promise-only consumers do not need Effect and
can adapt the Promise API to their own framework/runtime version.

```ts
import { Effect } from "effect"
import * as Hex from "@kitlangton/hex/effect"

const program = Effect.scoped(Effect.gen(function* () {
  const transcriber = yield* Hex.create({
    command: ["/path/to/hex-service", "service", "--embedded"],
    model: "parakeet_unified_en",
    language: "en",
    onProgress: console.log,
  })

  return yield* transcriber.transcribe(wav)
}))

const result = await Effect.runPromise(program)
```

The scope owns helper cleanup; there is no manual `close()` on the Effect
transcriber. Put that scope around the voice feature's lifetime when reusing the
model. Model preparation is interruptible after bounded helper startup, and
interrupting active transcription waits for helper cleanup even when the owning
scope remains open. Errors are schema-backed tagged failures; a scope finalizer
that cannot close the helper fails as a defect rather than silently leaking it.
The progress callback is synchronous, as in the Promise API.

## Low-Level Client

Existing `create()` calls **without a model** still return `{ pid, client, close }`
in the Promise API and a scoped `{ pid, client }` in the Effect API. This is useful
for catalog browsing and explicit model management:

```ts
const host = await Hex.create({
  command: ["/path/to/hex-service", "service", "--embedded"],
})
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
} finally {
  await host.close()
}
```

The low-level client retains request-only cancellation; callers own helper
shutdown if native work must stop. Effect's low-level `client.models.prepare()`
remains a progress `Stream`. `Hex.layer(options)` still supplies the low-level
`Hex.Service` and owns its helper for the layer scope.

## Connect to HEX Desktop

Alternatively, reuse the microphone and engine owned by the running HEX desktop
app through the Promise entrypoint:

```ts
import { connect } from "@kitlangton/hex"

const hex = await connect()
const recording = await hex.dictation.start({ source: "my-app" })

void (async () => {
  for await (const { rmsDb, peakDb } of recording.levels) {
    console.log({ rmsDb, peakDb })
  }
})()

const { transcript, durationMs } = await recording.finish()
```

Call `recording.cancel()` to discard. Subscribing to `recording.audio` enables a
bounded best-effort mono Float32 PCM tap at `recording.sampleRate`. Desktop capture
returns raw text and never pastes. `capabilities().serviceCapture` distinguishes
desktop capture from the transcription-only helper. The recording handle renews
a short lease; abandoned captures expire. Its unguessable `ownerToken` scopes
capture operations and observations automatically.

## Host Boundary

- Use the SDK in Node or Electron main code, not an untrusted renderer. Expose
  only narrow, validated recording/model operations across the renderer bridge.
- The direct-child helper does not capture microphone audio. Permissions,
  `NSMicrophoneUsageDescription`, the recording indicator, and recording consent
  belong to the host application. Desktop `connect()` instead uses HEX's capture.
- Encode PCM WAV in the host. Do not send compressed recordings such as M4A or
  WebM and label them as WAV.
- Persist the host's model selection in the host, not in HEX desktop preferences.
- Spawn the signed helper directly. Do not use LaunchServices, `open`, or
  `NSWorkspace`. Validate native packaging/signing in the final consuming app.
- Never expose the startup bearer token to renderer or web content. Credentials
  are exchanged over the child pipe and used for authenticated loopback requests.
