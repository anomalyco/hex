import Foundation

/// A dictation with nothing in it ends quietly, with no error: Done before speaking, a
/// capture too short to hold a word, a transcript with no words, or a cancel.
nonisolated enum EmptyCapture {
    /// Shorter than this is a tap, not speech (the app discards warm captures the same way).
    static let minimumDuration: TimeInterval = 0.30

    static func isTooShort(frames: Int64, sampleRate: Double) -> Bool {
        guard sampleRate > 0, frames > 0 else { return true }
        return Double(frames) / sampleRate < minimumDuration
    }

    /// Errors that mean nothing was said, or that the person stopped it.
    static func endsQuietly(_ error: Error) -> Bool {
        if error is CancellationError { return true }
        return (error as? NoSpeechError)?.meansNoSpeech ?? false
    }
}

/// A speech engine's error, which can say it heard no words.
protocol NoSpeechError: Error {
    var meansNoSpeech: Bool { get }
}
