// The autocorrect oracle's own copies of what `AutocorrectEngine` reads beside itself, as of dev
// commit d6efeddb: `TextChecking`, `LearnedWordsStore` (in memory, with the same JSON size cap
// and seeding), `OffensiveWords`, `GermanCapitals`, `TypingLearning`,
// `AutoCapitalization.shouldCapitalize`, `AppliedCorrection` and the `KeyboardLanguage` data
// autocorrect reads (as `aco…`). Plus the two orders the owner fixed in the port: own words by
// shortcut and typo (`ACOracle.ownWordsOrder`), and one never-suggest list read by base language
// (`ACOOffensiveWords.sharedList`). Delete with the exporter once iOS runs the Rust autocorrect.

import CoreGraphics
import Foundation
@testable import HEX

@MainActor
protocol ACOChecking: AnyObject {
    func isWordValid(_ word: String) -> Bool
    func guesses(for word: String) -> [String]
    func completions(for prefix: String) -> [String]
    func useLanguage(_ code: String)
}

/// Where the oracle reads the repository's files (the models, lists and capitals).
enum ACOracleFiles {
    nonisolated(unsafe) static var repo = ""
    static func read(_ path: String) -> String? {
        try? String(contentsOfFile: repo + "/" + path, encoding: .utf8)
    }
}

extension ACOracle {
    /// The order your text replacements' outputs and learned fixes reach the decoder and Swipe
    /// typing: the dictionary's (today's, random per process) or by shortcut and typo (the port's).
    enum OwnWordsOrder {
        case dictionary
        case byKey
    }

    nonisolated(unsafe) static var ownWordsOrder = OwnWordsOrder.dictionary

    static func orderedValues(_ dictionary: [String: String]) -> [String] {
        switch ownWordsOrder {
        case .dictionary: Array(dictionary.values)
        case .byKey: dictionary.sorted { $0.key < $1.key }.map(\.value)
        }
    }

    /// `AppliedCorrection`, copied.
    struct AppliedCorrection: Equatable {
        struct Change: Equatable {
            let from: String
            let to: String
            var language: KeyboardLanguage? = nil
        }

        let original: String
        let corrected: String
        let changes: [Change]
        let anchor: String

        static let anchorTail = 64

        func isAtEnd(of before: String?) -> Bool {
            guard let before, before.hasSuffix(corrected) else { return false }
            guard anchor.count >= Self.anchorTail, before.count >= Self.anchorTail else { return before == anchor }
            return before.suffix(Self.anchorTail) == anchor.suffix(Self.anchorTail)
        }
    }
}

/// `LearnedWordsStore`, its UserDefaults an in-memory box (`ACOLearnedWords.Storage`) whose
/// erase time is a counter.
@MainActor
final class ACOLearnedWords {
    final class Storage {
        var data: [String: Data] = [:]
        var erasedAt = 0.0
        var now = 1.0
    }

    static let defaultsKey = "keyboard.learnedWords"
    static let maxWords = 2_000
    static let maxBytes = 64 * 1024

    let defaults: Storage?
    let language: String?
    var storage: [String] = []
    var wordSet: Set<String> = []
    var sessionKeepCounts: [String: Int] = [:]
    var loadedAt: Double

    init(defaults: Storage?, language: String? = nil) {
        self.defaults = defaults
        self.language = language
        loadedAt = defaults?.now ?? 0
        load()
    }

    func scoped(to language: String) -> ACOLearnedWords {
        ACOLearnedWords(defaults: defaults, language: language)
    }

    var storageKey: String {
        language.map { Self.defaultsKey + "." + $0 } ?? Self.defaultsKey
    }

    var words: [String] {
        dropIfErased()
        return storage
    }

    func contains(_ word: String) -> Bool {
        dropIfErased()
        return wordSet.contains(word.lowercased())
    }

    func recordExplicitKeep(_ raw: String) {
        dropIfErased()
        let key = raw.lowercased()
        guard key.count >= 2, key.count <= 40 else { return }
        let count = (sessionKeepCounts[key] ?? 0) + 1
        sessionKeepCounts[key] = count
        if count >= 2 {
            learn(key)
        }
    }

    func learn(_ raw: String) {
        dropIfErased()
        let key = raw.lowercased()
        guard !key.isEmpty, !wordSet.contains(key) else { return }
        storage.append(key)
        wordSet.insert(key)
        while storage.count > Self.maxWords {
            let evicted = storage.removeFirst()
            wordSet.remove(evicted)
        }
        save()
    }

    func dropIfErased() {
        let erasedAt = defaults?.erasedAt ?? 0
        guard erasedAt > loadedAt else { return }
        storage = []
        wordSet = []
        sessionKeepCounts = [:]
        loadedAt = erasedAt
    }

    func load() {
        if let data = defaults?.data[storageKey], data.count <= Self.maxBytes,
           let decoded = try? JSONDecoder().decode([String].self, from: data) {
            storage = Array(decoded.suffix(Self.maxWords))
            wordSet = Set(storage)
            return
        }
        guard language != nil else { return }
        if let data = defaults?.data[Self.defaultsKey], data.count <= Self.maxBytes,
           let decoded = try? JSONDecoder().decode([String].self, from: data) {
            storage = Array(decoded.suffix(Self.maxWords))
            wordSet = Set(storage)
        }
        save()
    }

    func save() {
        guard let defaults else { return }
        while true {
            guard let data = try? JSONEncoder().encode(storage) else { return }
            if data.count <= Self.maxBytes || storage.isEmpty {
                defaults.data[storageKey] = data
                return
            }
            let evicted = storage.removeFirst()
            wordSet.remove(evicted)
        }
    }
}

/// `OffensiveWords`, copied; `bundled` reads the iOS list by layout code (today's) or, with
/// `sharedList`, Android's by base language (the port's).
nonisolated struct ACOOffensiveWords: Equatable, Sendable {
    enum Kind: Equatable, Sendable {
        case slur
        case swear
    }

    var entries: [String: Kind] = [:]

    init(entries: [String: Kind] = [:]) {
        self.entries = entries
    }

    init(tsv: String, language: String) {
        var entries: [String: Kind] = [:]
        for line in tsv.split(whereSeparator: \.isNewline) where !line.hasPrefix("#") {
            let fields = line.split(separator: "\t", omittingEmptySubsequences: false)
            guard fields.count == 3, fields[0] == "*" || fields[0] == language[...] else { continue }
            let kind: Kind
            switch fields[1] {
            case "slur": kind = .slur
            case "swear": kind = .swear
            default: continue
            }
            let spelling = fields[2].lowercased().split(separator: " ").joined(separator: " ")
            guard !spelling.isEmpty else { continue }
            if entries[spelling] != .slur { entries[spelling] = kind }
        }
        self.init(entries: entries)
    }

    var isEmpty: Bool { entries.isEmpty }

    func kind(of word: String) -> Kind? {
        entries[word.lowercased()]
    }

    func blocks(_ candidate: String, allowSwearing: Bool, after previous: String? = nil) -> Bool {
        func blocked(_ text: String) -> Bool {
            switch kind(of: text) {
            case .slur: true
            case .swear: !allowSwearing
            case nil: false
            }
        }
        let parts = candidate.split(whereSeparator: { $0 == " " || $0 == "-" }).map(String.init)
        guard let first = parts.first else { return false }
        if parts.contains(where: blocked) { return true }
        if let previous, blocked(previous + " " + first) { return true }
        return zip(parts, parts.dropFirst()).contains { blocked($0 + " " + $1) }
    }

    func blocked(allowSwearing: Bool) -> [String] {
        entries.compactMap { word, kind in kind == .slur || !allowSwearing ? word : nil }
    }

    static let iosPath = "ios/HEXShared/FullKeyboard/never-suggest.tsv"
    static let sharedPath = "android/app/src/main/assets/typing/never-suggest.tsv"
    nonisolated(unsafe) static var sharedList = false

    @MainActor static func bundled(_ language: KeyboardLanguage) -> ACOOffensiveWords {
        let key = sharedList ? "shared:" + language.baseLanguage : language.rawValue
        if let cached = loaded[key] { return cached }
        let tsv = ACOracleFiles.read(sharedList ? sharedPath : iosPath) ?? ""
        let words = ACOOffensiveWords(tsv: tsv, language: sharedList ? language.baseLanguage : language.rawValue)
        loaded[key] = words
        return words
    }

    @MainActor static var loaded: [String: ACOOffensiveWords] = [:]
}

/// `GermanCapitals`, copied; `bundled` reads the repository's `de.capitals`.
nonisolated struct ACOGermanCapitals: Sendable {
    static let alwaysShare = 95
    static let sometimesShare = 50

    let percentages: [String: UInt8]

    init(percentages: [String: UInt8]) {
        self.percentages = percentages
    }

    @MainActor static func bundled() -> ACOGermanCapitals? {
        if didRead { return loaded }
        didRead = true
        loaded = ACOracleFiles.read("ios/HEXKeyboard/TypingModel/de.capitals").flatMap { ACOGermanCapitals(text: $0) }
        return loaded
    }

    @MainActor static var loaded: ACOGermanCapitals?
    @MainActor static var didRead = false

    init?(text: String) {
        var percentages: [String: UInt8] = [:]
        for line in text.split(separator: "\n") {
            let fields = line.split(separator: "\t", omittingEmptySubsequences: false)
            guard fields.count == 2, let percent = UInt8(fields[1]),
                  (Self.sometimesShare...100).contains(Int(percent)) else { continue }
            percentages[String(fields[0])] = percent
        }
        guard !percentages.isEmpty else { return nil }
        self.percentages = percentages
    }

    func percent(of word: String) -> UInt8? {
        percentages[word.lowercased()]
    }

    func capitalises(_ word: String, after previous: String?) -> Bool? {
        guard word.count >= 2, word == word.lowercased(),
              let share = percent(of: word), share >= Self.sometimesShare else { return nil }
        if share >= Self.alwaysShare { return true }
        return previous.map { Self.determiners.contains($0) } ?? false
    }

    static let determiners: Set<String> = [
        "der", "die", "das", "den", "dem", "des",
        "ein", "eine", "einen", "einem", "einer", "eines",
        "kein", "keine", "keinen", "keinem", "keiner", "keines",
        "mein", "meine", "meinen", "meinem", "meiner", "meines",
        "dein", "deine", "deinen", "deinem", "deiner", "deines",
        "sein", "seine", "seinen", "seinem", "seiner", "seines",
        "ihre", "ihren", "ihrem", "ihrer", "ihres",
        "unser", "unsere", "unseren", "unserem", "unserer", "unseres",
        "euer", "eure", "euren", "eurem", "eurer", "eures",
        "dies", "dieser", "diese", "dieses", "diesem", "diesen",
        "jeder", "jede", "jedes", "jedem", "jeden",
        "im", "in", "am", "zum", "zur", "vom", "beim", "ins", "ans",
    ]
}

/// `TypingLearning`'s two lists.
nonisolated struct ACOLearning: Equatable, Sendable {
    var typos: [String: String] = [:]
    var rejected: Set<String> = []
}

/// `AutoCapitalization.shouldCapitalize`, copied.
nonisolated enum ACOAutoCapitalization {
    enum Mode: Equatable, Sendable {
        case none
        case words
        case sentences
        case allCharacters
    }

    static let sentenceEnders: Set<Character> = [".", "!", "?"]
    static let closers: Set<Character> = ["\"", "'", ")", "]", "\u{201D}", "\u{2019}"]

    static func shouldCapitalize(mode: Mode, before: String?, hasText: Bool, language: KeyboardLanguage = .english) -> Bool {
        switch mode {
        case .none:
            return false
        case .allCharacters:
            return true
        case .words, .sentences:
            break
        }
        guard let before else { return !hasText }
        guard let last = before.last else { return true }
        if last.isNewline { return true }
        guard last.isWhitespace else { return false }
        if mode == .words { return true }

        var trimmed = Substring(before)
        while let c = trimmed.last, c.isWhitespace, !c.isNewline {
            trimmed = trimmed.dropLast()
        }
        guard let lastWritten = trimmed.last else { return true }
        if lastWritten.isNewline { return true }
        while let c = trimmed.last, closers.contains(c) {
            trimmed = trimmed.dropLast()
        }
        guard let ender = trimmed.last else { return false }
        return sentenceEnders.contains(ender) || (language == .greek && (ender == ";" || ender == "\u{037E}"))
    }
}

/// The `KeyboardLanguage` data autocorrect reads, copied.
extension KeyboardLanguage {
    var acoSpellCheckCandidates: [String] {
        switch self {
        case .english: ["en_US", "en"]
        case .norwegian: ["nb", "nb_NO", "no", "no_NO"]
        case .spanish: ["es", "es_ES", "es_MX"]
        case .german: ["de", "de_DE"]
        case .italian: ["it", "it_IT"]
        case .french: ["fr", "fr_FR", "fr_CA"]
        case .frenchCanadian: ["fr_CA", "fr", "fr_FR"]
        case .frenchSwiss: ["fr_CH", "fr", "fr_FR"]
        case .portuguese: ["pt_BR", "pt", "pt_PT"]
        case .polish: ["pl", "pl_PL"]
        case .czech: ["cs", "cs_CZ"]
        case .romanian: ["ro", "ro_RO"]
        case .russian: ["ru", "ru_RU"]
        case .ukrainian: ["uk", "uk_UA"]
        case .greek: ["el", "el_GR"]
        case .serbianLatin: ["sr_Latn", "sr_Latn_RS", "sr-Latn"]
        }
    }

    func acoSpellCheckLanguage(available: [String]) -> String? {
        acoSpellCheckCandidates.first { available.contains($0) }
    }

    var acoUsesEnglishFixes: Bool { self == .english }

    var acoCommonTwoLetterWords: [String] {
        switch self {
        case .english:
            ["to", "of", "in", "it", "is", "be", "as", "at", "so", "we", "he", "by", "or", "on",
             "do", "if", "me", "my", "up", "an", "go", "no", "us", "am"]
        case .spanish:
            ["de", "la", "el", "en", "no", "se", "lo", "es", "un", "su", "me", "te", "ya", "mi",
             "tu", "yo", "le", "si", "al", "ha", "ni"]
        case .italian:
            ["di", "il", "la", "in", "un", "si", "le", "lo", "ha", "ho", "ma", "da", "mi", "ci",
             "ti", "se", "ne", "no", "io", "tu", "su", "al"]
        case .norwegian, .german, .french, .frenchCanadian, .frenchSwiss, .portuguese, .polish,
             .czech, .romanian, .russian, .ukrainian, .greek, .serbianLatin:
            []
        }
    }

    var acoAccentRestorations: [String: String] {
        switch self {
        case .italian:
            [
                "perche": "perché", "poiche": "poiché", "affinche": "affinché",
                "benche": "benché", "finche": "finché",
                "piu": "più", "gia": "già", "puo": "può", "cioe": "cioè", "percio": "perciò",
                "citta": "città", "societa": "società", "universita": "università",
                "qualita": "qualità", "liberta": "libertà", "verita": "verità",
                "realta": "realtà", "attivita": "attività", "possibilita": "possibilità",
                "difficolta": "difficoltà", "opportunita": "opportunità",
                "caffe": "caffè", "giu": "giù",
                "lunedi": "lunedì", "martedi": "martedì", "mercoledi": "mercoledì",
                "giovedi": "giovedì", "venerdi": "venerdì",
            ]
        default:
            [:]
        }
    }

    var acoAccentSuggestions: [String: String] {
        switch self {
        case .italian: ["e": "è", "ce": "c'è"]
        default: [:]
        }
    }

    var acoLetterLayouts: [KeyboardLetterLayout] {
        switch self {
        case .english: [.qwerty, .azerty, .qwertz, .dvorak]
        case .german: [.own, .qwertz, .qwerty, .azerty]
        case .spanish: [.own, .qwerty, .azerty, .qwertz]
        case .french: [.azerty, .qwerty, .qwertz]
        case .frenchCanadian: [.qwerty, .azerty, .qwertz]
        case .frenchSwiss: [.qwertz, .azerty, .qwerty]
        case .italian, .portuguese: [.qwerty, .azerty, .qwertz]
        case .polish, .romanian: [.qwerty, .qwertz, .azerty]
        case .czech: [.own, .qwerty]
        case .serbianLatin: [.qwertz, .qwerty, .azerty]
        case .norwegian, .russian, .ukrainian, .greek: [.own]
        }
    }

    func acoOffered(_ letters: KeyboardLetterLayout?) -> KeyboardLetterLayout {
        guard let letters, acoLetterLayouts.contains(letters) else { return acoLetterLayouts[0] }
        return letters
    }

    func acoLetterRows(_ letters: KeyboardLetterLayout) -> [[String]] {
        let rows: [String] = switch acoOffered(letters) {
        case .own: acoOwnLetterRows
        case .qwerty where self == .czech: ["qwertyuiop", "asdfghjkl\u{00B4}", "zxcvbnm\u{02C7}"]
        case .qwerty: ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
        case .qwertz: ["qwertzuiop", "asdfghjkl", "yxcvbnm"]
        case .azerty: ["azertyuiop", "qsdfghjklm", "wxcvbn"]
        case .dvorak: ["pyfgcrl", "aoeuidhtns", "qjkxbmwvz"]
        }
        return rows.map { $0.map(String.init) }
    }

    var acoOwnLetterRows: [String] {
        switch self {
        case .english, .italian, .frenchCanadian, .portuguese, .polish, .romanian:
            ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
        case .french: ["azertyuiop", "qsdfghjklm", "wxcvbn"]
        case .frenchSwiss: ["qwertzuiop", "asdfghjkl", "yxcvbnm"]
        case .norwegian: ["qwertyuiopå", "asdfghjkløæ", "zxcvbnm"]
        case .spanish: ["qwertyuiop", "asdfghjklñ", "zxcvbnm"]
        case .german: ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"]
        case .russian: ["йцукенгшщзх", "фывапролджэ", "ячсмитьбю"]
        case .ukrainian: ["йцукенгшщзхї", "фівапролджєʼ", "ячсмитьбюґ"]
        case .greek: ["ερτυθιοπ\u{00B4}", "ασδφγηξκλ", "ζχψωβνμ"]
        case .serbianLatin: ["qwertzuiop", "asdfghjkl", "yxcvbnm"]
        case .czech: ["qwertzuiop", "asdfghjkl\u{00B4}", "yxcvbnm\u{02C7}"]
        }
    }

    func acoKeyCoordinates(_ letters: KeyboardLetterLayout? = nil) -> [Character: (Double, Double)] {
        var map: [Character: (Double, Double)] = [:]
        let letters = acoOffered(letters)
        let offsets = switch letters {
        case .azerty: [0.0, 0.0, 2.0]
        case .dvorak: [1.85, 0.0, 1.33]
        case .own where self == .ukrainian || self == .greek || self == .czech: [0.0, 0.0, 1.0]
        case .qwerty where self == .czech: [0.0, 0.0, 1.0]
        default: [0.0, 0.5, 1.5]
        }
        for (row, letters) in acoLetterRows(letters).enumerated() {
            for (column, letter) in letters.enumerated() {
                guard let character = letter.first else { continue }
                map[character] = (Double(row), Double(column) + offsets[row])
            }
        }
        return map
    }
}
