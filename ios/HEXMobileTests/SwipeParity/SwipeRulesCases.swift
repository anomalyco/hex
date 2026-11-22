import CoreGraphics
import UIKit
@testable import HEX

/// Cases for Swipe typing's rules (`SwipeDecoder.typed`, `ranked`, `readsAsName`,
/// `needsSpace`, `isSlidTap`, `word(_:startsAndEndsWith:)`): deterministic (SplitMix64), over
/// profiles of your words (text replacements, Contacts names, vocabulary, learned words and
/// fixes, a dictionary that knows some words only capitalised, or none) in English, Spanish and
/// German, text before the cursor of many shapes, every shift and auto-capitalization state,
/// readings with names, own words, contractions and text replacements, and taps that slid.
enum SwipeRules {
    static let generatorVersion = 1
    static let seed: UInt64 = 0x5357_5255_4C45_5331

    /// A dictionary that knows exactly `valid`, as `UITextChecker` would for these words.
    final class Checker: TextChecking {
        let valid: Set<String>
        init(_ valid: [String]) { self.valid = Set(valid) }
        func isWordValid(_ word: String) -> Bool { valid.contains(word) }
        func guesses(for word: String) -> [String] { [] }
        func completions(for prefix: String) -> [String] { [] }
    }

    struct Profile {
        let id: Int
        let language: KeyboardLanguage
        let hasDictionary: Bool
        let valid: [String]
        let lexicon: [(userInput: String, documentText: String)]
        let vocabulary: [String]
        let typos: [String: String]
        let learned: [String]
    }

    static let profiles: [Profile] = {
        let en = englishValid, es = spanishValid, de = germanValid
        return [
            Profile(id: 0, language: .english, hasDictionary: true, valid: en, lexicon: englishLexicon, vocabulary: vocabulary, typos: typos, learned: learned),
            Profile(id: 1, language: .english, hasDictionary: true, valid: en, lexicon: [], vocabulary: [], typos: [:], learned: []),
            Profile(id: 2, language: .english, hasDictionary: false, valid: en, lexicon: englishLexicon, vocabulary: vocabulary, typos: typos, learned: learned),
            Profile(id: 3, language: .spanish, hasDictionary: true, valid: es, lexicon: spanishLexicon, vocabulary: vocabulary, typos: [:], learned: ["Begoña"]),
            Profile(id: 4, language: .german, hasDictionary: true, valid: de, lexicon: germanLexicon, vocabulary: vocabulary, typos: [:], learned: ["Bell"]),
            Profile(id: 5, language: .english, hasDictionary: true, valid: en, lexicon: englishLexicon, vocabulary: [], typos: [:], learned: []),
        ]
    }()

    /// An autocorrect engine with `profile`'s words, as the keyboard sets one up.
    @MainActor
    static func autocorrect(_ profile: Profile, defaults: UserDefaults) -> AutocorrectEngine {
        let engine = AutocorrectEngine(checker: Checker(profile.valid), learnedWords: LearnedWordsStore(defaults: defaults))
        engine.setLanguage(profile.language, available: profile.hasDictionary ? [profile.language.rawValue] : [])
        engine.loadLexiconEntries(profile.lexicon)
        engine.setVocabulary(profile.vocabulary)
        engine.applyLearning(TypingLearning(typos: profile.typos))
        for word in profile.learned { engine.learnedWords.learn(word) }
        return engine
    }

    static let englishValid = [
        "the", "hello", "Hello", "world", "World", "bell", "Bell", "will", "Will", "swift", "Swift", "many", "Kent", "York",
        "Monday", "Taylor", "Kate", "Paris", "English", "NASA", "I'm", "I'll", "don't", "receive", "at", "house", "lol", "home",
        "café", "Zoë", "iPhone", "a", "I", "yeet", "gs", "ut", "John", "john", "kubectl", "tis", "'tis", "ǅemal",
    ]
    static let spanishValid = ["hola", "mañana", "José", "Madrid", "señor", "qué", "casa", "Casa", "Begoña", "ñandú", "Ñandú", "el"]
    static let germanValid = ["Straße", "Haus", "haus", "Bell", "bell", "Berlin", "Swift", "und", "Mädchen"]

    static let englishLexicon: [(userInput: String, documentText: String)] = [
        ("omw", "On my way!"), ("home", "at my house"), ("brb", "be right back"), ("ty", "thank you"), ("mip", "My iPhone"),
        ("ad2", "2 Infinite Loop"), ("s.o", "significant other"), ("gm", "Good morning"), ("idk", "I don't know"), ("Tx", "Texas"),
        ("lol", "laughing out loud"), ("kate", "Kate Bell"), ("Kate", "Kate"), ("Bell", "Bell"), ("Will", "Will"),
        ("Taylor", "Taylor"), ("O'Brien", "O'Brien"), ("iPhone", "iPhone"), ("Zoë", "Zoë"), ("McDonald", "McDonald"),
        ("hexkb", "hexkb"), ("caf\u{E9}x", "cafe\u{301} express"),
    ]
    static let spanishLexicon: [(userInput: String, documentText: String)] = [
        ("qtal", "¿Qué tal?"), ("Begoña", "Begoña"), ("josé", "José"), ("mñn", "mañana"),
    ]
    static let germanLexicon: [(userInput: String, documentText: String)] = [
        ("mfg", "Mit freundlichen Grüßen"), ("Bell", "Bell"), ("Straße", "Straße"),
    ]
    static let vocabulary = ["GitHub", "Swift", "Kubernetes", "Swift UI", "kubectl", "TestFlight"]
    static let typos = ["teh": "the", "helo": "Hello", "wrld": "World", "recieve": "receive"]
    static let learned = ["Swift", "yeet", "Brexit", "bell"]

    static let readings = [
        "hello", "world", "bell", "will", "swift", "many", "kent", "york", "monday", "taylor", "kate", "paris", "english",
        "nasa", "the", "home", "omw", "ty", "brb", "lol", "ad2", "s.o", "gm", "idk", "tx", "mip", "I'm", "i'm", "don't",
        "I'll", "Kate", "Bell", "Will", "World", "Swift", "GitHub", "iPhone", "O'Brien", "McDonald", "Zoë", "zoë",
        "cafe\u{301}", "café", "cafex", "straße", "Straße", "οδος", "ΟΔΟΣ", "'tis", "a", "I", "x", "ß", "ǆemal", "hexkb",
        "yeet", "kubernetes", "Kubernetes", "kubectl", "teh", "Hello", "receive", "gs", "ut", "john", "hola", "mañana",
        "josé", "José", "madrid", "qtal", "begoña", "Begoña", "ñandú", "haus", "Haus", "berlin", "mfg", "mädchen",
        "Brexit", "brexit", "TestFlight", "testflight", "e\u{301}te\u{301}", "Ǆemal", "iPhone's",
    ]

    static let beforePieces = [
        "", "Hello", "hello", "Hi.", "Hi!", "ok?", "end;", "x", "word", " ", "  ", "\t", "\n", "\r\n", "\u{A0}", "\u{3000}",
        "(", "[", "{", "<", "«", "‹", "“", "‘", "„", "¿", "¡", "@", "#", "/", "\"", "'", "”", "’", ")", "]", "é", "e\u{301}",
        "👍", "👍🏽", "Τι;", "5", "#tag", "omw", "my ad2", "(omw", "hi,omw", "xomw", ".", "...", "\u{2028}", "Dr.", "a\"",
        " \"", "(\"", "x'", " '", "\n\"", "\u{301}", "ty", "Tx", "TX", "said \"", "\u{FFFC}", "Look \u{FFFC}",
    ]

    /// A key a point hits: its character (nil for one with none), and which key it is.
    struct Key: Equatable {
        var character: String?
        var id: Int
    }

    struct State {
        var before: String?
        var shift: ShiftState
        var shiftHeld: Bool
        var mode: AutoCapitalization.Mode
        var autoCapitalizes: Bool
        var autocorrection: Bool

        var typing: SwipeDecoder.TypingState {
            SwipeDecoder.TypingState(
                before: before, shift: shift, shiftHeld: shiftHeld, autoCapitalization: mode,
                autoCapitalizes: autoCapitalizes, autocorrection: autocorrection
            )
        }
    }

    enum Case {
        case typed(profile: Int, readings: [String], state: State)
        case cased(profile: Int, word: String, capital: Bool, state: State)
        case name(profile: Int, word: String)
        case space(before: String?)
        case rank(profile: Int, language: KeyboardLanguage, candidates: [SwipeDecoder.Candidate])
        /// `keys`: the key each point hits, as the layout's hit test finds it.
        case slid(points: [CGPoint], pitch: CGFloat, keys: [SwipeRules.Key?], first: String?)
        case ends(word: String, letter: String)
    }

    struct Generator {
        private var rng = SplitMix64(seed: SwipeRules.seed)

        mutating func before() -> String? {
            if rng.chance(0.1) { return nil }
            return (0..<rng.int(4)).map { _ in rng.pick(SwipeRules.beforePieces) }.joined()
        }

        mutating func state() -> State {
            let shift: ShiftState = rng.chance(0.7) ? .off : rng.chance(0.65) ? .on : .locked
            let modes: [AutoCapitalization.Mode] = [.sentences, .sentences, .sentences, .words, .none, .allCharacters]
            let autoCapitalizes = rng.chance(0.85)
            return State(
                before: before(), shift: shift, shiftHeld: rng.chance(0.1),
                mode: autoCapitalizes ? rng.pick(modes) : .none, autoCapitalizes: autoCapitalizes, autocorrection: rng.chance(0.85)
            )
        }

        mutating func next() -> Case {
            let profile = rng.int(SwipeRules.profiles.count)
            let roll = rng.unit()
            switch roll {
            case ..<0.35:
                let readings = (0..<(1 + rng.int(8))).map { _ in rng.pick(SwipeRules.readings) }
                return .typed(profile: profile, readings: readings, state: state())
            case ..<0.5:
                return .cased(profile: profile, word: rng.pick(SwipeRules.readings), capital: rng.chance(0.3), state: state())
            case ..<0.6:
                return .name(profile: profile, word: rng.pick(SwipeRules.readings))
            case ..<0.72:
                return .space(before: before())
            case ..<0.84:
                let language: KeyboardLanguage = rng.chance(0.7) ? .english : .spanish
                let ties = [12.5, 14.0, 14.0, 16.25]
                let candidates = (0..<(1 + rng.int(8))).map { _ in
                    SwipeDecoder.Candidate(text: rng.pick(SwipeRules.readings), cost: rng.chance(0.4) ? rng.pick(ties) : rng.uniform(8, 30))
                }
                return .rank(profile: profile, language: language, candidates: candidates)
            case ..<0.94:
                let pitch = rng.pick([38.3, 36.8, 57.0])
                // Touches on a quarter-point grid, which a fixture carries in a few digits.
                func grid(_ v: Double) -> Double { (v * 4).rounded() / 4 + 0 }
                let start = CGPoint(x: grid(rng.uniform(0, 400)), y: grid(rng.uniform(0, 200)))
                var points = [start]
                let spread = rng.pick([0.2, 0.8, 1.5, 1.7, 3]) * pitch
                for _ in 0..<rng.int(16) {
                    points.append(CGPoint(x: grid(start.x + rng.uniform(-spread, spread)), y: grid(start.y + rng.uniform(-spread / 2, spread / 2))))
                }
                if rng.chance(0.5) { points.append(start) }
                let letters: [String?] = ["a", "l", "ñ", "A", "ab", "é", "e\u{301}", "o", nil, "s"]
                // The key each point hits: the start's within half a pitch of it, else another
                // (G on the split layout's other half, say) or none.
                let own = SwipeRules.Key(character: rng.pick(letters), id: 0)
                let other = SwipeRules.Key(character: own.character, id: 1)
                let keys = points.map { p in hypot(p.x - start.x, p.y - start.y) <= pitch / 2 ? own : rng.chance(0.5) ? other : nil }
                let first: String? = rng.chance(0.15) ? nil : rng.pick(["lol", "as", "a", "aa", "àla", "ñoño", "sos", "Lol", "ese", "oslo", "éte", "e\u{301}te\u{301}", "abba", "llama"])
                return .slid(points: points, pitch: pitch, keys: keys, first: first)
            default:
                if rng.chance(0.5) {
                    // Words that come back to their first key, and its letter (or not quite).
                    let pairs = [("lol", "l"), ("pop", "p"), ("àla", "a"), ("ñoño", "ñ"), ("ñoño", "n"), ("Lol", "l"), ("Lol", "L"), ("sos", "s"),
                                 ("e\u{301}te\u{301}", "e"), ("éte", "é"), ("abba", "a"), ("oslo", "o"), ("aa", "a"), ("a", "a")]
                    let (word, letter) = rng.pick(pairs)
                    return .ends(word: word, letter: letter)
                }
                let letters = ["l", "a", "n", "ñ", "e", "é", "e\u{301}", "L", "", "ab", "o", "s"]
                return .ends(word: rng.pick(SwipeRules.readings), letter: rng.pick(letters))
            }
        }
    }
}

/// The two paths a case runs through: the Swift oracle, and the core as the keyboard calls it.
@MainActor
struct SwipeRulesRunner {
    let engines: [AutocorrectEngine]
    let decoders: [KeyboardLanguage: SwipeDecoder]

    init(defaults: UserDefaults) throws {
        engines = SwipeRules.profiles.map { SwipeRules.autocorrect($0, defaults: defaults) }
        var decoders: [KeyboardLanguage: SwipeDecoder] = [:]
        for language in [KeyboardLanguage.english, .spanish] {
            guard let model = TypingDecoderModel.bundled(language.rawValue) else { throw NSError(domain: "SwipeRules", code: 1) }
            decoders[language] = SwipeDecoder(model: model)
        }
        self.decoders = decoders
    }

    /// The case's outcome by the core, as the keyboard asks it, as text.
    func core(_ c: SwipeRules.Case) -> String {
        func words(_ profile: Int) -> SwipeDecoder.Words { engines[profile].swipeWords! }
        func check(_ profile: Int) -> (String) -> Bool { { [checker = engines[profile].checker] in checker.isWordValid($0) } }
        switch c {
        case let .typed(profile, readings, state):
            return describe(SwipeDecoder.typed(readings, in: state.typing, words: words(profile), isWordValid: check(profile))!)
        case let .cased(profile, word, capital, state):
            // As `FullKeyboardEngine.swipeCased` asks it.
            var typing = state.typing
            typing.before = nil
            typing.shiftHeld = false
            typing.shift = state.shift == .locked ? .locked : capital ? .on : .off
            return SwipeDecoder.typed([word], in: typing, words: words(profile), isWordValid: check(profile))!.word
        case let .name(profile, word):
            return String(SwipeDecoder.readsAsName(word, words: words(profile), isWordValid: check(profile)))
        case let .space(before):
            return String(SwipeDecoder.needsSpace(before: before))
        case let .rank(profile, language, candidates):
            return decoders[language]!.ranked(candidates, words: words(profile), isWordValid: check(profile)).map { candidates[$0].text }.joined(separator: "|")
        case let .slid(points, pitch, keys, first):
            // As `SwipeTyping.slidTapKey` finds the key: the one it went down and lifted on.
            let down = keys.first!, up = keys.last!
            let key = down != nil && down == up ? down?.character : nil
            return String(SwipeDecoder.isSlidTap(points, pitch: pitch, key: key, firstReading: first))
        case let .ends(word, letter):
            return String(SwipeDecoder.word(word, startsAndEndsWith: letter))
        }
    }

    /// `profile`'s words as the core gets them (`AutocorrectEngine.swipeWords`), and the
    /// dictionary's words, for the fixture.
    func record(profile: SwipeRules.Profile) -> [String: Any] {
        let engine = engines[profile.id]
        return [
            "kind": "profile", "id": profile.id, "language": engine.language.rawValue, "hasDictionary": engine.hasDictionary,
            "valid": profile.valid,
            "replacements": engine.lexiconShortcuts.sorted { $0.key < $1.key }.map { [$0.key, $0.value] },
            "own": engine.lexiconTerms + Array(engine.learning.typos.values).sorted() + engine.learnedWords.words,
            "vocabulary": engine.vocabulary.isEmpty ? [] : AutocorrectEngine.vocabularyWords(engine.vocabulary),
        ]
    }

    /// A case as the core is asked it, with what it answered, for the fixture: the inputs
    /// the core takes (a `cased` case as `FullKeyboardEngine.swipeCased` asks it, a `slid`
    /// case's key as `SwipeTyping.slidTapKey` finds it) and its outcome.
    func record(_ c: SwipeRules.Case) -> [String: Any] {
        func state(_ s: SwipeDecoder.TypingState) -> [String: Any] {
            [
                "before": s.before ?? NSNull(), "shift": s.shift == .locked ? 2 : s.shift == .on ? 1 : 0, "shiftHeld": s.shiftHeld,
                "autoCapitalization": [AutoCapitalization.Mode.none, .words, .sentences, .allCharacters].firstIndex(of: s.autoCapitalization)!,
                "autoCapitalizes": s.autoCapitalizes, "autocorrection": s.autocorrection,
            ]
        }
        func typed(_ profile: Int, _ readings: [String], _ s: SwipeDecoder.TypingState) -> [String: Any] {
            let out = SwipeDecoder.typed(readings, in: s, words: engines[profile].swipeWords!) { [checker = engines[profile].checker] in checker.isWordValid($0) }!
            return [
                "kind": "typed", "profile": profile, "readings": readings, "state": state(s),
                "out": ["space": out.space, "word": out.word, "alternatives": out.alternatives] as [String: Any],
            ]
        }
        switch c {
        case let .typed(profile, readings, s):
            return typed(profile, readings, s.typing)
        case let .cased(profile, word, capital, s):
            var typing = s.typing
            typing.before = nil
            typing.shiftHeld = false
            typing.shift = s.shift == .locked ? .locked : capital ? .on : .off
            return typed(profile, [word], typing)
        case let .name(profile, word):
            return ["kind": "name", "profile": profile, "word": word, "out": core(c) == "true"]
        case let .space(before):
            return ["kind": "space", "before": before ?? NSNull(), "out": core(c) == "true"]
        case let .rank(profile, language, candidates):
            let order = decoders[language]!.ranked(candidates, words: engines[profile].swipeWords!) { [checker = engines[profile].checker] in checker.isWordValid($0) }
            return [
                "kind": "rank", "profile": profile, "language": language.rawValue,
                "candidates": candidates.map { [$0.text, String(format: "%016llx", $0.cost.bitPattern)] }, "out": order,
            ]
        case let .slid(points, pitch, keys, first):
            let down = keys.first!, up = keys.last!
            let key = down != nil && down == up ? down?.character : nil
            return [
                "kind": "slid", "points": points.map { "\(Double($0.x)),\(Double($0.y))" }, "pitch": "\(Double(pitch))",
                "key": key ?? NSNull(), "first": first ?? NSNull(), "out": core(c) == "true",
            ]
        case let .ends(word, letter):
            return ["kind": "ends", "word": word, "letter": letter, "out": core(c) == "true"]
        }
    }

    func describe(_ typed: SwipeDecoder.Typed) -> String {
        "\(typed.space ? "␣" : "")\(typed.word) [\(typed.alternatives.joined(separator: "|"))]"
    }
}
