import AVFAudio
import Foundation
import Speech

/// The languages Apple Speech transcribes on this iPhone, shared by the app and in-keyboard
/// dictation so both pick the same language and the same downloaded model.
@available(iOS 26.0, *)
nonisolated enum AppleSpeechLanguages {
    /// The language dictation uses: Settings › Dictation language, else Automatic. Records
    /// the result in the App Group for the keyboard layout and translation.
    static func resolve(settings: LanguageSettings = LanguageSettings()) async -> SpeechLanguage? {
        let picked = await pick(explicit: settings.dictationLanguage)
        if let picked, settings.resolvedDictationLanguage != picked.identifier {
            settings.resolvedDictationLanguage = picked.identifier
        }
        return picked
    }

    /// What Automatic picks on this iPhone, whatever the setting says.
    static func automatic() async -> SpeechLanguage? {
        await pick(explicit: nil)
    }

    private static func pick(explicit: String?) async -> SpeechLanguage? {
        await SpeechLanguagePicker.pick(
            explicit: explicit,
            candidates: SpeechLanguagePicker.automaticCandidates(
                preferred: Locale.preferredLanguages,
                current: Locale.current.identifier
            ),
            speech: { locale in
                guard SpeechTranscriber.isAvailable else { return nil }
                return await SpeechTranscriber.supportedLocale(equivalentTo: locale)
            },
            dictation: { locale in
                await DictationTranscriber.supportedLocale(equivalentTo: locale)
            }
        )
    }

    /// Every language either module transcribes, sorted by name.
    static func options(display: Locale = .current) async -> [SpeechLanguageOption] {
        var identifiers: [String] = []
        var seen = Set<String>()
        let speech = SpeechTranscriber.isAvailable ? await SpeechTranscriber.supportedLocales : []
        for locale in speech + (await DictationTranscriber.supportedLocales) {
            let identifier = locale.identifier(.bcp47)
            if seen.insert(identifier).inserted {
                identifiers.append(identifier)
            }
        }
        return identifiers
            .map { SpeechLanguageOption(id: $0, name: SpeechLanguagePicker.displayName(for: $0, in: display)) }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }
}

/// One dictation's Apple Speech module: `SpeechTranscriber` or `DictationTranscriber`.
@available(iOS 26.0, *)
nonisolated enum AppleSpeechModule {
    case speech(SpeechTranscriber)
    case dictation(DictationTranscriber)

    init(_ language: SpeechLanguage) {
        switch language.module {
        case .speech:
            self = .speech(SpeechTranscriber(locale: language.locale, preset: .transcription))
        case .dictation:
            // Punctuated, without the short-form hint: dictations run up to a minute.
            self = .dictation(DictationTranscriber(locale: language.locale, preset: .longDictation))
        }
    }

    var module: any SpeechModule {
        switch self {
        case .speech(let transcriber): transcriber
        case .dictation(let transcriber): transcriber
        }
    }

    var name: String {
        switch self {
        case .speech: "SpeechTranscriber"
        case .dictation: "DictationTranscriber"
        }
    }

    /// The final results joined, once the analyzer has finished the file.
    func finalText() async throws -> String {
        var transcript = AttributedString()
        switch self {
        case .speech(let transcriber):
            for try await result in transcriber.results where result.isFinal {
                transcript += result.text
            }
        case .dictation(let transcriber):
            for try await result in transcriber.results where result.isFinal {
                transcript += result.text
            }
        }
        return String(transcript.characters).trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
