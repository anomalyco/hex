import CoreGraphics

/// Apple's key flicks on the docked iPad keyboard (Settings › General › Keyboard › Enable Key
/// Flicks): every letter key carries a small grey character above its letter, and pulling the
/// key down types it. Read on Apple's iPad mini keyboard (iPadOS 27.2 simulator, black box,
/// 2026-11-23), the rows Hex draws on every docked iPad (`KeyboardPadRows.swift`):
///
/// - English and German: 1 to 0 over row 1, @ # $ & * ( ) ' " over row 2, % - + = / ; : over
///   row 3; French (AZERTY): 1 to 0, @ # & " $ ( ! ) - *, and % _ + = / ;.
/// - The 123 page: the #+= page's character over each symbol (€ over @, § over %), not on the
///   digits.
/// - The two-symbol keys after the letters keep their own pair and flick nothing.
///
/// Drawn about 12.5 pt high in a grey a little above the key's middle top, the letter set
/// lower; while pulled, the character slides down to the middle and darkens as the letter
/// fades (none of it moves at 8 pt, about half way at 16 pt, all the way by 30 pt). A
/// keyboard extension can't read Apple's switch (the Custom Keyboard guide: no access to the
/// general keyboard settings), so Hex has its own (Settings › Keyboard › Key flicks).
nonisolated enum KeyboardFlicks {
    /// Settings › Keyboard › Key flicks, set by the keyboard view from the App Group.
    nonisolated(unsafe) static var isEnabled = true

    /// No change below this pull (pt).
    static let start: CGFloat = 8
    /// Pull (pt) from `start` to the character in place.
    static let travel: CGFloat = 18
    /// Let go at or past this share of the way and the character goes in; short of it (or
    /// pulled back up), the letter.
    static let commitProgress: CGFloat = 0.5

    /// How far a pull of `dy` points down has brought the character, 0 to 1.
    static func progress(pull dy: CGFloat) -> CGFloat {
        min(1, max(0, (dy - start) / travel))
    }

    private static let digits = "1234567890".map(String.init)
    private static let englishMiddle = ["@", "#", "$", "&", "*", "(", ")", "'", "\""]
    private static let englishBottom = ["%", "-", "+", "=", "/", ";", ":"]
    private static let frenchMiddle = ["@", "#", "&", "\"", "$", "(", "!", ")", "-", "*"]
    private static let frenchBottom = ["%", "_", "+", "=", "/", ";"]
    /// The 123 page's symbols and the #+= page's character Apple puts over each.
    private static let numbersPage: [String: String] = [
        "@": "€", "#": "£", "$": "¥", "&": "_", "*": "^", "(": "[", ")": "]", "'": "{", "\"": "}",
        "%": "§", "-": "|", "+": "~", "=": "…", "/": "\\", ";": "<", ":": ">",
    ]

    /// Each key's flick on the letters and 123 pages. Measured for English, German and French;
    /// every other language gets the digits over a ten-letter row 1 (Apple's own for them
    /// unmeasured), and rows 2 and 3 only where they have English's nine and seven letters.
    static func add(to keys: inout [KeyboardKey], layer: KeyboardLayer, language: KeyboardLanguage) {
        switch layer {
        case .symbols:
            return
        case .numbers:
            for i in keys.indices where keys[i].shifted == nil && keys[i].row > 0 {
                if let c = keys[i].character { keys[i].flick = numbersPage[c] }
            }
        case .letters:
            let measured = language == .english || language == .german
            let rows: [[String]] = language.isFrench
                ? [digits, frenchMiddle, frenchBottom]
                : [digits, measured ? englishMiddle : [], measured ? englishBottom : []]
            for row in 0..<3 {
                let letters = keys.indices
                    .filter { keys[$0].row == row && keys[$0].shifted == nil && keys[$0].character?.first?.isLetter == true }
                    .sorted { keys[$0].frame.minX < keys[$1].frame.minX }
                let flicks = rows[row]
                if row == 0, letters.count != 10 { continue }
                if row > 0, !language.isFrench, letters.count != flicks.count { continue }
                for (n, i) in letters.enumerated() where n < flicks.count {
                    keys[i].flick = flicks[n]
                }
            }
        }
    }
}
