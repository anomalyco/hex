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

  /**
   * Returns the transformed text. It must finish within its `timeout`; if it
   * throws or runs out of time, Hex pastes the text it was given.
   */
  export type Transformation = (text: string, context: HandlerContext) => string | Promise<string>

  export interface TransformationDefinition {
    readonly name: string
    readonly description?: string
    readonly transform: Transformation
    /**
     * Milliseconds, 1 to 60000. Defaults to 1000, or 30000 when the config
     * uses `fetch` or `node:child_process`. When it runs out, Hex aborts its
     * requests and ends every program it started.
     */
    readonly timeout?: number
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
    readonly commands?: Readonly<Record<string, CommandDefinition>>
  }

  export function defineHexConfig<
    const Captures extends Readonly<Record<string, unknown>> = Record<string, never>,
    const Extra extends Omit<HexConfig, "commands"> = Omit<HexConfig, "commands">,
  >(
    config: Extra & {
      readonly commands?: { readonly [Id in keyof Captures]: CommandFor<Captures[Id]> }
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

// Host APIs. `fetch` and `node:child_process` work only after the person
// allows this exact config in Hex's Output pane; Hex asks again whenever a
// file changes. Docs: https://anoma.ly/hex/docs/transforms

type HeadersInit = Headers | readonly (readonly [string, string])[] | Readonly<Record<string, string>>

/** WHATWG Headers. Names are case-insensitive. */
declare class Headers implements Iterable<[string, string]> {
  constructor(init?: HeadersInit)
  append(name: string, value: string): void
  delete(name: string): void
  get(name: string): string | null
  has(name: string): boolean
  set(name: string, value: string): void
  forEach(callback: (value: string, name: string, headers: Headers) => void, thisArg?: unknown): void
  entries(): IterableIterator<[string, string]>
  keys(): IterableIterator<string>
  values(): IterableIterator<string>
  [Symbol.iterator](): IterableIterator<[string, string]>
}

interface RequestInit {
  /** GET by default. */
  readonly method?: string
  readonly headers?: HeadersInit
  /** Strings only; send JSON with `JSON.stringify`. Not allowed with GET or HEAD. */
  readonly body?: string | null
  readonly signal?: AbortSignal | null
}

/** The whole response, read before `fetch` resolves; bodies are not streamed. */
declare class Response {
  constructor(body?: string | null, init?: { status?: number; statusText?: string; headers?: HeadersInit })
  readonly ok: boolean
  readonly status: number
  readonly statusText: string
  readonly headers: Headers
  /** The final URL, after redirects. */
  readonly url: string
  readonly bodyUsed: boolean
  text(): Promise<string>
  json(): Promise<any>
  arrayBuffer(): Promise<ArrayBuffer>
}

/**
 * WHATWG fetch over http and https. Rejects with a TypeError on network
 * errors (not on HTTP error statuses), or with the signal's reason on abort.
 */
declare function fetch(input: string | { readonly url: string }, init?: RequestInit): Promise<Response>

declare class AbortSignal {
  readonly aborted: boolean
  readonly reason: any
  onabort: ((event: { readonly type: "abort" }) => void) | null
  throwIfAborted(): void
  addEventListener(type: "abort", listener: (event: { readonly type: "abort" }) => void, options?: { once?: boolean }): void
  removeEventListener(type: "abort", listener: (event: { readonly type: "abort" }) => void): void
  static abort(reason?: unknown): AbortSignal
  static timeout(milliseconds: number): AbortSignal
  static any(signals: readonly AbortSignal[]): AbortSignal
}

declare class AbortController {
  readonly signal: AbortSignal
  abort(reason?: unknown): void
}

declare class DOMException extends Error {
  constructor(message?: string, name?: string)
}

declare function setTimeout<Args extends unknown[]>(callback: (...args: Args) => void, delay?: number, ...args: Args): number
declare function clearTimeout(id: number | undefined): void
declare function queueMicrotask(callback: () => void): void

declare module "node:child_process" {
  export interface ExecOptions {
    /** Defaults to the config folder. */
    readonly cwd?: string
    /** Added to a minimal environment (PATH, HOME, locale, temp folder), not Hex's. */
    readonly env?: Readonly<Record<string, string | undefined>>
    /** Milliseconds before Hex sends SIGTERM to the program and its children. */
    readonly timeout?: number
    /** Bytes of stdout or stderr to keep; 1 MiB by default, 16 MiB at most. */
    readonly maxBuffer?: number
    readonly signal?: AbortSignal
    /** Hex's addition: written to stdin, which is then closed. Without it stdin is empty. */
    readonly input?: string
  }

  /** Node's error for a failed run: `code` is the exit code, or a string like "ENOENT". */
  export interface ExecException extends Error {
    code?: number | string | null
    killed?: boolean
    signal?: string | null
    cmd?: string
    stdout?: string
    stderr?: string
  }

  export type ExecCallback = (error: ExecException | null, stdout: string, stderr: string) => void

  export interface ChildProcess {
    readonly pid: undefined
    killed: boolean
    /** Sends SIGTERM to the program and its children. */
    kill(): boolean
  }

  /** Runs `file` with `args`, no shell. Prefer it for anything containing dictated text. */
  export function execFile(file: string, callback?: ExecCallback): ChildProcess
  export function execFile(file: string, args: readonly string[] | undefined | null, callback?: ExecCallback): ChildProcess
  export function execFile(file: string, options: ExecOptions, callback?: ExecCallback): ChildProcess
  export function execFile(
    file: string,
    args: readonly string[] | undefined | null,
    options: ExecOptions | undefined | null,
    callback?: ExecCallback,
  ): ChildProcess
  export namespace execFile {
    function __promisify__(
      file: string,
      args?: readonly string[] | null,
      options?: ExecOptions | null,
    ): Promise<{ stdout: string; stderr: string }> & { child: ChildProcess }
  }

  /** Runs `command` through `/bin/sh -c`, or `cmd.exe /d /s /c` on Windows. */
  export function exec(command: string, callback?: ExecCallback): ChildProcess
  export function exec(command: string, options: ExecOptions | undefined | null, callback?: ExecCallback): ChildProcess
  export namespace exec {
    function __promisify__(
      command: string,
      options?: ExecOptions | null,
    ): Promise<{ stdout: string; stderr: string }> & { child: ChildProcess }
  }

  const childProcess: { readonly exec: typeof exec; readonly execFile: typeof execFile }
  export default childProcess
}

declare module "node:util" {
  type Custom<T> = T extends { __promisify__: infer P } ? P : never
  export function promisify<T extends { __promisify__: (...args: any[]) => any }>(original: T): Custom<T>
  export function promisify<Result>(
    original: (callback: (error: unknown, result: Result) => void) => void,
  ): () => Promise<Result>
  export function promisify<A, Result>(
    original: (a: A, callback: (error: unknown, result: Result) => void) => void,
  ): (a: A) => Promise<Result>
  export function promisify<A, B, Result>(
    original: (a: A, b: B, callback: (error: unknown, result: Result) => void) => void,
  ): (a: A, b: B) => Promise<Result>
  export namespace promisify {
    const custom: unique symbol
  }
  const util: { readonly promisify: typeof promisify }
  export default util
}
