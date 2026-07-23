export const PROTOCOL_VERSION = 1 as const

export type Modifier = "command" | "control" | "option" | "shift"

export type NativeAction =
  | { readonly type: "openUrl"; readonly url: string }
  | { readonly type: "openApplication"; readonly application: string }
  | { readonly type: "openPath"; readonly path: string }
  | {
    readonly type: "press"
    readonly key: string
    readonly modifiers?: readonly Modifier[]
    readonly repeat?: number
  }
  | { readonly type: "typeText"; readonly text: string }

export interface PressOptions {
  readonly key: string
  readonly modifiers?: readonly Modifier[]
  readonly repeat?: number
}

export const openUrl = (url: string): NativeAction => Object.freeze({ type: "openUrl", url })

export const openApplication = (application: string): NativeAction =>
  Object.freeze({ type: "openApplication", application })

export const openPath = (path: string): NativeAction => Object.freeze({ type: "openPath", path })

export function press(input: string | PressOptions): NativeAction {
  return typeof input === "string"
    ? Object.freeze({ type: "press", key: input })
    : Object.freeze({
      type: "press",
      key: input.key,
      ...(input.modifiers === undefined ? {} : { modifiers: Object.freeze([...input.modifiers]) }),
      ...(input.repeat === undefined ? {} : { repeat: input.repeat }),
    })
}

export const typeText = (text: string): NativeAction => Object.freeze({ type: "typeText", text })

export interface HexCapabilities<Result> {
  readonly openUrl: (url: string) => Result
  readonly openApplication: (application: string) => Result
  readonly openPath: (path: string) => Result
  readonly press: (input: string | PressOptions) => Result
  readonly typeText: (text: string) => Result
}

export interface PromiseHex extends HexCapabilities<Promise<void>> {}

export interface HandlerArguments {
  readonly hex: PromiseHex
  readonly context: HandlerContext
}

export interface HandlerContext {
  readonly application?: string
  readonly browserHost?: string
  readonly browserUrl?: string
  readonly windowTitle?: string
}

export type Handler = (arguments_: HandlerArguments) => void | Promise<void>

export type Transformation = (text: string, context: HandlerContext) => string | Promise<string>

export interface TransformationDefinition {
  readonly name: string
  readonly description?: string
  readonly transform: Transformation
}

export interface CommandMetadata {
  readonly phrases: readonly [string, ...string[]]
  readonly group?: string
  readonly description?: string
  readonly when?:
    | { readonly application: string; readonly browserHost?: never }
    | { readonly browserHost: string; readonly application?: never }
}

export interface DictationProtocolConfig {
  readonly start: readonly [string, ...string[]]
  readonly stop: readonly [string, ...string[]]
  readonly send: readonly [string, ...string[]]
  readonly cancel: readonly [string, ...string[]]
}

export type CommandDefinitionFor<Run> = CommandMetadata & (
  | { readonly action: NativeAction; readonly run?: never }
  | { readonly action?: never; readonly run: NativeAction | Run }
)

export interface HexConfigFor<Definition extends CommandMetadata> {
  readonly dictation?: DictationProtocolConfig
  readonly transformations?: Readonly<Record<string, TransformationDefinition>>
  readonly commands: Readonly<Record<string, Definition>>
}

export type CommandDefinition = CommandDefinitionFor<Handler>

export interface HexConfig extends HexConfigFor<CommandDefinition> {}

export const defineHexConfig = <const Config extends HexConfig>(config: Config): Config => config
