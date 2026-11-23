import Foundation

/// The strings a typing config brings in (the note, words, fixes, dictation fixes), as the review
/// shows them and the keyboard loads them. Removed:
/// - controls (general category Cc: C0, DEL, C1), the note's newlines and tabs as spaces;
/// - bidirectional formatting: the embeddings and overrides U+202A–U+202E, the isolates
///   U+2066–U+2069, and the marks U+200E, U+200F and U+061C;
/// - invisible format characters (general category Cf): U+200B ZERO WIDTH SPACE, U+2060 WORD
///   JOINER, U+FEFF, U+00AD SOFT HYPHEN, the invisible operators U+2061–U+2064, and the rest;
/// - ZERO WIDTH NON-JOINER and ZERO WIDTH JOINER (U+200C, U+200D) only where they aren't part of
///   a spelling. They are kept between two letters or marks of a script that spells with them
///   (Arabic and Persian, Syriac, the Indic scripts, Sinhala, Myanmar, Khmer), and the joiner
///   between emoji. Tag characters are kept after a black flag (subdivision flags).
/// Words are then NFC-normalised, trimmed, and runs of spaces become one.
nonisolated enum TypingConfigText {
    /// `text` cleaned, and whether anything was removed from inside it (not trimming or NFC).
    static func clean(_ text: String) -> (text: String, removedHidden: Bool) {
        let scalars = Array(text.unicodeScalars)
        var out = String.UnicodeScalarView()
        var removed = false
        for (i, s) in scalars.enumerated() {
            if s == "\n" || s == "\t" || s == "\r" {
                out.append(" ")
                continue
            }
            if keeps(s, before: i > 0 ? scalars[i - 1] : nil, after: i + 1 < scalars.count ? scalars[i + 1] : nil, previous: scalars[..<i]) {
                out.append(s)
            } else {
                removed = true
            }
        }
        var cleaned = String(out).precomposedStringWithCanonicalMapping
        cleaned = cleaned.split(separator: " ", omittingEmptySubsequences: true).joined(separator: " ")
        return (cleaned.trimmingCharacters(in: .whitespacesAndNewlines), removed)
    }

    private static func keeps(_ s: Unicode.Scalar, before: Unicode.Scalar?, after: Unicode.Scalar?, previous: ArraySlice<Unicode.Scalar>) -> Bool {
        switch s.properties.generalCategory {
        case .control:
            return false
        case .format:
            if s.value == 0x200C || s.value == 0x200D {
                if let before, let after, joins(before), joins(after) { return true }
                if s.value == 0x200D, let before, let after, before.properties.isEmoji, !before.isASCII, after.properties.isEmoji, !after.isASCII { return true }
                return false
            }
            // Tag characters spell subdivision flags after U+1F3F4.
            if (0xE0020...0xE007F).contains(s.value), let start = previous.last(where: { !(0xE0020...0xE007F).contains($0.value) }), start.value == 0x1F3F4 {
                return true
            }
            return false
        default:
            return true
        }
    }

    /// A letter or mark of a script whose spelling uses the joiners.
    private static func joins(_ s: Unicode.Scalar) -> Bool {
        let category = s.properties.generalCategory
        guard category == .otherLetter || category == .nonspacingMark || category == .spacingMark || category == .enclosingMark else { return false }
        switch s.value {
        case 0x0600...0x06FF, 0x0750...0x077F, 0x08A0...0x08FF, 0xFB50...0xFDFF, 0xFE70...0xFEFF: return true  // Arabic, Persian
        case 0x0700...0x074F: return true  // Syriac
        case 0x0900...0x0DFF: return true  // Devanagari through Sinhala
        case 0x1000...0x109F: return true  // Myanmar
        case 0x1780...0x17FF: return true  // Khmer
        default: return false
        }
    }
}
