import Foundation

/// Labs › Greeklish: Greek dictation in Latin letters ("kalimera ti kaneis"), so it reads
/// the way people text Greek. Apple's Greek speech model writes Greek in its own alphabet, so
/// each Greek word is transliterated by rule: accents and diaeresis go first (\u{0301} tonos,
/// \u{0308} diaeresis, and the polytonic marks), then the digraphs people write as one sound
/// (ου ou, αι ai, μπ b/mp, ντ d/nt, γκ g/gk, γγ ng, τζ tz, τσ ts), then the letters. A diaeresis
/// blocks a digraph across it, so "οϋ" is "oy", not "ou". An all-caps Greek word becomes
/// all-caps Latin and a capitalized one capitalized ("Θα" -> "Tha", "ΘΑ" -> "THA"). Greek
/// punctuation becomes the Latin it looks like: U+037E is "?" and U+0387 is ";", and since
/// Unicode normalization usually writes them as a plain ";" (after Greek) and "·" (U+00B7), a
/// ";" directly after a Greek letter becomes "?" and a "·" directly after one becomes ";".
/// Everything that isn't Greek is left exactly as it is, Latin words, digits, emoji and
/// whitespace included.
///
/// String rules and no table, no model, so in-keyboard dictation needs no more memory.
nonisolated enum GreeklishRomanizer {
    static func romanize(_ text: String) -> String {
        var output = ""
        var word: [Letter] = []

        func flush() {
            guard !word.isEmpty else { return }
            output += latin(for: word)
            word = []
        }

        let scalars = Array(text.unicodeScalars)
        var index = 0
        while index < scalars.count {
            let scalar = scalars[index]
            let followsGreek = !word.isEmpty
            switch scalar.value {
            case 0x037E: // Greek question mark
                flush()
                output += "?"
                index += 1
            case 0x0387: // ano teleia
                flush()
                output += ";"
                index += 1
            case 0x003B: // ";" after Greek: normalization of the Greek question mark
                flush()
                output += followsGreek ? "?" : ";"
                index += 1
            case 0x00B7: // "·" after Greek: normalization of the ano teleia
                flush()
                output += followsGreek ? ";" : "·"
                index += 1
            default:
                if isGreekLetter(scalar) {
                    let decomposed = Array(String(scalar).decomposedStringWithCanonicalMapping.unicodeScalars)
                    let barred = decomposed.dropFirst().contains { $0.value == 0x0308 }
                    let uppercase = decomposed.first?.properties.isUppercase ?? false
                    let base = decomposed.first.map { String($0).lowercased() } ?? ""
                    if let character = base.first {
                        word.append(Letter(base: character, barred: barred, uppercase: uppercase))
                    } else {
                        flush()
                        output.append(String(scalar))
                    }
                    index += 1
                    // A mark typed on its own (NFD input) belongs to the letter before it.
                    while index < scalars.count, isCombiningMark(scalars[index]) {
                        if scalars[index].value == 0x0308, !word.isEmpty {
                            word[word.count - 1].barred = true
                        }
                        index += 1
                    }
                } else if isCombiningMark(scalar) {
                    // A mark that follows non-Greek text is part of that text, not ours.
                    flush()
                    output.append(String(scalar))
                    index += 1
                } else {
                    flush()
                    output.append(String(scalar))
                    index += 1
                }
            }
        }
        flush()
        return output
    }

    // MARK: A Greek word

    private struct Letter {
        /// The lowercase Greek letter.
        var base: Character
        /// It carried a diaeresis (ϊ, ϋ, ΐ, ΰ), which keeps it out of a digraph.
        var barred: Bool
        /// It was written uppercase, for the word's case.
        var uppercase: Bool
    }

    /// The word in Latin letters, in the case it was written: all caps stays all caps (but a
    /// one-letter word only takes its capital: "Θ" -> "Th"), a capitalized word keeps its
    /// capital, anything else is lowercase.
    private static func latin(for word: [Letter]) -> String {
        var output = ""
        var index = 0
        while index < word.count {
            if index + 1 < word.count, !word[index].barred, !word[index + 1].barred,
               let merged = digraph(word[index].base, word[index + 1].base, wordStart: index == 0) {
                output += merged
                index += 2
            } else {
                output += letter(word[index].base)
                index += 1
            }
        }
        if word.count > 1, word.allSatisfy({ $0.uppercase }) {
            return output.uppercased()
        }
        let capitalized = word[0].uppercase && word.dropFirst().allSatisfy { !$0.uppercase }
        if capitalized, let first = output.first {
            return String(first).uppercased() + output.dropFirst()
        }
        return output
    }

    /// The digraphs people text, longest match first. μπ, ντ and γκ are b, d and g only at the
    /// start of a word ("μπαμπάς" -> babas, "ντουλάπα" -> doulapa, "γκαράζ" -> garaz) and mp,
    /// nt and gk inside one ("αμπέλι" -> ampeli, "πέντε" -> pente, "αγκάλη" -> agkali).
    private static func digraph(_ first: Character, _ second: Character, wordStart: Bool) -> String? {
        switch (first, second) {
        case ("ο", "υ"): return "ou"
        case ("α", "ι"): return "ai"
        case ("ε", "ι"): return "ei"
        case ("ο", "ι"): return "oi"
        case ("α", "υ"): return "av"
        case ("ε", "υ"): return "ev"
        case ("μ", "π"): return wordStart ? "b" : "mp"
        case ("ν", "τ"): return wordStart ? "d" : "nt"
        case ("γ", "κ"): return wordStart ? "g" : "gk"
        case ("γ", "γ"): return "ng"
        case ("τ", "ζ"): return "tz"
        case ("τ", "σ"): return "ts"
        default: return nil
        }
    }

    private static func letter(_ letter: Character) -> String {
        switch letter {
        case "α": return "a"
        case "β": return "v"
        case "γ": return "g"
        case "δ": return "d"
        case "ε": return "e"
        case "ζ": return "z"
        case "η": return "i"
        case "θ": return "th"
        case "ι": return "i"
        case "κ": return "k"
        case "λ": return "l"
        case "μ": return "m"
        case "ν": return "n"
        case "ξ": return "x"
        case "ο": return "o"
        case "π": return "p"
        case "ρ": return "r"
        case "σ", "ς": return "s"
        case "τ": return "t"
        case "υ": return "y"
        case "φ": return "f"
        case "χ": return "ch"
        case "ψ": return "ps"
        case "ω": return "o"
        // Variant Greek letters people's keyboards and fonts use, read as their base letter.
        case "ϐ": return "v"
        case "ϑ": return "th"
        case "ϕ": return "f"
        case "ϖ": return "p"
        case "ϰ": return "k"
        case "ϱ": return "r"
        case "ϲ": return "s"
        case "ϵ": return "e"
        default: return String(letter)
        }
    }

    // MARK: Script

    /// Greek letters, including the polytonic letters Unicode decomposes for us, but not the
    /// Coptic letters that share the block, the symbol at U+03F6, or Greek's spacing marks and
    /// symbols (U+1FBD, U+1FBF–1FC1, U+1FCD–1FCF, U+1FDD–1FDF, U+1FED–1FEF, U+1FFD–1FFE).
    private static func isGreekLetter(_ scalar: Unicode.Scalar) -> Bool {
        guard scalar.properties.isAlphabetic else { return false }
        switch scalar.value {
        case 0x0370...0x03FF:
            return !(0x03E2...0x03EF).contains(scalar.value) && scalar.value != 0x03F6
        case 0x1F00...0x1FFF:
            switch scalar.value {
            case 0x1FBD, 0x1FBF...0x1FC1, 0x1FCD...0x1FCF,
                 0x1FDD...0x1FDF, 0x1FED...0x1FEF, 0x1FFD...0x1FFE:
                return false
            default:
                return true
            }
        default:
            return false
        }
    }

    private static func isCombiningMark(_ scalar: Unicode.Scalar) -> Bool {
        switch scalar.value {
        case 0x0300...0x036F, 0x1AB0...0x1AFF, 0x1DC0...0x1DFF, 0x20D0...0x20FF, 0xFE20...0xFE2F:
            return true
        default:
            return false
        }
    }
}
