import CoreGraphics
import UIKit
@testable import HEX

// Swipe typing's rules as they were in Swift before the keyboard core took them, kept as the
// parity oracle for the core's (`crates/keyboard/core/src/swipe/typing.rs`): copied verbatim
// from `FullKeyboardEngine+SwipeTyping.swift` (ee8e480bddbf3404ea6796d9389384196176f159) and `SwipeTyping.swift`
// (b719d45e5361ae04e220ec8b6822a40ea0dc1cd4). The only edits: what the rules read from the engine and the touch (shift, the
// field, the text before, the layout's hit test) is the oracle's own state, `ranked` takes
// the model, and `slidTapLetter` the threshold. They still call the real AutocorrectEngine
// (`isOwnSpelling`, `withoutStrayCapital`, `textReplacement`, its checker), which taps keep
// using. Delete this once the core is proven to type every case alike.
@MainActor
struct SwipeRulesOracle {
    struct Shift {
        var state: ShiftState
        var isHeld: Bool
        var isUppercase: Bool { state != .off || isHeld }
    }

    struct Target {
        var autocorrectionType: UITextAutocorrectionType
    }

    /// A swipe's touches, and which key (by character) each point would hit.
    struct Stroke {
        typealias Key = SwipeRules.Key

        var points: [CGPoint]
        var hitTest: (CGPoint) -> Key?
    }

    var shift: Shift
    var target: Target
    var autoCapitalizes: Bool
    var autoCapitalizationMode: AutoCapitalization.Mode
    var typingContext: String?
    let autocorrect: AutocorrectEngine

    /// Whether a swiped word starts with a capital: shift (one-shot, held, caps lock, or
    /// automatic), or the automatic capital the space the swipe adds brings ("Hi." then a
    /// swipe types " There").
    var swipeCapital: Bool {
        if shift.isUppercase { return true }
        guard shift.state == .off, !shift.isHeld else { return false }
        let before = typingContext
        guard Self.needsSpace(before: before) else { return false }
        return AutoCapitalization.shouldCapitalize(
            mode: autoCapitalizationMode,
            before: (before ?? "") + " ",
            hasText: true
        )
    }

    /// A reading as it's typed: all capitals with caps lock, a capital first letter with
    /// `capital`, else a name the dictionary only knows capitalised ("monday" -> "Monday").
    /// One of your own words with a capital that is also a common word (a Contacts name such
    /// as "Bell" or "Will") is written as the sentence needs it, as Apple's keyboard swipes
    /// "the bell rang" beside a contact Kate Bell (`AutocorrectEngine.withoutStrayCapital`).
    /// With Auto-capitalization off, no capital you didn't ask for ("i'm", "monday"), but
    /// your own spellings keep theirs.
    func swipeCased(_ word: String, capital: Bool) -> String {
        if shift.state == .locked { return word.uppercased() }
        guard let first = word.first else { return word }
        if capital { return first.uppercased() + word.dropFirst() }
        if !autoCapitalizes {
            return autocorrect.isOwnSpelling(word) ? word : word.lowercased()
        }
        if swipeReadsAsName(word) { return first.uppercased() + word.dropFirst() }
        return autocorrect.withoutStrayCapital(word, typed: "")
    }

    /// The strip's other readings after a swipe, cased as its word is (`readings` best first,
    /// the first the one typed). A reading that is a text replacement's shortcut shows as its
    /// replacement ("home" as "at my house"), the swiped word's first, as Apple's keyboard
    /// offers one after a swipe (black box, iOS 27.2); space never puts it in.
    func swipeAlternatives(_ readings: [String], capital: Bool) -> [String] {
        func replacement(_ reading: String) -> String? {
            guard target.autocorrectionType != .no else { return nil }
            let typed = capital ? reading.prefix(1).uppercased() + reading.dropFirst() : reading
            guard let replaced = autocorrect.textReplacement(before: typed), replaced.typed.count == typed.count else { return nil }
            return replaced.replacement
        }
        var out: [String] = []
        if let first = readings.first, let phrase = replacement(first) { out.append(phrase) }
        for reading in readings.dropFirst() {
            let item = replacement(reading) ?? swipeCased(reading, capital: capital)
            if !out.contains(item) { out.append(item) }
        }
        return Array(out.prefix(3))
    }

    /// Whether a swipe's reading, as the model spells it (lowercase), is a word the dictionary
    /// knows only with a capital: a name, day or place ("monday"), or an abbreviation.
    func swipeReadsAsName(_ word: String) -> Bool {
        guard autocorrect.hasDictionary, word == word.lowercased(), word.count >= 2, let first = word.first else { return false }
        return autocorrect.checker.isWordValid(first.uppercased() + word.dropFirst()) && !autocorrect.checker.isWordValid(word)
    }

    /// Nats more a rare reading the dictionary knows only with a capital costs, unless it is
    /// one of your own words: the model's lowercase list holds names and abbreviations ("gs",
    /// "ut", "kent") that a loose swipe of a common word could read as, and they would go in
    /// capitalised mid-sentence. Common ones ("john", "york", "english") cost nothing more.
    var nameCost = 4.0
    /// The model's cost (-ln p) past which such a word counts as rare: about its 2,500th word.
    static let commonNameCost = 10.0

    /// How far (key pitches) a swipe may go from where it went down and still be a tap
    /// that slid, when it lifts on the key it started on.
    static let slidTapReach: CGFloat = 1.6

    /// The letter of a swipe that came back to the key it went down on without going more
    /// than `slidTapReach` pitches from it ("a" dragged a little too far and back): the
    /// decoder reads no word of one letter, so it read "as". Nil for any other swipe.
    func slidTapLetter(_ stroke: Stroke, threshold: CGFloat) -> String? {
        guard let first = stroke.points.first, let last = stroke.points.last else { return nil }
        let reach = threshold * Self.slidTapReach
        guard stroke.points.allSatisfy({ hypot($0.x - first.x, $0.y - first.y) <= reach }) else { return nil }
        guard let down = stroke.hitTest(first), down == stroke.hitTest(last),
              let letter = down.character, letter.count == 1 else { return nil }
        return letter.lowercased()
    }

    /// Whether `word` begins and ends on `letter`'s key ("lol" on l, "pop" on p): a word a
    /// swipe that comes back to its key can mean.
    static func word(_ word: String, startsAndEndsWith letter: String) -> Bool {
        func base(_ character: Character?) -> String? {
            character.map { String($0).lowercased().decomposedStringWithCanonicalMapping.unicodeScalars.first.map(String.init) ?? "" }
        }
        return word.count > 1 && base(word.first) == letter && base(word.last) == letter
    }

    /// The readings best first once a rare name or abbreviation costs `nameCost` more.
    func ranked(_ candidates: [SwipeDecoder.Candidate], model: TypingDecoderModel) -> [String] {
        func rareName(_ text: String) -> Bool {
            guard swipeReadsAsName(text) else { return false }
            return model.index(of: text).map { model.unigramCost($0) > Self.commonNameCost } ?? true
        }
        return candidates
            .map { ($0.text, $0.cost + (rareName($0.text) ? nameCost : 0)) }
            .enumerated()
            .sorted { $0.element.1 != $1.element.1 ? $0.element.1 < $1.element.1 : $0.offset < $1.offset }
            .map(\.element.0)
    }

    /// Whether a swiped word needs a space before it, as Apple's keyboard adds one: unless
    /// the text before the cursor is empty or ends in a space, a line break or opening
    /// punctuation (a bracket, an opening quote, ¿ or ¡, or a mark that starts a word such
    /// as @ # /). A straight quote opens when nothing or a space comes before it.
    static func needsSpace(before: String?) -> Bool {
        guard let before, let last = before.last else { return false }
        if last.isWhitespace || last.isNewline || opening.contains(last) { return false }
        if last == "\"" || last == "'" {
            let previous = before.dropLast().last
            return !(previous == nil || previous!.isWhitespace || previous!.isNewline || opening.contains(previous!))
        }
        return true
    }

    private static let opening: Set<Character> = ["(", "[", "{", "<", "«", "‹", "“", "‘", "„", "¿", "¡", "@", "#", "/"]
}

extension SwipeRulesRunner {
    func oracle(_ profile: Int, _ state: SwipeRules.State) -> SwipeRulesOracle {
        SwipeRulesOracle(
            shift: .init(state: state.shift, isHeld: state.shiftHeld),
            target: .init(autocorrectionType: state.autocorrection ? .default : .no),
            autoCapitalizes: state.autoCapitalizes, autoCapitalizationMode: state.mode, typingContext: state.before,
            autocorrect: engines[profile]
        )
    }

    static let plain = SwipeRules.State(before: nil, shift: .off, shiftHeld: false, mode: .sentences, autoCapitalizes: true, autocorrection: true)

    /// The case's outcome by the Swift oracle, as text.
    func swift(_ c: SwipeRules.Case) -> String {
        switch c {
        case let .typed(profile, readings, state):
            let o = oracle(profile, state)
            let capital = o.swipeCapital
            return describe(SwipeDecoder.Typed(
                space: SwipeRulesOracle.needsSpace(before: state.before), word: o.swipeCased(readings[0], capital: capital),
                alternatives: o.swipeAlternatives(readings, capital: capital)
            ))
        case let .cased(profile, word, capital, state):
            return oracle(profile, state).swipeCased(word, capital: capital)
        case let .name(profile, word):
            return String(oracle(profile, Self.plain).swipeReadsAsName(word))
        case let .space(before):
            return String(SwipeRulesOracle.needsSpace(before: before))
        case let .rank(profile, language, candidates):
            return oracle(profile, Self.plain).ranked(candidates, model: decoders[language]!.model).joined(separator: "|")
        case let .slid(points, pitch, keys, first):
            // The first and last points are the ones hit-tested.
            let stroke = SwipeRulesOracle.Stroke(points: points) { point in point == points.first ? keys.first! : keys.last! }
            let letter = oracle(0, Self.plain).slidTapLetter(stroke, threshold: pitch)
            let tap = letter.map { letter in first.map { !SwipeRulesOracle.word($0, startsAndEndsWith: letter) } ?? true } ?? false
            return String(tap)
        case let .ends(word, letter):
            return String(SwipeRulesOracle.word(word, startsAndEndsWith: letter))
        }
    }
}
