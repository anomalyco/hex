import Foundation
import Speech

/// Speech Recognition access, for `OnDeviceSpeechTranscriber` alone: iOS asks for it only on
/// that path (before iOS 26), never for Apple Speech's `SpeechAnalyzer`, which runs without it
/// (App Review notes). Tests stand in for it.
nonisolated struct SpeechRecognitionAccess: Sendable {
    var status: @Sendable () -> SFSpeechRecognizerAuthorizationStatus
    var request: @Sendable () async -> SFSpeechRecognizerAuthorizationStatus

    static let system = SpeechRecognitionAccess(
        status: { SFSpeechRecognizer.authorizationStatus() },
        request: {
            await withCheckedContinuation { continuation in
                SFSpeechRecognizer.requestAuthorization { continuation.resume(returning: $0) }
            }
        }
    )
}

/// The languages `SFSpeechRecognizer` transcribes on this iPhone without a network, for
/// dictation before iOS 26: Settings › Dictation language, else the keyboard's language
/// Automatic follows, else the iPhone's languages in order, else US English.
nonisolated enum OnDeviceSpeechLanguages {
    /// The locale `SFSpeechRecognizer` lists for `identifier`: the same identifier, else the
    /// same language and region, else the same language in the script's usual region
    /// (Simplified Chinese in China, Traditional in Taiwan), else the language in its own
    /// region (`fr-FR`, US English), else the first with that language.
    static func match(_ identifier: String, in supported: [String]) -> String? {
        let wanted = Locale(identifier: identifier)
        let key = wanted.identifier(.bcp47)
        if let exact = supported.first(where: { Locale(identifier: $0).identifier(.bcp47) == key }) {
            return exact
        }
        guard let code = wanted.language.languageCode?.identifier else { return nil }
        let sameLanguage = supported
            .filter { Locale(identifier: $0).language.languageCode?.identifier == code }
            .sorted()
        guard !sameLanguage.isEmpty else { return nil }
        func region(_ id: String) -> String? { Locale(identifier: id).region?.identifier }
        if let wantedRegion = wanted.region?.identifier,
           let found = sameLanguage.first(where: { region($0) == wantedRegion }) {
            return found
        }
        if let script = wanted.language.script?.identifier,
           let usual = ["Hans": "CN", "Hant": "TW"][script],
           let found = sameLanguage.first(where: { region($0) == usual }) {
            return found
        }
        return sameLanguage.first(where: { region($0) == (code == "en" ? "US" : code.uppercased()) })
            ?? sameLanguage.first
    }

    /// The language to dictate in, as `SFSpeechRecognizer` names it, or nil when it has none
    /// of the candidates on this iPhone. An explicit choice is tried as it is; Automatic passes
    /// over a language set aside (`LanguageSettings.isUnavailable`).
    static func resolve(
        settings: LanguageSettings = LanguageSettings(),
        preferred: [String] = Locale.preferredLanguages,
        supported: [String] = supportedIdentifiers(),
        onDevice: (String) -> Bool = supportsOnDevice
    ) -> String? {
        if let explicit = settings.dictationLanguage ?? settings.keyboardDictationLanguage(preferred: preferred),
           let found = match(explicit, in: supported) {
            return found
        }
        return automatic(settings: settings, preferred: preferred, supported: supported, onDevice: onDevice)
    }

    /// What Automatic picks, whatever the setting says: the first of the iPhone's languages the
    /// recognizer has on device. An Americas Spanish candidate keeps its own region (es-AR is
    /// not the es-ES the recognizer would answer), so es-US, then es-MX, then Spain's follow.
    static func automatic(
        settings: LanguageSettings = LanguageSettings(),
        preferred: [String] = Locale.preferredLanguages,
        supported: [String] = supportedIdentifiers(),
        onDevice: (String) -> Bool = supportsOnDevice
    ) -> String? {
        let candidates = SpeechLanguagePicker.automaticCandidates(preferred: preferred, current: Locale.current.identifier)
        for candidate in candidates {
            guard let found = match(candidate, in: supported),
                  accepts(found, for: candidate),
                  !settings.isUnavailable(found) else { continue }
            if onDevice(found) { return found }
        }
        return nil
    }

    /// Whether `found` may stand for `candidate`: every Spanish candidate keeps its own
    /// region (es-CO on es-CO, not the es-ES another fallback answers), so the next candidate
    /// is tried. Only non-Spanish languages resolve loosely.
    private static func accepts(_ found: String, for candidate: String) -> Bool {
        let wanted = Locale(identifier: candidate)
        guard wanted.language.languageCode?.identifier == "es",
              let region = wanted.region?.identifier else { return true }
        return Locale(identifier: found).region?.identifier == region
    }

    static func supportedIdentifiers() -> [String] {
        SFSpeechRecognizer.supportedLocales().map { $0.identifier(.bcp47) }.sorted()
    }

    static func supportsOnDevice(_ identifier: String) -> Bool {
        SFSpeechRecognizer(locale: Locale(identifier: identifier))?.supportsOnDeviceRecognition == true
    }

    /// Settings › Dictation language before iOS 26: every language this iPhone transcribes on
    /// device, sorted by name. iOS adds one once its dictation is on the iPhone (Settings ›
    /// General › Keyboard).
    static func options(display: Locale = .current) -> [SpeechLanguageOption] {
        supportedIdentifiers()
            .filter(supportsOnDevice)
            .map { SpeechLanguageOption(id: $0, name: SpeechLanguagePicker.displayName(for: $0, in: display)) }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }
}

/// Dictation before iOS 26 on the iPhones Parakeet doesn't fit, and in every language but
/// English: Apple's `SFSpeechRecognizer`, on the iPhone only (`requiresOnDeviceRecognition`).
/// Its model runs in Apple's speech service, not in Hex, so it costs Hex no memory to load,
/// and nothing downloads in Hex: iOS keeps a language's model once its dictation is on.
actor OnDeviceSpeechTranscriber: LocalTranscribing {
    enum TranscriberError: HexDescribedError, Equatable {
        case notAllowed
        case emptyTranscript
        case unavailable

        var errorDescription: String? {
            switch self {
            case .notAllowed:
                String(localized: "Allow Speech Recognition for Hex in Settings.")
            case .emptyTranscript:
                String(localized: "No speech heard.")
            case .unavailable:
                String(localized: "Apple's speech recognition can't dictate this language on this iPhone.")
            }
        }
    }

    nonisolated let displayName = "Apple Speech"
    /// BCP 47, as `SFSpeechRecognizer` lists it.
    private let language: String
    private let access: SpeechRecognitionAccess
    /// `LanguageSettings`' suite: the App Group, another in tests.
    private let suite: String
    private var recognizer: SFSpeechRecognizer?

    private var languages: LanguageSettings {
        LanguageSettings(defaults: UserDefaults(suiteName: suite))
    }

    var languageName: String {
        SpeechModelUnavailable.interfaceName(language)
    }

    var languageIdentifier: String {
        language
    }

    init(language: String, access: SpeechRecognitionAccess = .system, suite: String = KeyboardBridge.appGroup) {
        self.language = language
        self.access = access
        self.suite = suite
    }

    /// Asks for Speech Recognition only now, as Hex gets this engine ready, saying first that
    /// it stays on the iPhone. A language this iPhone can't transcribe on device is set aside
    /// for Automatic, which moves on to the next one.
    func prepare(
        progressHandler: @escaping @Sendable (ModelPreparationProgress) -> Void
    ) async throws {
        guard recognizer == nil else { return }
        progressHandler(ModelPreparationProgress(fractionCompleted: 0.2, detail: String(localized: "Checking speech recognition")))
        var status = access.status()
        if status == .notDetermined {
            progressHandler(
                ModelPreparationProgress(
                    fractionCompleted: 0.4,
                    detail: String(localized: "Allow Speech Recognition")
                )
            )
            status = await access.request()
        }
        guard status == .authorized else { throw TranscriberError.notAllowed }

        let languages = languages
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: language)),
              recognizer.supportsOnDeviceRecognition else {
            languages.markUnavailable(language)
            var unavailable = SpeechModelUnavailable(language: language, reason: .notOnThisIPhone)
            unavailable.automatic = languages.dictationLanguage == nil && languages.keyboardDictationLanguage() == nil
            if let next = OnDeviceSpeechLanguages.resolve(settings: languages),
               Locale(identifier: next).language.languageCode != Locale(identifier: language).language.languageCode {
                unavailable.fallback = next
            }
            throw unavailable
        }
        languages.clearUnavailable(language)
        // Translate what I say follows the language dictation runs in.
        languages.resolvedDictationLanguage = language
        recognizer.queue = OperationQueue()
        self.recognizer = recognizer
        progressHandler(ModelPreparationProgress(fractionCompleted: 1, detail: String(localized: "Ready")))
    }

    func transcribe(_ audioURL: URL) async throws -> String {
        try await transcribe(audioURL, vocabulary: [])
    }

    func transcribe(_ audioURL: URL, vocabulary: [String]) async throws -> String {
        guard let recognizer, recognizer.isAvailable else { throw TranscriberError.unavailable }
        let request = SFSpeechURLRecognitionRequest(url: audioURL)
        request.requiresOnDeviceRecognition = true
        request.shouldReportPartialResults = false
        request.addsPunctuation = true
        request.taskHint = .dictation
        request.contextualStrings = vocabulary

        let once = RecognitionResume()
        let text: String = try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                once.start(continuation) {
                    recognizer.recognitionTask(with: request) { result, error in
                        if let result, result.isFinal {
                            once.finish(.success(result.bestTranscription.formattedString))
                        } else if let error {
                            once.finish(Self.isNoSpeech(error) ? .success("") : .failure(error))
                        }
                    }
                }
            }
        } onCancel: {
            once.cancel()
        }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { throw TranscriberError.emptyTranscript }
        return trimmed
    }

    /// "No speech detected" (`kAFAssistantErrorDomain` 1110), from a recording with no words.
    nonisolated static func isNoSpeech(_ error: Error) -> Bool {
        let error = error as NSError
        return error.domain == "kAFAssistantErrorDomain" && error.code == 1110
    }
}

/// Resumes one recognition's continuation once: Speech can call back again after the final
/// result, and with an error after a cancel.
private nonisolated final class RecognitionResume: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<String, Error>?
    private var task: SFSpeechRecognitionTask?
    private var cancelled = false

    func start(_ continuation: CheckedContinuation<String, Error>, task make: () -> SFSpeechRecognitionTask) {
        let cancelled = lock.withLock {
            self.continuation = continuation
            return self.cancelled
        }
        guard !cancelled else { return finish(.failure(CancellationError())) }
        let task = make()
        lock.withLock { self.task = task }
    }

    func finish(_ result: Result<String, Error>) {
        let continuation = lock.withLock {
            defer { self.continuation = nil }
            return self.continuation
        }
        continuation?.resume(with: result)
    }

    func cancel() {
        let task = lock.withLock {
            cancelled = true
            return self.task
        }
        task?.cancel()
        finish(.failure(CancellationError()))
    }
}

extension OnDeviceSpeechTranscriber.TranscriberError: NoSpeechError {
    var meansNoSpeech: Bool { self == .emptyTranscript }
}
