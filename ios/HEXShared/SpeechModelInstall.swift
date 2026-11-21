import Foundation
import Network

/// A language Hex can't get Apple's speech model for on this iPhone. Its message is written
/// for people, in Hex's interface language, so it names languages in that language.
nonisolated struct SpeechModelUnavailable: HexDescribedError, Equatable {
    enum Reason: Equatable, Sendable {
        /// Neither Apple Speech module installs it here, though the iPhone is online.
        case notOnThisIPhone
        /// The download failed or stalled while offline or on a bad connection.
        case downloadFailed
    }

    /// BCP 47, such as `ru-RU`.
    let language: String
    let reason: Reason
    /// What Automatic picks now that this language is set aside (BCP 47), when it's another one.
    var fallback: String?
    /// Whether dictation follows Automatic, which moves on to `fallback` by itself.
    var automatic: Bool

    init(language: String, reason: Reason, fallback: String? = nil, automatic: Bool = true) {
        self.language = language
        self.reason = reason
        self.fallback = fallback
        self.automatic = automatic
    }

    /// Automatic has already moved on to another language for the next dictations.
    var switchedAutomatically: Bool {
        reason == .notOnThisIPhone && automatic && fallback != nil
    }

    /// Short enough for the full keyboard's strip (about 60 characters).
    var errorDescription: String? {
        let name = Self.interfaceName(language)
        switch reason {
        case .downloadFailed:
            return String(localized: "\(name) speech didn't download. Check your connection.")
        case .notOnThisIPhone:
            guard let fallback else {
                return String(localized: "\(name) isn't available for dictation on this iPhone yet.")
            }
            let other = Self.interfaceName(fallback)
            return automatic
                ? String(localized: "\(name) isn't available on this iPhone yet. Using \(other).")
                : String(localized: "\(name) isn't available on this iPhone yet. Try \(other).")
        }
    }

    /// "Russian", "Norwegian Bokmål": the language alone, in Hex's interface language.
    static func interfaceName(_ identifier: String) -> String {
        let code = Locale(identifier: identifier).language.languageCode?.identifier ?? identifier
        let name = Locale.hexInterface.localizedString(forLanguageCode: code) ?? identifier
        return name.prefix(1).uppercased() + name.dropFirst()
    }
}

/// In-keyboard dictation couldn't get its language's speech model ready itself: an asset
/// released or removed since Hex downloaded it (each process holds its own reservation, and an
/// unreserved one reads as not installed), a download the keyboard couldn't finish, or Speech
/// listing nothing in the keyboard. Hex downloads it in the foreground, so the Hex key opens
/// Hex until a model installs (`LanguageSettings.keyboardNeedsHex`). Never "Apple doesn't
/// have your language": Speech listed the language, or listed nothing at all.
nonisolated struct SpeechModelNeedsHex: HexDescribedError, Equatable {
    /// BCP 47, such as `en-US`.
    let language: String

    /// Short enough for the full keyboard's strip (about 60 characters).
    var errorDescription: String? {
        String(localized: "Open Hex to download \(SpeechModelUnavailable.interfaceName(language)).")
    }

    /// Hands the keyboard's dictation to Hex for `language` and says so.
    static func handOff(_ language: String, settings: LanguageSettings) -> SpeechModelNeedsHex {
        settings.markKeyboardNeedsHex()
        return SpeechModelNeedsHex(language: language)
    }

    /// What the keyboard says when installing `language`'s model failed there. Automatic
    /// moving to another language, or a choice that can't be had with one to try instead,
    /// keeps `SpeechModelUnavailable`'s sentence: the next dictation works without Hex.
    /// Anything else is Hex's to download (`handOff`).
    static func keyboardError(for error: Error, language: String, settings: LanguageSettings) -> Error {
        if error is CancellationError { return error }
        if let unavailable = error as? SpeechModelUnavailable, unavailable.fallback != nil {
            return unavailable
        }
        return handOff(language, settings: settings)
    }
}

/// How Hex gets a language's speech model onto the iPhone, shared by the app and in-keyboard
/// dictation: the module the picker chose, then the same language on `DictationTranscriber`
/// when `SpeechTranscriber`'s won't install (Apple lists some languages whose
/// `SpeechTranscriber` asset never arrives: "transcription.ar asset not found after attempted
/// download"). A download whose progress stops moving is given up on rather than awaited.
/// When nothing installs while the iPhone is online, the language is set aside for Automatic
/// (`LanguageSettings.markUnavailable`) and the error says so plainly.
nonisolated enum SpeechModelInstall {
    enum Availability: Equatable, Sendable {
        case installed
        case downloadable
        case unsupported
    }

    /// The Apple Speech calls, swapped out in tests.
    struct Steps: Sendable {
        /// The same language on `DictationTranscriber`, or nil when it doesn't have it.
        var dictationEquivalent: @Sendable (Locale) async -> Locale?
        var status: @Sendable (SpeechLanguage) async -> Availability
        /// Downloads and installs, reporting a fraction in 0...1.
        var download: @Sendable (SpeechLanguage, @escaping @Sendable (Double) -> Void) async throws -> Void
        var isOnline: @Sendable () async -> Bool
    }

    /// How long a download may sit at the same fraction before Hex stops waiting on it.
    static let stallLimit: Duration = .seconds(45)

    /// Installs `language`'s model, or the same language's on `DictationTranscriber`, and
    /// returns the language it installed. Throws `SpeechModelUnavailable` when neither does.
    static func run(
        _ language: SpeechLanguage,
        steps: Steps,
        settings: LanguageSettings,
        stallLimit: Duration = Self.stallLimit,
        log: @Sendable (String) -> Void = { _ in },
        progress: @escaping @Sendable (Double) -> Void = { _ in }
    ) async throws -> SpeechLanguage {
        var attempts = [language]
        if language.module == .speech, let equivalent = await steps.dictationEquivalent(language.locale) {
            attempts.append(SpeechLanguage(locale: equivalent, module: .dictation))
        }
        var downloadable = false
        var stalled = false
        // A model in place ends the keyboard's handoff to Hex, whichever process installed it.
        func installed(_ attempt: SpeechLanguage) -> SpeechLanguage {
            settings.clearKeyboardNeedsHex()
            return attempt
        }
        for attempt in attempts {
            try Task.checkCancellation()
            switch await steps.status(attempt) {
            case .installed:
                return installed(attempt)
            case .unsupported:
                log("speech \(attempt.identifier) \(attempt.module.rawValue): unsupported")
                continue
            case .downloadable:
                downloadable = true
            }
            do {
                let fraction = ProgressReading()
                try await StallWatch.run(limit: stallLimit, progress: { fraction.value }) {
                    try await steps.download(attempt) { value in
                        fraction.value = value
                        progress(value)
                    }
                }
                if await steps.status(attempt) == .installed {
                    return installed(attempt)
                }
                log("speech \(attempt.identifier) \(attempt.module.rawValue): not installed after download")
            } catch is CancellationError {
                throw CancellationError()
            } catch {
                if error is StallWatch.Stalled { stalled = true }
                log("speech \(attempt.identifier) \(attempt.module.rawValue): \(Self.describe(error))")
            }
            if attempt.module == .speech, attempts.count > 1 {
                // Try DictationTranscriber next, and go straight to it for a while.
                settings.markSpeechModuleFailed(attempt.identifier)
            }
        }
        if downloadable {
            let online = await steps.isOnline()
            if stalled || !online {
                throw SpeechModelUnavailable(language: language.identifier, reason: .downloadFailed)
            }
        }
        settings.markUnavailable(language.identifier)
        throw SpeechModelUnavailable(language: language.identifier, reason: .notOnThisIPhone)
    }

    /// Whether the iPhone has a usable network path right now.
    static func isOnline() async -> Bool {
        let monitor = NWPathMonitor()
        defer { monitor.cancel() }
        let status = await withCheckedContinuation { (continuation: CheckedContinuation<NWPath.Status, Never>) in
            let once = OnceFlag()
            monitor.pathUpdateHandler = { path in
                if once.claim() { continuation.resume(returning: path.status) }
            }
            monitor.start(queue: DispatchQueue(label: "anomaly.hex.speech.path"))
        }
        return status == .satisfied
    }

    private static func describe(_ error: Error) -> String {
        let ns = error as NSError
        return "\(ns.domain) \(ns.code) \(ns.localizedDescription)"
    }
}

/// Ends an operation whose progress stopped moving instead of waiting on it forever: after
/// `limit` at the same value it cancels the operation and throws `Stalled` without waiting
/// for it to notice. A nil reading pauses the watch (a step that reports nothing, such as
/// compiling a model, which can't be cancelled anyway).
nonisolated enum StallWatch {
    struct Stalled: HexDescribedError {
        var errorDescription: String? { String(localized: "The download stopped. Check your connection and try again.") }
    }

    static func run<T: Sendable>(
        limit: Duration,
        poll: Duration = .milliseconds(250),
        progress: @escaping @Sendable () -> Double?,
        operation: @escaping @Sendable () async throws -> T
    ) async throws -> T {
        let outcome = StallOutcome<T>()
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                outcome.attach(continuation)
                let work = Task {
                    do {
                        outcome.finish(.success(try await operation()))
                    } catch {
                        outcome.finish(.failure(error))
                    }
                }
                outcome.attach(work)
                Task {
                    let clock = ContinuousClock()
                    var last = progress()
                    var since = clock.now
                    while !outcome.isFinished {
                        try? await Task.sleep(for: poll)
                        let now = progress()
                        if now == nil || now != last {
                            last = now
                            since = clock.now
                        } else if clock.now - since >= limit {
                            work.cancel()
                            outcome.finish(.failure(Stalled()))
                            return
                        }
                    }
                }
            }
        } onCancel: {
            outcome.cancel()
        }
    }
}

/// The first result `StallWatch` gets wins; later ones are dropped.
private nonisolated final class StallOutcome<T: Sendable>: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<T, Error>?
    private var pending: Result<T, Error>?
    private var done = false
    private var work: Task<Void, Never>?

    var isFinished: Bool { lock.withLock { done } }

    func attach(_ continuation: CheckedContinuation<T, Error>) {
        let ready: Result<T, Error>? = lock.withLock {
            if let pending {
                self.pending = nil
                return pending
            }
            self.continuation = continuation
            return nil
        }
        if let ready { continuation.resume(with: ready) }
    }

    func attach(_ work: Task<Void, Never>) {
        let finished = lock.withLock {
            self.work = work
            return done
        }
        if finished { work.cancel() }
    }

    func finish(_ result: Result<T, Error>) {
        let target: CheckedContinuation<T, Error>? = lock.withLock {
            guard !done else { return nil }
            done = true
            guard let continuation else {
                pending = result
                return nil
            }
            self.continuation = nil
            return continuation
        }
        target?.resume(with: result)
    }

    func cancel() {
        let running = lock.withLock { work }
        running?.cancel()
        finish(.failure(CancellationError()))
    }
}

/// A progress fraction written by one task and read by another.
nonisolated final class ProgressReading: @unchecked Sendable {
    private let lock = NSLock()
    private var current: Double = 0

    var value: Double {
        get { lock.withLock { current } }
        set { lock.withLock { current = newValue } }
    }
}

private nonisolated final class OnceFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var claimed = false

    func claim() -> Bool {
        lock.withLock {
            guard !claimed else { return false }
            claimed = true
            return true
        }
    }
}
