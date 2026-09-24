import { validateUnion } from "./captures.js"

export const PROTOCOL_VERSION = 2 as const

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

/** Open an absolute web URL or app deep link with its macOS handler. */
export const openUrl = (url: string): NativeAction => Object.freeze({ type: "openUrl", url })

/** Launch an application by name or path. Use openUrl for app deep links. */
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
  /** Open an absolute web URL or app deep link, such as slack://channel?team=...&id=... . */
  readonly openUrl: (url: string) => Result
  /** Launch an application by name or path. Use openUrl for app deep links. */
  readonly openApplication: (application: string) => Result
  readonly openPath: (path: string) => Result
  readonly press: (input: string | PressOptions) => Result
  readonly typeText: (text: string) => Result
}

export interface PromiseHex extends HexCapabilities<Promise<void>> {}

export interface DigitCapture {
  readonly type: "digit"
  readonly min: number
  readonly max: number
}

export interface TextCapture {
  readonly type: "text"
}

export type Letter =
  | "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m"
  | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z"

export interface LetterCapture {
  readonly type: "letter"
}

export interface ChoiceCapture<Value extends string = string> {
  readonly type: "choice"
  readonly choices: Readonly<Record<Value, readonly string[]>>
}

export type AtomicCaptureDescriptor = DigitCapture | LetterCapture | ChoiceCapture

export interface UnionCapture<Members extends readonly AtomicCaptureDescriptor[] = readonly AtomicCaptureDescriptor[]> {
  readonly type: "union"
  readonly members: Members
}

export type BoundedCaptureDescriptor = AtomicCaptureDescriptor | UnionCapture
export type CaptureDescriptor = BoundedCaptureDescriptor | TextCapture
export type CaptureSchema = Readonly<Record<string, CaptureDescriptor>>
type CaptureValue<Descriptor> = Descriptor extends DigitCapture
  ? number
  : Descriptor extends LetterCapture
    ? Letter
    : Descriptor extends ChoiceCapture<infer Value>
      ? Value
      : Descriptor extends UnionCapture<infer Members>
        ? CaptureValue<Members[number]>
        : string
export type CaptureValues<Schema extends CaptureSchema> = Readonly<{
  [Name in keyof Schema]: CaptureValue<Schema[Name]>
}>
export type UntypedCaptures = Readonly<Record<string, string | number>>

export const digit = (options: { readonly min: number; readonly max: number } = { min: 0, max: 9 }): DigitCapture =>
  Object.freeze({ type: "digit", min: options.min, max: options.max })

export const text = (): TextCapture => Object.freeze({ type: "text" })

export const letter = (): LetterCapture => Object.freeze({ type: "letter" })

export function choice<const Values extends readonly [string, ...string[]]>(
  values: Values,
): ChoiceCapture<Values[number]>
export function choice<const Choices extends Readonly<Record<string, readonly [string, ...string[]]>>>(
  choices: Choices,
): ChoiceCapture<Extract<keyof Choices, string>>
export function choice(
  input: readonly string[] | Readonly<Record<string, readonly string[]>>,
): ChoiceCapture {
  const entries: readonly (readonly [string, readonly string[]])[] = Array.isArray(input)
    ? input.map((value) => [value, [value]] as const)
    : Object.entries(input)
  const grouped = new Map<string, string[]>()
  for (const [value, aliases] of entries) {
    grouped.set(value, [...(grouped.get(value) ?? []), ...aliases])
  }
  const choices = Object.fromEntries(
    [...grouped].map(([value, aliases]) => [value, Object.freeze(aliases)]),
  )
  return Object.freeze({ type: "choice", choices: Object.freeze(choices) })
}

type FlattenedUnionMember<Member> = Member extends UnionCapture<infer Nested> ? Nested[number] : Member

export function union<
  const Members extends readonly [BoundedCaptureDescriptor, BoundedCaptureDescriptor, ...BoundedCaptureDescriptor[]],
>(...members: Members): UnionCapture<readonly Extract<FlattenedUnionMember<Members[number]>, AtomicCaptureDescriptor>[]>
export function union(...members: readonly CaptureDescriptor[]): UnionCapture {
  return validateUnion(members, "union()")
}

export interface HandlerArguments<Captures extends UntypedCaptures = Readonly<Record<string, string>>> {
  readonly hex: PromiseHex
  readonly context: HandlerContext
  /**
   * Free text matched by `{name}` placeholders in the spoken phrase, keyed by
   * placeholder name. A placeholder captures the normalized trailing words,
   * so "search amazon for {query}" invoked as "search amazon for wool socks"
   * yields `{ query: "wool socks" }`. Empty for plain phrases.
   */
  readonly captures: Captures
}

export interface HandlerContext {
  readonly application?: string
  readonly browserHost?: string
  readonly browserUrl?: string
  readonly windowTitle?: string
}

export type Handler<Captures extends UntypedCaptures = Readonly<Record<string, string>>> =
  (arguments_: HandlerArguments<Captures>) => void | Promise<void>

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
  | { readonly captures?: never; readonly action: NativeAction; readonly run?: never }
  | { readonly captures?: never; readonly action?: never; readonly run: NativeAction | Run }
)

type CapturesFor<Schema> = [Schema] extends [never]
  ? Readonly<Record<string, string>>
  : [Schema] extends [CaptureSchema]
  ? CaptureValues<Extract<NoInfer<Schema>, CaptureSchema>>
  : Readonly<Record<string, string>>

type PromiseCommandFor<Schema> = CommandMetadata
  & { readonly captures?: Schema & CaptureSchema }
  & (
    | {
      readonly action: [Schema] extends [never]
        ? NativeAction
        : [Schema] extends [CaptureSchema] ? never : NativeAction
      readonly run?: never
    }
    | {
      readonly action?: never
      readonly run: [Schema] extends [CaptureSchema]
        ? Handler<CapturesFor<Schema>>
        : NativeAction | Handler<CapturesFor<Schema>>
    }
  )

export type TypedCommandDefinition<Schema extends CaptureSchema> = PromiseCommandFor<Schema>

export interface HexConfigFor<Definition extends CommandMetadata> {
  readonly dictation?: DictationProtocolConfig
  readonly transformations?: Readonly<Record<string, TransformationDefinition>>
  readonly commands: Readonly<Record<string, Definition>>
}

export type CommandDefinition = CommandDefinitionFor<Handler>

export interface HexConfig extends HexConfigFor<CommandDefinition> {}

export const defineHexConfig = <
  const Captures extends Readonly<Record<string, unknown>>,
  const Extra extends Omit<HexConfig, "commands">,
>(config: Extra & {
  readonly commands: { readonly [Id in keyof Captures]: PromiseCommandFor<Captures[Id]> }
}): typeof config => config
