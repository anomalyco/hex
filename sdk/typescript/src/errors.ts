export type HexErrorCode =
  | "startup-failed"
  | "startup-timeout"
  | "invalid-handshake"
  | "incompatible-api"
  | "service-exited"
  | "cancelled"
  | "request-failed"
  | "invalid-response"
  | "model-prepare-failed"
  | "shutdown-failed"

export class HexError extends Error {
  readonly code: HexErrorCode
  readonly status: number | undefined
  readonly remoteCode: string | undefined
  override readonly cause: unknown

  constructor(
    code: HexErrorCode,
    message: string,
    options?: {
      readonly cause?: unknown
      readonly status?: number
      readonly remoteCode?: string
    },
  ) {
    super(message)
    this.name = "HexError"
    this.code = code
    this.status = options?.status
    this.remoteCode = options?.remoteCode
    this.cause = options?.cause
  }
}

export const isHexError = (value: unknown): value is HexError => value instanceof HexError
