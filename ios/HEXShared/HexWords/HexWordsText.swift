import Foundation

/// The Hex words file's text rules (`docs/specs/hex-words.md`, "Cleaning text"): the same on
/// every platform, so each step works on Unicode scalars, never on Swift's graphemes or
/// Foundation's own idea of whitespace.
nonisolated enum HexWordsText {
    /// Unicode's White_Space set, exactly the spec's list.
    static func isWhitespace(_ scalar: Unicode.Scalar) -> Bool {
        switch scalar.value {
        case 0x09...0x0D, 0x20, 0x85, 0xA0, 0x1680, 0x2000...0x200A, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000:
            true
        default:
            false
        }
    }

    /// Characters an entry may not hold after cleaning; `write` may keep LF and TAB.
    static func isHidden(_ scalar: Unicode.Scalar, inWrite: Bool) -> Bool {
        switch scalar.value {
        case 0x09, 0x0A:
            !inWrite
        case 0x00...0x1F, 0x7F...0x9F:
            true
        case 0xAD, 0x61C, 0x180E, 0x200B, 0x200E, 0x200F, 0x202A...0x202E, 0x2060...0x2064,
             0x2066...0x2069, 0xFEFF, 0xFFF9...0xFFFB:
            true
        default:
            false
        }
    }

    /// NFC, then every run of whitespace as one space, none at either end: `term`,
    /// `soundsLike`, `heard` and `madeBy`.
    static func cleanLine(_ text: String) -> String {
        var scalars = String.UnicodeScalarView()
        var pendingSpace = false
        for scalar in text.precomposedStringWithCanonicalMapping.unicodeScalars {
            if isWhitespace(scalar) {
                pendingSpace = true
                continue
            }
            if pendingSpace, !scalars.isEmpty { scalars.append(" ") }
            pendingSpace = false
            scalars.append(scalar)
        }
        return String(scalars)
    }

    /// NFC, and CR LF or a lone CR as LF; nothing else changes (`write`).
    static func cleanWrite(_ text: String) -> String {
        var scalars = String.UnicodeScalarView()
        var afterCR = false
        for scalar in text.precomposedStringWithCanonicalMapping.unicodeScalars {
            if scalar == "\r" {
                scalars.append("\n")
                afterCR = true
                continue
            }
            if scalar == "\n", afterCR {
                afterCR = false
                continue
            }
            afterCR = false
            scalars.append(scalar)
        }
        return String(scalars)
    }

    static func hasHidden(_ text: String, inWrite: Bool = false) -> Bool {
        text.unicodeScalars.contains { isHidden($0, inWrite: inWrite) }
    }

    /// General category L or N.
    static func isLetterOrDigit(_ scalar: Unicode.Scalar) -> Bool {
        switch scalar.properties.generalCategory {
        case .uppercaseLetter, .lowercaseLetter, .titlecaseLetter, .modifierLetter, .otherLetter,
             .decimalNumber, .letterNumber, .otherNumber:
            true
        default:
            false
        }
    }

    static func hasLetterOrDigit(_ text: String) -> Bool {
        text.unicodeScalars.contains(where: isLetterOrDigit)
    }

    /// Characters are Unicode code points.
    static func length(_ text: String) -> Int {
        text.unicodeScalars.count
    }

    /// The pieces between single spaces of a cleaned line.
    static func wordCount(_ cleaned: String) -> Int {
        cleaned.isEmpty ? 0 : cleaned.unicodeScalars.reduce(1) { $0 + ($1 == " " ? 1 : 0) }
    }

    /// Only whitespace, or nothing: a deletion.
    static func isBlank(_ text: String) -> Bool {
        text.unicodeScalars.allSatisfy(isWhitespace)
    }

    /// The comparison key: the cleaned text with Unicode's full, locale-independent lowercase
    /// mapping, then every final sigma as σ.
    static func key(_ text: String) -> String {
        var scalars = String.UnicodeScalarView()
        for scalar in cleanLine(text).unicodeScalars {
            for lower in scalar.properties.lowercaseMapping.unicodeScalars {
                scalars.append(lower == "\u{03C2}" ? "\u{03C3}" : lower)
            }
        }
        return String(scalars)
    }

    /// A phone's term key: the key's letters and digits only, so "Next.js" and "NextJS" match.
    static func phoneTermKey(_ text: String) -> String {
        String(String.UnicodeScalarView(key(text).unicodeScalars.filter(isLetterOrDigit)))
    }

    /// For the review sheet: the first 64 characters and "…", in its own direction-isolated run
    /// so a right-to-left string can't reorder the labels around it.
    static func display(_ text: String, limit: Int = 64) -> String {
        let characters = Array(text)
        let shown = characters.count > limit ? String(characters.prefix(limit)) + "…" : text
        return "\u{2068}" + shown + "\u{2069}"
    }
}
