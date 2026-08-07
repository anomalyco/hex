export const MODEL_IDS = [
  "parakeet_unified_en",
  "parakeet_v2",
  "parakeet_v3",
  "whisper_large_v3_turbo",
  "qwen3_asr06_b",
  "sense_voice_small",
  "cohere_transcribe",
  "apple_speech",
] as const

export type ModelId = typeof MODEL_IDS[number]

export interface Health {
  readonly version: string
  readonly apiVersion: "2"
}

export interface Capabilities {
  readonly audioFormats: readonly ["audio/wav"]
  readonly partialTranscripts: false
  readonly serviceCapture: boolean
}

export interface ModelInfo {
  readonly id: ModelId
  readonly name: string
  readonly installed: boolean
  readonly verified: boolean
  readonly managed: boolean
  readonly downloadBytes: number | null
  readonly languages: readonly string[]
  readonly supportsLanguageDetection: boolean
}

export type ModelProgress =
  | { readonly type: "downloading"; readonly downloadedBytes: number; readonly totalBytes: number }
  | { readonly type: "verifying" }
  | { readonly type: "loading" }

export interface RequestOptions {
  readonly signal?: AbortSignal
}

export interface PrepareModelOptions extends RequestOptions {
  readonly language?: string
  readonly onProgress?: (progress: ModelProgress) => void
}

export interface ListModelsOptions extends RequestOptions {
  readonly language?: string
}

export interface AudioClip {
  readonly data: ArrayBuffer | Uint8Array
  readonly contentType: "audio/wav"
}

export interface TranscriptionRequest extends RequestOptions {
  readonly audio: AudioClip
  readonly model: ModelId
  readonly language?: string
}

export interface TranscriptionResult {
  readonly transcript: string
  readonly durationMs: number
}

export interface DictationStartOptions {
  readonly source: string
}

export interface DictationLevel {
  readonly rmsDb: number
  readonly peakDb: number
}

export interface DictationHandle {
  readonly id: number
  /** Unguessable owner credential required by every operation on this capture. */
  readonly ownerToken: string
  /** Source sample rate for the optional mono Float32 PCM audio tap. */
  readonly sampleRate: number
  readonly levels: AsyncIterable<DictationLevel>
  /** Best-effort raw mono PCM from HEX's capture; no work occurs until iterated. */
  readonly audio: AsyncIterable<Float32Array>
  finish(): Promise<TranscriptionResult>
  cancel(): Promise<void>
}

export interface HexClient {
  health(options?: RequestOptions): Promise<Health>
  capabilities(options?: RequestOptions): Promise<Capabilities>
  readonly models: {
    list(options?: ListModelsOptions): Promise<readonly ModelInfo[]>
    prepare(id: ModelId, options?: PrepareModelOptions): Promise<void>
  }
  transcribe(request: TranscriptionRequest): Promise<TranscriptionResult>
  readonly dictation: {
    start(options: DictationStartOptions): Promise<DictationHandle>
  }
}

export interface CreateOptions {
  /** Advanced override for development and tests. Normal consumers omit this. */
  readonly command?: readonly [executable: string, ...arguments: readonly string[]]
  readonly cwd?: string
  readonly env?: Readonly<Record<string, string | undefined>>
  readonly startupTimeoutMs?: number
  readonly shutdownTimeoutMs?: number
  readonly signal?: AbortSignal
  /** Platform transport override, primarily for tests and Electron adapters. */
  readonly fetch?: typeof globalThis.fetch
}

export interface ConnectOptions extends RequestOptions {
  /** Advanced discovery override for tests or isolated application instances. */
  readonly discoveryPath?: string
  /** Platform transport override, primarily for tests and Electron adapters. */
  readonly fetch?: typeof globalThis.fetch
}

export interface HexHost {
  readonly pid: number
  readonly client: HexClient
  close(): Promise<void>
}
