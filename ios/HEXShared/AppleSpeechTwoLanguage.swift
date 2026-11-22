import AVFAudio
import Foundation
import Speech

/// Experiments › Two-language dictation on Apple Speech: one recording read by two
/// `SpeechAnalyzer`s, each in a language whose `SpeechTranscriber` model is already
/// installed, then merged word by word by `TwoLanguageMerger`. Shared by the app
/// (`AppleSpeechTranscriber`) and in-keyboard dictation (`KeyboardSpeech`), so both take
/// the same pair and the same fallbacks.
@available(iOS 26.0, *)
nonisolated enum AppleSpeechTwoLanguage {
    /// The language dictation resolved and the second installed language the same recording
    /// is read in.
    struct Pair: Equatable, Sendable {
        let primary: Locale
        let secondary: Locale
    }

    /// What a two-language dictation heard.
    struct Transcript: Sendable {
        /// What Hex inserts; the primary reading's text whenever the second added nothing.
        let text: String
        /// Which reading held most of the audio, for the log only.
        let winner: TwoLanguageWinner
        /// The second language, for the log only.
        let secondaryLanguage: String
    }

    /// The pair to read a recording in, or nil to dictate exactly as before: the experiment
    /// off, Apple Speech dictating with `DictationTranscriber`, no keyboard language other
    /// than the primary, or the second language's model not installed or not reservable now.
    /// It never downloads anything for the second language and never releases another
    /// language's reservation to make room.
    static func usablePair(for language: SpeechLanguage) async -> Pair? {
        guard TwoLanguageDictationSettings().isEnabled, language.module == .speech else {
            return nil
        }
        guard let primary = await SpeechTranscriber.supportedLocale(equivalentTo: language.locale) else {
            return nil
        }
        let primaryCode = primary.language.languageCode?.identifier ?? ""
        let settings = LanguageSettings()
        // The first keyboard language that isn't the primary one, mapped to a dictation
        // locale by the same rule the keyboard-follow language uses.
        guard let keyboard = settings.keyboardLanguages.first(where: {
            $0.baseLanguage != primaryCode
        }),
        let identifier = settings.dictationLanguage(for: keyboard)
        else { return nil }
        guard let secondary = await SpeechTranscriber.supportedLocale(
            equivalentTo: Locale(identifier: identifier)
        ),
        (secondary.language.languageCode?.identifier ?? "") != primaryCode
        else { return nil }
        guard await isInstalled(secondary), await reserve(secondary) else { return nil }
        return Pair(primary: primary, secondary: secondary)
    }

    /// Reads `audioURL` twice and merges the readings. Nil means the primary reading
    /// couldn't be used, so the caller takes its single-language path and reports its own
    /// errors; a second reading that fails to start or finish leaves the primary's text.
    static func transcribe(
        _ audioURL: URL,
        pair: Pair,
        vocabulary: [String],
        lingeringModels: Bool
    ) async -> Transcript? {
        let primaryTranscriber = transcriber(for: pair.primary)
        let secondaryTranscriber = transcriber(for: pair.secondary)
        let primaryAnalyzer = analyzer(primaryTranscriber, lingeringModels: lingeringModels)
        let secondaryAnalyzer = analyzer(secondaryTranscriber, lingeringModels: lingeringModels)
        defer {
            withExtendedLifetime(primaryAnalyzer) {}
            withExtendedLifetime(secondaryAnalyzer) {}
        }

        let primaryFile: AVAudioFile
        let secondaryFile: AVAudioFile
        do {
            primaryFile = try AVAudioFile(forReading: audioURL)
            secondaryFile = try AVAudioFile(forReading: audioURL)
        } catch {
            return nil
        }

        // The same recognition hints on both readings, each with its own context.
        if !vocabulary.isEmpty {
            await setContext(vocabulary, on: primaryAnalyzer)
            await setContext(vocabulary, on: secondaryAnalyzer)
        }

        do {
            try await primaryAnalyzer.start(inputAudioFile: primaryFile, finishAfterFile: true)
        } catch {
            return nil
        }

        var secondaryStarted = false
        do {
            try await secondaryAnalyzer.start(inputAudioFile: secondaryFile, finishAfterFile: true)
            secondaryStarted = true
        } catch {
            secondaryStarted = false
        }

        async let primaryReading = reading(of: primaryTranscriber)
        async let secondaryReading = reading(of: secondaryStarted ? secondaryTranscriber : nil)

        guard let primary = try? await primaryReading, !primary.text.isEmpty else {
            return nil
        }
        let secondary = (try? await secondaryReading) ?? Reading()
        let fallback = Transcript(
            text: primary.text,
            winner: .primary,
            secondaryLanguage: pair.secondary.identifier(.bcp47)
        )
        // Nothing usable from the second or from either reading's timed words: the primary's
        // own text, as dictation would have given it alone.
        guard !secondary.words.isEmpty, !primary.words.isEmpty else { return fallback }
        let merged = TwoLanguageMerger.merge(primary: primary.words, secondary: secondary.words)
        guard !merged.text.isEmpty else { return fallback }
        return Transcript(
            text: merged.text,
            winner: merged.winner,
            secondaryLanguage: pair.secondary.identifier(.bcp47)
        )
    }

    /// The words of one Apple Speech result: each non-whitespace token of its runs, with its
    /// run's audio time range and transcription confidence. With these attribute options
    /// Apple puts one word in a run; a run that holds several still enters the merge at its
    /// tokens' shared place, punctuation attached to its word.
    static func words(in text: AttributedString) -> [TwoLanguageTimedWord] {
        var words: [TwoLanguageTimedWord] = []
        for run in text.runs {
            guard let range = run.audioTimeRange else { continue }
            let start = range.start.seconds
            let end = range.end.seconds
            guard start.isFinite, end.isFinite, end >= start else { continue }
            // A confidence Apple left out counts as neither good nor bad.
            let confidence = min(1, max(0, run.transcriptionConfidence ?? 0.5))
            let piece = String(text[run.range].characters)
            for token in piece.split(whereSeparator: { $0.isWhitespace }) {
                let word = String(token)
                guard word.contains(where: { $0.isLetter || $0.isNumber }) else { continue }
                words.append(
                    TwoLanguageTimedWord(
                        text: word,
                        start: max(0, start),
                        end: max(0, end),
                        confidence: confidence
                    )
                )
            }
        }
        return words
    }

    /// One dictation's module: `SpeechTranscriber` with the attributes the merge reads.
    private static func transcriber(for locale: Locale) -> SpeechTranscriber {
        SpeechTranscriber(
            locale: locale,
            transcriptionOptions: [],
            reportingOptions: [],
            attributeOptions: [.audioTimeRange, .transcriptionConfidence]
        )
    }

    private static func analyzer(
        _ transcriber: SpeechTranscriber,
        lingeringModels: Bool
    ) -> SpeechAnalyzer {
        SpeechAnalyzer(
            modules: [transcriber],
            options: SpeechAnalyzer.Options(
                priority: .userInitiated,
                modelRetention: lingeringModels ? .lingering : .whileInUse
            )
        )
    }

    private static func setContext(_ vocabulary: [String], on analyzer: SpeechAnalyzer) async {
        let context = AnalysisContext()
        context.contextualStrings[.general] = vocabulary
        try? await analyzer.setContext(context)
    }

    /// Final results' text and words; a transcriber that didn't start has no reading.
    private static func reading(of transcriber: SpeechTranscriber?) async throws -> Reading {
        guard let transcriber else { return Reading() }
        var reading = Reading()
        for try await result in transcriber.results where result.isFinal {
            reading.text += String(result.text.characters)
            reading.words += words(in: result.text)
        }
        reading.text = reading.text.trimmingCharacters(in: .whitespacesAndNewlines)
        return reading
    }

    private static func isInstalled(_ locale: Locale) async -> Bool {
        let module: any SpeechModule = transcriber(for: locale)
        return await AssetInventory.status(forModules: [module]) == .installed
    }

    /// Reserves the second language's assets when Apple has room, and gives up rather than
    /// releasing anyone else's: a language picked by hand still keeps its model.
    private static func reserve(_ locale: Locale) async -> Bool {
        let key = locale.identifier(.bcp47)
        let reserved = await AssetInventory.reservedLocales
        if reserved.contains(where: { $0.identifier(.bcp47) == key }) {
            return true
        }
        do {
            return try await AssetInventory.reserve(locale: locale)
        } catch {
            return false
        }
    }

    private struct Reading: Sendable {
        var text = ""
        var words: [TwoLanguageTimedWord] = []
    }
}
