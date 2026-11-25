import Foundation

/// Standard Korean two-set (Dubeolsik, 두벌식) composition, without UIKit: the jamo typed since
/// the last commit go in, and the text a field should show comes out. A run is the jamo between
/// commit points (space, return, punctuation, a language or page change); `backspace()` takes
/// the last one back, so a compound vowel or final splits one step at a time and 한 becomes 하,
/// then ㅎ, then nothing. The keyboard keeps the run in the field itself, so a commit is only
/// `reset()` (`FullKeyboardEngine+Hangul`).
nonisolated struct HangulComposer {
    /// The jamo typed since the last commit, in order: what `backspace()` steps through.
    private(set) var jamo: [String] = []
    /// What the run reads as now, one precomposed syllable per initial and medial.
    private(set) var text = ""

    var isEmpty: Bool { jamo.isEmpty }

    /// Adds a compatibility jamo and returns the run's new text. A jamo that isn't one changes
    /// nothing.
    @discardableResult
    mutating func type(_ jamo: String) -> String {
        guard Self.isJamo(jamo) else { return text }
        self.jamo.append(jamo)
        text = Self.compose(self.jamo)
        return text
    }

    /// Takes the last typed jamo back and returns the run's new text; nil when nothing is left
    /// to take back.
    @discardableResult
    mutating func backspace() -> String? {
        guard !jamo.isEmpty else { return nil }
        jamo.removeLast()
        text = Self.compose(jamo)
        return text
    }

    /// The run is in the field: forget it, so the next jamo starts a new syllable.
    mutating func reset() {
        jamo.removeAll()
        text = ""
    }

    // MARK: - Jamo

    /// The compatibility jamo a letter key types: the nineteen initial consonants (the five
    /// doubled ones on shift) and the twenty-one vowels.
    static let initials = [
        "ㄱ", "ㄲ", "ㄴ", "ㄷ", "ㄸ", "ㄹ", "ㅁ", "ㅂ", "ㅃ", "ㅅ",
        "ㅆ", "ㅇ", "ㅈ", "ㅉ", "ㅊ", "ㅋ", "ㅌ", "ㅍ", "ㅎ",
    ]

    /// The vowels in Unicode's order, compound ones included (`compoundVowel` makes those).
    static let medials = [
        "ㅏ", "ㅐ", "ㅑ", "ㅒ", "ㅓ", "ㅔ", "ㅕ", "ㅖ", "ㅗ", "ㅘ", "ㅙ",
        "ㅚ", "ㅛ", "ㅜ", "ㅝ", "ㅞ", "ㅟ", "ㅠ", "ㅡ", "ㅢ", "ㅣ",
    ]

    /// The finals in Unicode's order, none first. ㄸ ㅃ ㅉ are initials only.
    static let finals = [
        "", "ㄱ", "ㄲ", "ㄳ", "ㄴ", "ㄵ", "ㄶ", "ㄷ", "ㄹ", "ㄺ", "ㄻ", "ㄼ", "ㄽ", "ㄾ",
        "ㄿ", "ㅀ", "ㅁ", "ㅂ", "ㅄ", "ㅅ", "ㅆ", "ㅇ", "ㅈ", "ㅊ", "ㅋ", "ㅌ", "ㅍ", "ㅎ",
    ]

    /// A letter key's shifted jamo: the five doubled consonants and the two shifted vowels.
    static func shifted(_ jamo: String) -> String {
        switch jamo {
        case "ㅂ": "ㅃ"
        case "ㅈ": "ㅉ"
        case "ㄷ": "ㄸ"
        case "ㄱ": "ㄲ"
        case "ㅅ": "ㅆ"
        case "ㅐ": "ㅒ"
        case "ㅔ": "ㅖ"
        default: jamo
        }
    }

    /// Whether `text` is one compatibility jamo a key may type.
    static func isJamo(_ text: String) -> Bool {
        text.count == 1 && (initialIndex[text] != nil || medialIndex[text] != nil)
    }

    // MARK: - Composition

    private struct Syllable {
        var initial: String?
        var medial: String?
        var final: String?
    }

    /// Composes a run left to right, as Dubeolsik does:
    ///
    /// - a consonant after an initial and a medial becomes its final, or joins a compound final
    ///   (ㄹ then ㄱ is ㄺ), and otherwise starts a new syllable (한 then ㄸ is 한ㄸ);
    /// - a vowel after an initial makes a syllable, after a medial makes a compound vowel (ㅗ then
    ///   ㅏ is ㅘ) or a vowel of its own, and after a final moves that final (the second half of
    ///   a compound one) to the next syllable (한 then ㅏ is 하나, 닭 then ㅏ is 달가);
    /// - a lone consonant or vowel is shown as its compatibility jamo (ㅎ, ㅘ).
    private static func compose(_ jamo: [String]) -> String {
        var out = ""
        var current: Syllable?

        for typed in jamo {
            if medialIndex[typed] != nil {
                guard var syllable = current else {
                    current = Syllable(medial: typed)
                    continue
                }
                if syllable.medial == nil {
                    syllable.medial = typed
                    current = syllable
                } else if syllable.final == nil, let compound = compoundVowel(syllable.medial!, typed) {
                    syllable.medial = compound
                    current = syllable
                } else if let final = syllable.final {
                    let (kept, moved) = splitFinal(final)
                    syllable.final = kept
                    out += render(syllable)
                    current = Syllable(initial: moved, medial: typed)
                } else {
                    out += render(syllable)
                    current = Syllable(medial: typed)
                }
            } else {
                guard var syllable = current else {
                    current = Syllable(initial: typed)
                    continue
                }
                guard syllable.medial != nil, syllable.initial != nil else {
                    // A lone initial or vowel can't take a final.
                    out += render(syllable)
                    current = Syllable(initial: typed)
                    continue
                }
                if syllable.final == nil, isFinal(typed) {
                    syllable.final = typed
                    current = syllable
                } else if let final = syllable.final, let compound = compoundFinal(final, typed) {
                    syllable.final = compound
                    current = syllable
                } else {
                    out += render(syllable)
                    current = Syllable(initial: typed)
                }
            }
        }
        if let current { out += render(current) }
        return out
    }

    /// One syllable as Unicode writes it: 0xAC00 + (initial × 21 + medial) × 28 + final, and a
    /// lone jamo as its compatibility character.
    private static func render(_ syllable: Syllable) -> String {
        if let initial = syllable.initial, let medial = syllable.medial,
           let l = initialIndex[initial], let v = medialIndex[medial] {
            let t = syllable.final.flatMap { finalIndex[$0] } ?? 0
            let code = 0xAC00 + (l * 21 + v) * 28 + t
            guard let scalar = UnicodeScalar(UInt32(code)) else { return "" }
            return String(Character(scalar))
        }
        return syllable.initial ?? syllable.medial ?? syllable.final ?? ""
    }

    private static func isFinal(_ jamo: String) -> Bool {
        finalIndex[jamo] != nil
    }

    /// The compound vowel `second` makes with `first`, or nil.
    private static func compoundVowel(_ first: String, _ second: String) -> String? {
        compoundVowels["\(first)\(second)"]
    }

    /// The compound final `second` makes with `first`, or nil.
    private static func compoundFinal(_ first: String, _ second: String) -> String? {
        compoundFinals["\(first)\(second)"]
    }

    /// A final split into what stays and what starts the next syllable: a compound final's
    /// halves, or nil and the whole final.
    private static func splitFinal(_ jamo: String) -> (kept: String?, moved: String) {
        guard let halves = finalHalves[jamo], let moved = halves.last else {
            return (nil, jamo)
        }
        return (kept: halves.first, moved: moved)
    }

    // MARK: - Tables

    private static let initialIndex: [String: Int] = Dictionary(uniqueKeysWithValues: initials.enumerated().map { pair -> (String, Int) in
        (pair.element, pair.offset)
    })
    private static let medialIndex: [String: Int] = Dictionary(uniqueKeysWithValues: medials.enumerated().map { pair -> (String, Int) in
        (pair.element, pair.offset)
    })
    private static let finalIndex: [String: Int] = Dictionary(uniqueKeysWithValues: finals.enumerated().compactMap { pair -> (String, Int)? in
        pair.element.isEmpty ? nil : (pair.element, pair.offset)
    })

    private static let compoundVowels: [String: String] = [
        "ㅗㅏ": "ㅘ", "ㅗㅐ": "ㅙ", "ㅗㅣ": "ㅚ",
        "ㅜㅓ": "ㅝ", "ㅜㅔ": "ㅞ", "ㅜㅣ": "ㅟ",
        "ㅡㅣ": "ㅢ",
    ]

    private static let compoundFinals: [String: String] = [
        "ㄱㅅ": "ㄳ", "ㄴㅈ": "ㄵ", "ㄴㅎ": "ㄶ", "ㄹㄱ": "ㄺ", "ㄹㅁ": "ㄻ",
        "ㄹㅂ": "ㄼ", "ㄹㅅ": "ㄽ", "ㄹㅌ": "ㄾ", "ㄹㅍ": "ㄿ", "ㄹㅎ": "ㅀ",
        "ㅂㅅ": "ㅄ",
    ]

    private static let finalHalves: [String: [String]] = [
        "ㄳ": ["ㄱ", "ㅅ"], "ㄵ": ["ㄴ", "ㅈ"], "ㄶ": ["ㄴ", "ㅎ"],
        "ㄺ": ["ㄹ", "ㄱ"], "ㄻ": ["ㄹ", "ㅁ"], "ㄼ": ["ㄹ", "ㅂ"],
        "ㄽ": ["ㄹ", "ㅅ"], "ㄾ": ["ㄹ", "ㅌ"], "ㄿ": ["ㄹ", "ㅍ"],
        "ㅀ": ["ㄹ", "ㅎ"], "ㅄ": ["ㅂ", "ㅅ"],
    ]
}

/// Settings › Labs › Korean keyboard, and the composing run it feeds
/// (`FullKeyboardEngine+Hangul`). The switch is read once when the engine is made and again
/// whenever Settings applies.
nonisolated struct KoreanKeyboardState {
    var isEnabled = KoreanKeyboardExperiment.isEnabled
    var composer = HangulComposer()
    /// The last 32 characters before the run, the field's text after the cursor, and the
    /// field itself when the run was last written: a cursor move to text that ends alike
    /// still has to commit the run.
    var beforeTail = ""
    var afterText: String? = nil
    var documentIdentifier: UUID? = nil
    /// True while `replaceHangul` writes the field. Host notifications arrive asynchronously,
    /// so the recorded text comparison is the real protection; this only fences a callback
    /// that lands during the write.
    var isUpdating = false

    /// The run is in the field: forget it and what the host looked like around it.
    mutating func reset() {
        composer.reset()
        beforeTail = ""
        afterText = nil
        documentIdentifier = nil
    }
}
