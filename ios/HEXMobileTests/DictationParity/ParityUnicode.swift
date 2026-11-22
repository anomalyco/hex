import Foundation

/// What the rules read from Unicode, per scalar, as this iOS has it: `Character`'s
/// properties, case mappings, `folding`, Foundation's character sets and the ICU sets
/// `NSRegularExpression` matches. The Rust port's Unicode test reads it
/// (`HEX_DICTATION_UNICODE`), so a property the port gets wrong anywhere shows up.
///
/// One line per scalar: its hex value, its flags, then `lowercased()`, `uppercased()`,
/// `folding` of the scalar and of "a" followed by it, each as hex scalars or `=` when
/// unchanged.
enum ParityUnicode {
    /// L letter, N number, W whitespace, U uppercase, l lowercase, C cased, n newline, A ASCII.
    static func characterFlags(_ character: Character) -> String {
        var flags = ""
        if character.isLetter { flags += "L" }
        if character.isNumber { flags += "N" }
        if character.isWhitespace { flags += "W" }
        if character.isUppercase { flags += "U" }
        if character.isLowercase { flags += "l" }
        if character.isCased { flags += "C" }
        if character.isNewline { flags += "n" }
        if character.isASCII { flags += "A" }
        return flags
    }

    static func export(to path: String) throws {
        let scalars = (0...0x10FFFF).compactMap { Unicode.Scalar(UInt32($0)) }
        // s and t: CharacterSet.whitespaces and .whitespacesAndNewlines. ICU: p \p{L}, d \p{N},
        // S \s, w the word set \b reads, e what \b never stops at (Grapheme_Extend, Cf).
        let icu: [(String, String)] = [
            ("p", "\\p{L}"), ("d", "\\p{N}"), ("S", "\\s"),
            ("w", "[\\p{Alphabetic}\\p{M}\\p{Nd}\\p{Pc}\\u200c\\u200d]"), ("e", "[\\p{Grapheme_Extend}\\p{Cf}]"),
        ]
        var members: [String: Set<UInt32>] = [:]
        var all = String.UnicodeScalarView()
        all.append(contentsOf: scalars)
        let text = String(all)
        let utf16 = Array(text.utf16)
        var scalarAt: [Int: UInt32] = [:]
        var offset = 0
        for scalar in scalars {
            scalarAt[offset] = scalar.value
            offset += scalar.value > 0xFFFF ? 2 : 1
        }
        precondition(offset == utf16.count)
        for (flag, pattern) in icu {
            let regex = try NSRegularExpression(pattern: pattern)
            var set = Set<UInt32>()
            regex.enumerateMatches(in: text, range: NSRange(location: 0, length: utf16.count)) { match, _, _ in
                if let match, let value = scalarAt[match.range.location] { set.insert(value) }
            }
            members[flag] = set
        }
        var out = ""
        out.reserveCapacity(40_000_000)
        func hex(_ string: String, unless original: String) -> String {
            string == original && string.unicodeScalars.elementsEqual(original.unicodeScalars)
                ? "=" : string.unicodeScalars.map { String($0.value, radix: 16) }.joined(separator: " ")
        }
        for scalar in scalars {
            let string = String(scalar)
            var flags = characterFlags(Character(scalar))
            if CharacterSet.whitespaces.contains(scalar) { flags += "s" }
            if CharacterSet.whitespacesAndNewlines.contains(scalar) { flags += "t" }
            for (flag, _) in icu where members[flag]!.contains(scalar.value) { flags += flag }
            let fold = string.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
            let foldA = ("a" + string).folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
            out += String(scalar.value, radix: 16) + "\t" + flags + "\t" + hex(string.lowercased(), unless: string)
                + "\t" + hex(string.uppercased(), unless: string) + "\t" + hex(fold, unless: string)
                + "\t" + hex(foldA, unless: "a" + string) + "\n"
        }
        try out.write(toFile: path, atomically: true, encoding: .utf8)
        print("dictation parity: \(scalars.count) scalars in \(path)")
    }
}
