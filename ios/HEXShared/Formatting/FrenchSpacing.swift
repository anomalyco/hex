import Foundation

/// French typography's spaces before `? ! ; :` and inside `« »`, the dictation pipeline's last
/// step (`DictationPipeline.format`). Apple Speech doesn't write them (black box, macOS 27.2.1:
/// fr-FR and fr-CH write a plain space before "?", fr-CA's DictationTranscriber none) and the
/// styles take out every space before a mark, so French came out as "ça va?". Apple's French
/// keyboards don't add one either (iOS 27.2 simulator), so the full keyboard types marks as you
/// tap them.
///
/// - France (and every French but Canada's and Switzerland's): a narrow no-break space
///   (U+202F) before `? ! ;`, a no-break space (U+00A0) before `:` and inside `« »`.
/// - Canada (OQLF): no space before `? ! ;`, a no-break space before `:` and inside `« »`.
/// - Switzerland (Guide du typographe): narrow no-break spaces before `? ! ;` and inside
///   `« »`, a no-break space before `:`.
///
/// Whatever space was there (none, one, several, or another kind) becomes exactly that one. A
/// mark is spaced only where it ends a word: followed by a space, a line break, the end,
/// another mark or a closing quote or bracket. So times (10:30), ratios, URLs ("https://",
/// "?q=1"), a URL or address the mark is typed against, emoticons (":)", ";-)", ":D"), code in
/// backticks, a mark at a line's start, and the second mark of "?!" stay as they are.
nonisolated enum FrenchSpacing {
    enum Convention: Equatable {
        case france
        case canada
        case switzerland
    }

    static let narrowNoBreakSpace: Character = "\u{202F}"
    static let noBreakSpace: Character = "\u{00A0}"

    /// The convention for `language` (BCP 47), or nil when it isn't French.
    static func convention(for language: String?) -> Convention? {
        guard let language else { return nil }
        let locale = Locale(identifier: language)
        guard locale.language.languageCode?.identifier == "fr" else { return nil }
        switch locale.region?.identifier {
        case "CA": return .canada
        case "CH": return .switzerland
        default: return .france
        }
    }

    /// `text` with French spacing when `language` is French; otherwise unchanged.
    static func apply(_ text: String, language: String?) -> String {
        guard let convention = convention(for: language) else { return text }
        return apply(text, convention: convention)
    }

    static func apply(_ text: String, convention: Convention) -> String {
        let chars = Array(text)
        guard chars.contains(where: { "?!;:«»".contains($0) }) else { return text }
        let beforeQuestion: Character? = convention == .canada ? nil : narrowNoBreakSpace
        let insideGuillemets = convention == .switzerland ? narrowNoBreakSpace : noBreakSpace

        var out: [Character] = []
        out.reserveCapacity(chars.count + 8)
        // Spaces read but not yet written: a mark after them may replace them.
        var pending: [Character] = []
        var inCode = false
        var afterOpeningGuillemet = false

        func flush() {
            out.append(contentsOf: pending)
            pending.removeAll()
        }

        for (index, ch) in chars.enumerated() {
            let next = index + 1 < chars.count ? chars[index + 1] : nil
            if ch == "`" {
                inCode.toggle()
                afterOpeningGuillemet = false
                flush()
                out.append(ch)
                continue
            }
            if inCode {
                flush()
                out.append(ch)
                continue
            }
            if isHorizontalSpace(ch) {
                pending.append(ch)
                continue
            }
            if afterOpeningGuillemet {
                afterOpeningGuillemet = false
                if !ch.isNewline, ch != "»" {
                    pending = [insideGuillemets]
                }
            }
            switch ch {
            case "?", "!", ";", ":":
                if endsWord(ch, next: next),
                   let space = spaceBefore(ch, out: out, pending: pending, question: beforeQuestion) {
                    pending.removeAll()
                    if let space { out.append(space) }
                } else {
                    flush()
                }
                out.append(ch)
            case "«":
                flush()
                out.append(ch)
                afterOpeningGuillemet = true
            case "»":
                if let last = out.last, !last.isNewline, last != "«" {
                    pending = [insideGuillemets]
                }
                flush()
                out.append(ch)
            default:
                flush()
                out.append(ch)
            }
        }
        if !afterOpeningGuillemet { flush() }
        return String(out)
    }

    private static func isHorizontalSpace(_ ch: Character) -> Bool {
        ch == " " || ch == "\t" || ch == noBreakSpace || ch == narrowNoBreakSpace || ch == "\u{2009}"
    }

    /// Whether `mark` ends a word here, rather than sitting inside a time, a URL or an
    /// emoticon: what follows is nothing, a space, a line break, another mark or a closing
    /// quote or bracket, and for `:` and `;` not an emoticon's mouth (":)", ";(").
    private static func endsWord(_ mark: Character, next: Character?) -> Bool {
        guard let next else { return true }
        if mark == ":" || mark == ";", ")(][|".contains(next) { return false }
        return next.isWhitespace || "?!.…,»”\"’')]}".contains(next)
    }

    /// The space to write before `mark` (`.some(nil)` for none), or nil to leave what is
    /// there: at a line's start, after an opening quote or bracket or another ";" or ":"
    /// ("::"), and for a mark typed
    /// against a URL or an address. The second mark of "?!" goes against the first.
    private static func spaceBefore(
        _ mark: Character, out: [Character], pending: [Character], question: Character?
    ) -> Character?? {
        guard let last = out.last, !last.isNewline, !"(«“[{".contains(last) else { return nil }
        if "?!".contains(mark), "?!".contains(last) { return .some(nil) }
        if ";:".contains(last) { return nil }
        if pending.isEmpty {
            let word = String(out.reversed().prefix { !$0.isWhitespace }.reversed())
            if word.contains("://") || word.contains("@") || word.lowercased().hasPrefix("www.") { return nil }
        }
        return .some(mark == ":" ? noBreakSpace : question)
    }
}
