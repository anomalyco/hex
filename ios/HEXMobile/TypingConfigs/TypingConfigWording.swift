import Foundation

/// What the review sheet says about a typing config's parts, in plain words. The review
/// (`TypingConfigReview`) decides; this only names what it decided.
extension TypingConfigReview.Reason {
    /// One sentence a person can read.
    var sentence: String {
        switch self {
        case .notAConfig: String(localized: "This file isn't a typing config Hex can read.")
        case .tooLarge: String(localized: "This file is larger than a typing config can be.")
        case .newerFormat: String(localized: "Made for a newer Hex. Update Hex to use it.")
        case .unknownField: String(localized: "Hex doesn't know this part of the file.")
        case .invalidValue: String(localized: "Incomplete, or holds a value Hex can't use.")
        case .tooMany: String(localized: "More than a typing config can hold.")
        case .duplicate: String(localized: "Already in the file once.")
        case .notEnoughEvidence: String(localized: "The file doesn't show this often enough to be sure.")
        case .forAnotherKeyboard: String(localized: "Made for another keyboard.")
        case .needsCheckingAgain: String(localized: "Measured on an older version of Hex's touch areas, so it needs checking again.")
        case .keyNotOnKeyboard: String(localized: "That key isn't on this keyboard.")
        case .edgeNotAllowed: String(localized: "That edge of the key can't move.")
        case .twoZonesClaimPoint: String(localized: "Two changes claim the same spot, so neither applies.")
        case .keyTooSmall: String(localized: "It would leave a key too small to tap reliably.")
        case .hiddenCharacters: String(localized: "It has hidden characters, so what it types wouldn't be what you see.")
        }
    }
}

/// The groups a review's switchable parts fall in, in the order the sheet shows them.
enum TypingConfigGroup: Int, CaseIterable, Comparable {
    case keep
    case neverCorrectTo
    case fixes
    case dictation
    case aim
    case zones

    init?(_ part: TypingConfigReview.Part) {
        switch part {
        case .keep: self = .keep
        case .neverCorrectTo: self = .neverCorrectTo
        case .fix: self = .fixes
        case .dictation: self = .dictation
        case .aim: self = .aim
        case .zone: self = .zones
        case .note, .unreadable: return nil
        }
    }

    var title: String {
        switch self {
        case .keep: String(localized: "Words to keep")
        case .neverCorrectTo: String(localized: "Never correct to")
        case .fixes: String(localized: "Typo fixes")
        case .dictation: String(localized: "Dictation fixes")
        case .aim: String(localized: "Where you aim")
        case .zones: String(localized: "Touch areas")
        }
    }

    var detail: String {
        switch self {
        case .keep: String(localized: "Autocorrect leaves these alone and offers them.")
        case .neverCorrectTo: String(localized: "Autocorrect never puts these in place of what you typed.")
        case .fixes: String(localized: "A typo you make, and the word you meant.")
        case .dictation: String(localized: "Words dictation mishears, fixed after you dictate.")
        case .aim: String(localized: "Autocorrect and the next-letter guess read taps on these letters from where you really aim. The keys stay where they are.")
        // The next-letter odds still decide a tap near an edge, so this says "more often".
        case .zones: String(localized: "A tap near these edges counts for the key more often. Hex's guess at the next letter can still pick its neighbour. The keys stay where they are.")
        }
    }

    static func < (a: Self, b: Self) -> Bool { a.rawValue < b.rawValue }
}

/// A review item in words: its title and, when it has one, what it does.
struct TypingConfigItemWords {
    let title: String
    let detail: String?

    init(_ item: TypingConfigReview.Item, review: TypingConfigReview) {
        switch item.part {
        case .note:
            title = String(localized: "Note")
            detail = nil
        case .keep(let word), .neverCorrectTo(let word):
            title = item.text ?? word
            detail = nil
        case .fix(let typo, let word):
            title = item.text ?? "\(typo) → \(word)"
            let mode = review.applied.fixes.first { $0.typo == typo && $0.word == word }?.mode
            detail = mode == .apply
                ? String(localized: "Put in when you press space")
                : String(localized: "Offered in the suggestions, never put in by space")
        case .dictation(let heard, let meant):
            title = "“\(heard)” → “\(meant)”"
            detail = nil
        case .aim(let block, let letter):
            title = Self.keyName(letter)
            let applied = review.applied.blocks.first { $0.block == block }?.aim[Character(letter)]
            let offset = Self.clampedTo(item.outcome) ?? applied.map { [Double($0.dx), Double($0.dy)] }
                ?? review.file?.keys?[safe: block]?.value?.aim?[letter]?.value?.offset
            detail = offset.map(Self.aimWords)
        case .zone(let block, let index):
            let zone = review.file?.keys?[safe: block]?.value?.zones?[safe: index]?.value
            title = zone.map { String(localized: "\(Self.keyName($0.key)), \(Self.edgeName($0.edge))") } ?? item.path
            let move = Self.clampedTo(item.outcome)?.first ?? zone?.move
            detail = zone.flatMap { z in move.map { Self.zoneWords(edge: z.edge, move: $0) } }
        case .unreadable(let path):
            title = Self.entryName(item, path: path)
            detail = nil
        }
    }

    /// An entry Hex couldn't read, by its list and place ("Words to keep, number 6"); a field
    /// Hex doesn't know keeps its name in the file.
    static func entryName(_ item: TypingConfigReview.Item, path: String) -> String {
        let group: TypingConfigGroup? = switch item.list {
        case .keep: .keep
        case .neverCorrectTo: .neverCorrectTo
        case .fixes: .fixes
        case .dictation: .dictation
        case .aim: .aim
        case .zones: .zones
        case .file, .keyBlocks: nil
        }
        // A field Hex doesn't know, even inside an entry, is named as the file names it.
        if case .skipped(let reason) = item.outcome, reason == .unknownField || reason == .newerFormat { return path }
        if let group, let position = item.position {
            return String(localized: "\(group.title), number \(position + 1)")
        }
        if item.list == .keyBlocks, let position = item.position {
            return String(localized: "Key positions, number \(position + 1)")
        }
        return path
    }

    /// The short name the review lists a skipped item by.
    static func label(_ item: TypingConfigReview.Item, review: TypingConfigReview) -> String {
        let words = TypingConfigItemWords(item, review: review)
        switch item.part {
        case .aim: return String(localized: "Aim for \(words.title)")
        default: return words.title
        }
    }

    static func keyName(_ key: String) -> String {
        switch key {
        case "space": String(localized: "Space bar")
        case "return": String(localized: "Return")
        case "shift": String(localized: "Shift")
        case "delete": String(localized: "Delete")
        case "layer:numbers": "123"
        // ß has no one-letter capital in Swift's rules.
        default: key.uppercased().count == key.count ? key.uppercased() : key
        }
    }

    static func edgeName(_ edge: String) -> String {
        switch edge {
        case "top": String(localized: "top edge")
        case "bottom": String(localized: "bottom edge")
        case "left": String(localized: "left edge")
        case "right": String(localized: "right edge")
        default: edge
        }
    }

    private static func clampedTo(_ outcome: TypingConfigReview.Outcome) -> [Double]? {
        if case .clamped(_, let to) = outcome { return to }
        return nil
    }

    static func number(_ value: Double) -> String {
        abs(value).formatted(.number.precision(.fractionLength(0...2)))
    }

    /// "You aim 0.28 key lower", "You aim 0.1 key to the left and 0.2 key lower".
    static func aimWords(_ offset: [Double]) -> String {
        guard let parts = aimParts(offset) else { return String(localized: "At the key's centre") }
        return String(localized: "You aim \(parts)")
    }

    /// What a clamped value was in the file, in the same words.
    static func asked(_ item: TypingConfigReview.Item) -> String? {
        guard case .clamped(let from, _) = item.outcome else { return nil }
        let value: String? = switch item.part {
        case .aim: aimParts(from)
        case .zone: from.first.map { String(localized: "\(number($0)) key") }
        default: from.map(number).joined(separator: ", ")
        }
        return value.map { String(localized: "Hex limited this. The file asked for \($0).") }
    }

    /// "0.28 key lower", or nil at the centre.
    static func aimParts(_ offset: [Double]) -> String? {
        guard offset.count == 2 else { return nil }
        var parts: [String] = []
        // A value that rounds to 0 at two decimals isn't named.
        if number(offset[0]) != "0" {
            parts.append(offset[0] > 0
                ? String(localized: "\(number(offset[0])) key to the right")
                : String(localized: "\(number(offset[0])) key to the left"))
        }
        if number(offset[1]) != "0" {
            parts.append(offset[1] > 0
                ? String(localized: "\(number(offset[1])) key lower")
                : String(localized: "\(number(offset[1])) key higher"))
        }
        return parts.isEmpty ? nil : parts.formatted(.list(type: .and))
    }

    /// What moving an edge does, from the key's side.
    static func zoneWords(edge: String, move: Double) -> String {
        let amount = number(move)
        switch (edge, move >= 0) {
        case ("top", true): return String(localized: "Its touch area reaches \(amount) key higher")
        case ("top", false): return String(localized: "Its touch area stops \(amount) key short at the top")
        case ("bottom", true): return String(localized: "Its touch area reaches \(amount) key lower")
        case ("bottom", false): return String(localized: "Its touch area stops \(amount) key short at the bottom")
        case ("left", true): return String(localized: "Its touch area reaches \(amount) key further left")
        case ("left", false): return String(localized: "Its touch area stops \(amount) key short on the left")
        case (_, true): return String(localized: "Its touch area reaches \(amount) key further right")
        default: return String(localized: "Its touch area stops \(amount) key short on the right")
        }
    }

    /// The file's own numbers, labelled as its claim.
    static func evidence(_ item: TypingConfigReview.Item, review: TypingConfigReview) -> String? {
        guard let e = item.evidence else { return nil }
        let claim: String?
        switch item.part {
        case .keep:
            claim = e.times.map { t in e.sessions.map { String(localized: "left alone \(t) times in \($0) sessions") } ?? String(localized: "left alone \(t) times") }
        case .neverCorrectTo:
            claim = e.takenBack.map { String(localized: "taken back \($0) times") }
        case .fix:
            claim = e.times.map { t in e.sessions.map { String(localized: "fixed by hand \(t) times in \($0) sessions") } ?? String(localized: "fixed by hand \(t) times") }
        case .dictation:
            claim = e.times.map { String(localized: "fixed \($0) times after dictating") }
        case .aim, .zone:
            claim = e.checked.flatMap { c in c.gained.map { g in String(localized: "\(g) taps gained and \(c.lost ?? 0) lost on the typing it was checked on") } }
        case .note, .unreadable:
            claim = nil
        }
        return claim.map { String(localized: "The file says: \($0)") }
    }
}

/// A key block's keyboard in words: each field that differs, the file's and this iPhone's.
enum TypingConfigContextWords {
    static func differences(_ check: TypingConfigReview.ContextCheck) -> [String] {
        guard let keyboard = check.keyboard else {
            // No keyboard of theirs recorded yet: what it was made for.
            let made = ["width", "language", "letters"].map { "\(fieldName($0)): \(value($0, check.madeFor))" }.joined(separator: " · ")
            return [String(localized: "Made for \(made)")]
        }
        return check.differences.map { field in
            let name = fieldName(field)
            return String(localized: "\(name): \(value(field, check.madeFor)) in the file, \(value(field, keyboard)) here")
        }
    }

    static func fieldName(_ field: String) -> String {
        switch field {
        case "width": String(localized: "Width")
        case "orientation": String(localized: "Orientation")
        case "posture": String(localized: "Posture")
        case "language": String(localized: "Language")
        case "letters": String(localized: "Layout")
        case "split": String(localized: "Split keyboard")
        case "hexPlacement": String(localized: "Hex key")
        case "numberRow": String(localized: "Number row")
        case "keyHeight": String(localized: "Key height")
        case "rowPitch": String(localized: "Row spacing")
        case "letterWidth": String(localized: "Key width")
        default: field
        }
    }

    static func value(_ field: String, _ c: TypingConfigFile.Context) -> String {
        func points(_ v: Double) -> String { String(localized: "\(v.formatted(.number.precision(.fractionLength(0...1)))) pt") }
        func onOff(_ v: Bool) -> String { v ? String(localized: "On") : String(localized: "Off") }
        switch field {
        case "width": return points(c.width)
        case "orientation": return c.orientation == "portrait" ? String(localized: "Upright") : String(localized: "Sideways")
        case "posture": return c.posture
        case "language": return KeyboardLanguage(rawValue: c.language)?.displayName ?? c.language
        case "letters":
            let language = KeyboardLanguage(rawValue: c.language) ?? .english
            return KeyboardLetterLayout(rawValue: c.letters).map { language.layoutName($0) } ?? c.letters
        case "split": return onOff(c.split)
        case "hexPlacement":
            return switch c.hexPlacement {
            case "stripLeading", "leftOfSpace": String(localized: "Left")
            case "stripTrailing", "rightOfSpace": String(localized: "Right")
            default: c.hexPlacement
            }
        case "numberRow": return onOff(c.numberRow)
        case "keyHeight": return points(c.keyHeight)
        case "rowPitch": return points(c.rowPitch)
        case "letterWidth": return points(c.letterWidth)
        default: return ""
        }
    }
}

private extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
