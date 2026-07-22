import { Context, Effect, Schema } from "effect"
import {
  openApplication,
  openPath,
  openUrl,
  press,
  PROTOCOL_VERSION,
  typeText,
} from "./model.js"
import type { CommandMetadata, HandlerContext, HexConfig, NativeAction, PressOptions } from "./model.js"

export class ToolCallError extends Schema.TaggedErrorClass<ToolCallError>()(
  "Hex.ToolCallError",
  { message: Schema.String, code: Schema.optionalKey(Schema.String) },
) {}

export interface HexService {
  readonly context: HandlerContext
  readonly openUrl: (url: string) => Effect.Effect<void, ToolCallError>
  readonly openApplication: (application: string) => Effect.Effect<void, ToolCallError>
  readonly openPath: (path: string) => Effect.Effect<void, ToolCallError>
  readonly press: (input: string | PressOptions) => Effect.Effect<void, ToolCallError>
  readonly typeText: (text: string) => Effect.Effect<void, ToolCallError>
}

export class Hex extends Context.Service<Hex, HexService>()("@hex/commands/Hex") {}

export interface EffectHandlerArguments {
  readonly context: HandlerContext
}

export type EffectHandler<E = unknown> =
  | Effect.Effect<void, E, Hex>
  | ((arguments_: EffectHandlerArguments) => Effect.Effect<void, E, Hex>)

export type EffectCommandDefinition = CommandMetadata & (
  | { readonly action: NativeAction; readonly run?: never }
  | { readonly action?: never; readonly run: NativeAction | EffectHandler }
)

export interface EffectHexConfig {
  readonly commands: Readonly<Record<string, EffectCommandDefinition>>
}

export const defineHexConfig = <const Config extends EffectHexConfig>(config: Config): Config => config

export {
  openApplication,
  openPath,
  openUrl,
  press,
  PROTOCOL_VERSION,
  typeText,
}
export type {
  CommandMetadata,
  HandlerContext,
  HexConfig,
  Modifier,
  NativeAction,
  PressOptions,
} from "./model.js"
