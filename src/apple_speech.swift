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
private actor AppleSpeechSession {
    let locale: Locale
    private var prepared: (SpeechTranscriber, SpeechAnalyzer)?

    init(locale: Locale) {
        self.locale = locale
    }

    func prepare() async throws {
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
        _ = try await AssetInventory.reserve(locale: locale)

        let analyzer = makeAnalyzer(transcriber)
        try await analyzer.prepareToAnalyze(in: Self.audioFormat)
        prepared = (transcriber, analyzer)
    }

    func transcribe(samples: [Float]) async throws -> BridgeTranscript {
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
        async let analyzed = analyzer.analyzeSequence(input)
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
        _ = try await analyzed
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
                modelRetention: .processLifetime
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
    _ operation: @escaping @Sendable () async throws -> Value
) -> Result<Value, Error> {
    let semaphore = DispatchSemaphore(value: 0)
    let result = BlockingResult<Value>()
    Task.detached {
        do {
            result.store(.success(try await operation()))
        } catch {
            result.store(.failure(error))
        }
        semaphore.signal()
    }
    semaphore.wait()
    return result.take()
}

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
    return blocking {
        await SpeechTranscriber.supportedLocale(equivalentTo: requested) != nil
    }.getOr(false) ? 1 : 0
}

@_cdecl("hex_apple_speech_ready")
public func hexAppleSpeechReady(_ identifier: UnsafePointer<CChar>?) -> Int32 {
    guard #available(macOS 26.0, *),
          SpeechTranscriber.isAvailable,
          SFSpeechRecognizer.authorizationStatus() == .authorized,
          let requested = locale(identifier) else {
        return 0
    }
    return blocking {
        guard let supported = await SpeechTranscriber.supportedLocale(
            equivalentTo: requested
        ) else { return false }
        let transcriber = SpeechTranscriber(locale: supported, preset: .transcription)
        return await AssetInventory.status(forModules: [transcriber]) == .installed
    }.getOr(false) ? 1 : 0
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
    let result: Result<AppleSpeechSession, Error> = blocking {
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
    let result: Result<BridgeTranscript, Error> = blocking {
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
