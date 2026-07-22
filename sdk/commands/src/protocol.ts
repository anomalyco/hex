import type { HandlerContext, NativeAction } from "./model.js"

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

export interface Registration {
  readonly type: "registration"
  readonly protocolVersion: 1
  readonly commands: readonly RegistrationCommand[]
}

export type HostInput =
  | {
    readonly type: "invoke"
    readonly invocationId: string
    readonly commandId: string
    readonly context: HandlerContext
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
