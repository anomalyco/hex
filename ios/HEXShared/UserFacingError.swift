import AVFAudio
import Foundation

/// Short messages for the errors Hex shows people, in the app and the keyboard. Never a
/// raw OSStatus, error domain, or Swift type name: those belong in logs.
nonisolated enum UserFacingError {
    enum Context {
        case microphone
        case transcription
        case model

        var fallback: String {
            switch self {
            case .microphone: "Hex couldn't start the microphone. Try again."
            case .transcription: "Hex couldn't transcribe that. Try again."
            case .model: "Hex couldn't get speech recognition ready. Try again."
            }
        }
    }

    /// Audio session errors get their own message; anything else keeps its description
    /// only when it reads as a sentence, and the context's fallback otherwise. Microphone
    /// errors never show a description: AVFoundation's are codes, not sentences.
    static func message(for error: Error, context: Context) -> String {
        if let code = audioSessionCode(of: error) {
            return message(for: code) ?? context.fallback
        }
        guard context != .microphone else { return context.fallback }
        let text = error.localizedDescription.trimmingCharacters(in: .whitespacesAndNewlines)
        return isReadable(text) ? text : context.fallback
    }

    /// The `AVAudioSession` error an error carries (they arrive as `NSOSStatusErrorDomain`).
    static func audioSessionCode(of error: Error) -> AVAudioSession.ErrorCode? {
        let ns = error as NSError
        guard ns.domain == NSOSStatusErrorDomain else { return nil }
        return AVAudioSession.ErrorCode(rawValue: ns.code)
    }

    static func message(for code: AVAudioSession.ErrorCode) -> String? {
        switch code {
        case .cannotInterruptOthers:
            "Another app is using audio. Try again."
        case .insufficientPriority:
            "Another app is using the microphone. Try again."
        case .siriIsRecording:
            "Siri is using the microphone. Try again."
        case .isBusy:
            "The microphone is busy. Try again."
        case .cannotStartRecording, .cannotStartPlaying:
            "Open Hex, then try again."
        case .mediaServicesFailed:
            "Audio restarted on this iPhone. Try again."
        case .resourceNotAvailable:
            "No microphone is available right now."
        default:
            nil
        }
    }

    /// Foundation's stand-in for an error without a description ("The operation couldn't
    /// be completed. (OSStatus error 560557684.)") and Swift's type names are not sentences.
    private static func isReadable(_ text: String) -> Bool {
        guard !text.isEmpty else { return false }
        let markers = ["OSStatus", "couldn’t be completed", "couldn't be completed", "unknown context", "Error Domain="]
        return !markers.contains { text.localizedCaseInsensitiveContains($0) }
    }
}
