import type { DictationProtocolConfig, HandlerContext, NativeAction } from "./model.js"

export interface RegistrationCommand {
  readonly id: string
  readonly phrases: readonly string[]
  readonly group?: string
  readonly description?: string
  readonly when?: { readonly application: string } | { readonly browserHost: string }
  readonly execution:
    | { readonly type: "native"; readonly action: NativeAction }
    | { readonly type: "handler" }
}

export interface RegistrationTransformation {
  readonly id: string
  readonly name: string
  readonly description?: string
}

export interface Registration {
  readonly type: "registration"
  readonly protocolVersion: 1
  readonly dictation?: DictationProtocolConfig
  readonly transformations: readonly RegistrationTransformation[]
  readonly commands: readonly RegistrationCommand[]
}

export type HostInput =
  | {
    readonly type: "transform"
    readonly invocationId: string
    readonly transformationIds: readonly string[]
    readonly text: string
    readonly context: HandlerContext
  }
  | {
    readonly type: "invoke"
    readonly invocationId: string
    readonly commandId: string
    readonly context: HandlerContext
    readonly captures: Readonly<Record<string, string>>
  }
  | {
    readonly type: "toolResult"
    readonly invocationId: string
    readonly toolCallId: string
    readonly result: { readonly type: "success" } | { readonly type: "failure"; readonly message: string; readonly code?: string }
  }
  | { readonly type: "shutdown" }

export type HostOutput =
  | Registration
  | {
    readonly type: "transformationResult"
    readonly invocationId: string
    readonly result:
      | { readonly type: "success"; readonly text: string }
      | { readonly type: "failure"; readonly message: string }
  }
  | {
    readonly type: "toolCall"
    readonly invocationId: string
    readonly toolCallId: string
    readonly action: NativeAction
  }
  | {
    readonly type: "invocationResult"
    readonly invocationId: string
    readonly result: { readonly type: "success" } | { readonly type: "failure"; readonly message: string }
  }
