import type { Effect, Scope } from "effect"
import * as Hex from "../src/index.js"
import * as EffectHex from "../src/effect.js"

// Compile-only consumer checks: never start a helper from a type assertion.
export function createTypes(
  low: Hex.CreateOptions,
  ready: Hex.TranscriberOptions,
  ambiguous: Omit<Hex.CreateOptions, "model"> & { model?: Hex.ModelId },
) {
  const host: Promise<Hex.HexHost> = Hex.create(low)
  const transcriber: Promise<Hex.Transcriber> = Hex.create(ready)
  const effectHost: Effect.Effect<EffectHex.Host, EffectHex.HexError, Scope.Scope> = EffectHex.create(low)
  const effectTranscriber: Effect.Effect<EffectHex.Transcriber, EffectHex.HexError, Scope.Scope> = EffectHex.create(ready)
  const invalid = { model: "fictional-model" }

  // @ts-expect-error A model-bound transcriber cannot masquerade as a low-level host.
  const widened: Hex.CreateOptions = ready
  // @ts-expect-error Optional models cannot determine which resource is returned.
  Hex.create(ambiguous)
  // @ts-expect-error Optional models cannot determine which resource is returned.
  EffectHex.create(ambiguous)
  // @ts-expect-error Invalid model variables must not fall back to the low-level overload.
  Hex.create(invalid)
  // @ts-expect-error Invalid model variables must not fall back to the low-level overload.
  EffectHex.create(invalid)
  // @ts-expect-error The existing service layer deliberately provides only the low-level client.
  EffectHex.layer(ready)

  return { host, transcriber, effectHost, effectTranscriber, widened }
}
