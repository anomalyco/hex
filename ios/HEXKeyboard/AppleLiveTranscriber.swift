import AVFAudio
import CoreMedia
import Foundation
import Speech

/// Live words from Apple's speech recognition while the keyboard records: the capture's 16-bit
/// chunks go to a `SpeechAnalyzer` as they arrive, its volatile results become the tentative
/// tail and its final ones the settled words. Only for the preview: the dictation's own text
/// still comes from the recording once it stops (`KeyboardTranscription`). Its model must
/// already be on the iPhone: it never downloads one.
@available(iOS 26.0, macOS 26.0, *)
@MainActor
final class AppleLiveTranscriber: LiveTranscriptSource {
    enum Module: String {
        /// `SpeechTranscriber` with volatile and fast results.
        case speech
        /// `SpeechTranscriber` with volatile results only (the probe's comparison).
        case speechSlow
        /// `DictationTranscriber`'s progressive long dictation, for its languages.
        case dictation
    }

    var onUpdate: ((LiveTranscript) -> Void)?
    var onFailure: ((String) -> Void)?
    /// Every result as it came (probe and log): seconds since the first chunk, final, text,
    /// and the audio it covers.
    var onResult: ((_ at: TimeInterval, _ isFinal: Bool, _ text: String, _ start: Double, _ end: Double) -> Void)?

    private let locale: Locale
    private let module: Module
    private let inputFormat: AVAudioFormat
    private let log: (String) -> Void
    private var analyzerFormat: AVAudioFormat?
    private var converter: AVAudioConverter?
    private var pending: [Data] = []
    private var pendingBytes = 0
    private let input: AsyncStream<AnalyzerInput>
    private let continuation: AsyncStream<AnalyzerInput>.Continuation
    private var analyzer: SpeechAnalyzer?
    private var setup: Task<Void, Never>?
    private var reading: Task<Void, Never>?
    private var accumulator = LiveTranscriptAccumulator()
    private var stopped = false
    private var firstChunkAt: Date?
    /// Audio fed to the analyzer, in its own frames, for each buffer's start time.
    private var fedFrames: AVAudioFramePosition = 0
    /// When the analyzer was ready, from `init`, for the log.
    private(set) var readyAfter: TimeInterval?
    private let createdAt = Date()

    /// Audio held while the analyzer gets ready; past this it gives up (`onFailure`).
    static let maximumPendingSeconds: Double = 6

    init?(locale: Locale, module: Module, sampleRate: Double, log: @escaping (String) -> Void) {
        guard let format = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: sampleRate, channels: 1, interleaved: true) else {
            return nil
        }
        self.locale = locale
        self.module = module
        self.log = log
        inputFormat = format
        (input, continuation) = AsyncStream.makeStream(of: AnalyzerInput.self, bufferingPolicy: .unbounded)
        start()
    }

    private func start() {
        let locale = locale
        let module = module
        let input = input
        setup = Task { [weak self] in
            let modules: [any SpeechModule]
            let speech: SpeechTranscriber?
            let dictation: DictationTranscriber?
            switch module {
            case .speech, .speechSlow:
                let options: Set<SpeechTranscriber.ReportingOption> = module == .speech ? [.volatileResults, .fastResults] : [.volatileResults]
                let transcriber = SpeechTranscriber(locale: locale, transcriptionOptions: [], reportingOptions: options, attributeOptions: [])
                speech = transcriber
                dictation = nil
                modules = [transcriber]
            case .dictation:
                let transcriber = DictationTranscriber(locale: locale, preset: .progressiveLongDictation)
                speech = nil
                dictation = transcriber
                modules = [transcriber]
            }
            guard let format = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: modules) else {
                self?.fail("no audio format for \(locale.identifier)")
                return
            }
            // Lingering: the dictation's own transcription, right after, finds the model loaded.
            let analyzer = SpeechAnalyzer(
                modules: modules,
                options: SpeechAnalyzer.Options(priority: .userInitiated, modelRetention: .lingering)
            )
            do {
                try await analyzer.prepareToAnalyze(in: format)
            } catch {
                self?.fail("prepare failed: \(Self.describe(error))")
                return
            }
            guard let self, !self.stopped else {
                await analyzer.cancelAndFinishNow()
                return
            }
            self.analyzer = analyzer
            self.analyzerFormat = format
            if format != self.inputFormat {
                self.converter = AVAudioConverter(from: self.inputFormat, to: format)
            }
            self.reading = Task { [weak self] in
                do {
                    if let speech {
                        for try await result in speech.results {
                            self?.receive(String(result.text.characters), isFinal: result.isFinal, range: result.range)
                        }
                    } else if let dictation {
                        for try await result in dictation.results {
                            self?.receive(String(result.text.characters), isFinal: result.isFinal, range: result.range)
                        }
                    }
                } catch {
                    self?.fail("results failed: \(Self.describe(error))")
                }
            }
            do {
                try await analyzer.start(inputSequence: input)
            } catch {
                self.fail("start failed: \(Self.describe(error))")
                return
            }
            self.readyAfter = Date().timeIntervalSince(self.createdAt)
            self.log("live: \(module.rawValue) \(locale.identifier) ready in \(Int((self.readyAfter ?? 0) * 1000)) ms, \(Int(format.sampleRate)) Hz")
            let held = self.pending
            self.pending = []
            self.pendingBytes = 0
            held.forEach { self.feed($0) }
        }
    }

    func append(_ pcm: Data) {
        guard !stopped, !pcm.isEmpty else { return }
        if firstChunkAt == nil { firstChunkAt = Date() }
        guard analyzerFormat != nil else {
            pending.append(pcm)
            pendingBytes += pcm.count
            if Double(pendingBytes / 2) / inputFormat.sampleRate > Self.maximumPendingSeconds {
                fail("not ready after \(Int(Self.maximumPendingSeconds)) s of audio")
            }
            return
        }
        feed(pcm)
    }

    func stop() {
        guard !stopped else { return }
        stopped = true
        onUpdate = nil
        continuation.finish()
        setup?.cancel()
        reading?.cancel()
        pending = []
        if let analyzer {
            Task { await analyzer.cancelAndFinishNow() }
        }
        analyzer = nil
        converter = nil
    }

    private func feed(_ pcm: Data) {
        let frames = pcm.count / 2
        guard frames > 0, let format = analyzerFormat,
              let source = AVAudioPCMBuffer(pcmFormat: inputFormat, frameCapacity: AVAudioFrameCount(frames)) else { return }
        source.frameLength = AVAudioFrameCount(frames)
        pcm.withUnsafeBytes { raw in
            guard let base = raw.baseAddress, let target = source.int16ChannelData?[0] else { return }
            memcpy(target, base, frames * 2)
        }
        let buffer: AVAudioPCMBuffer
        if let converter {
            let capacity = AVAudioFrameCount((Double(frames) * format.sampleRate / inputFormat.sampleRate).rounded(.up) + 32)
            guard let converted = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: capacity) else { return }
            let once = OneShotInput(source)
            var error: NSError?
            converter.convert(to: converted, error: &error) { _, status in once.next(status) }
            guard error == nil, converted.frameLength > 0 else { return }
            buffer = converted
        } else {
            buffer = source
        }
        let start = CMTime(value: fedFrames, timescale: CMTimeScale(format.sampleRate))
        fedFrames += AVAudioFramePosition(buffer.frameLength)
        continuation.yield(AnalyzerInput(buffer: buffer, bufferStartTime: start))
    }

    private func receive(_ text: String, isFinal: Bool, range: CMTimeRange) {
        guard !stopped else { return }
        let at = firstChunkAt.map { Date().timeIntervalSince($0) } ?? 0
        onResult?(at, isFinal, text, range.start.seconds, range.end.seconds)
        onUpdate?(accumulator.apply(text, isFinal: isFinal))
    }

    private func fail(_ reason: String) {
        guard !stopped else { return }
        log("live: \(module.rawValue) gave up: \(reason)")
        let failure = onFailure
        stop()
        failure?(reason)
    }

    nonisolated private static func describe(_ error: Error) -> String {
        let ns = error as NSError
        return "\(ns.domain) \(ns.code) \(ns.localizedDescription)"
    }
}

/// Hands `AVAudioConverter` one buffer, then says there's no more for now.
final class OneShotInput: @unchecked Sendable {
    private var buffer: AVAudioPCMBuffer?

    init(_ buffer: AVAudioPCMBuffer) {
        self.buffer = buffer
    }

    func next(_ status: UnsafeMutablePointer<AVAudioConverterInputStatus>) -> AVAudioBuffer? {
        guard let buffer else {
            status.pointee = .noDataNow
            return nil
        }
        self.buffer = nil
        status.pointee = .haveData
        return buffer
    }
}
