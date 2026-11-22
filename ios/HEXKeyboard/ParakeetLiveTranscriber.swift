import AVFAudio
import Foundation

/// Live words from the keyboard's own speech model, which can't stream: the audio so far is
/// transcribed again about once a second (Parakeet V2 on the Neural Engine reads 15 s in
/// about 0.15 s and a minute in 0.25–0.5 s on an iPhone 17 Pro). Words two passes agree on,
/// short of the last two, are settled; the rest stays tentative. Once a pass covers more than
/// `windowSeconds`, the words ending well before its end are kept as they are and later
/// passes start after them, so a pass never grows past the window. Not yet used:
/// `KeyboardLiveDictation` takes it once the speech model is the keyboard's engine; until
/// then `transcribe` is the model's pass over 16 kHz samples (`KeyboardParakeet`), nil when
/// it can't run one now (another pass or the dictation's own transcription running, short
/// of memory).
@MainActor
final class ParakeetLiveTranscriber: LiveTranscriptSource {
    struct Pass {
        let text: String
        /// Each word's end in the samples passed, when the model gives token timings.
        let wordEnds: [Double]?
    }

    var onUpdate: ((LiveTranscript) -> Void)?
    var onFailure: ((String) -> Void)?

    private let transcribe: ([Float]) async -> Pass?
    private let inputFormat: AVAudioFormat
    private let modelFormat: AVAudioFormat
    private let converter: AVAudioConverter?
    private var samples: [Float] = []
    /// Where the window starts in `samples`; the words before it are `frozen`.
    private var windowStart = 0
    private var frozen = ""
    private var previousWords: [String] = []
    private var settledCount = 0
    private var samplesAtLastPass = 0
    private var pass: Task<Void, Never>?
    private var stopped = false

    static let sampleRate: Double = 16_000
    /// New audio before another pass.
    static let interval: Double = 1.0
    static let windowSeconds: Double = 20
    /// Words ending this close to a pass's end are never frozen.
    static let keepTentative: Double = 5

    init?(sampleRate: Double, transcribe: @escaping ([Float]) async -> Pass?) {
        guard let input = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: sampleRate, channels: 1, interleaved: true),
              let model = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: Self.sampleRate, channels: 1, interleaved: false) else {
            return nil
        }
        inputFormat = input
        modelFormat = model
        converter = AVAudioConverter(from: input, to: model)
        self.transcribe = transcribe
    }

    func append(_ pcm: Data) {
        guard !stopped, let converter else { return }
        let frames = pcm.count / 2
        guard frames > 0, let source = AVAudioPCMBuffer(pcmFormat: inputFormat, frameCapacity: AVAudioFrameCount(frames)) else { return }
        source.frameLength = AVAudioFrameCount(frames)
        pcm.withUnsafeBytes { raw in
            guard let base = raw.baseAddress, let target = source.int16ChannelData?[0] else { return }
            memcpy(target, base, frames * 2)
        }
        let capacity = AVAudioFrameCount((Double(frames) * Self.sampleRate / inputFormat.sampleRate).rounded(.up) + 32)
        guard let converted = AVAudioPCMBuffer(pcmFormat: modelFormat, frameCapacity: capacity) else { return }
        let once = OneShotInput(source)
        var error: NSError?
        converter.convert(to: converted, error: &error) { _, status in once.next(status) }
        guard error == nil, let channel = converted.floatChannelData?[0] else { return }
        samples.append(contentsOf: UnsafeBufferPointer(start: channel, count: Int(converted.frameLength)))
        runPassIfDue()
    }

    func stop() {
        stopped = true
        onUpdate = nil
        pass?.cancel()
        pass = nil
        samples = []
    }

    private func runPassIfDue() {
        guard pass == nil, Double(samples.count - samplesAtLastPass) >= Self.interval * Self.sampleRate else { return }
        samplesAtLastPass = samples.count
        let window = Array(samples[windowStart...])
        pass = Task { [weak self] in
            let result = await self?.transcribe(window)
            guard let self, !self.stopped else { return }
            self.pass = nil
            if let result { self.take(result, windowSamples: window.count) }
            self.runPassIfDue()
        }
    }

    private func take(_ result: Pass, windowSamples: Int) {
        let words = result.text.split(whereSeparator: \.isWhitespace).map(String.init)
        var agreed = 0
        while agreed < min(words.count, previousWords.count), words[agreed] == previousWords[agreed] { agreed += 1 }
        settledCount = min(words.count, max(settledCount, agreed - 2))
        previousWords = words
        let settled = (frozen.isEmpty ? [] : [frozen]) + words.prefix(settledCount)
        let tentative = words.dropFirst(settledCount)
        onUpdate?(LiveTranscript(settled: settled.joined(separator: " "), tentative: tentative.joined(separator: " ")))
        freezeIfLong(words: words, ends: result.wordEnds, windowSamples: windowSamples)
    }

    /// Past the window, the settled words that end `keepTentative` before the pass's end are
    /// kept and the next window starts after the last of them.
    private func freezeIfLong(words: [String], ends: [Double]?, windowSamples: Int) {
        let seconds = Double(windowSamples) / Self.sampleRate
        guard seconds > Self.windowSeconds, let ends, ends.count == words.count else { return }
        var count = 0
        while count < settledCount, ends[count] < seconds - Self.keepTentative { count += 1 }
        guard count > 0 else { return }
        frozen = ([frozen] + words.prefix(count)).filter { !$0.isEmpty }.joined(separator: " ")
        windowStart += Int(ends[count - 1] * Self.sampleRate)
        previousWords = Array(previousWords.dropFirst(count))
        settledCount -= count
    }
}
