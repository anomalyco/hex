import AVFAudio
import Foundation
import Speech

/// The languages Apple Speech transcribes on this iPhone, shared by the app and in-keyboard
/// dictation so both pick the same language and the same downloaded model.
@available(iOS 26.0, *)
nonisolated enum AppleSpeechLanguages {
    /// The language dictation uses: Settings › Dictation language, else the keyboard's
    /// language Automatic follows (`LanguageSettings.keyboardDictationLanguage`), else
    /// Automatic. Records the result in the App Group for the keyboard layout and translation.
    static func resolve(settings: LanguageSettings = LanguageSettings()) async -> SpeechLanguage? {
        let explicit = settings.dictationLanguage ?? settings.keyboardDictationLanguage()
        let picked = await pick(explicit: explicit, settings: settings)
        if let picked, settings.resolvedDictationLanguage != picked.identifier {
            settings.resolvedDictationLanguage = picked.identifier
        }
        if explicit == nil, let picked, settings.automaticDictationLanguage != picked.identifier {
            settings.automaticDictationLanguage = picked.identifier
        }
        return picked
    }

    /// What Automatic picks on this iPhone, whatever the setting says. Records it for the
    /// keyboard's language switch.
    static func automatic(settings: LanguageSettings = LanguageSettings()) async -> SpeechLanguage? {
        let picked = await pick(explicit: nil, settings: settings)
        if let picked, settings.automaticDictationLanguage != picked.identifier {
            settings.automaticDictationLanguage = picked.identifier
        }
        return picked
    }

    private static func pick(explicit: String?, settings: LanguageSettings) async -> SpeechLanguage? {
        await SpeechLanguagePicker.pick(
            explicit: explicit,
            candidates: SpeechLanguagePicker.automaticCandidates(
                preferred: Locale.preferredLanguages,
                current: Locale.current.identifier
            ),
            speech: { locale in
                guard SpeechTranscriber.isAvailable,
                      let found = await SpeechTranscriber.supportedLocale(equivalentTo: locale) else { return nil }
                // Its model wouldn't install lately: `DictationTranscriber`'s, if it has one.
                guard !settings.speechModuleFailed(found.identifier(.bcp47)) else { return nil }
                return found
            },
            dictation: { locale in
                await DictationTranscriber.supportedLocale(equivalentTo: locale)
            },
            skip: { settings.isUnavailable($0.identifier(.bcp47)) }
        )
    }

    /// Whether neither module lists a single language here: Speech not answering (in a
    /// keyboard, say), which isn't Speech lacking the iPhone's language.
    static func listsNone() async -> Bool {
        if SpeechTranscriber.isAvailable, !(await SpeechTranscriber.supportedLocales).isEmpty {
            return false
        }
        return (await DictationTranscriber.supportedLocales).isEmpty
    }

    /// Every language either module transcribes, sorted by name (Hindi as Hinglish while that
    /// experiment is on). Records the list for the keyboard's language switch.
    static func options(
        display: Locale = .current,
        settings: LanguageSettings = LanguageSettings(),
        hinglish: HinglishSettings = HinglishSettings()
    ) async -> [SpeechLanguageOption] {
        var identifiers: [String] = []
        var seen = Set<String>()
        let speech = SpeechTranscriber.isAvailable ? await SpeechTranscriber.supportedLocales : []
        for locale in speech + (await DictationTranscriber.supportedLocales) {
            let identifier = locale.identifier(.bcp47)
            if seen.insert(identifier).inserted {
                identifiers.append(identifier)
            }
        }
        if !identifiers.isEmpty, settings.supportedDictationLanguages != identifiers {
            settings.supportedDictationLanguages = identifiers
        }
        return identifiers
            .map {
                SpeechLanguageOption(
                    id: $0,
                    name: hinglish.name(for: $0) ?? SpeechLanguagePicker.displayName(for: $0, in: display)
                )
            }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }
}

/// Apple Speech's model downloads, for the app and in-keyboard dictation alike.
@available(iOS 26.0, *)
nonisolated enum AppleSpeechAssets {
    /// Installs `language`'s model (`SpeechModelInstall`: `SpeechTranscriber`, then
    /// `DictationTranscriber`) and returns the language it installed. When neither installs,
    /// the error names what Automatic picks instead, to offer or to say it switched.
    static func install(
        _ language: SpeechLanguage,
        settings: LanguageSettings = LanguageSettings(),
        log: @escaping @Sendable (String) -> Void = { _ in },
        progress: @escaping @Sendable (Double) -> Void = { _ in }
    ) async throws -> SpeechLanguage {
        do {
            let installed = try await SpeechModelInstall.run(
                language,
                steps: steps,
                settings: settings,
                stallLimit: .seconds(60),
                log: log,
                progress: progress
            )
            settings.clearUnavailable(language.identifier)
            return installed
        } catch var unavailable as SpeechModelUnavailable {
            unavailable.automatic = settings.dictationLanguage == nil && settings.keyboardDictationLanguage() == nil
            if unavailable.reason == .notOnThisIPhone,
               let next = await AppleSpeechLanguages.automatic(settings: settings),
               next.languageCode != language.languageCode {
                unavailable.fallback = next.identifier
            }
            throw unavailable
        }
    }

    static let steps = SpeechModelInstall.Steps(
        dictationEquivalent: { locale in
            await DictationTranscriber.supportedLocale(equivalentTo: locale)
        },
        status: { language in
            switch await AssetInventory.status(forModules: [AppleSpeechModule(language).module]) {
            case .installed: .installed
            case .unsupported: .unsupported
            default: .downloadable
            }
        },
        download: { language, progress in
            let module = AppleSpeechModule(language)
            await reserve(language.locale)
            guard let request = try await AssetInventory.assetInstallationRequest(supporting: [module.module]) else {
                return
            }
            let watched = request.progress
            let monitor = Task {
                while !Task.isCancelled {
                    progress(watched.fractionCompleted)
                    try? await Task.sleep(for: .milliseconds(200))
                }
            }
            defer { monitor.cancel() }
            try await withTaskCancellationHandler {
                try await request.downloadAndInstall()
            } onCancel: {
                watched.cancel()
            }
        },
        isOnline: { await SpeechModelInstall.isOnline() }
    )

    /// Reserves `locale`'s assets, first releasing others when Hex already holds
    /// `AssetInventory.maximumReservedLocales` (each language picked in the keyboard reserves
    /// one, and the limit shrinks with free storage). Best effort: without a reservation the
    /// download still runs, or fails with its own reason.
    static func reserve(_ locale: Locale) async {
        let key = locale.identifier(.bcp47)
        var reserved = await AssetInventory.reservedLocales
        guard !reserved.contains(where: { $0.identifier(.bcp47) == key }) else { return }
        let limit = AssetInventory.maximumReservedLocales
        while reserved.count >= max(1, limit), let oldest = reserved.first {
            reserved.removeFirst()
            _ = await AssetInventory.release(reservedLocale: oldest)
        }
        _ = try? await AssetInventory.reserve(locale: locale)
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
