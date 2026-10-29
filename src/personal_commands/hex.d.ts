// Types for Hex config scripts. Hex writes this file and replaces it when it
// changes, so edit hex.config.ts instead.

declare module "@hex/commands" {
  export type Modifier = "command" | "control" | "option" | "shift"

  /** Something Hex does natively once a command's handler returns. */
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
    /** A single character, or home, end, up, down, left, right, enter, or escape. */
    readonly key: string
    readonly modifiers?: readonly Modifier[]
    /** 1 through 100. */
    readonly repeat?: number
  }

  /** Open an absolute web URL or app deep link, such as slack://channel?team=...&id=... */
  export function openUrl(url: string): NativeAction
  /** Launch an application by name or path. Use openUrl for app deep links. */
  export function openApplication(application: string): NativeAction
  export function openPath(path: string): NativeAction
  export function press(input: string | PressOptions): NativeAction
  export function typeText(text: string): NativeAction

  /**
   * The actions a handler can ask for. Each call queues its action and
   * resolves at once; Hex runs the queued actions in order after the handler
   * returns, and a failing action fails the command.
   */
  export interface Hex {
    readonly openUrl: (url: string) => Promise<void>
    readonly openApplication: (application: string) => Promise<void>
    readonly openPath: (path: string) => Promise<void>
    readonly press: (input: string | PressOptions) => Promise<void>
    readonly typeText: (text: string) => Promise<void>
  }

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

  export interface UnionCapture<
    Members extends readonly AtomicCaptureDescriptor[] = readonly AtomicCaptureDescriptor[],
  > {
    readonly type: "union"
    readonly members: Members
  }

  export type BoundedCaptureDescriptor = AtomicCaptureDescriptor | UnionCapture
  export type CaptureDescriptor = BoundedCaptureDescriptor | TextCapture
  export type CaptureSchema = Readonly<Record<string, CaptureDescriptor>>
  type CaptureValue<Descriptor> = Descriptor extends DigitCapture ? number
    : Descriptor extends LetterCapture ? Letter
    : Descriptor extends ChoiceCapture<infer Value> ? Value
    : Descriptor extends UnionCapture<infer Members> ? CaptureValue<Members[number]>
    : string
  export type CaptureValues<Schema extends CaptureSchema> = Readonly<{
    [Name in keyof Schema]: CaptureValue<Schema[Name]>
  }>
  export type UntypedCaptures = Readonly<Record<string, string | number>>

  /** One spoken digit, zero through nine unless restricted. Arrives as a number. */
  export function digit(options?: { readonly min?: number; readonly max?: number }): DigitCapture
  /** The rest of the phrase as a string. Must be the last word of every phrase. */
  export function text(): TextCapture
  /** A letter, spoken plainly or in the NATO alphabet. Arrives lowercase. */
  export function letter(): LetterCapture
  export function choice<const Values extends readonly [string, ...string[]]>(
    values: Values,
  ): ChoiceCapture<Values[number]>
  export function choice<const Choices extends Readonly<Record<string, readonly [string, ...string[]]>>>(
    choices: Choices,
  ): ChoiceCapture<Extract<keyof Choices, string>>
  type FlattenedUnionMember<Member> = Member extends UnionCapture<infer Nested> ? Nested[number] : Member
  export function union<
    const Members extends readonly [BoundedCaptureDescriptor, BoundedCaptureDescriptor, ...BoundedCaptureDescriptor[]],
  >(...members: Members): UnionCapture<readonly Extract<FlattenedUnionMember<Members[number]>, AtomicCaptureDescriptor>[]>

  export interface HandlerContext {
    readonly application?: string
    readonly browserHost?: string
    readonly browserUrl?: string
    readonly windowTitle?: string
  }

  export interface HandlerArguments<Captures extends UntypedCaptures = Readonly<Record<string, string>>> {
    readonly hex: Hex
    readonly context: HandlerContext
    /**
     * Values matched by `{name}` placeholders in the spoken phrase. Without a
     * captures schema, a trailing placeholder captures the remaining words:
     * "search amazon for {query}" said as "search amazon for wool socks"
     * yields `{ query: "wool socks" }`.
     */
    readonly captures: Captures
  }

  export type HandlerResult = void | NativeAction | readonly NativeAction[]

  /** Returns actions, or queues them with `hex`, or both. Runs to completion within one second. */
  export type Handler<Captures extends UntypedCaptures = Readonly<Record<string, string>>> =
    (arguments_: HandlerArguments<Captures>) => HandlerResult | Promise<HandlerResult>

  /** Returns the transformed text. Runs to completion within one second. */
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

  type CapturesFor<Schema> = [Schema] extends [never] ? Readonly<Record<string, string>>
    : [Schema] extends [CaptureSchema] ? CaptureValues<Extract<NoInfer<Schema>, CaptureSchema>>
    : Readonly<Record<string, string>>

  type CommandFor<Schema> =
    & CommandMetadata
    & { readonly captures?: Schema & CaptureSchema }
    & (
      | {
        readonly action: [Schema] extends [never] ? NativeAction
          : [Schema] extends [CaptureSchema] ? never
          : NativeAction
        readonly run?: never
      }
      | {
        readonly action?: never
        readonly run: [Schema] extends [CaptureSchema] ? Handler<CapturesFor<Schema>>
          : NativeAction | Handler<CapturesFor<Schema>>
      }
    )

  export type CommandDefinition = CommandFor<never>

  export interface HexConfig {
    readonly dictation?: DictationProtocolConfig
    readonly transformations?: Readonly<Record<string, TransformationDefinition>>
    readonly commands: Readonly<Record<string, CommandDefinition>>
  }

  export function defineHexConfig<
    const Captures extends Readonly<Record<string, unknown>>,
    const Extra extends Omit<HexConfig, "commands">,
  >(
    config: Extra & {
      readonly commands: { readonly [Id in keyof Captures]: CommandFor<Captures[Id]> }
    },
  ): typeof config
}

/** Writes to Hex's log. */
interface Console {
  log(...values: unknown[]): void
  info(...values: unknown[]): void
  debug(...values: unknown[]): void
  warn(...values: unknown[]): void
  error(...values: unknown[]): void
}
declare var console: Console
