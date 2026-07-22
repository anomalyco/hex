import { Context, Effect, Schema } from "effect"
import {
  openApplication,
  openPath,
  openUrl,
  press,
  PROTOCOL_VERSION,
  typeText,
} from "./model.js"
import type {
  CommandDefinitionFor,
  HandlerContext,
  HexCapabilities,
  HexConfigFor,
} from "./model.js"

export class ToolCallError extends Schema.TaggedErrorClass<ToolCallError>()(
  "Hex.ToolCallError",
  { message: Schema.String, code: Schema.optionalKey(Schema.String) },
) {}

export interface HexService extends HexCapabilities<Effect.Effect<void, ToolCallError>> {
  readonly context: HandlerContext
}

export class Hex extends Context.Service<Hex, HexService>()("@hex/commands/Hex") {}

export interface EffectHandlerArguments {
  readonly context: HandlerContext
}

export type EffectHandler<E = unknown> =
  | Effect.Effect<void, E, Hex>
  | ((arguments_: EffectHandlerArguments) => Effect.Effect<void, E, Hex>)

export type EffectCommandDefinition = CommandDefinitionFor<EffectHandler>

export interface EffectHexConfig extends HexConfigFor<EffectCommandDefinition> {}

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
  DictationProtocolConfig,
  HandlerContext,
  HexConfig,
  Modifier,
  NativeAction,
  PressOptions,
} from "./model.js"
