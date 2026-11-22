import Foundation
import HexKeyboardCore

nonisolated enum DictationStyle: String, Codable, CaseIterable, Sendable {
    case polished
    case casual
    case notes
    case verbatim

    var label: String {
        switch self {
        case .polished: String(localized: "Polished")
        case .casual: String(localized: "Casual")
        case .notes: String(localized: "Notes")
        case .verbatim: String(localized: "Verbatim")
        }
    }

    var shortHint: String {
        switch self {
        case .polished: String(localized: "Full sentences, fillers trimmed")
        case .casual: String(localized: "Texting: lowercase start, no final period")
        case .notes: String(localized: "Short lines, a list for several items")
        case .verbatim: String(localized: "Exact words from the model")
        }
    }

    var next: DictationStyle {
        let all = Self.allCases
        guard let index = all.firstIndex(of: self) else { return .polished }
        return all[(index + 1) % all.count]
    }
}

nonisolated enum TranscriptFormatter {
    /// `text` in `style`. `language` (BCP 47) picks the fillers a style trims and the
    /// English-only rules ("I", Casual's interjections, Notes' connectives); nil is English.
    /// - Verbatim: as the speech model wrote it.
    /// - Polished: full sentences: fillers trimmed, each sentence capitalized ("I" too) and
    ///   ended, and English "gonna", "wanna", "gotta", "kinda" and "lemme" written out.
    /// - Casual: texting: fillers trimmed, sentences may start lowercase (never an acronym
    ///   or a name with inner capitals), English "i", no comma after an opening "yeah" or
    ///   "ok", and no final period on a short message (one line, up to three sentences).
    /// - Notes: concise lines: spoken "bullet" and "new line" markers, else several
    ///   sentences become a list (and a list after a colon becomes bullets), with no
    ///   opening "so", "yeah" or "and then" and no final periods.
    ///
    /// `lists` (Settings › Dictation › Format lists) writes spoken lists and line breaks as
    /// lines (`SpokenLists`) after the fillers and before the style's punctuation, in every
    /// style but Verbatim, which keeps the model's exact words. `punctuation` (Settings ›
    /// Dictation › Spoken punctuation) then writes punctuation said by name as the mark
    /// (`SpokenPunctuation`), in the same styles.
    ///
    /// `endsWithPeriod` (Settings › Dictation › End with a period) off, no line ends with a full
    /// stop ("." or "。"), whether the speech model or the style put it there: a question or an
    /// exclamation keeps its mark, and marks inside a line stay. Verbatim keeps the model's.
    ///
    /// The rules are the keyboard core's (`hk_dictation_format`, crates/keyboard/core/src/
    /// dictation/formatter.rs, casual.rs, notes.rs).
    static func format(
        _ raw: String,
        style: DictationStyle,
        language: String? = nil,
        lists: Bool = false,
        punctuation: Bool = false,
        endsWithPeriod: Bool = true
    ) -> String {
        var options = HkFormatOptions(
            style: style.coreStyle,
            lists: lists ? 1 : 0,
            punctuation: punctuation ? 1 : 0,
            ends_with_period: endsWithPeriod ? 1 : 0
        )
        return DictationCore.stage(raw, language: language) { text, language, out in
            hk_dictation_format(text, language, &options, out)
        }
    }

    /// Joins dictated text cleanly with existing text around the insertion cursor: one space
    /// between it and a word before it, and, mid-sentence, a lowercase first word
    /// (`continuingSentence`). `isWord` says whether a lowercase word is in the dictation
    /// language's dictionary; nil leaves the case alone.
    static func insertionText(
        for formatted: String,
        beforeContext: String?,
        afterContext: String?,
        isWord: ((String) -> Bool)? = nil
    ) -> String {
        DictationCore.withText(formatted) { text in
            DictationCore.withOptionalText(beforeContext) { before in
                DictationCore.withOptionalText(afterContext) { after in
                    DictationCore.withDictionary(isWord) { callback, context in
                        DictationCore.text(formatted) {
                            hk_dictation_insertion_text(text, before, after, callback, context, $0)
                        }
                    }
                }
            }
        }
    }

    /// Whether text typed before the cursor leaves the next words at a sentence start: none
    /// (or only spaces), a line break, or `.` `!` `?` `…` (closing quotes or brackets
    /// after them count too).
    static func startsSentence(after before: String?) -> Bool {
        DictationCore.withOptionalText(before) { before in
            DictationCore.flag(true) { hk_dictation_starts_sentence(before, $0) }
        }
    }

    /// German and Luxembourgish capitalize every noun, and their dictionaries accept nouns in
    /// lowercase, so a dictation there keeps its case rather than lose a noun's capital.
    static func lowercasesContinuations(in language: String) -> Bool {
        DictationCore.withLanguage(language) { language in
            DictationCore.flag(true) { hk_dictation_lowercases_continuations(language, $0) }
        }
    }

    /// Lowercases the first letter of words that continue a sentence, as typing would. Keeps
    /// "I" and its contractions, acronyms ("NASA"), words with capitals inside ("McDonald's"),
    /// and words whose lowercase form `isWord` doesn't know, which are likely names.
    static func continuingSentence(_ text: String, isWord: (String) -> Bool) -> String {
        DictationCore.withText(text) { string in
            DictationCore.withDictionary(isWord) { callback, context in
                DictationCore.text(text) { hk_dictation_continuing_sentence(string, callback, context, $0) }
            }
        }
    }

    /// The hesitation sounds a style trims, per language. English "er" is a word in
    /// Norwegian, Danish, Swedish ("er" = is) and German, and "um" is one in Portuguese, so
    /// each language trims only its own; a language without a list trims nothing. Spanish
    /// trims "eh", "em" and "mmm" (never "e", which is "and"), German "äh", "ähm" and "öhm"
    /// (never "eh", which is "anyway").
    static func fillerPattern(for language: String?) -> String? {
        DictationCore.withLanguage(language) { language in
            hk_dictation_filler_pattern(language).map { String(cString: $0) }
        }
    }

    /// `line` without the full stop that ends it ("." "。" "．" "｡", before any closing quotes or
    /// brackets). An ellipsis, an abbreviation's ("etc.") or an initialism's ("U.S.") stays.
    static func droppingFinalFullStop(_ line: String) -> String {
        DictationCore.withText(line) { text in
            DictationCore.text(line) { hk_dictation_dropping_final_full_stop(text, $0) }
        }
    }
}

private extension DictationStyle {
    /// `HkFormatOptions.style`.
    var coreStyle: UInt32 {
        switch self {
        case .polished: 0
        case .casual: 1
        case .notes: 2
        case .verbatim: 3
        }
    }
}
