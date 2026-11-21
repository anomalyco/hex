import AVFAudio
import Foundation

/// An error Hex wrote for people: its description is shown as it is. Every other error's
/// description (Apple's, Foundation's, a package's) is for logs, however readable it looks:
/// Speech's "transcription.ru asset not found after attempted download." reads as a sentence.
protocol HexDescribedError: LocalizedError {}

extension Locale {
    /// The language Hex's own words are in: the app's when the iPhone's language is one Hex
    /// is translated into, else English. The keyboard shares the app's String Catalog, so it
    /// follows the same language (its own labels are mostly English still).
    static var hexInterface: Locale {
        Locale(identifier: Bundle.main.preferredLocalizations.first ?? "en")
    }
}

/// Short messages for the errors Hex shows people, in the app and the keyboard. Never a
/// raw OSStatus, error domain, asset name, or Swift type name: those belong in logs.
nonisolated enum UserFacingError {
    enum Context {
        case microphone
        case transcription
        case model

        var fallback: String {
            switch self {
            case .microphone: String(localized: "Hex couldn't start the microphone. Try again.")
            case .transcription: String(localized: "Hex couldn't transcribe that. Try again.")
            case .model: String(localized: "Hex couldn't get speech recognition ready. Try again.")
            }
        }
    }

    static var offline: String {
        String(localized: "Hex couldn't download the speech model. Check your connection and try again.")
    }

    /// Audio session errors get their own message, Hex's own errors (`HexDescribedError`)
    /// their description, a lost connection while getting the model ready says so, and
    /// anything else gets the context's fallback. An allowlist, not a filter: #2035 kept any
    /// description without an error code in it, which let Apple Speech's asset errors through
    /// to the keyboard. Microphone errors never show a description.
    static func message(for error: Error, context: Context) -> String {
        if let code = audioSessionCode(of: error) {
            return message(for: code) ?? context.fallback
        }
        guard context != .microphone else { return context.fallback }
        if error is HexDescribedError,
           let text = (error as? LocalizedError)?.errorDescription?.trimmingCharacters(in: .whitespacesAndNewlines),
           !text.isEmpty {
            return text
        }
        if context == .model, isConnectionError(error) {
            return offline
        }
        return context.fallback
    }

    /// A URL loading error, itself or underneath another.
    static func isConnectionError(_ error: Error) -> Bool {
        var current: NSError? = error as NSError
        for _ in 0..<4 {
            guard let ns = current else { return false }
            if ns.domain == NSURLErrorDomain { return true }
            current = ns.userInfo[NSUnderlyingErrorKey] as? NSError
        }
        return false
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
            String(localized: "Another app is using audio. Try again.")
        case .insufficientPriority:
            String(localized: "Another app is using the microphone. Try again.")
        case .siriIsRecording:
            String(localized: "Siri is using the microphone. Try again.")
        case .isBusy:
            String(localized: "The microphone is busy. Try again.")
        case .cannotStartRecording, .cannotStartPlaying:
            String(localized: "Open Hex, then try again.")
        case .mediaServicesFailed:
            String(localized: "Audio restarted on this iPhone. Try again.")
        case .resourceNotAvailable:
            String(localized: "No microphone is available right now.")
        default:
            nil
        }
    }
}
