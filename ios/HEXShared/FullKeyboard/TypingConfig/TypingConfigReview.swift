import CryptoKit
import CoreGraphics
import Foundation

/// What Hex would do with a typing config file: for every part, applied, clamped or skipped and
/// why, and each key block's keyboard against the one it would run on. The review sheet renders
/// this and nothing else; `compiled(switchedOn:)` turns it into `active.json`, and the keyboard
/// reads that back through the same checks (`checksEvidence: false`).
nonisolated struct TypingConfigReview: Equatable, Sendable {
    /// Why a part isn't applied, in the order a review lists them.
    enum Reason: Int, Comparable, CaseIterable, Sendable {
        /// Not JSON, or not a typing config (`format`): nothing is read.
        case notAConfig
        /// Larger than `TypingConfigLimits.maxBytes`: nothing is read.
        case tooLarge
        /// A field from a newer format version than this Hex reads.
        case newerFormat
        /// A field Hex doesn't know.
        case unknownField
        /// A value of the wrong kind or length, or an entry that doesn't parse.
        case invalidValue
        /// Past the list's limit.
        case tooMany
        /// The same entry again.
        case duplicate
        /// The file claims less evidence than the part needs.
        case notEnoughEvidence
        /// The key block is for another keyboard (`ContextCheck.differences`).
        case forAnotherKeyboard
        /// Zones measured on another version of Hex's touch areas (`KeyboardZone.hitModel`).
        case needsCheckingAgain
        /// A zone on a key this keyboard doesn't have.
        case keyNotOnKeyboard
        /// A zone on an edge that may not move (the keyboard's outer edge, or one the key
        /// doesn't allow).
        case edgeNotAllowed
        /// Two zones claim the same point: neither applies.
        case twoZonesClaimPoint
        /// The zone would leave a key less than its keycap's centre needs.
        case keyTooSmall

        static func < (a: Self, b: Self) -> Bool { a.rawValue < b.rawValue }
    }

    enum Outcome: Equatable, Sendable {
        case applied
        /// A number out of range, read as the nearest one in it.
        case clamped(from: [Double], to: [Double])
        /// The note, cut to `TypingConfigLimits.noteMaxCharacters`.
        case shortened(from: Int, to: Int)
        case skipped(Reason)

        var applies: Bool {
            if case .skipped = self { return false }
            return true
        }
    }

    /// One part a person can switch on or off.
    enum Part: Hashable, Sendable {
        case note
        case keep(String)
        case neverCorrectTo(String)
        case fix(typo: String, word: String)
        case dictation(heard: String, meant: String)
        case aim(block: Int, letter: String)
        case zone(block: Int, index: Int)
        /// A field Hex doesn't know or couldn't read, by its path (`words.keep[3]`).
        case unreadable(path: String)
    }

    struct Item: Equatable, Sendable {
        var part: Part
        /// Where it is in the file (`words.keep[0]`, `keys[0].zones[1]`).
        var path: String
        var outcome: Outcome
    }

    /// A key block's keyboard against the one the review was made for.
    struct ContextCheck: Equatable, Sendable {
        var block: Int
        var madeFor: TypingConfigFile.Context
        var keyboard: TypingConfigFile.Context
        /// The fields that differ (`TypingConfigFile.Context.differences`); empty when it applies.
        var differences: [String]
        var hitModelMatches: Bool
        var matches: Bool { differences.isEmpty }
    }

    /// What the keyboard uses from a config: its words and, for one keyboard, its key positions.
    struct Applied: Equatable, Sendable {
        var keep: [String] = []
        var neverCorrectTo: [String] = []
        var fixes: [TypingConfigFile.FixEntry] = []
        var dictation: [DictationCorrection] = []
        /// By letter, in keycaps (clamped).
        var aim: [Character: CGVector] = [:]
        var zones: [KeyboardZone] = []

        var isEmpty: Bool { self == Applied() }
    }

    /// Set when nothing could be read (`notAConfig`, `tooLarge`).
    var refused: Reason?
    var file: TypingConfigFile?
    /// SHA-256 of the file's bytes: its identity in the history (the file's own `id` is a name
    /// its writer gave it, not checked here).
    var contentHash: String
    var items: [Item] = []
    var contexts: [ContextCheck] = []
    /// Everything that applies on the keyboard reviewed for, with every part switched on.
    var applied = Applied()

    /// Reads `data` for `keyboard` (a letters layout). `checksEvidence` is false for
    /// `active.json`, which drops the evidence once a person has applied a config.
    init(data: Data, keyboard: KeyboardLayout, checksEvidence: Bool = true) {
        contentHash = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        guard data.count <= TypingConfigLimits.maxBytes else {
            refused = .tooLarge
            return
        }
        guard let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let file = try? JSONDecoder().decode(TypingConfigFile.self, from: data),
              file.format == TypingConfigLimits.format, file.version >= 0 else {
            refused = .notAConfig
            return
        }
        self.file = file
        var review = Builder(newer: file.version > TypingConfigLimits.version, checksEvidence: checksEvidence)
        review.unknownKeys(object, level: "", path: "")
        review.note(file.note)
        review.words(file.words)
        review.keys(file.keys ?? [], keyboard: keyboard)
        items = review.items
        contexts = review.contexts
        applied = review.applied
    }

    /// The parts that apply, by default all switched on.
    var switchable: [Part] { items.filter { $0.outcome.applies && $0.part != .note }.map(\.part) }

    /// `active.json` for the parts in `switchedOn` that apply: the file's envelope, no note and
    /// no evidence, values as clamped, and only the key block for the keyboard reviewed for.
    func compiled(switchedOn: Set<Part>) -> TypingConfigFile? {
        guard var out = file else { return nil }
        out.note = nil
        let on = Set(items.filter { $0.outcome.applies && switchedOn.contains($0.part) }.map(\.part))
        let a = applied
        func word(_ w: String) -> TypingConfigFile.Lossy<TypingConfigFile.WordEntry> { .init(.init(word: w)) }
        let keep = a.keep.filter { on.contains(.keep($0)) }.map(word)
        let never = a.neverCorrectTo.filter { on.contains(.neverCorrectTo($0)) }.map(word)
        let fixes = a.fixes.filter { on.contains(.fix(typo: $0.typo, word: $0.word)) }
            .map { TypingConfigFile.Lossy(TypingConfigFile.FixEntry(typo: $0.typo, word: $0.word, mode: $0.mode)) }
        let dictation = a.dictation.filter { on.contains(.dictation(heard: $0.heard, meant: $0.meant)) }
            .map { TypingConfigFile.Lossy(TypingConfigFile.DictationEntry(heard: $0.heard, meant: $0.meant)) }
        out.words = TypingConfigFile.Words(keep: keep, neverCorrectTo: never, fixes: fixes, dictation: dictation)
        out.keys = []
        if let check = contexts.first(where: \.matches) {
            let b = check.block
            var aim: [String: TypingConfigFile.Lossy<TypingConfigFile.Aim>] = [:]
            for (letter, v) in a.aim where on.contains(.aim(block: b, letter: String(letter))) {
                aim[String(letter)] = .init(.init(offset: [Double(v.dx), Double(v.dy)]))
            }
            let zoneParts = items.compactMap { item -> Int? in
                if case .zone(b, let i) = item.part, on.contains(item.part) { return i }
                return nil
            }
            let zoneOrder = items.compactMap { item -> Int? in
                if case .zone(b, let i) = item.part, item.outcome.applies { return i }
                return nil
            }
            let zones = zip(zoneOrder, a.zones).filter { zoneParts.contains($0.0) }.map { _, z in
                TypingConfigFile.Lossy(TypingConfigFile.Zone(key: z.key, edge: z.edge.rawValue, move: Double(z.move)))
            }
            if !aim.isEmpty || !zones.isEmpty {
                out.keys = [.init(.init(context: check.madeFor, aim: aim, zones: zones))]
            }
        }
        return out
    }
}

/// Walks the file in its order, recording an item per part.
private nonisolated struct Builder {
    typealias R = TypingConfigReview
    let newer: Bool
    let checksEvidence: Bool
    var items: [R.Item] = []
    var contexts: [R.ContextCheck] = []
    var applied = R.Applied()

    init(newer: Bool, checksEvidence: Bool) {
        self.newer = newer
        self.checksEvidence = checksEvidence
    }

    mutating func add(_ part: R.Part, _ path: String, _ outcome: R.Outcome) {
        items.append(R.Item(part: part, path: path, outcome: outcome))
    }

    /// Keys at each level the format doesn't have.
    mutating func unknownKeys(_ value: Any, level: String, path: String) {
        if let object = value as? [String: Any], level == "keys[].aim" {
            for letter in object.keys.sorted() {
                if let child = object[letter] { unknownKeys(child, level: "keys[].aim.*", path: "\(path).\(letter)") }
            }
        } else if let object = value as? [String: Any] {
            let known = TypingConfigFile.knownKeys[level]
            for key in object.keys.sorted() {
                let here = path.isEmpty ? key : "\(path).\(key)"
                if let known, !known.contains(key) {
                    add(.unreadable(path: here), here, .skipped(newer ? .newerFormat : .unknownField))
                } else if let next = Self.childLevel(level, key), let child = object[key] {
                    unknownKeys(child, level: next, path: here)
                }
            }
        } else if let array = value as? [Any] {
            for (i, element) in array.enumerated() { unknownKeys(element, level: level + "[]", path: "\(path)[\(i)]") }
        }
    }

    /// The level of `key`'s value inside `level` (`TypingConfigFile.knownKeys`), or nil for a leaf.
    static func childLevel(_ level: String, _ key: String) -> String? {
        switch (level, key) {
        case ("", "madeFrom"), ("", "madeFor"), ("", "words"), ("", "keys"): key
        case ("madeFrom", "heldOut"): "madeFrom.heldOut"
        case ("words", _): "words.\(key)"
        case ("keys[]", "context"): "keys[].context"
        case ("keys[]", "aim"): "keys[].aim"
        case ("keys[]", "zones"): "keys[].zones"
        case (_, "evidence"): "evidence"
        case ("evidence", "tuned"), ("evidence", "checked"): "tally"
        default: nil
        }
    }

    mutating func note(_ note: String?) {
        guard let note else { return }
        let limit = TypingConfigLimits.noteMaxCharacters
        add(.note, "note", note.count > limit ? .shortened(from: note.count, to: limit) : .applied)
    }

    private func letters(_ word: String) -> Bool { TypingConfigLimits.wordLetters.contains(word.count) }

    private func enough(_ e: TypingConfigFile.Evidence?, _ check: (TypingConfigFile.Evidence) -> Bool) -> Bool {
        !checksEvidence || e.map(check) == true
    }

    mutating func words(_ words: TypingConfigFile.Words?) {
        guard let words else { return }
        var seen = Set<String>()
        for (i, entry) in (words.keep ?? []).enumerated() {
            let path = "words.keep[\(i)]"
            guard let e = entry.value, letters(e.word) else { add(.unreadable(path: path), path, .skipped(.invalidValue)); continue }
            let outcome: R.Outcome = if i >= TypingConfigLimits.maxKeep { .skipped(.tooMany) }
                else if !seen.insert("k" + e.word).inserted { .skipped(.duplicate) }
                else if !enough(e.evidence, { ($0.times ?? 0) >= TypingConfigLimits.keepMinTimes && ($0.sessions ?? 0) >= TypingConfigLimits.keepMinSessions }) { .skipped(.notEnoughEvidence) }
                else { .applied }
            add(.keep(e.word), path, outcome)
            if outcome.applies { applied.keep.append(e.word) }
        }
        for (i, entry) in (words.neverCorrectTo ?? []).enumerated() {
            let path = "words.neverCorrectTo[\(i)]"
            guard let e = entry.value, letters(e.word) else { add(.unreadable(path: path), path, .skipped(.invalidValue)); continue }
            let outcome: R.Outcome = if i >= TypingConfigLimits.maxNeverCorrectTo { .skipped(.tooMany) }
                else if !seen.insert("n" + e.word).inserted { .skipped(.duplicate) }
                else if !enough(e.evidence, { ($0.takenBack ?? 0) >= TypingConfigLimits.neverCorrectToMinTakenBack }) { .skipped(.notEnoughEvidence) }
                else { .applied }
            add(.neverCorrectTo(e.word), path, outcome)
            if outcome.applies { applied.neverCorrectTo.append(e.word) }
        }
        for (i, entry) in (words.fixes ?? []).enumerated() {
            let path = "words.fixes[\(i)]"
            guard let e = entry.value, letters(e.typo), letters(e.word), e.typo.lowercased() != e.word.lowercased() else {
                add(.unreadable(path: path), path, .skipped(.invalidValue)); continue
            }
            let outcome: R.Outcome = if i >= TypingConfigLimits.maxFixes { .skipped(.tooMany) }
                else if !seen.insert("f" + e.typo.lowercased()).inserted { .skipped(.duplicate) }
                else if !enough(e.evidence, { ($0.times ?? 0) >= TypingConfigLimits.fixMinTimes && ($0.sessions ?? 0) >= TypingConfigLimits.fixMinSessions }) { .skipped(.notEnoughEvidence) }
                else { .applied }
            add(.fix(typo: e.typo, word: e.word), path, outcome)
            if outcome.applies { applied.fixes.append(TypingConfigFile.FixEntry(typo: e.typo, word: e.word, mode: e.mode)) }
        }
        for (i, entry) in (words.dictation ?? []).enumerated() {
            let path = "words.dictation[\(i)]"
            let maxWords = TypingConfigLimits.dictationMaxWords
            func short(_ s: String) -> Bool {
                let n = s.split(separator: " ").count
                return n >= 1 && n <= maxWords && s.count <= 96
            }
            guard let e = entry.value, short(e.heard), short(e.meant), e.heard != e.meant else {
                add(.unreadable(path: path), path, .skipped(.invalidValue)); continue
            }
            let outcome: R.Outcome = if i >= TypingConfigLimits.maxDictation { .skipped(.tooMany) }
                else if !seen.insert("d" + e.heard.lowercased()).inserted { .skipped(.duplicate) }
                else if !enough(e.evidence, { ($0.times ?? 0) >= TypingConfigLimits.dictationMinTimes }) { .skipped(.notEnoughEvidence) }
                else { .applied }
            add(.dictation(heard: e.heard, meant: e.meant), path, outcome)
            if outcome.applies { applied.dictation.append(DictationCorrection(heard: e.heard, meant: e.meant)) }
        }
    }

    mutating func keys(_ blocks: [TypingConfigFile.Lossy<TypingConfigFile.KeyBlock>], keyboard: KeyboardLayout) {
        let live = TypingConfigFile.Context(layout: keyboard)
        var used = false
        for (b, entry) in blocks.enumerated() {
            let path = "keys[\(b)]"
            guard let block = entry.value else { add(.unreadable(path: path), path, .skipped(.invalidValue)); continue }
            let check = R.ContextCheck(block: b, madeFor: block.context, keyboard: live,
                                       differences: block.context.differences(from: live),
                                       hitModelMatches: block.context.hitModel == KeyboardZone.hitModel)
            contexts.append(check)
            // Only the first block for this keyboard applies; later ones are for another.
            let here = check.matches && !used && b < TypingConfigLimits.maxContexts
            if here { used = true }
            let blockReason: R.Reason? = b >= TypingConfigLimits.maxContexts ? .tooMany : here ? nil : .forAnotherKeyboard
            for letter in (block.aim ?? [:]).keys.sorted() {
                let aimPath = "\(path).aim.\(letter)"
                guard let aim = block.aim?[letter]?.value, letter.count == 1, letter.first?.isLetter == true,
                      letter == letter.lowercased(), aim.offset.count == 2, aim.offset.allSatisfy(\.isFinite) else {
                    add(.unreadable(path: aimPath), aimPath, .skipped(.invalidValue)); continue
                }
                let bound = Double(TypingConfigLimits.aimBound)
                let to = aim.offset.map { max(-bound, min(bound, $0)) }
                let outcome: R.Outcome = if let blockReason { .skipped(blockReason) }
                    else if !keyboard.keys.contains(where: { $0.character == letter }) { .skipped(.keyNotOnKeyboard) }
                    else if !enough(aim.evidence, { e in
                        (e.tuned?.taps ?? 0) >= TypingConfigLimits.aimMinTunedTaps && (e.checked?.taps ?? 0) >= TypingConfigLimits.aimMinCheckedTaps
                            && (e.checked?.gained ?? 0) >= (e.checked?.lost ?? Int.max)
                    }) { .skipped(.notEnoughEvidence) }
                    else if to != aim.offset { .clamped(from: aim.offset, to: to) }
                    else { .applied }
                add(.aim(block: b, letter: letter), aimPath, outcome)
                if outcome.applies, let c = letter.first { applied.aim[c] = CGVector(dx: to[0], dy: to[1]) }
            }
            var candidates: [(index: Int, zone: KeyboardZone, outcome: R.Outcome)] = []
            for (i, entry) in (block.zones ?? []).enumerated() {
                let zonePath = "\(path).zones[\(i)]"
                guard let zone = entry.value, let edge = KeyboardZone.Edge(rawValue: zone.edge), zone.move.isFinite else {
                    add(.unreadable(path: zonePath), zonePath, .skipped(.invalidValue)); continue
                }
                let bound = Double(TypingConfigLimits.zoneMaxMove)
                let to = max(-bound, min(bound, zone.move))
                let outcome: R.Outcome = if let blockReason { .skipped(blockReason) }
                    else if !check.hitModelMatches { .skipped(.needsCheckingAgain) }
                    else if !enough(zone.evidence, { e in
                        let gained = e.checked?.gained ?? 0, lost = e.checked?.lost ?? 0
                        return (e.tuned?.taps ?? 0) >= TypingConfigLimits.zoneMinTunedTaps
                            && gained - lost >= TypingConfigLimits.zoneMinNetCheckedTaps
                            && Double(lost) <= TypingConfigLimits.zoneMaxLostShare * Double(gained)
                    }) { .skipped(.notEnoughEvidence) }
                    else if to != zone.move { .clamped(from: [zone.move], to: [to]) }
                    else { .applied }
                candidates.append((i, KeyboardZone(key: zone.key, edge: edge, move: CGFloat(to)), outcome))
            }
            // The geometry: the zones that still apply, together, on this keyboard.
            let applying = candidates.filter { $0.outcome.applies }
            let map = keyboard.zoneMap(applying.map(\.zone))
            var geometry: [Int: R.Reason] = [:]
            for (n, c) in applying.enumerated() { if let reason = map.skipped[n] { geometry[c.index] = reason } }
            for c in candidates {
                let outcome = geometry[c.index].map { R.Outcome.skipped($0) } ?? c.outcome
                add(.zone(block: b, index: c.index), "\(path).zones[\(c.index)]", outcome)
                if outcome.applies { applied.zones.append(c.zone) }
            }
        }
    }
}
