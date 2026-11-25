import AVFAudio
import Foundation
import Speech

@available(iOS 26.0, *)
enum KeyboardSpeech {
    enum Failure: HexDescribedError, NoSpeechError {
        case noLocale
        case empty

        var errorDescription: String? {
            switch self {
            case .noLocale: String(localized: "Apple's speech recognition can't dictate this language on this iPhone.")
            case .empty: String(localized: "No speech heard.")
            }
        }

        var meansNoSpeech: Bool {
            if case .empty = self { return true }
            return false
        }
    }

    /// The transcript and the language it was transcribed in.
    static func transcribe(_ url: URL, hints: [String]) async throws -> (String, SpeechLanguage) {
        let log = KeyboardProbeLog.shared
        // The same rule as the app's (Settings › Dictation language, else Automatic), so the
        // keyboard uses the assets Hex downloaded.
        let settings = LanguageSettings()
        guard let picked = await AppleSpeechLanguages.resolve(settings: settings) else {
            // Only Speech listing languages, none of them the iPhone's, means it lacks yours.
            guard await AppleSpeechLanguages.listsNone() else { throw Failure.noLocale }
            log.write("speech lists no languages in the keyboard: handing over to Hex")
            throw SpeechModelNeedsHex.handOff(settings.effectiveDictationLanguage ?? "en-US", settings: settings)
        }
        log.write("speech locale=\(picked.identifier) module=\(picked.module.rawValue) available=\(SpeechTranscriber.isAvailable)")
        // The model the app uses, else the same language on `DictationTranscriber`. One the
        // keyboard can't get (released since Hex downloaded it, or a download that failed here)
        // is Hex's to download: the Hex key opens Hex until it installs.
        let started = Date()
        let language: SpeechLanguage
        do {
            // The keyboard's failures set nothing aside for Hex (`setsAside`): they are often
            // the keyboard's own reservation, not the language.
            language = try await AppleSpeechAssets.install(picked, settings: settings, setsAside: false, log: { log.write($0) })
        } catch {
            throw SpeechModelNeedsHex.keyboardError(for: error, language: picked.identifier, settings: settings)
        }
        if language != picked || Date().timeIntervalSince(started) > 0.5 {
            log.write("speech assets ready (\(language.module.rawValue) \(language.identifier)) after \(String(format: "%.1f", Date().timeIntervalSince(started))) s \(KeyboardProbeLog.memory())")
        }
        let speech = AppleSpeechModule(language)

        // Labs › Two-language dictation: the same recording read by a second installed
        // language too, merged word by word; the pipeline still runs as the primary language.
        let twoLanguage = TwoLanguageDictationSettings()
        if twoLanguage.isEnabled,
           let pair = await AppleSpeechTwoLanguage.usablePair(for: language),
           let merged = await AppleSpeechTwoLanguage.transcribe(
               url,
               pair: pair,
               vocabulary: hints,
               lingeringModels: false
           ) {
            guard !merged.text.isEmpty else { throw Failure.empty }
            log.write("two-language \(pair.secondary.identifier(.bcp47)) won=\(merged.winner.rawValue) \(KeyboardProbeLog.memory())")
            return (merged.text, language)
        }

        let analyzer = SpeechAnalyzer(
            modules: [speech.module],
            options: SpeechAnalyzer.Options(priority: .userInitiated, modelRetention: .whileInUse)
        )
        let file = try AVAudioFile(forReading: url)
        // Custom vocabulary and Learn how I type's meant words, best effort, as the app does.
        if !hints.isEmpty {
            let context = AnalysisContext()
            context.contextualStrings[.general] = hints
            try? await analyzer.setContext(context)
        }
        try await analyzer.start(inputAudioFile: file, finishAfterFile: true)
        log.write("analyzer started \(KeyboardProbeLog.memory())")

        let text = try await speech.finalText()
        guard !text.isEmpty else { throw Failure.empty }
        return (text, language)
    }
}
