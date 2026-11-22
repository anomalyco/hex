import Foundation
import XCTest

/// Exports the dictation rules' golden cases for the Rust port's parity test
/// (crates/keyboard/core/tests/dictation_parity.rs), from the oracle copies of today's Swift
/// in this folder. Off unless asked: xcodebuild passes `TEST_RUNNER_`-prefixed variables to
/// the test with the prefix stripped.
///
///     TEST_RUNNER_HEX_DICTATION_PARITY_OUT=/tmp/dictation-parity.jsonl \
///     TEST_RUNNER_HEX_DICTATION_PARITY_COUNT=150000 \
///     TEST_RUNNER_HEX_DICTATION_UNICODE_OUT=/tmp/dictation-unicode.tsv \
///     xcodebuild test … -only-testing:HEXMobileTests/DictationParityTests
///
/// `TEST_RUNNER_HEX_DICTATION_PARITY_SEED` picks other generated transcripts.
///
/// The committed fixture is `COUNT=600` (the default): each test string once, through one
/// operation with settings picked for it, plus 600 generated transcripts. From
/// `COUNT=100000` every test string runs through every style, setting and language. One JSON
/// object per line: the operation, its inputs (a language as its identifier plus the code and
/// region `Locale` reads from it) and `out`.
final class DictationParityTests: XCTestCase {
    func testExportDictationParityCases() throws {
        let environment = ProcessInfo.processInfo.environment
        if let path = environment["HEX_DICTATION_UNICODE_OUT"] {
            try ParityUnicode.export(to: path)
        }
        guard let path = environment["HEX_DICTATION_PARITY_OUT"] else {
            throw XCTSkip("Set TEST_RUNNER_HEX_DICTATION_PARITY_OUT to export the dictation parity cases.")
        }
        let count = environment["HEX_DICTATION_PARITY_COUNT"].flatMap(Int.init) ?? 600
        var writer = try ParityWriter(path: path)
        var cases = ParityCases(writer: writer)
        cases.corpus(full: count >= 100_000)
        let seed = environment["HEX_DICTATION_PARITY_SEED"].flatMap { UInt64($0) } ?? 0x4845_5844_4943_5441
        var random = DictationRandom(seed: seed)
        for _ in 0..<count {
            cases.generated(&random)
        }
        writer = cases.writer
        try writer.close()
        print("dictation parity: \(writer.lines) cases in \(path)")
    }
}

// MARK: Cases

struct ParityLanguage {
    let identifier: String?

    var json: [String: Any] {
        guard let identifier else { return [:] }
        let locale = Locale(identifier: identifier)
        var fields: [String: Any] = ["lang": identifier]
        if let code = locale.language.languageCode?.identifier { fields["code"] = code }
        if let region = locale.region?.identifier { fields["region"] = region }
        return fields
    }

    static let all: [ParityLanguage] = ([nil] + [
        "en-US", "en", "en-IN", "en_GB", "es-ES", "es-419", "es-US", "fr-FR", "fr-CA", "fr-CH", "fr-BE", "fr",
        "fr-Latn-CA", "de-DE", "de-AT", "de_CH", "it-IT", "pt-BR", "pt-PT", "nb-NO", "nn", "no", "da-DK", "sv-SE",
        "lb", "zh-CN", "ja-JP", "uk-UA", "pl-PL", "cs-CZ", "el-GR", "hi-IN", "", "eng", "fra", "-en", "iw", "x",
    ]).map(ParityLanguage.init)

    /// The languages each rule has a table for, and a few without.
    static let main: [ParityLanguage] = [nil, "en-US", "es-ES", "fr-FR", "fr-CA", "fr-CH", "de-DE", "it-IT", "pt-BR",
                                         "nb-NO", "da-DK", "zh-CN", "uk-UA"].map(ParityLanguage.init)
}

struct ParityCases {
    var writer: ParityWriter
    static let styles = DictationStyleOracle.allCases
    static let dictionary: [String] = ["the", "hello", "world", "black", "that's", "it", "and", "see", "you",
                                       "great", "sounds", "good", "hola", "i", "okay", "ok", "is", "a", "send"]

    /// Every test string, through every operation.
    mutating func corpus(full: Bool) {
        var random = DictationRandom(seed: 0x434F_5250_5553)
        for text in ParityCorpus.tests {
            if full {
                for language in ParityLanguage.main {
                    for style in Self.styles {
                        for flags in 0..<8 {
                            format(text, style: style, language: language, flags: flags)
                        }
                    }
                    others(text, language: language, random: &random)
                }
            } else {
                let language = random.pick(ParityLanguage.main)
                if random.chance(0.5) {
                    format(text, style: random.pick(Self.styles), language: language, flags: random.below(8))
                } else {
                    one(random.below(13), text, language: language, random: &random)
                }
            }
        }
        for language in ParityLanguage.all {
            write("lowercases_continuations", language: language, ["out": lowercasesContinuations(language)])
            write("supports", language: language, ["out": SpokenCleanupRulesOracle.supports(language.identifier)])
            write("filler_pattern", language: language,
                  ["out": TranscriptFormatterOracle.fillerPattern(for: language.identifier) as Any? ?? NSNull()])
        }
    }

    /// One generated transcript through one operation.
    mutating func generated(_ random: inout DictationRandom) {
        let text = ParityGenerator.transcript(&random)
        let language = random.chance(0.8) ? random.pick(ParityLanguage.main) : random.pick(ParityLanguage.all)
        switch random.below(16) {
        case 0..<7:
            format(text, style: random.pick(Self.styles), language: language, flags: random.below(8))
        default:
            one(random.below(13), text, language: language, random: &random)
        }
    }

    private mutating func others(_ text: String, language: ParityLanguage, random: inout DictationRandom) {
        for kind in 0..<13 {
            one(kind, text, language: language, random: &random)
        }
    }

    private mutating func one(_ kind: Int, _ text: String, language: ParityLanguage, random: inout DictationRandom) {
        let identifier = language.identifier
        switch kind {
        case 0: write("spoken_punctuation", text, language: language,
                      SpokenPunctuationOracle.format(text, language: identifier))
        case 1: write("spoken_lists", text, language: language, SpokenListsOracle.format(text, language: identifier))
        case 2: write("cleanup_rules", text, language: language,
                      SpokenCleanupRulesOracle.clean(text, language: identifier))
        case 3:
            guard !Self.crashesSpokenSymbols(text) else { return }
            write("spoken_symbols", text, language: language,
                  SpokenCleanupRulesOracle.spokenSymbols(in: text, language: identifier))
        case 4: write("spoken_counts", text, language: language,
                      SpokenCleanupRulesOracle.spokenCounts(in: text, language: identifier))
        case 5: write("french_spacing", text, language: language, FrenchSpacingOracle.apply(text, language: identifier))
        case 6:
            let words = CleanupTaggerOracle.words(in: text)
            let deletes = words.map { _ in random.chance(0.3) }
            write("reconstruct", language: ParityLanguage(identifier: nil), [
                "words": words, "deletes": deletes,
                "out": CleanupReconstructionOracle.reconstruct(words, deletes: deletes),
            ])
        case 7:
            let before: String? = random.chance(0.15) ? nil : ParityGenerator.context(&random)
            let after: String? = random.chance(0.4) ? nil : ParityGenerator.context(&random)
            let checks = random.chance(0.5)
            let dictionary = Set(Self.dictionary)
            var fields: [String: Any] = ["text": text, "out": TranscriptFormatterOracle.insertionText(
                for: text, beforeContext: before, afterContext: after, isWord: checks ? { dictionary.contains($0) } : nil)]
            if let before { fields["before"] = before }
            if let after { fields["after"] = after }
            if checks { fields["words"] = Self.dictionary }
            write("insertion", language: ParityLanguage(identifier: nil), fields)
        case 8:
            write("starts_sentence", language: ParityLanguage(identifier: nil),
                  ["text": text, "out": TranscriptFormatterOracle.startsSentence(after: text)])
        case 9:
            let dictionary = Set(Self.dictionary)
            write("dropping_final_full_stop", text, language: ParityLanguage(identifier: nil),
                  TranscriptFormatterOracle.droppingFinalFullStop(text))
            write("continuing_sentence", language: ParityLanguage(identifier: nil), [
                "text": text, "words": Self.dictionary,
                "out": TranscriptFormatterOracle.continuingSentence(text, isWord: { dictionary.contains($0) }),
            ])
        case 10:
            write("words", language: ParityLanguage(identifier: nil),
                  ["text": text, "out": CleanupTaggerOracle.words(in: text)])
            write("characters", language: ParityLanguage(identifier: nil), [
                "text": text,
                "out": text.map { ParityUnicode.characterFlags($0) + ":\($0.unicodeScalars.count)" },
            ])
        case 11:
            // `folding` of each word, as the rules fold them (SpokenPunctuation, SpokenLists).
            for word in CleanupTaggerOracle.words(in: text).prefix(8) {
                write("fold", language: ParityLanguage(identifier: nil), [
                    "text": word, "out": word.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil),
                ])
            }
        default:
            guard !Self.crashesSpokenSymbols(text) else { return }
            // The cleanup steps as Clean up what I say runs them without the tagger.
            let symbols = SpokenCleanupRulesOracle.spokenSymbols(in: text, language: identifier)
            let counted = SpokenCleanupRulesOracle.spokenCounts(in: symbols, language: identifier)
            write("cleanup_chain", text, language: language, SpokenCleanupRulesOracle.clean(counted, language: identifier))
        }
    }

    /// Whether `spokenSymbols` would trap on `text`: two addresses back to back ("a at sign
    /// b.com at sign c.com") make edits that overlap, and `text[cursor..<start]` traps. Two
    /// symbol names in a text skip it, which covers every such case.
    static func crashesSpokenSymbols(_ text: String) -> Bool {
        let words = CleanupTaggerOracle.words(in: text).map { $0.lowercased() }
        var names = 0
        for (index, word) in words.enumerated() {
            let bare = SpokenCleanupRulesOracle.bare(word)
            if bare.hasPrefix("at-sign") { names += 1 }
            if word == "at", index + 1 < words.count,
               words[index + 1].hasPrefix("sign") || words[index + 1] == "the" { names += 1 }
        }
        return names >= 2
    }

    private mutating func format(_ text: String, style: DictationStyleOracle, language: ParityLanguage, flags: Int) {
        let lists = flags & 1 != 0, punctuation = flags & 2 != 0, period = flags & 4 == 0
        var fields: [String: Any] = ["text": text, "style": style.rawValue, "lists": lists, "punct": punctuation,
                                     "period": period]
        fields["out"] = TranscriptFormatterOracle.format(text, style: style, language: language.identifier,
                                                         lists: lists, punctuation: punctuation, endsWithPeriod: period)
        write("format", language: language, fields)
    }

    private func lowercasesContinuations(_ language: ParityLanguage) -> Any {
        guard let identifier = language.identifier else { return NSNull() }
        return TranscriptFormatterOracle.lowercasesContinuations(in: identifier)
    }

    private mutating func write(_ op: String, _ text: String, language: ParityLanguage, _ out: String) {
        write(op, language: language, ["text": text, "out": out])
    }

    private mutating func write(_ op: String, language: ParityLanguage, _ fields: [String: Any]) {
        var record = fields.merging(language.json) { first, _ in first }
        record["op"] = op
        writer.write(record)
    }
}

// MARK: Writing

struct ParityWriter {
    private let handle: FileHandle
    private var buffer = Data()
    private(set) var lines = 0

    init(path: String) throws {
        FileManager.default.createFile(atPath: path, contents: nil)
        guard let handle = FileHandle(forWritingAtPath: path) else {
            throw CocoaError(.fileWriteUnknown, userInfo: [NSFilePathErrorKey: path])
        }
        self.handle = handle
    }

    mutating func write(_ record: [String: Any]) {
        // Sorted keys and no escaped slashes, so a fixture's diff reads.
        guard let line = try? JSONSerialization.data(withJSONObject: record,
                                                     options: [.sortedKeys, .withoutEscapingSlashes]) else {
            return
        }
        buffer.append(line)
        buffer.append(0x0A)
        lines += 1
        if buffer.count > 1 << 20 { flush() }
    }

    mutating func close() throws {
        flush()
        try handle.close()
    }

    private mutating func flush() {
        handle.write(buffer)
        buffer.removeAll(keepingCapacity: true)
    }
}

// MARK: Random

/// SplitMix64 (Steele, Lea and Flood), so the cases are the same on every run.
struct DictationRandom {
    private var state: UInt64

    init(seed: UInt64) { state = seed }

    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }

    mutating func below(_ bound: Int) -> Int { Int(next() % UInt64(bound)) }
    mutating func chance(_ probability: Double) -> Bool { Double(next() >> 11) / Double(1 << 53) < probability }
    mutating func pick<T>(_ items: [T]) -> T { items[below(items.count)] }
}
