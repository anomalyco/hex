import AVFAudio
import Foundation
import Speech

/// Experiments › Two-language dictation on Apple Speech: one recording read by two
/// `SpeechAnalyzer`s, each in a language whose `SpeechTranscriber` model is installed, then
/// merged word by word by `TwoLanguageMerger`. Shared by the app
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
        /// What Hex inserts; empty means the primary reading finished without hearing
        /// anything, so the caller throws its own no-speech error.
        let text: String
        /// Which reading held most of the audio, for the log only.
        let winner: TwoLanguageWinner
        /// The second language, for the log only.
        let secondaryLanguage: String
    }

    /// The pair to read a recording in, or nil to dictate exactly as before: Apple Speech
    /// dictating with `DictationTranscriber`, no keyboard language other than the primary,
    /// or the second language's `SpeechTranscriber` model not installed. It never downloads
    /// anything for the second language.
    static func usablePair(for language: SpeechLanguage) async -> Pair? {
        guard language.module == .speech else { return nil }
        guard let primary = await SpeechTranscriber.supportedLocale(equivalentTo: language.locale) else {
            return nil
        }
        let primaryCode = primary.language.languageCode?.identifier ?? ""
        let settings = LanguageSettings()
        // The first keyboard language that isn't the primary one, mapped to a dictation
        // locale by the same rule the keyboard-follow language uses. One whose model
        // wouldn't install lately is set aside, as Automatic sets it aside.
        for keyboard in settings.keyboardLanguages where keyboard.baseLanguage != primaryCode {
            guard let identifier = settings.dictationLanguage(for: keyboard) else { continue }
            let wanted = Locale(identifier: identifier)
            guard !settings.speechModuleFailed(wanted.identifier(.bcp47)) else { continue }
            guard let secondary = await SpeechTranscriber.supportedLocale(equivalentTo: wanted),
                  (secondary.language.languageCode?.identifier ?? "") != primaryCode else {
                continue
            }
            guard await isInstalled(secondary) else { continue }
            return Pair(primary: primary, secondary: secondary)
        }
        return nil
    }

    /// Reads `audioURL` twice and merges the readings. Nil means the primary reading failed,
    /// so the caller takes its single-language path and reports its own errors. Empty text
    /// means the primary finished but heard nothing, so the caller throws its no-speech
    /// error without running the recording again. A second reading that fails leaves the
    /// primary's text, and so does one that never wins a piece.
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

        // Prepare both before starting, as the app prepares its analyzer. A second language
        // that can't prepare leaves the primary reading alone.
        do {
            try await primaryAnalyzer.prepareToAnalyze(in: nil)
        } catch {
            return nil
        }
        var secondaryStarted = false
        do {
            try await secondaryAnalyzer.prepareToAnalyze(in: nil)
            secondaryStarted = true
        } catch {
            secondaryStarted = false
        }

        // The same recognition hints on both readings, each with its own context.
        if !vocabulary.isEmpty {
            await setContext(vocabulary, on: primaryAnalyzer)
            if secondaryStarted {
                await setContext(vocabulary, on: secondaryAnalyzer)
            }
        }

        do {
            try await primaryAnalyzer.start(inputAudioFile: primaryFile, finishAfterFile: true)
        } catch {
            return nil
        }
        if secondaryStarted {
            do {
                try await secondaryAnalyzer.start(inputAudioFile: secondaryFile, finishAfterFile: true)
            } catch {
                secondaryStarted = false
            }
        }

        async let primaryReading = reading(of: primaryTranscriber)
        async let secondaryReading = reading(of: secondaryStarted ? secondaryTranscriber : nil)

        let primary: Reading
        do {
            primary = try await primaryReading
        } catch {
            return nil
        }
        // The primary finished but heard nothing: say so instead of making the caller run
        // the whole single-language path again.
        guard !primary.text.isEmpty else {
            return Transcript(
                text: "",
                winner: .primary,
                secondaryLanguage: pair.secondary.identifier(.bcp47)
            )
        }
        let secondary = (try? await secondaryReading) ?? Reading()
        let fallback = Transcript(
            text: primary.text,
            winner: .primary,
            secondaryLanguage: pair.secondary.identifier(.bcp47)
        )
        // Nothing usable from the second or from either reading's timed words: the primary's
        // own text, exactly as dictation would have given it alone.
        guard !secondary.words.isEmpty, !primary.words.isEmpty else { return fallback }
        let merged = TwoLanguageMerger.merge(primary: primary.words, secondary: secondary.words)
        guard merged.usedSecondary, !merged.text.isEmpty else { return fallback }
        return Transcript(
            text: merged.text,
            winner: merged.winner,
            secondaryLanguage: pair.secondary.identifier(.bcp47)
        )
    }

    /// The words of one Apple Speech result: one per timed run, with the run's raw characters
    /// (its leading whitespace included) and its audio time range and transcription
    /// confidence. A run with no letters or numbers, and any run without a usable time range,
    /// keeps its characters on the word before it; such a run before the first word waits
    /// for it.
    static func words(in text: AttributedString) -> [TwoLanguageTimedWord] {
        var words: [TwoLanguageTimedWord] = []
        var prefix = ""
        for run in text.runs {
            let characters = String(text[run.range].characters)
            guard !characters.isEmpty else { continue }
            let hasLetterOrNumber = characters.contains { $0.isLetter || $0.isNumber }
            if let range = run.audioTimeRange {
                let start = range.start.seconds
                let end = range.end.seconds
                if start.isFinite, end.isFinite, end >= start {
                    let confidence = min(1, max(0, run.transcriptionConfidence ?? 0.5))
                    if hasLetterOrNumber {
                        words.append(
                            TwoLanguageTimedWord(
                                text: prefix + characters,
                                start: max(0, start),
                                end: max(0, end),
                                confidence: confidence
                            )
                        )
                        prefix = ""
                    } else if !words.isEmpty {
                        words[words.count - 1] = words[words.count - 1].appending(
                            characters,
                            endingAt: max(0, end)
                        )
                    } else {
                        prefix += characters
                    }
                    continue
                }
            }
            // No time range, or one that can't be placed: keep the characters on the word
            // before it, or wait for the first one.
            if !words.isEmpty {
                words[words.count - 1] = words[words.count - 1].appending(characters, endingAt: nil)
            } else {
                prefix += characters
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

    private struct Reading: Sendable {
        var text = ""
        var words: [TwoLanguageTimedWord] = []
    }
}
