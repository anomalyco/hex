import { Context, Effect, Schema } from "effect"
import {
  openApplication,
  openPath,
  openUrl,
  press,
  PROTOCOL_VERSION,
  typeText,
  choice,
  digit,
  letter,
  text,
  union,
} from "./model.js"
import type {
  CommandDefinitionFor,
  CaptureSchema,
  CaptureValues,
  CommandMetadata,
  HandlerContext,
  HexCapabilities,
  HexConfigFor,
  NativeAction,
} from "./model.js"

export class ToolCallError extends Schema.TaggedError<ToolCallError>()(
  "Hex.ToolCallError",
  { message: Schema.String, code: Schema.optionalKey(Schema.String) },
) {}

export interface HexService extends HexCapabilities<Effect.Effect<void, ToolCallError>> {
  readonly context: HandlerContext
  readonly captures: Readonly<Record<string, string | number>>
}

export class Hex extends Context.Service<Hex, HexService>()("@hex/commands/Hex") {}

export interface EffectHandlerArguments<Captures extends Readonly<Record<string, string | number>> = Readonly<Record<string, string>>> {
  readonly context: HandlerContext
  readonly captures: Captures
}

export type EffectHandler<E = unknown> =
  | Effect.Effect<void, E, Hex>
  | ((arguments_: EffectHandlerArguments) => Effect.Effect<void, E, Hex>)

export type EffectCommandDefinition = CommandDefinitionFor<EffectHandler>

export interface EffectHexConfig extends HexConfigFor<EffectCommandDefinition> {}

type EffectCapturesFor<Schema> = [Schema] extends [never]
  ? Readonly<Record<string, string>>
  : [Schema] extends [CaptureSchema]
  ? CaptureValues<Extract<NoInfer<Schema>, CaptureSchema>>
  : Readonly<Record<string, string>>

type EffectCommandFor<Schema> = CommandMetadata
  & { readonly captures?: Schema & CaptureSchema }
  & (
    | {
      readonly action: [Schema] extends [never]
        ? import("./model.js").NativeAction
        : [Schema] extends [CaptureSchema] ? never : import("./model.js").NativeAction
      readonly run?: never
    }
    | {
      readonly action?: never
      readonly run: [Schema] extends [CaptureSchema]
        ? Effect.Effect<void, unknown, Hex>
          | ((arguments_: EffectHandlerArguments<EffectCapturesFor<Schema>>) => Effect.Effect<void, unknown, Hex>)
        : NativeAction
          | Effect.Effect<void, unknown, Hex>
          | ((arguments_: EffectHandlerArguments<EffectCapturesFor<Schema>>) => Effect.Effect<void, unknown, Hex>)
    }
  )

export type EffectTypedCommandDefinition<Schema extends CaptureSchema> = EffectCommandFor<Schema>

export const defineHexConfig = <
  const Captures extends Readonly<Record<string, unknown>>,
  const Extra extends Omit<EffectHexConfig, "commands">,
>(config: Extra & {
  readonly commands: { readonly [Id in keyof Captures]: EffectCommandFor<Captures[Id]> }
}): typeof config => config

export {
  openApplication,
  openPath,
  openUrl,
  press,
  PROTOCOL_VERSION,
  typeText,
  choice,
  digit,
  letter,
  text,
  union,
}
export type {
  CommandMetadata,
  DictationProtocolConfig,
  HandlerContext,
  HexConfig,
  Modifier,
  NativeAction,
  PressOptions,
  Transformation,
  TransformationDefinition,
  CaptureDescriptor,
  CaptureSchema,
  CaptureValues,
  ChoiceCapture,
  DigitCapture,
  Letter,
  LetterCapture,
  TextCapture,
  AtomicCaptureDescriptor,
  BoundedCaptureDescriptor,
  UnionCapture,
} from "./model.js"
