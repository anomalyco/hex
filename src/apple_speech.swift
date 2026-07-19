import AVFAudio
import Foundation
import Speech

private struct BridgeSegment: Codable {
    let startMs: Int64
    let endMs: Int64
    let text: String
}

private struct BridgeTranscript: Codable {
    let text: String
    let segments: [BridgeSegment]
}

@available(macOS 26.0, *)
private actor AppleSpeechAssetReservations {
    static let shared = AppleSpeechAssetReservations()

    private enum State {
        case acquiring(Int, Task<Void, Error>, Int)
        case reserved(Int)
        case releasing(Int, Task<Void, Never>)
    }

    private var generation = 0
    private var states: [String: State] = [:]

    func acquire(_ locale: Locale) async throws {
        let key = locale.identifier(.bcp47)
        while true {
            switch states[key] {
            case nil:
                generation += 1
                let currentGeneration = generation
                let acquisition = Task {
                    _ = try await AssetInventory.reserve(locale: locale)
                }
                states[key] = .acquiring(currentGeneration, acquisition, 1)
                try await finishAcquisition(acquisition, generation: currentGeneration, key: key)
                return
            case .reserved(let references):
                states[key] = .reserved(references + 1)
                return
            case .acquiring(let currentGeneration, let acquisition, let references):
                states[key] = .acquiring(currentGeneration, acquisition, references + 1)
                try await finishAcquisition(acquisition, generation: currentGeneration, key: key)
                return
            case .releasing(let currentGeneration, let release):
                await release.value
                if case .releasing(let generation, _) = states[key],
                   generation == currentGeneration {
                    states[key] = nil
                }
            }
        }
    }

    private func finishAcquisition(
        _ acquisition: Task<Void, Error>,
        generation currentGeneration: Int,
        key: String
    ) async throws {
        do {
            try await acquisition.value
            if case .acquiring(let generation, _, let references) = states[key],
               generation == currentGeneration {
                states[key] = .reserved(references)
            }
        } catch {
            if case .acquiring(let generation, _, _) = states[key],
               generation == currentGeneration {
                states[key] = nil
            }
            throw error
        }
    }

    func release(_ locale: Locale) async {
        let key = locale.identifier(.bcp47)
        guard case .reserved(let references) = states[key] else { return }
        if references > 1 {
            states[key] = .reserved(references - 1)
        } else {
            generation += 1
            let currentGeneration = generation
            let release = Task {
                _ = await AssetInventory.release(reservedLocale: locale)
            }
            states[key] = .releasing(currentGeneration, release)
            await release.value
            if case .releasing(let generation, _) = states[key],
               generation == currentGeneration {
                states[key] = nil
            }
        }
    }
}

@available(macOS 26.0, *)
private actor AppleSpeechSession {
    let locale: Locale
    private var prepared: (SpeechTranscriber, SpeechAnalyzer)?
    private var localeReserved = false
    private var releaseWhenIdle = false
    private var transcriptionActive = false

    init(locale: Locale) {
        self.locale = locale
    }

    func prepare() async throws {
        try await AppleSpeechAssetReservations.shared.acquire(locale)
        localeReserved = true

        do {
            let transcriber = SpeechTranscriber(locale: locale, preset: .transcription)
            let status = await AssetInventory.status(forModules: [transcriber])
            if status != .installed,
               let request = try await AssetInventory.assetInstallationRequest(
                   supporting: [transcriber]
               ) {
                try await request.downloadAndInstall()
            }
            guard await AssetInventory.status(forModules: [transcriber]) == .installed else {
                throw BridgeError("Apple Speech assets are not installed for \(locale.identifier).")
            }
            let analyzer = makeAnalyzer(transcriber)
            try await analyzer.prepareToAnalyze(in: Self.audioFormat)
            prepared = (transcriber, analyzer)
        } catch {
            await releaseAssets()
            throw error
        }
    }

    func transcribe(samples: [Float]) async throws -> BridgeTranscript {
        guard !transcriptionActive else {
            throw BridgeError("Apple Speech is still canceling the previous transcription.")
        }
        transcriptionActive = true
        do {
            let transcript = try await performTranscription(samples: samples)
            await finishTranscription()
            return transcript
        } catch {
            await finishTranscription()
            throw error
        }
    }

    func releaseAssets() async {
        prepared = nil
        if transcriptionActive {
            releaseWhenIdle = true
            return
        }
        await releaseReservedLocale()
    }

    deinit {
        guard localeReserved else { return }
        let locale = locale
        Task {
            await AppleSpeechAssetReservations.shared.release(locale)
        }
    }

    private func performTranscription(samples: [Float]) async throws -> BridgeTranscript {
        let transcriber: SpeechTranscriber
        let analyzer: SpeechAnalyzer
        if let prepared {
            (transcriber, analyzer) = prepared
            self.prepared = nil
        } else {
            transcriber = SpeechTranscriber(locale: locale, preset: .transcription)
            analyzer = makeAnalyzer(transcriber)
        }

        guard let buffer = AVAudioPCMBuffer(
            pcmFormat: Self.audioFormat,
            frameCapacity: AVAudioFrameCount(samples.count)
        ) else {
            throw BridgeError("Could not allocate an Apple Speech audio buffer.")
        }
        buffer.frameLength = AVAudioFrameCount(samples.count)
        guard let channel = buffer.floatChannelData?[0] else {
            throw BridgeError("Apple Speech did not provide a mono audio channel.")
        }
        samples.withUnsafeBufferPointer { source in
            channel.update(from: source.baseAddress!, count: samples.count)
        }

        let input = AsyncStream<AnalyzerInput> { continuation in
            continuation.yield(AnalyzerInput(buffer: buffer))
            continuation.finish()
        }
        let results = Task {
            try await Self.collectTranscript(from: transcriber)
        }

        return try await withTaskCancellationHandler {
            do {
                if let lastSampleTime = try await analyzer.analyzeSequence(input) {
                    try await analyzer.finalizeAndFinish(through: lastSampleTime)
                } else {
                    await analyzer.cancelAndFinishNow()
                }
                return try await results.value
            } catch {
                results.cancel()
                await analyzer.cancelAndFinishNow()
                _ = try? await results.value
                throw error
            }
        } onCancel: {
            results.cancel()
            Task {
                await analyzer.cancelAndFinishNow()
            }
        }
    }

    private func finishTranscription() async {
        transcriptionActive = false
        if releaseWhenIdle {
            releaseWhenIdle = false
            await releaseReservedLocale()
        }
    }

    private func releaseReservedLocale() async {
        guard localeReserved else { return }
        localeReserved = false
        await AppleSpeechAssetReservations.shared.release(locale)
    }

    private static func collectTranscript(
        from transcriber: SpeechTranscriber
    ) async throws -> BridgeTranscript {
        var text = AttributedString()
        var segments: [BridgeSegment] = []
        for try await result in transcriber.results where result.isFinal {
            text += result.text
            let resultText = String(result.text.characters)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if !resultText.isEmpty {
                let start = result.range.start.seconds
                let end = result.range.end.seconds
                segments.append(
                    BridgeSegment(
                        startMs: Int64((start * 1_000).rounded()),
                        endMs: Int64((end * 1_000).rounded()),
                        text: resultText
                    )
                )
            }
        }
        let finalText = String(text.characters)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !finalText.isEmpty else {
            throw BridgeError("Apple Speech did not detect any speech.")
        }
        return BridgeTranscript(text: finalText, segments: segments)
    }

    private func makeAnalyzer(_ transcriber: SpeechTranscriber) -> SpeechAnalyzer {
        SpeechAnalyzer(
            modules: [transcriber],
            options: SpeechAnalyzer.Options(
                priority: .userInitiated,
                modelRetention: .lingering
            )
        )
    }

    private static let audioFormat = AVAudioFormat(
        commonFormat: .pcmFormatFloat32,
        sampleRate: 16_000,
        channels: 1,
        interleaved: false
    )!
}

private struct BridgeError: LocalizedError {
    let message: String

    init(_ message: String) {
        self.message = message
    }

    var errorDescription: String? { message }
}

private final class BlockingResult<Value: Sendable>: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Result<Value, Error>?

    func store(_ value: Result<Value, Error>) {
        lock.withLock { self.value = value }
    }

    func take() -> Result<Value, Error> {
        lock.withLock { value! }
    }
}

private func blocking<Value: Sendable>(
    timeout: TimeInterval,
    operationName: String,
    _ operation: @escaping @Sendable () async throws -> Value
) -> Result<Value, Error> {
    let semaphore = DispatchSemaphore(value: 0)
    let result = BlockingResult<Value>()
    let task = Task.detached {
        do {
            result.store(.success(try await operation()))
        } catch {
            result.store(.failure(error))
        }
        semaphore.signal()
    }
    guard semaphore.wait(timeout: .now() + timeout) == .success else {
        task.cancel()
        return .failure(BridgeError("\(operationName) timed out."))
    }
    return result.take()
}

private let capabilityTimeout: TimeInterval = 5
private let preparationTimeout: TimeInterval = 15 * 60
private let transcriptionTimeout: TimeInterval = 2 * 60
private let releaseTimeout: TimeInterval = 5

private func locale(_ identifier: UnsafePointer<CChar>?) -> Locale? {
    guard let identifier else { return nil }
    return Locale(identifier: String(cString: identifier))
}

private func writeError(
    _ error: Error,
    to destination: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) {
    destination?.pointee = strdup(
        (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
    )
}

private func authorization() async -> SFSpeechRecognizerAuthorizationStatus {
    let status = SFSpeechRecognizer.authorizationStatus()
    guard status == .notDetermined else { return status }
    return await withCheckedContinuation { continuation in
        SFSpeechRecognizer.requestAuthorization { status in
            continuation.resume(returning: status)
        }
    }
}

@_cdecl("hex_apple_speech_supported")
public func hexAppleSpeechSupported(_ identifier: UnsafePointer<CChar>?) -> Int32 {
    guard #available(macOS 26.0, *),
          SpeechTranscriber.isAvailable,
          let requested = locale(identifier) else {
        return 0
    }
    let result = blocking(timeout: capabilityTimeout, operationName: "Apple Speech capability check") {
        await SpeechTranscriber.supportedLocale(equivalentTo: requested) != nil
    }
    return switch result {
    case .success(let supported): supported ? 1 : 0
    case .failure: -1
    }
}

@_cdecl("hex_apple_speech_ready")
public func hexAppleSpeechReady(_ identifier: UnsafePointer<CChar>?) -> Int32 {
    guard #available(macOS 26.0, *),
          SpeechTranscriber.isAvailable,
          SFSpeechRecognizer.authorizationStatus() == .authorized,
          let requested = locale(identifier) else {
        return 0
    }
    let result = blocking(timeout: capabilityTimeout, operationName: "Apple Speech readiness check") {
        guard let supported = await SpeechTranscriber.supportedLocale(
            equivalentTo: requested
        ) else { return false }
        let transcriber = SpeechTranscriber(locale: supported, preset: .transcription)
        return await AssetInventory.status(forModules: [transcriber]) == .installed
    }
    return switch result {
    case .success(let ready): ready ? 1 : 0
    case .failure: -1
    }
}

@_cdecl("hex_apple_speech_prepare")
public func hexAppleSpeechPrepare(
    _ identifier: UnsafePointer<CChar>?,
    _ error: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    guard #available(macOS 26.0, *),
          SpeechTranscriber.isAvailable,
          let requested = locale(identifier) else {
        writeError(BridgeError("Apple Speech requires macOS 26."), to: error)
        return nil
    }
    let result: Result<AppleSpeechSession, Error> = blocking(
        timeout: preparationTimeout,
        operationName: "Apple Speech preparation"
    ) {
        guard await authorization() == .authorized else {
            throw BridgeError("Speech Recognition access is required to use Apple Speech.")
        }
        guard let supported = await SpeechTranscriber.supportedLocale(
            equivalentTo: requested
        ) else {
            throw BridgeError("Apple Speech does not support \(requested.identifier).")
        }
        let session = AppleSpeechSession(locale: supported)
        try await session.prepare()
        return session
    }
    switch result {
    case .success(let session):
        return Unmanaged.passRetained(session).toOpaque()
    case .failure(let failure):
        writeError(failure, to: error)
        return nil
    }
}

@_cdecl("hex_apple_speech_transcribe")
public func hexAppleSpeechTranscribe(
    _ opaque: UnsafeMutableRawPointer?,
    _ samples: UnsafePointer<Float>?,
    _ count: Int,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutablePointer<CChar>? {
    guard #available(macOS 26.0, *), let opaque, let samples, count > 0 else {
        writeError(BridgeError("Apple Speech received no audio."), to: errorOutput)
        return nil
    }
    let session = Unmanaged<AppleSpeechSession>.fromOpaque(opaque).takeUnretainedValue()
    let audio = Array(UnsafeBufferPointer(start: samples, count: count))
    let result: Result<BridgeTranscript, Error> = blocking(
        timeout: transcriptionTimeout,
        operationName: "Apple Speech transcription"
    ) {
        try await session.transcribe(samples: audio)
    }
    switch result {
    case .success(let transcript):
        do {
            let data = try JSONEncoder().encode(transcript)
            return strdup(String(decoding: data, as: UTF8.self))
        } catch {
            writeError(error, to: errorOutput)
            return nil
        }
    case .failure(let failure):
        writeError(failure, to: errorOutput)
        return nil
    }
}

@_cdecl("hex_apple_speech_release")
public func hexAppleSpeechRelease(_ opaque: UnsafeMutableRawPointer?) {
    guard #available(macOS 26.0, *), let opaque else { return }
    let session = Unmanaged<AppleSpeechSession>.fromOpaque(opaque).takeUnretainedValue()
    _ = blocking(timeout: releaseTimeout, operationName: "Apple Speech cleanup") {
        await session.releaseAssets()
    }
    Unmanaged<AppleSpeechSession>.fromOpaque(opaque).release()
}

@_cdecl("hex_apple_speech_free_string")
public func hexAppleSpeechFreeString(_ string: UnsafeMutablePointer<CChar>?) {
    free(string)
}

private extension Result {
    func getOr(_ fallback: Success) -> Success {
        switch self {
        case .success(let value): value
        case .failure: fallback
        }
    }
}
