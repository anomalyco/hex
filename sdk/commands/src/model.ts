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

export function press(key: string): NativeAction
export function press(options: PressOptions): NativeAction
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

export interface PromiseHex {
  readonly openUrl: (url: string) => Promise<void>
  readonly openApplication: (application: string) => Promise<void>
  readonly openPath: (path: string) => Promise<void>
  readonly press: (input: string | PressOptions) => Promise<void>
  readonly typeText: (text: string) => Promise<void>
}

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

export interface CommandMetadata {
  readonly phrases: readonly [string, ...string[]]
  readonly group?: string
  readonly description?: string
  readonly when?:
    | { readonly application: string; readonly browserHost?: never }
    | { readonly browserHost: string; readonly application?: never }
}

export type CommandDefinition = CommandMetadata & (
  | { readonly action: NativeAction; readonly run?: never }
  | { readonly action?: never; readonly run: NativeAction | Handler }
)

export interface HexConfig {
  readonly commands: Readonly<Record<string, CommandDefinition>>
}

export const defineHexConfig = <const Config extends HexConfig>(config: Config): Config => config

export const isNativeAction = (value: unknown): value is NativeAction => {
  if (typeof value !== "object" || value === null || !("type" in value)) return false
  switch (value.type) {
    case "openUrl":
      return "url" in value && typeof value.url === "string"
    case "openApplication":
      return "application" in value && typeof value.application === "string"
    case "openPath":
      return "path" in value && typeof value.path === "string"
    case "press":
      return "key" in value && typeof value.key === "string"
    case "typeText":
      return "text" in value && typeof value.text === "string"
    default:
      return false
  }
}
